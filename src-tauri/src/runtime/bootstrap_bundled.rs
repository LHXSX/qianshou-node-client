//! 首次启动 · 把 .app/Contents/Resources/runtime/ 拷到 ~/.qianshou/runtime/
//!
//! 设计:
//!   bundle 里预烘焙了 (scripts/prebake-runtime.sh):
//!     resources/runtime/cpython/...          完整 portable Python 3.11
//!     resources/runtime/envs/base/           pip + setuptools
//!     resources/runtime/envs/image/          pillow + numpy
//!     resources/runtime/manifest.json
//!
//!   首启时把它整棵拷到 ~/.qianshou/runtime/ (一次性 · 之后跳过)
//!   拷的时候保留 symlink (envs 里的 python 是相对 symlink → cpython)
//!
//!   bundle 没烘焙 (dev build 或离线 build) → 静默跳过 · 走老 uv install 路径
//!
//! 用户感知:
//!   首启延迟 1-3 秒 (cp 132MB · SSD 1GB/s)
//!   之后开包即跑 image_resize / word_count · 0 联网 0 下载

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use tauri::{AppHandle, Manager};

use super::paths;

/// 当前编译目标的 platform 标签 · 跟 bundle prebake 时写入 runtime/manifest.json 的 "platform" 字段对齐
pub fn current_platform_label() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-aarch64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else {
        "unknown"
    }
}

/// 读 runtime/manifest.json 的 platform 字段 · 失败返 ""
fn read_stored_platform(manifest_path: &Path) -> String {
    let txt = match std::fs::read_to_string(manifest_path) {
        Ok(s) => s,
        Err(_) => return String::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&txt) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    v.get("platform")
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string()
}

/// 首次启动入口 · 不抛错 (失败就让 installer 走老路径)
///
/// 2026-05-28 · 加 platform mismatch 检测 ·
///   - 老 8.0.x 用户装过 cpython 到 ~/.qianshou/runtime/ · 升 8.1.0 跨架构装
///     (比如 Intel mac 上装到 arm64 的客户端 · 或 Win 上首次装) 时
///   - bundle 里 prebake 的是正确平台 · 但本地已存 manifest.json 平台不对
///   - 旧逻辑只看 marker 存在就跳 · 永远用错的 cpython 跑 → 所有 v2 task 失败
///   - 新逻辑: marker 平台不匹配 → 整个 dest 清空重做
pub async fn ensure_bundled_runtime(app: &AppHandle) -> Result<()> {
    let dest = paths::runtime_root();

    // 1. 已经 bootstrap 过 · 检查 platform 是否匹配
    let marker = dest.join("manifest.json");
    if marker.exists() {
        let current = current_platform_label();
        let stored = read_stored_platform(&marker);
        if stored == current {
            tracing::debug!("runtime/manifest.json 已存在且 platform={} 匹配 · 跳过 bundle bootstrap", current);
            return Ok(());
        }
        tracing::warn!(
            "runtime/manifest.json platform 不匹配 (stored='{}' current='{}') · 删除重做",
            stored, current
        );
        // 删整个 dest · 让后续逻辑重新拷
        if let Err(e) = std::fs::remove_dir_all(&dest) {
            tracing::warn!("删除旧 runtime 失败: {} · 强行继续 (新文件覆盖老的)", e);
        }
    }

    // 2. bundle 里有预烘焙吗
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| anyhow!("无法解析 resource_dir: {}", e))?;
    // Tauri 2 把 resources/runtime/**/* 实际放在 Contents/Resources/resources/runtime/
    // (多套了一层 `resources/` · 跟 tauri.conf.json bundle.resources 数组里的路径有关)
    // 优先试两个候选 · 兼容未来 Tauri 行为变化
    let candidates = [
        resource_dir.join("resources").join("runtime"),  // Tauri 2 实际布局
        resource_dir.join("runtime"),                    // 理论布局 (兼容)
    ];
    let src = match candidates.iter().find(|p| p.exists()) {
        Some(p) => p.clone(),
        None => {
            tracing::info!(
                "bundle 无预烘焙 runtime (找过 {:?}) · 跳过 · 用户首次装 tier 会走 uv install 路径",
                candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
            );
            return Ok(());
        }
    };
    if !src.join("manifest.json").exists() {
        tracing::warn!(
            "bundle 有 runtime/ 但缺 manifest.json · 跳过 (烘焙可能不完整) · 路径: {}",
            src.display()
        );
        return Ok(());
    }

    // 3. 拷
    tracing::info!(
        "首次启动 · 拷贝预烘焙 runtime: {} → {}",
        src.display(),
        dest.display()
    );
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all(&dest)
        .map_err(|e| anyhow!("创建 {} 失败: {}", dest.display(), e))?;
    copy_tree(&src, &dest)?;
    tracing::info!(
        "✅ 预烘焙 runtime 就绪 · 耗时 {:.1}s",
        t0.elapsed().as_secs_f64()
    );

    // 4. 写 installed.json (让 WS hello 上报 image tier 已就绪)
    write_installed_meta(&dest).ok();

    Ok(())
}

