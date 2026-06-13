//! 节点本机能力探测
//!
//! 两套能力来源:
//!   1. 本机 Python 是否满足 manifest.python.min_version (用于第一次安装)
//!   2. ~/.qianshou/runtime/installed.json (用于上报和 UI 展示)
//!
//! 不再依赖 `python3 -c "import PIL"` 这种系统级探测 ·
//! 探测的目标变成 venv 自检结果。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use super::paths;

/// installed.json 顶层结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledMeta {
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub install_mode: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub host_python: Option<String>,
    #[serde(default)]
    pub tiers: BTreeMap<String, InstalledTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledTier {
    /// 自检是否通过
    #[serde(default)]
    pub ok: bool,
    /// venv python 绝对路径
    #[serde(default)]
    pub python: String,
    /// 该 tier 安装的 package
    #[serde(default)]
    pub packages: Vec<String>,
    /// 上报给后端调度器的 software (planner.py 用这个匹配)
    #[serde(default)]
    pub software: Vec<String>,
    /// 选中的源 (用于 UI 展示)
    #[serde(default)]
    pub mirror_label: String,
    /// 最近一次安装时间 (ISO8601)
    #[serde(default)]
    pub installed_at: String,
    /// 最后一次自检消息 (失败时显示)
    #[serde(default)]
    pub last_message: String,
    /// imageio-ffmpeg 等 tier 内置的二进制路径 (executor 注入 EC_FFMPEG)
    #[serde(default)]
    pub binaries: BTreeMap<String, String>,
    /// 装完 tier 后从后端拉的 skill 安装快照 (id -> version) · UI 展示用
    #[serde(default)]
    pub installed_skills: BTreeMap<String, String>,
}

/// 读 installed.json · 不存在返回默认 (空)
pub fn read_installed_meta() -> InstalledMeta {
    let path = paths::installed_meta_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => InstalledMeta::default(),
    }
}

// ════════════════════════════════════════════════════════════════════
// 8.1.8 · 能力探针 (probe) · 治"假装"
//
// 问题: 此前只要 installed.json 里 tier.ok==true,就把 manifest 声称的
//       software 全部上报。8GB 弱机装了 vision-ai tier 就吹自己有 torch,
//       结果接到 LLM 任务直接 OOM。
//
// 方案: 用每个 tier **自己 venv 的 python** 真 import 验证声称的包,
//       只上报真能 import 的;系统工具 (ffmpeg/blender) 用 which 验。
//       结果缓存,自愈装新包后调 invalidate_probe_cache() 刷新。
// ════════════════════════════════════════════════════════════════════

use std::collections::BTreeSet;
use std::sync::RwLock;

static PROBED_SW: RwLock<Option<Vec<String>>> = RwLock::new(None);

/// 系统命令行工具 (用 which 验,不是 python 包)
fn is_system_tool(sw: &str) -> bool {
    matches!(
        sw,
        "ffmpeg" | "ffprobe" | "blender" | "ollama" | "imagemagick"
            | "convert" | "unzip" | "git" | "tesseract"
    )
}

/// pip 包名 → import 名 (probe 时用 import 名 · 常见不一致的映射)
fn pip_to_import(pip: &str) -> &str {
    match pip {
        "pillow" => "PIL",
        "pymupdf" => "fitz",
        "opencv-python" | "opencv-python-headless" => "cv2",
        "scikit-learn" => "sklearn",
        "beautifulsoup4" => "bs4",
        "pyyaml" => "yaml",
        "scikit-image" => "skimage",
        "faster-whisper" => "faster_whisper",
        "python-dateutil" => "dateutil",
        other => other,
    }
}

