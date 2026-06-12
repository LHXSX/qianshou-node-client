//! Runtime 标准路径
//!
//! 设计:
//!   ~/.qianshou/runtime/
//!     installed.json        所有已装 tier 的快照 · WS hello 读这个上报
//!     venvs/
//!       lite/               每个 tier 一个独立 venv (Python 隔离)
//!         bin/python
//!         lib/python3.x/site-packages/
//!         ...
//!       ocr/
//!       speech/
//!     logs/                 安装日志 (debug 用)

use std::path::PathBuf;

/// 运行时根目录 ~/.qianshou/runtime
pub fn runtime_root() -> PathBuf {
    if let Some(home) = dirs_home() {
        home.join(".qianshou").join("runtime")
    } else {
        PathBuf::from(".qianshou/runtime")
    }
}

/// venvs 总目录
pub fn venvs_root() -> PathBuf {
    runtime_root().join("venvs")
}

/// 2026-05-24 · tier 二进制安装总目录 (静态二进制 · 如 ffmpeg)
///   ~/.qianshou/runtime/tiers/<tier>/
///       bin/ffmpeg
///       bin/ffprobe
///       (其它解出来的文件)
pub fn tiers_root() -> PathBuf {
    runtime_root().join("tiers")
}

/// 指定 tier 的二进制安装根目录
pub fn tier_root(tier: &str) -> PathBuf {
    tiers_root().join(sanitize_tier(tier))
}

/// 指定 tier 的 venv 目录
pub fn venv_dir(tier: &str) -> PathBuf {
    venvs_root().join(sanitize_tier(tier))
}

/// 指定 tier 的 venv python 可执行文件路径
pub fn venv_python(tier: &str) -> PathBuf {
    let base = venv_dir(tier);
    #[cfg(target_os = "windows")]
    {
        base.join("Scripts").join("python.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        base.join("bin").join("python")
    }
}

/// installed.json 路径 · WS hello 读这个上报能力
pub fn installed_meta_path() -> PathBuf {
    runtime_root().join("installed.json")
}

/// 安装日志目录
pub fn logs_dir() -> PathBuf {
    runtime_root().join("logs")
}

/// V8.1 (2026-05-27) · 直下系统二进制总目录 (如 blender · 绕开 brew/winget)
/// 例:
///   ~/.qianshou/runtime/system_bin/render/
///       Blender.app/Contents/MacOS/Blender  (macOS dmg 解出)
///       blender-4.2.0-windows-x64/blender.exe  (Win zip 解出)
///       blender-4.2.0-linux-x64/blender  (Linux tarxz 解出)
pub fn system_bin_root(tier: &str) -> PathBuf {
    runtime_root().join("system_bin").join(sanitize_tier(tier))
}

/// uv 二进制缓存目录 (从 bundled resource 拷贝过来 · 或 HTTP 下载)
pub fn uv_bin_dir() -> PathBuf {
    runtime_root().join("bin")
}

/// 本机 uv 可执行路径
pub fn uv_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        uv_bin_dir().join("uv.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        uv_bin_dir().join("uv")
    }
}

/// uv 管理的 Python 安装根目录 (用 UV_PYTHON_INSTALL_DIR env 隔离到我们自家)
pub fn uv_python_dir() -> PathBuf {
    runtime_root().join("python")
}

/// 2026-05-23 · 内置预烘焙的 cpython 真二进制 (不是 venv wrapper)
///
/// 为啥不直接用 envs/<env>/bin/python:
///   Tauri bundle 打包时 deref symlink · venv 的 python 变成 cpython 的真二进制副本
///   导致 sys.prefix 算错 (找不到 stdlib · 报 "No module named 'encodings'")
///   → 直接用 cpython 自身 python 二进制 (它能找到自己的 stdlib)
///   → 用 PYTHONPATH 指 envs/<env>/lib/python3.11/site-packages 喂第三方包
///
/// 返回 None: bundle 没烘焙 → 调用方 fallback 系统 `python3`
pub fn bundled_python_bin() -> Option<PathBuf> {
    let cpython_root = runtime_root().join("cpython");
    if !cpython_root.is_dir() {
        return None;
    }
    // 找 cpython-3.11.x-{triple} 目录 (只应有一个)
    let entries = std::fs::read_dir(&cpython_root).ok()?;
    for entry in entries.flatten() {
        if entry.file_type().ok()?.is_dir() {
            let name = entry.file_name();
            let name_s = name.to_string_lossy();
            if !name_s.starts_with("cpython-") {
                continue;
            }
            let p = if cfg!(target_os = "windows") {
                entry.path().join("python.exe")
            } else {
                entry.path().join("bin").join("python3.11")
            };
            if p.exists() {
                return Some(p);
            }
            // fallback: bin/python
            let p2 = if cfg!(target_os = "windows") {
                p.clone()
            } else {
                entry.path().join("bin").join("python")
            };
            if p2.exists() {
                return Some(p2);
            }
        }
    }
    None
}