/// 递归拷贝目录 · 保留 symlink (不 follow)
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("源不存在: {}", src.display()));
    }
    std::fs::create_dir_all(dst).ok();

    for entry in std::fs::read_dir(src)
        .map_err(|e| anyhow!("read_dir {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| anyhow!("read_dir entry: {}", e))?;
        let src_p = entry.path();
        let name = entry.file_name();
        let dst_p = dst.join(&name);

        let ft = entry
            .file_type()
            .map_err(|e| anyhow!("file_type {}: {}", src_p.display(), e))?;

        if ft.is_symlink() {
            // 读 symlink target · 重建 symlink (保留 relative)
            let link_target = std::fs::read_link(&src_p)
                .map_err(|e| anyhow!("read_link {}: {}", src_p.display(), e))?;
            if dst_p.exists() || dst_p.is_symlink() {
                let _ = std::fs::remove_file(&dst_p);
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&link_target, &dst_p)
                    .map_err(|e| anyhow!("symlink {}: {}", dst_p.display(), e))?;
            }
            #[cfg(windows)]
            {
                // Windows symlink 需要管理员权限 · 直接 deref 拷文件兜底
                // (Win 端 venv 用 .exe 真二进制 · 不依赖 symlink)
                let abs_target = if link_target.is_absolute() {
                    link_target.clone()
                } else {
                    src_p.parent().unwrap_or(src).join(&link_target)
                };
                if abs_target.is_dir() {
                    copy_tree(&abs_target, &dst_p)?;
                } else if abs_target.is_file() {
                    std::fs::copy(&abs_target, &dst_p)
                        .map_err(|e| anyhow!("copy {}: {}", dst_p.display(), e))?;
                }
            }
        } else if ft.is_dir() {
            copy_tree(&src_p, &dst_p)?;
        } else {
            std::fs::copy(&src_p, &dst_p)
                .map_err(|e| anyhow!("copy {} → {}: {}", src_p.display(), dst_p.display(), e))?;
            // 保留可执行权限 (Python 二进制需要)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = entry.metadata() {
                    let perm = meta.permissions();
                    let _ = std::fs::set_permissions(&dst_p, perm);
                }
            }
        }
    }
    Ok(())
}