/// 用指定 venv python 一次性 import 一批模块 · 返回真能 import 的 import 名集合。
/// 单进程跑完所有 import (省解释器启动开销),带整批超时防卡死。
fn probe_imports_batch(venv_py: &str, import_names: &[&str]) -> BTreeSet<String> {
    use std::process::{Command as StdCommand, Stdio};
    let mut out = BTreeSet::new();
    if import_names.is_empty() {
        return out;
    }
    // python: 逐个 try import · 成功的打印一行
    let script = r#"
import sys
for m in sys.argv[1:]:
    try:
        __import__(m)
        print(m, flush=True)
    except Exception:
        pass
"#;
    let mut cmd = StdCommand::new(venv_py);
    cmd.arg("-c").arg(script);
    for m in import_names {
        cmd.arg(m);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    crate::proc_util::hide_window_std(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return out, // venv python 起不来 → 全不算有
    };
    // 整批超时 90s (torch/transformers import 慢 · 但不能无限卡)
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    return out; // 卡死 = 这批都不算可用 (宁缺毋滥)
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(_) => return out,
        }
    }
    if let Ok(output) = child.wait_with_output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let m = line.trim();
            if !m.is_empty() {
                out.insert(m.to_string());
            }
        }
    }
    out
}

/// 2026-06-06 · 验证 ffmpeg tier 的 imageio-ffmpeg (ffmpeg binary 在 venv 内·系统 which 找不到)
/// 用 venv python 跑 imageio_ffmpeg.get_ffmpeg_exe() · binary 文件存在则 ffmpeg 真可用。
fn probe_ffmpeg_via_imageio(venv_py: &str) -> bool {
    use std::process::{Command as StdCommand, Stdio};
    let mut cmd = StdCommand::new(venv_py);
    cmd.arg("-c").arg(
        "import imageio_ffmpeg,os,sys; p=imageio_ffmpeg.get_ffmpeg_exe(); sys.exit(0 if (p and os.path.exists(p)) else 1)",
    );
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    crate::proc_util::hide_window_std(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => return false,
        }
    }
}

/// 对单个 tier 做真实探针 · 返回**验证通过**的 software 子集
pub fn probe_tier_software(tier: &InstalledTier) -> Vec<String> {
    let mut verified: Vec<String> = Vec::new();
    let mut py_pkgs: Vec<(String, String)> = Vec::new(); // (import_name, sw_name)

    for sw in &tier.software {
        if is_system_tool(sw) {
            if which::which(sw).is_ok() {
                verified.push(sw.clone());
            } else if sw == "ffmpeg"
                && !tier.python.is_empty()
                && probe_ffmpeg_via_imageio(&tier.python)
            {
                // 2026-06-06 · imageio-ffmpeg 的 ffmpeg 在 venv 内(不在系统 PATH)·which 找不到
                // 用 venv python 跑 imageio_ffmpeg.get_ffmpeg_exe() 验证 → 修 ffmpeg 内置不上报导致任务派不出
                verified.push(sw.clone());
            }
        } else {
            py_pkgs.push((pip_to_import(sw).to_string(), sw.clone()));
        }
    }

    let py_ok = !tier.python.is_empty() && std::path::Path::new(&tier.python).exists();
    if !py_pkgs.is_empty() && py_ok {
        let imports: Vec<&str> = py_pkgs.iter().map(|(i, _)| i.as_str()).collect();
        let ok_set = probe_imports_batch(&tier.python, &imports);
        for (imp, sw) in &py_pkgs {
            if ok_set.contains(imp) {
                verified.push(sw.clone());
            }
        }
    }
    verified
}

/// 读 installed.json → 对每个 ok tier 跑探针 → 真实可用 software 列表 (带缓存)。
/// force=true 强制重新探针 (自愈装新包后用)。
pub fn probed_software(force: bool) -> Vec<String> {
    if !force {
        if let Ok(guard) = PROBED_SW.read() {
            if let Some(v) = guard.as_ref() {
                return v.clone();
            }
        }
    }
    let installed = read_installed_meta();
    let mut all: BTreeSet<String> = BTreeSet::new();
    for (tier_name, tier) in installed.tiers.iter() {
        if !tier.ok {
            continue;
        }
        let verified = probe_tier_software(tier);
        let claimed = tier.software.len();
        if verified.len() < claimed {
            tracing::warn!(
                "probe · tier={} 声称 {} 个 software · 实测可用 {} 个 (剔除 {} 个假装)",
                tier_name, claimed, verified.len(), claimed - verified.len()
            );
        }
        for s in verified {
            all.insert(s);
        }
    }
    let result: Vec<String> = all.into_iter().collect();
    if let Ok(mut guard) = PROBED_SW.write() {
        *guard = Some(result.clone());
    }
    result
}