/// 烘焙 env 里的 site-packages 路径 (跑任务时 PYTHONPATH 指它)
/// 例: bundled_site_packages("image") → ~/.qianshou/runtime/envs/image/lib/python3.11/site-packages
pub fn bundled_site_packages(env_name: &str) -> Option<PathBuf> {
    let env_root = runtime_root().join("envs").join(env_name);
    if !env_root.is_dir() {
        return None;
    }
    if cfg!(target_os = "windows") {
        let p = env_root.join("Lib").join("site-packages");
        if p.exists() {
            return Some(p);
        }
    } else {
        // Unix: lib/python3.X/site-packages · 找匹配的子目录
        let lib = env_root.join("lib");
        if let Ok(entries) = std::fs::read_dir(&lib) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("python") {
                    let sp = entry.path().join("site-packages");
                    if sp.exists() {
                        return Some(sp);
                    }
                }
            }
        }
    }
    None
}

/// 按 env 优先级返回 (python 路径, PYTHONPATH 列表)
/// preference 例: &["image", "base"]
/// 返回 None: 没有任何 env / 没烘焙 → fallback 系统 python3
pub fn bundled_runtime_for(envs: &[&str]) -> Option<(PathBuf, Vec<PathBuf>)> {
    let py = bundled_python_bin()?;
    let mut path: Vec<PathBuf> = Vec::new();
    for e in envs {
        if let Some(sp) = bundled_site_packages(e) {
            path.push(sp);
        }
    }
    Some((py, path))
}

/// 本地技能安装根目录 · 跟 client-v3/src-tauri/src/task/skill_registry.rs default_skill_roots() 一致
///
/// 节点装完 tier 后会按 tier.skills[] 列表把 zip 解压到这里 · skill_registry 启动时扫这里
pub fn skills_install_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("EdgeCompute").join("skills");
        }
        // Win 缺 LOCALAPPDATA 时兜底 · 罕见
        return runtime_root().join("skills");
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = dirs_home() {
            return home
                .join(".local")
                .join("lib")
                .join("edgecompute")
                .join("skills");
        }
        runtime_root().join("skills")
    }
}

