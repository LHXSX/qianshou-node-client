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

/// bundle / 已装 runtime 的身份信息 (从 manifest.json 读)
struct ManifestIdentity {
    platform: String,
    python: String,
    bundled_at: String,
}

/// 读 manifest.json 的 platform / python / bundled_at · 缺字段返 ""
fn read_manifest_identity(manifest_path: &Path) -> ManifestIdentity {
    let txt = std::fs::read_to_string(manifest_path).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&txt).unwrap_or(serde_json::Value::Null);
    let get = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string()
    };
    ManifestIdentity {
        platform: get("platform"),
        python: get("python"),
        bundled_at: get("bundled_at"),
    }
}

/// 刷新时删旧 cpython 目录 · 防新旧版本目录 (cpython-3.11.10 / cpython-3.11.13) 并存
/// (bundled_python_bin 假设只有一个 cpython-* 目录)
/// venvs/ 用的是 uv 装的 python (uv_python_dir) · 与 cpython/ 无关 · 删之安全
fn remove_bundled_cpython(dest: &Path) {
    let cpython = dest.join("cpython");
    if cpython.exists() {
        if let Err(e) = std::fs::remove_dir_all(&cpython) {
            tracing::warn!("刷新时删旧 cpython 失败: {} · 继续 (copy_tree 会覆盖)", e);
        }
    }
}

/// 2026-05-28 v8.1.3 · 归一化 platform label · 兼容老 8.0.x 用 Python triple
/// 历史标签:
///   8.0.x 老 prebake.sh 写: "aarch64-apple-darwin" / "x86_64-apple-darwin" /
///                          "x86_64-pc-windows-msvc-shared" / "x86_64-pc-windows-msvc"
///   8.1.x 新 prebake.sh 写: "macos-aarch64" / "macos-x86_64" / "windows-x86_64"
///   8.0.x detector schema:  "macos-arm64" / "macos-x86_64" / "windows-x86_64"
/// 全部归一到 8.1.x 短名格式
fn normalize_platform_label(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    match s {
        // Python triple → 短名
        "aarch64-apple-darwin" => "macos-aarch64".into(),
        "x86_64-apple-darwin" => "macos-x86_64".into(),
        "x86_64-pc-windows-msvc-shared"
        | "x86_64-pc-windows-msvc" => "windows-x86_64".into(),
        "x86_64-unknown-linux-gnu" => "linux-x86_64".into(),
        // 历史 detector schema 短名 → 跟 current_platform_label() 对齐
        "macos-arm64" => "macos-aarch64".into(),
        // 已经是新短名 · 原样返
        other => other.into(),
    }
}

