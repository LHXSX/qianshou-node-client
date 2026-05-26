//! uv (astral-sh) 引导 · 替代旧的 venv + pip 方案
//!
//! 解决问题: 新机没装 Python · 用户点"安装环境"直接失败
//!
//! 设计:
//!   1. 优先用 ~/.qianshou/runtime/bin/uv (已 bootstrap)
//!   2. 没有则从 app bundle resources/bin/uv-{target-triple} 拷贝
//!   3. 都没有则从 GitHub Release / 自家镜像 HTTP 下载 + 校验
//!   4. uv 装好后: `uv python install 3.11` 一行解决 Python 缺失
//!
//! 这样客户端真正"自己下载环境" · 用户什么都不用装。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tauri::{AppHandle, Manager};
use tokio::process::Command;

use super::paths;

/// 当前平台对应的 bundled uv 文件名 (resources/bin/<这里>)
fn bundled_uv_filename() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "uv-aarch64-apple-darwin" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "uv-x86_64-apple-darwin" }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "uv-x86_64-pc-windows-msvc.exe" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "uv-x86_64-unknown-linux-gnu" }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
    )))]
    { "uv" }
}

/// HTTP fallback URL · 没 bundle 时下这个
fn fallback_uv_url() -> &'static str {
    // uv 0.11.x · 稳定版 · 后续可以让后端 manifest 下发动态版本
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    { "https://github.com/astral-sh/uv/releases/download/0.11.15/uv-aarch64-apple-darwin.tar.gz" }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    { "https://github.com/astral-sh/uv/releases/download/0.11.15/uv-x86_64-apple-darwin.tar.gz" }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    { "https://github.com/astral-sh/uv/releases/download/0.11.15/uv-x86_64-pc-windows-msvc.zip" }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    { "https://github.com/astral-sh/uv/releases/download/0.11.15/uv-x86_64-unknown-linux-gnu.tar.gz" }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
    )))]
    { "https://github.com/astral-sh/uv/releases/latest" }
}

/// 确保 uv 二进制可用 · 返回 uv 路径
///
/// 流程:
///   1. ~/.qianshou/runtime/bin/uv 已存在且可跑 → 直接返
///   2. 从 app resource_dir()/bin/uv-{target} 拷过来 → 设可执行 → 返
///   3. 否则 HTTP 下载 fallback
///
/// 不抛错 · 出错由调用方决定是否降级到旧 venv 路径
pub async fn ensure_uv(app: &AppHandle) -> Result<PathBuf> {
    let uv = paths::uv_path();

    // 1) 已 bootstrap
    if uv.exists() && verify_uv(&uv).await.is_ok() {
        return Ok(uv);
    }

    // 2) 从 bundle 拷
    std::fs::create_dir_all(paths::uv_bin_dir())
        .map_err(|e| anyhow!("创建 {} 失败: {}", paths::uv_bin_dir().display(), e))?;

    if let Ok(resource_dir) = app.path().resource_dir() {
        let src = resource_dir.join("resources").join("bin").join(bundled_uv_filename());
        if src.exists() {
            std::fs::copy(&src, &uv)
                .map_err(|e| anyhow!("拷贝 bundled uv 失败 ({} → {}): {}", src.display(), uv.display(), e))?;
            ensure_executable(&uv)?;
            verify_uv(&uv).await?;
            return Ok(uv);
        }
    }

    // 3) HTTP 下载 fallback (后续可走自家镜像)
    download_uv(&uv).await?;
    ensure_executable(&uv)?;
    verify_uv(&uv).await?;
    Ok(uv)
}

/// HTTP 下载 uv (从 GitHub Release)
async fn download_uv(dest: &Path) -> Result<()> {
    let url = fallback_uv_url();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| anyhow!("初始化 http 失败: {}", e))?;
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("下载 uv 失败 ({}): {}", url, e))?
        .error_for_status()
        .map_err(|e| anyhow!("uv 下载响应非 2xx: {}", e))?
        .bytes()
        .await
        .map_err(|e| anyhow!("读取 uv 字节流失败: {}", e))?;

    // 临时文件 → 解压 → 拷出 uv 二进制
    let tmp_archive = paths::uv_bin_dir().join("uv-download.tmp");
    std::fs::write(&tmp_archive, &bytes)
        .map_err(|e| anyhow!("写下载临时文件失败: {}", e))?;

    let parent = paths::uv_bin_dir();
    let extracted = extract_uv_archive(&tmp_archive, &parent)?;
    std::fs::rename(&extracted, dest)
        .map_err(|e| anyhow!("rename uv ({} → {}) 失败: {}", extracted.display(), dest.display(), e))?;
    let _ = std::fs::remove_file(&tmp_archive);
    Ok(())
}

