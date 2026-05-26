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