/// 首次启动入口 · 不抛错 (失败就让 installer 走老路径)
///
/// 2026-05-28 · 加 platform mismatch 检测 ·
///   - 老 8.0.x 用户装过 cpython 到 ~/.qianshou/runtime/ · 升 8.1.0 跨架构装
///     (比如 Intel mac 上装到 arm64 的客户端 · 或 Win 上首次装) 时
///   - bundle 里 prebake 的是正确平台 · 但本地已存 manifest.json 平台不对
///   - 旧逻辑只看 marker 存在就跳 · 永远用错的 cpython 跑 → 所有 v2 task 失败
///   - 新逻辑: marker 平台不匹配 → 整个 dest 清空重做
///
/// 2026-05-28 v8.1.3 · 灾难级 bug 修复 ·
///   - 8.0.x 老 prebake 写的 platform 是 "aarch64-apple-darwin" (Python triple)
///   - current_platform_label() 是 "macos-aarch64" (短名)
///   - 字符串直接 == 比较永远不匹配 → 触发 remove_dir_all(&dest)
///   - 用户费时装好的 venvs/lite/ venvs/crawl/ 全删 → 升级一次重装一次
///   - 修复 1: 走 normalize_platform_label 归一化后再比 (兼容历史所有格式)
///   - 修复 2: 即使真的 mismatch (跨架构覆盖) · 也只重写 manifest.json · 不动 venvs/
///            cpython 旧文件让 bundle copy_tree 覆盖即可 · venvs 是用户数据严禁动
pub async fn ensure_bundled_runtime(app: &AppHandle) -> Result<()> {
    let dest = paths::runtime_root();

    // ── 1. 先定位 bundle 里的预烘焙 runtime (版本比对需先拿到 src manifest) ──
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
    let src_manifest = src.join("manifest.json");
    if !src_manifest.exists() {
        tracing::warn!(
            "bundle 有 runtime/ 但缺 manifest.json · 跳过 (烘焙可能不完整) · 路径: {}",
            src.display()
        );
        return Ok(());
    }

    // ── 2. 版本比对 · 决定 skip / 刷新 cpython / 跨架构重做 ──
    // (旧逻辑: platform 匹配就 return → OTA 升级永不刷 cpython · 这里改为按 python/bundled_at 比对)
    let src_id = read_manifest_identity(&src_manifest);
    let current = normalize_platform_label(current_platform_label());
    let marker = dest.join("manifest.json");
    if marker.exists() {
        let dest_id = read_manifest_identity(&marker);
        let stored = normalize_platform_label(&dest_id.platform);
        if stored == current {
            // 平台一致 · 仅当 bundle 的 python / 烘焙批次变化时才刷新
            // (升级带了新版 bundled Python → 刷 cpython · 但保留用户 venvs + installed.json 记录)
            if src_id.python == dest_id.python && src_id.bundled_at == dest_id.bundled_at {
                tracing::debug!(
                    "bundled runtime 已最新 (python={} bundled_at={}) · 跳过 bootstrap",
                    dest_id.python, dest_id.bundled_at
                );
                return Ok(());
            }
            tracing::info!(
                "检测到更新版 bundled runtime (python {}→{} · bundled_at {}→{}) · 刷新 cpython · 保留 venvs/ 与已装 tier 记录",
                dest_id.python, src_id.python, dest_id.bundled_at, src_id.bundled_at
            );
            // 删旧 cpython · 让 copy_tree 重拷新版 · venvs/ 不受影响 (用 uv 装的 python)
            remove_bundled_cpython(&dest);
        } else {
            // 跨架构覆盖 (Intel↔arm / 装错平台) · 只删平台相关文件 · venvs 保持原样
            // 2026-05-28 修复: 不再 remove_dir_all(&dest) · 那会清光用户已装的 venvs/
            tracing::warn!(
                "runtime/manifest.json platform 不匹配 (stored='{}' current='{}') · 仅覆盖 cpython · 保留 venvs/ 用户数据",
                stored, current
            );
            for sub in ["cpython", "bin", "manifest.json"] {
                let p = dest.join(sub);
                if p.exists() {
                    let r = if p.is_dir() {
                        std::fs::remove_dir_all(&p)
                    } else {
                        std::fs::remove_file(&p)
                    };
                    if let Err(e) = r {
                        tracing::warn!("删除 {} 失败: {} · 继续 (copy_tree 会覆盖)", p.display(), e);
                    }
                }
            }
        }
    }

    // ── 3. 拷贝 · SKIP 用户已存在的 venvs/* (装一次永久 · OTA 升级不覆盖用户已装依赖) ──
    tracing::info!(
        "拷贝预烘焙 runtime: {} → {}",
        src.display(),
        dest.display()
    );
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all(&dest)
        .map_err(|e| anyhow!("创建 {} 失败: {}", dest.display(), e))?;
    // 列出用户已存在的 venvs/<tier> · 拷贝时跳过这些 · 保留用户已装的依赖
    let preserve_venvs: std::collections::HashSet<String> = {
        let mut s = std::collections::HashSet::new();
        let venvs_dir = dest.join("venvs");
        if venvs_dir.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&venvs_dir) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        // 只保留有 python 二进制的 venv (已装好的) · 半装空目录不保
                        let py_path = if cfg!(target_os = "windows") {
                            e.path().join("Scripts").join("python.exe")
                        } else {
                            e.path().join("bin").join("python")
                        };
                        if py_path.exists() {
                            if let Some(name) = e.file_name().to_str() {
                                s.insert(format!("venvs/{}", name));
                                tracing::info!("preserve 用户已装 venv · 跳过 bundle 覆盖: venvs/{}", name);
                            }
                        }
                    }
                }
            }
        }
        s
    };
    copy_tree_with_skip(&src, &dest, "", &preserve_venvs)?;
    tracing::info!(
        "✅ 预烘焙 runtime 就绪 · 耗时 {:.1}s · 保留 {} 个用户已装 venv",
        t0.elapsed().as_secs_f64(),
        preserve_venvs.len()
    );

    // 4. 写 installed.json (让 WS hello 上报 image tier 已就绪)
    write_installed_meta(&dest).ok();

    Ok(())
}