/// 解压 .tar.gz / .zip · 返回里面 uv 二进制的临时路径
fn extract_uv_archive(archive: &Path, work_dir: &Path) -> Result<PathBuf> {
    let name = archive.file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("无效 archive 路径"))?;

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        // tar -xzf <archive> -C <work_dir>
        let mut tar_cmd = std::process::Command::new("tar");
        tar_cmd.args(["-xzf", archive.to_str().unwrap(), "-C", work_dir.to_str().unwrap()]);
        crate::proc_util::hide_window_std(&mut tar_cmd);
        let status = tar_cmd
            .status()
            .map_err(|e| anyhow!("执行 tar 失败: {}", e))?;
        if !status.success() {
            return Err(anyhow!("tar 解压失败 (exit {:?})", status.code()));
        }
        // 找解压出的目录里的 uv 二进制
        for entry in std::fs::read_dir(work_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let candidate = path.join("uv");
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
        Err(anyhow!("tar 解压后未找到 uv 二进制"))
    } else if name.ends_with(".zip") {
        // Windows: unzip 或者 PowerShell Expand-Archive
        #[cfg(target_os = "windows")]
        {
            let mut ps_cmd = std::process::Command::new("powershell");
            ps_cmd.args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Force -Path '{}' -DestinationPath '{}'",
                    archive.display(),
                    work_dir.display()
                ),
            ]);
            crate::proc_util::hide_window_std(&mut ps_cmd);
            let status = ps_cmd
                .status()
                .map_err(|e| anyhow!("执行 Expand-Archive 失败: {}", e))?;
            if !status.success() {
                return Err(anyhow!("Expand-Archive 失败 (exit {:?})", status.code()));
            }
            let candidate = work_dir.join("uv.exe");
            if candidate.exists() {
                return Ok(candidate);
            }
            // 有时候解压会展开到子目录
            for entry in std::fs::read_dir(work_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    let candidate = path.join("uv.exe");
                    if candidate.exists() {
                        return Ok(candidate);
                    }
                }
            }
            Err(anyhow!("zip 解压后未找到 uv.exe"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            Err(anyhow!("当前平台不支持解 zip"))
        }
    } else {
        Err(anyhow!("未知 uv archive 格式: {}", name))
    }
}

/// 给 uv 加可执行权限 (mac/linux)
fn ensure_executable(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(_path)
            .map_err(|e| anyhow!("读 uv 文件权限失败: {}", e))?
            .permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(_path, perm)
            .map_err(|e| anyhow!("设 uv 可执行权限失败: {}", e))?;
    }
    Ok(())
}

/// 跑 `uv --version` 验证可用
async fn verify_uv(uv: &Path) -> Result<String> {
    let mut cmd = Command::new(uv);
    cmd.arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    crate::proc_util::hide_window_tokio(&mut cmd);

    let out = tokio::time::timeout(Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| anyhow!("uv --version 超时"))?
        .map_err(|e| anyhow!("uv --version 执行失败: {}", e))?;
    if !out.status.success() {
        return Err(anyhow!(
            "uv --version 退出码 {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 用 uv 装一个 Python 版本 · 返回该 Python 解释器路径
pub async fn ensure_python(uv: &Path, version: &str) -> Result<PathBuf> {
    // 先用 `uv python find <version>` 看是否已经有
    if let Ok(p) = uv_python_find(uv, version).await {
        return Ok(p);
    }
    // 没有则装
    uv_python_install(uv, version).await?;
    uv_python_find(uv, version).await
}

async fn uv_python_find(uv: &Path, version: &str) -> Result<PathBuf> {
    let mut cmd = Command::new(uv);
    cmd.env("UV_PYTHON_INSTALL_DIR", paths::uv_python_dir())
        .arg("python")
        .arg("find")
        .arg(version)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    crate::proc_util::hide_window_tokio(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| anyhow!("uv python find {} 超时", version))?
        .map_err(|e| anyhow!("uv python find 执行失败: {}", e))?;
    if !out.status.success() {
        return Err(anyhow!(
            "uv python find {} 未找到: {}",
            version,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        return Err(anyhow!("uv python find 返回空"));
    }
    Ok(PathBuf::from(path))
}

async fn uv_python_install(uv: &Path, version: &str) -> Result<()> {
    let mut cmd = Command::new(uv);
    cmd.env("UV_PYTHON_INSTALL_DIR", paths::uv_python_dir())
        .arg("python")
        .arg("install")
        .arg(version)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    crate::proc_util::hide_window_tokio(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(600), cmd.output())
        .await
        .map_err(|_| anyhow!("uv python install {} 超时", version))?
        .map_err(|e| anyhow!("uv python install 执行失败: {}", e))?;
    if !out.status.success() {
        return Err(anyhow!(
            "uv python install {} 失败 (exit {:?}): {}",
            version,
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