/// 防止 tier 名带 .. / 等异常字符破坏路径
fn sanitize_tier(tier: &str) -> String {
    tier.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

fn dirs_home() -> Option<PathBuf> {
    // 不引入 dirs crate 以减少依赖 · 直接读 HOME / USERPROFILE
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// V8.1 (2026-05-27) · 通用 python 路由 · executor + tool_caller 共用
///
/// 返回 (python_bin, bundled_pythonpath)
///   - python_bin: 绝对路径或命令名
///   - bundled_pythonpath: 老路径 fallback 时塞 PYTHONPATH 的目录列表 (新路径走 venv 自带 site-packages · 列表为空)
///
/// 路由优先级 (高 → 低):
///   1. required_tier (调用方指定 · 如 task.required_tier)
///   2. fallback_tiers 按顺序 try
///   3. venvs/lite (auto-install · 大概率装好)
///   4. 老路径: bundled_runtime_for(["image","base"])
///   5. 最终兜底: 系统 python3
///
/// 双端: venv_python(tier) 已 cfg(windows) 区分 bin/python vs Scripts/python.exe
///
/// 2026-06-03 · 改为取 python_candidates 的第一个 · 行为兼容老调用方
pub fn pick_python_with_hint(
    required_tier: Option<&str>,
    fallback_tiers: &[String],
) -> (String, Vec<PathBuf>) {
    python_candidates(required_tier, fallback_tiers)
        .into_iter()
        .next()
        .unwrap_or_else(|| ("python3".to_string(), Vec::new()))
}

/// uv 托管的 cpython 真二进制 (UV_PYTHON_INSTALL_DIR = uv_python_dir)
/// 结构: <uv_python_dir>/cpython-3.x.y-<triple>/python.exe (Win) | bin/python3.x (Unix)
pub fn uv_managed_python() -> Option<PathBuf> {
    let root = uv_python_dir();
    if !root.is_dir() {
        return None;
    }
    let entries = std::fs::read_dir(&root).ok()?;
    for entry in entries.flatten() {
        if !entry.file_type().ok()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("cpython-") {
            continue;
        }
        if cfg!(target_os = "windows") {
            let p = entry.path().join("python.exe");
            if p.exists() {
                return Some(p);
            }
        } else {
            // bin/python3.x 或 bin/python
            let bin = entry.path().join("bin");
            if let Ok(rd) = std::fs::read_dir(&bin) {
                for f in rd.flatten() {
                    let fname = f.file_name().to_string_lossy().into_owned();
                    if fname.starts_with("python3") || fname == "python" {
                        return Some(f.path());
                    }
                }
            }
        }
    }
    None
}

/// 系统 python 兜底候选 (绝对路径优先 · 最后才用裸命令)
/// Windows: py 启动器 + LOCALAPPDATA / Program Files 常见安装位 · 绝不只靠 PATH 里的 python3
fn system_python_candidates() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    #[cfg(target_os = "windows")]
    {
        // 1. 常见安装目录里的 python.exe (绝对路径 · 最稳)
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("Programs").join("Python"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            roots.push(PathBuf::from(pf));
        }
        roots.push(PathBuf::from(r"C:\Python311"));
        roots.push(PathBuf::from(r"C:\Python312"));
        roots.push(PathBuf::from(r"C:\Python310"));
        for root in roots {
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    if n.starts_with("python") {
                        let p = e.path().join("python.exe");
                        if p.exists() {
                            out.push(p.to_string_lossy().into_owned());
                        }
                    }
                }
            }
            // root 本身就是 pythonXX 目录的情况
            let direct = root.join("python.exe");
            if direct.exists() {
                out.push(direct.to_string_lossy().into_owned());
            }
        }
        // 2. py 启动器 (微软推荐 · 能自动找已装 python) · 作为命令兜底
        out.push("py".to_string());
        // 3. 最后才裸 python (PATH 里有就用) · python3 在 Win 常是 Store 占位,放最后
        out.push("python".to_string());
        out.push("python3".to_string());
    }
    #[cfg(not(target_os = "windows"))]
    {
        for p in [
            "/usr/bin/python3",
            "/usr/local/bin/python3",
            "/opt/homebrew/bin/python3",
        ] {
            if std::path::Path::new(p).exists() {
                out.push(p.to_string());
            }
        }
        out.push("python3".to_string());
        out.push("python".to_string());
    }
    out
}