/// 2026-05-29 v8.1.4 · 递归拷贝 · 支持跳过用户已装的子路径
///
/// rel_prefix: 当前递归层级的相对路径前缀 (从根算起 · 用 "/" 分隔 · 跟 skip set 元素格式对齐)
/// skip: 跳过的相对路径集合 (如 {"venvs/lite", "venvs/crawl"})
///
/// 例: copy_tree_with_skip(bundle, dest, "", {"venvs/lite"})
///   - 拷 bundle/cpython → dest/cpython
///   - 拷 bundle/venvs → dest/venvs (进入子目录)
///     - 跳 bundle/venvs/lite (rel="venvs/lite" 在 skip)
///     - 拷 bundle/venvs/crawl → dest/venvs/crawl
fn copy_tree_with_skip(
    src: &Path,
    dst: &Path,
    rel_prefix: &str,
    skip: &std::collections::HashSet<String>,
) -> Result<()> {
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
        let name_str = name.to_string_lossy();
        let cur_rel = if rel_prefix.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", rel_prefix, name_str)
        };

        // 跳过用户已装的 venv (保护用户数据 · "装一次永久")
        if skip.contains(&cur_rel) {
            continue;
        }

        let dst_p = dst.join(&name);
        let ft = entry
            .file_type()
            .map_err(|e| anyhow!("file_type {}: {}", src_p.display(), e))?;

        if ft.is_symlink() {
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
                let abs_target = if link_target.is_absolute() {
                    link_target.clone()
                } else {
                    src_p.parent().unwrap_or(src).join(&link_target)
                };
                if abs_target.is_dir() {
                    copy_tree_with_skip(&abs_target, &dst_p, &cur_rel, skip)?;
                } else if abs_target.is_file() {
                    std::fs::copy(&abs_target, &dst_p)
                        .map_err(|e| anyhow!("copy {}: {}", dst_p.display(), e))?;
                }
            }
        } else if ft.is_dir() {
            copy_tree_with_skip(&src_p, &dst_p, &cur_rel, skip)?;
        } else {
            std::fs::copy(&src_p, &dst_p)
                .map_err(|e| anyhow!("copy {} → {}: {}", src_p.display(), dst_p.display(), e))?;
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

/// 递归拷贝目录 · 保留 symlink (不 follow)
#[allow(dead_code)]
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
        read_installed_meta, write_installed_meta as detector_write, InstalledTier,
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

    // 合并写: 读现有 installed.json · 仅覆盖/插入 bundled tier (image/lite/crawl) ·
    // 保留用户自装的 tier (ocr/speech/vision-ai 等) · 避免升级刷新后这些 tier 从上报里消失
    // (detector_write 是整体覆盖 · 若直接构造新 meta 会丢用户 tier 记录)
    let mut meta = read_installed_meta();
    if meta.schema_version.is_empty() {
        meta.schema_version = "2".into();
    }
    if meta.install_mode.is_empty() {
        meta.install_mode = "bundled".into();
    }
    // platform 始终对齐当前 bundle (跨架构刷新后要更新)
    meta.platform = platform_label.into();
    if !python_bin.is_empty() {
        meta.host_python = Some(python_bin);
    }
    // 用 bundled tier 覆盖同名 key · 其余 (用户自装) 原样保留
    for (k, v) in tiers {
        meta.tiers.insert(k, v);
    }

    detector_write(&meta).map_err(|e| anyhow!("写 installed.json 失败: {}", e))
}