/// 自愈装了新包后调用 · 下次 probed_software 会重新探针
pub fn invalidate_probe_cache() {
    if let Ok(mut guard) = PROBED_SW.write() {
        *guard = None;
    }
}

// ════════════════════════════════════════════════════════════════════
// 8.2.1 · 启动 boot smoke test · 治 bundled venv 假装 ok 的问题
//
// 问题: NSIS 烘焙的 cpython + venvs 解到 ~/.qianshou/runtime 后 ·
//       bootstrap_bundled.rs 直接把所有 tier 标 t.ok=true · 不真跑验证 ·
//       venv 内 pyvenv.cfg / site-packages 任何问题都被吹成 "已装好" ·
//       broker 派任务到节点后子进程秒崩 (exit code 1 · stderr 为空)。
//
// 方案: 启动时同步并发跑 `<venv>/python --version` 验证每个 tier 的
//       python.exe 能否正常启动 · 不通过的标 t.ok=false 持久化 ·
//       hello 阶段 capabilities.runtime_tiers 只上报真能启动的子集。
// ════════════════════════════════════════════════════════════════════

/// 单 tier smoke: 用 venv python 跑 `--version` · 5s 超时 · 返回 (ok, 失败原因)
fn smoke_one_tier(tier_name: &str, python_path: &str) -> (bool, String) {
    use std::process::{Command as StdCommand, Stdio};
    if python_path.is_empty() {
        return (false, format!("tier={} 无 venv python 路径", tier_name));
    }
    if !std::path::Path::new(python_path).exists() {
        return (false, format!("tier={} python 路径不存在: {}", tier_name, python_path));
    }
    let mut cmd = StdCommand::new(python_path);
    cmd.arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    crate::proc_util::hide_window_std(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            // OS 层错误 (DLL not found / 找不到文件 / 权限不足 ...)
            return (false, format!("spawn 失败: {} ({})", python_path, e));
        }
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                if st.success() {
                    return (true, String::new());
                }
                let stderr = child
                    .wait_with_output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stderr).chars().take(300).collect::<String>())
                    .unwrap_or_default();
                return (
                    false,
                    format!(
                        "python --version exit={} stderr={}",
                        st.code().unwrap_or(-1),
                        stderr
                    ),
                );
            }
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    return (false, format!("python --version 超时 5s · {}", python_path));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return (false, format!("wait 失败: {}", e)),
        }
    }
}

/// 启动 boot smoke test (阻塞 · 并发跑所有 tier · 整批超时 10s)
///
/// 对每个 installed.json 里 t.ok=true 的 tier 跑 `python --version` ·
/// 失败的标 t.ok=false + last_message="boot_smoke_failed: ..." · 持久化 ·
/// 返回 (验证通过的 tier 名集合, 假装 ok 的 tier 名集合)。
///
/// 在 hello 前调用一次 · 让 capabilities 上报真实可用 tier。
pub fn boot_smoke_test_and_persist() -> (Vec<String>, Vec<String>) {
    let mut meta = read_installed_meta();
    let tiers_to_check: Vec<(String, String)> = meta
        .tiers
        .iter()
        .filter(|(_, t)| t.ok && !t.python.is_empty())
        .map(|(name, t)| (name.clone(), t.python.clone()))
        .collect();

    if tiers_to_check.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // 并发跑所有 tier · 各自 5s 超时 · 整批 10s 兜底
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel::<(String, bool, String)>();
    let total = tiers_to_check.len();
    for (name, py) in tiers_to_check {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let (ok, msg) = smoke_one_tier(&name, &py);
            let _ = tx.send((name, ok, msg));
        });
    }
    drop(tx);

    let mut alive: Vec<String> = Vec::new();
    let mut fake: Vec<String> = Vec::new();
    let batch_deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut received = 0;
    while received < total {
        let remaining = batch_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok((name, ok, msg)) => {
                received += 1;
                if ok {
                    alive.push(name);
                } else {
                    if let Some(t) = meta.tiers.get_mut(&name) {
                        t.ok = false;
                        t.last_message = format!("boot_smoke_failed: {}", msg);
                    }
                    fake.push(name.clone());
                    tracing::warn!("boot_smoke · tier={} FAIL · {}", name, msg);
                }
            }
            Err(_) => break,
        }
    }

    if !fake.is_empty() {
        if let Err(e) = write_installed_meta(&meta) {
            tracing::warn!("boot_smoke · 持久化 installed.json 失败: {}", e);
        } else {
            tracing::info!(
                "boot_smoke · 完成 · 真实可用 {} 个 · 剔除假装 {} 个: {:?}",
                alive.len(), fake.len(), fake
            );
        }
    } else {
        tracing::info!("boot_smoke · 完成 · {} 个 tier 全部通过验证", alive.len());
    }
    (alive, fake)
}