/// V8.1+ · 返回有序的 python 候选列表 (绝对路径优先 · 末尾系统兜底)
///
/// executor 逐个 try-spawn · 直到某个真能起来:
///   - 覆盖 required/fallback/lite venv 都没装、但别的 tier venv 装了的情况
///   - 覆盖 venv python 存在却起不来 (重定位失败 / 缺 DLL) → 自动退到下一个
///   - Windows 绝不静默退到裸 "python3" (那是 103 灾难的根因)
///
/// 路由优先级 (高 → 低):
///   1. required_tier venv
///   2. fallback_tiers venv (按序)
///   3. lite venv (auto_install)
///   4. 其它任意已存在的 venv (扫 venvs/)
///   5. uv 托管 cpython / 内置烘焙 cpython (无 venv 也能跑基础脚本)
///   6. 老内置 runtime (cpython + envs site-packages · 带 PYTHONPATH)
///   7. 系统 python (Windows: 绝对路径 → py 启动器 → 裸命令)
pub fn python_candidates(
    required_tier: Option<&str>,
    fallback_tiers: &[String],
) -> Vec<(String, Vec<PathBuf>)> {
    let mut tiers: Vec<String> = Vec::new();
    if let Some(rt) = required_tier {
        if !rt.is_empty() {
            tiers.push(rt.to_string());
        }
    }
    for fb in fallback_tiers {
        if !fb.is_empty() && !tiers.contains(fb) {
            tiers.push(fb.clone());
        }
    }
    if !tiers.iter().any(|t| t == "lite") {
        tiers.push("lite".to_string());
    }

    // 绝对路径候选 (venv python + cpython) · 去重
    let mut abs: Vec<PathBuf> = Vec::new();
    let mut push_abs = |p: PathBuf, abs: &mut Vec<PathBuf>| {
        if p.exists() && !abs.contains(&p) {
            abs.push(p);
        }
    };
    for t in &tiers {
        push_abs(venv_python(t), &mut abs);
    }
    // 扫所有已存在 venv
    if let Ok(rd) = std::fs::read_dir(venvs_root()) {
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let name = e.file_name().to_string_lossy().into_owned();
                push_abs(venv_python(&name), &mut abs);
            }
        }
    }
    // uv 托管 / 内置 cpython (无 venv 兜底)
    if let Some(p) = uv_managed_python() {
        push_abs(p, &mut abs);
    }
    if let Some(p) = bundled_python_bin() {
        push_abs(p, &mut abs);
    }

    let mut out: Vec<(String, Vec<PathBuf>)> = abs
        .into_iter()
        .map(|p| (p.to_string_lossy().into_owned(), Vec::new()))
        .collect();

    // 老内置 runtime (cpython + envs site-packages) · 带 pythonpath
    if let Some((p, pp)) = bundled_runtime_for(&["image", "base"]) {
        let s = p.to_string_lossy().into_owned();
        if !out.iter().any(|(x, _)| x == &s) {
            out.push((s, pp));
        }
    }

    // 系统兜底
    for sysp in system_python_candidates() {
        if !out.iter().any(|(x, _)| x == &sysp) {
            out.push((sysp, Vec::new()));
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════
// V8.2 (2026-06-11 RFC 节点执行层重构) · Native binary 路径解析
// ════════════════════════════════════════════════════════════════

/// 当前进程能访问的 Tauri resource 目录 (内置 binary 落地点)
/// .app/Contents/Resources/ (macOS) | resources\ (Win) | resources/ (Linux)
fn tauri_resource_dir() -> Option<PathBuf> {
    // exe_dir/../Resources (mac) | exe_dir/resources (win/linux Tauri bundle)
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?.to_path_buf();
    let mac_resources = exe_dir.join("..").join("Resources");
    if mac_resources.is_dir() {
        // 规整化路径
        if let Ok(can) = mac_resources.canonicalize() {
            return Some(can);
        }
    }
    let other_resources = exe_dir.join("resources");
    if other_resources.is_dir() {
        return Some(other_resources);
    }
    None
}

/// 给定 binary 逻辑名 · 找绝对可执行路径
///
/// 查找顺序 (高 → 低优先):
///   1. Tauri bundle 内置: <resources>/bin/<name>(.exe)
///        prebake-runtime.sh 把 uv 放这里 · prebake-natives.sh 放 ffmpeg/vips/...
///   2. ffmpeg tier (老 ffmpeg tier 用了不同结构): tier_root("ffmpeg")/bin/<name>
///        ~/.qianshou/runtime/tiers/ffmpeg/bin/ffmpeg
///   3. ocr tier (tesseract): tier_root("ocr")/tesseract/<name>
///        ~/.qianshou/runtime/tiers/ocr/tesseract/tesseract
///        + Tauri bundle: <resources>/ocr/tesseract/tesseract (prebake-ocr-macos.sh)
///   4. 系统 PATH: 用 which crate 查
///   5. 都找不到 → None · 上层 fallback python3
///
/// V8.2 设计原则:
///   - 内置 > tier 安装 > 系统 · 离线优先
///   - which 兜底为了开发期方便 · 生产应该全部走 1/2/3
pub fn find_native_binary(name: &str) -> Option<PathBuf> {
    let name_lower = name.to_lowercase();
    let exe_name = if cfg!(target_os = "windows") {
        if name_lower.ends_with(".exe") { name_lower.clone() } else { format!("{}.exe", name_lower) }
    } else {
        name_lower.clone()
    };

    // 1. Tauri resources/bin/<name>
    if let Some(res) = tauri_resource_dir() {
        let p = res.join("bin").join(&exe_name);
        if p.is_file() {
            return Some(p);
        }
        // 平台后缀名变体: bin/ffmpeg-aarch64-apple-darwin 之类
        // 仅在直接命中失败时扫一下 · 避免每次 IO
        if let Ok(rd) = std::fs::read_dir(res.join("bin")) {
            for e in rd.flatten() {
                let fname = e.file_name().to_string_lossy().to_lowercase();
                if fname == exe_name || fname.starts_with(&format!("{}-", name_lower)) {
                    let path = e.path();
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        // OCR tesseract 走单独子目录
        if name_lower == "tesseract" {
            let tess = res.join("ocr").join("tesseract").join(&exe_name);
            if tess.is_file() {
                return Some(tess);
            }
        }
    }

    // 2. ffmpeg tier (老路径 · ~/.qianshou/runtime/tiers/ffmpeg/bin/)
    for tier in &["ffmpeg", "ocr", "speech", "vision-ai", "lite"] {
        let p = tier_root(tier).join("bin").join(&exe_name);
        if p.is_file() {
            return Some(p);
        }
    }

    // 3. ocr tesseract 单独路径
    if name_lower == "tesseract" {
        let p = tier_root("ocr").join("tesseract").join(&exe_name);
        if p.is_file() {
            return Some(p);
        }
    }

    // 4. 系统 PATH (which crate)
    if let Ok(p) = which::which(&name_lower) {
        return Some(p);
    }
    None
}

/// V8.2 · 列出节点当前支持的所有 native binary
/// hello 上报给后端 · planner 用此过滤
pub fn detect_native_binaries() -> Vec<String> {
    // 跟 task_registry.required_native_binaries() 对齐 · 不在此列的不上报
    let candidates = [
        "ffmpeg", "ffprobe",
        "vips",
        "pdftotext", "pdfinfo", "pdftoppm",
        "whisper-cli",
        "tesseract",
        "curl",
    ];
    candidates
        .iter()
        .filter(|n| find_native_binary(n).is_some())
        .map(|s| s.to_string())
        .collect()
}

/// V8.2 · ONNX 模型存放根目录 (manifest 下发 · 节点拉到这)
///   ~/.qianshou/runtime/onnx/<model_name>/
///       det.onnx
///       rec.onnx
///       keys.txt
///       ...
pub fn onnx_models_root() -> PathBuf {
    runtime_root().join("onnx")
}

pub fn onnx_model_dir(model: &str) -> PathBuf {
    onnx_models_root().join(sanitize_tier(model))
}

/// V8.2 · 列出节点已装的 ONNX 模型 (hello 上报)
pub fn detect_onnx_models() -> Vec<String> {
    let root = onnx_models_root();
    if !root.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                // 校验目录非空(至少有一个 .onnx 文件)
                let dir = e.path();
                let has_onnx = std::fs::read_dir(&dir)
                    .map(|rd2| {
                        rd2.flatten().any(|f| {
                            f.file_name()
                                .to_string_lossy()
                                .to_lowercase()
                                .ends_with(".onnx")
                        })
                    })
                    .unwrap_or(false);
                if has_onnx {
                    out.push(e.file_name().to_string_lossy().to_string());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filters_traversal() {
        assert_eq!(sanitize_tier("../etc"), "etc");
        assert_eq!(sanitize_tier("lite"), "lite");
        assert_eq!(sanitize_tier("vision-ai"), "vision-ai");
    }

    #[test]
    fn onnx_paths_are_under_runtime_root() {
        let dir = onnx_model_dir("rapid_ocr_v1");
        assert!(dir.to_string_lossy().contains("onnx"));
        assert!(dir.to_string_lossy().contains("rapid_ocr_v1"));
    }
}