/// 2026-05-24 · 写 installed.json 用真实 InstalledTier schema
/// 让 v8_ws hello 能正确上报 software · executor 能找到 ffmpeg binary
fn write_installed_meta(_dest: &PathBuf) -> Result<()> {
    use super::detector::{
        write_installed_meta as detector_write, InstalledMeta, InstalledTier,
    };
    use std::collections::BTreeMap;

    // bundled cpython 真二进制 (executor 跑任务用)
    let python_bin = paths::bundled_python_bin()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    // imageio_ffmpeg 自带 ffmpeg static binary · 探测路径
    let ffmpeg_bin = paths::bundled_site_packages("image")
        .and_then(|sp| {
            let bin_dir = sp.join("imageio_ffmpeg").join("binaries");
            std::fs::read_dir(&bin_dir).ok().and_then(|it| {
                it.filter_map(|e| e.ok())
                    .find(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with("ffmpeg-")
                    })
                    .map(|e| e.path().to_string_lossy().into_owned())
            })
        })
        .unwrap_or_default();

    let mut binaries: BTreeMap<String, String> = BTreeMap::new();
    if !ffmpeg_bin.is_empty() {
        binaries.insert("ffmpeg".into(), ffmpeg_bin);
    }

    // 2026-05-28 · 复用 current_platform_label() 顶层定义 · 跟 ensure_bundled_runtime 校验逻辑统一
    //              注意: detector.InstalledMeta.platform 老格式是 macos-arm64 (不带 e) · 这里保留兼容
    //              ensure_bundled_runtime 用的 macos-aarch64 是 bundle prebake manifest 的字段
    let platform_label = match current_platform_label() {
        "macos-aarch64" => "macos-arm64",  // detector schema 历史用 arm64 短名 · 不破老 backend
        other => other,
    };

    let mut tiers: BTreeMap<String, InstalledTier> = BTreeMap::new();

    // 老 image tier (8.0.x 兼容 · 保留)
    tiers.insert(
        "image".into(),
        InstalledTier {
            ok: true,
            python: python_bin.clone(),
            packages: vec![
                "pillow", "numpy", "onnxruntime", "pymupdf", "pdfplumber", "imageio-ffmpeg",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            software: vec![
                "pillow", "numpy", "onnxruntime", "pymupdf", "pdfplumber", "ffmpeg",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            mirror_label: "bundled".into(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            last_message: "已从客户端内置 runtime 解压安装 · 开箱即用 · 无需联网".into(),
            binaries,
            installed_skills: BTreeMap::new(),
        },
    );

    // 2026-05-28 v8.1.2 · prebake-runtime.sh 已烘焙 lite + crawl venv 进 bundle
    // 节点首启 bootstrap 拷过来后这两个 venv 已就绪 · 标 ok · ws hello 上报 software
    // 让 planner 立刻能派算力/GEO/爬虫任务 (不等 auto_install_tiers 跑 pip install 30-60s)
    //
    // 检测方式: 看 ~/.qianshou/runtime/venvs/<tier>/bin/python (或 win Scripts/python.exe) 是否存在
    let lite_venv_py = paths::venv_python("lite");
    if lite_venv_py.exists() {
        tiers.insert(
            "lite".into(),
            InstalledTier {
                ok: true,
                python: lite_venv_py.to_string_lossy().into_owned(),
                packages: vec!["pillow", "numpy", "onnxruntime", "PyMuPDF", "pdfplumber"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                software: vec!["pillow", "numpy", "onnxruntime", "pymupdf", "pdfplumber"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                mirror_label: "bundled".into(),
                installed_at: chrono::Utc::now().to_rfc3339(),
                last_message: "客户端内置 lite venv · 开箱即跑算力任务 · 0 装机 0 网络".into(),
                binaries: BTreeMap::new(),
                installed_skills: BTreeMap::new(),
            },
        );
    }

    let crawl_venv_py = paths::venv_python("crawl");
    if crawl_venv_py.exists() {
        tiers.insert(
            "crawl".into(),
            InstalledTier {
                ok: true,
                python: crawl_venv_py.to_string_lossy().into_owned(),
                packages: vec!["requests", "selectolax", "tldextract", "readability-lxml", "lxml"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                software: vec!["requests", "selectolax", "readability", "tldextract"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                mirror_label: "bundled".into(),
                installed_at: chrono::Utc::now().to_rfc3339(),
                last_message: "客户端内置 crawl venv · 开箱即跑爬虫/GEO 任务 · 0 装机 0 网络".into(),
                binaries: BTreeMap::new(),
                installed_skills: BTreeMap::new(),
            },
        );
    }

    let meta = InstalledMeta {
        schema_version: "2".into(),
        install_mode: "bundled".into(),
        platform: platform_label.into(),
        host_python: if python_bin.is_empty() { None } else { Some(python_bin) },
        tiers,
    };

    detector_write(&meta).map_err(|e| anyhow!("写 installed.json 失败: {}", e))
}