/// 8.1.9 · 非阻塞读缓存 (绝不触发同步探针) · 给 WS 主循环用。
/// 缓存未就绪返回 None,调用方应回退到 manifest 兜底,**不能**在主循环里同步跑 probe
/// (probe 要起 python import 重型包 · 最长 90s · 会阻塞 WS 事件循环 → 节点收不到任务/不回结果)。
pub fn probed_software_cached() -> Option<Vec<String>> {
    PROBED_SW.read().ok().and_then(|g| g.clone())
}

/// 8.1.9 · 后台预热探针缓存 (在专用阻塞线程跑,不占 tokio 异步线程)。
/// 节点启动/连接后调用一次 · 几十秒后缓存就绪 · 之后心跳就能上报真实能力。
pub fn spawn_probe_warmup() {
    std::thread::spawn(|| {
        let sw = probed_software(true);
        tracing::info!("probe · 后台预热完成 · 真实可用 software {} 个", sw.len());
    });
}

/// 写 installed.json · 原子写 (先写临时文件 + rename)
pub fn write_installed_meta(meta: &InstalledMeta) -> Result<()> {
    let path = paths::installed_meta_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| anyhow!("创建 runtime 目录失败: {}", e))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(meta)?;
    std::fs::write(&tmp, body).map_err(|e| anyhow!("写 installed.json.tmp 失败: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| anyhow!("rename installed.json 失败: {}", e))?;
    Ok(())
}

/// 寻找一个可用的本机 Python (做 venv 用)
///
/// 顺序:
///   1. EDGECOMPUTE_HOST_PYTHON env 强制指定
///   2. python3.11 / python3.10 / python3.9 / python3 / python
///   3. macOS 常见路径 (`/usr/bin/python3`, `/opt/homebrew/bin/python3`)
///   4. Windows `py -3`
pub async fn detect_host_python() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("EDGECOMPUTE_HOST_PYTHON") {
        let pb = PathBuf::from(&p);
        if check_python(&pb).await.is_some() {
            return Some(pb);
        }
    }

    let candidates_cmd = [
        "python3.11", "python3.10", "python3.9", "python3", "python",
    ];
    for cmd in candidates_cmd {
        if let Some(p) = which_python(cmd).await {
            if check_python(&p).await.is_some() {
                return Some(p);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for hard in ["/opt/homebrew/bin/python3", "/usr/local/bin/python3", "/usr/bin/python3"] {
            let p = PathBuf::from(hard);
            if p.exists() && check_python(&p).await.is_some() {
                return Some(p);
            }
        }
    }

    None
}

async fn which_python(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

/// 检查 Python 版本 · 返回 (major,minor,patch)
pub async fn check_python(p: &PathBuf) -> Option<(u32, u32, u32)> {
    let mut cmd = Command::new(p);
    cmd.arg("-c")
        .arg("import sys;print('.'.join(str(x) for x in sys.version_info[:3]))")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    crate::proc_util::hide_window_tokio(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(5), cmd.output()).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut parts = s.split('.').map(|x| x.parse::<u32>().unwrap_or(0));
    Some((parts.next()?, parts.next()?, parts.next().unwrap_or(0)))
}
