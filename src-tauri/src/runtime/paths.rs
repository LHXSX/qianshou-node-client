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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filters_traversal() {
        assert_eq!(sanitize_tier("../etc"), "etc");
        assert_eq!(sanitize_tier("lite"), "lite");
        assert_eq!(sanitize_tier("vision-ai"), "vision-ai");
    }
}
