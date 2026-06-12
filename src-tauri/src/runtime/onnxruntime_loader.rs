//! V8.2 (2026-06-11 RFC) · libonnxruntime 动态库装载器
//!
//! 问题:
//!   - ort crate 用 load-dynamic feature · 运行时 dlopen libonnxruntime
//!   - 节点系统没装 libonnxruntime → ort::Session::builder() 直接 panic
//!   - 不能 panic · 必须自动装 + 配 ORT_DYLIB_PATH
//!
//! 解法:
//!   - 客户端启动时(在 ort 调用之前)调 ensure_onnxruntime_loaded()
//!   - 按平台从 manifest.onnx_runtime 拉 tarball/zip · 解压 · 找到 dylib
//!   - 用 std::env::set_var("ORT_DYLIB_PATH", path)
//!   - 后续 ort::Session::builder() dlopen 时优先用此路径
//!
//! 装到:
//!   ~/.qianshou/runtime/onnxruntime/v<ver>/<extract_dir>/lib/libonnxruntime.{dylib,so.X,dll}
//!
//! Feature gating:
//!   - 仅 `feature = "onnx"` 启用时编译 · 跟 onnx_runner 同步

#![cfg(feature = "onnx")]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

use super::manifest::{OnnxRuntimeSpec, RuntimeManifest};
use super::paths;

const DOWNLOAD_TIMEOUT_SECS: u64 = 600; // 60MB Windows zip 走慢源可能要 5+ 分钟

/// 客户端启动时调用 · 按 manifest 装 libonnxruntime + setenv
///
/// 三种结果:
///   - 任务列表无 onnx 任务            → 跳过(不浪费带宽)
///   - 平台支持 + 装好 + setenv 成功  → Ok · ort 可正常用
///   - 平台不支持 / 装失败            → Err · 节点 onnx_runner 全部失败 · fallback python3
pub async fn ensure_onnxruntime_loaded(m: &RuntimeManifest) -> Result<PathBuf> {
    // 没 onnx 任务就别装(节约带宽)
    let need = m.task_executors.values().any(|s| s.executor == "onnx");
    if !need {
        tracing::info!("onnxruntime_loader · manifest 无 onnx 任务 · 跳过装载");
        return Err(anyhow!("no_onnx_tasks_in_manifest"));
    }

    let spec = m.onnx_runtime.as_ref().ok_or_else(|| {
        anyhow!("manifest.onnx_runtime 缺失 · 平台 {} 暂不支持 onnx", m.platform)
    })?;

    // 检测已装(尊重外部 ORT_DYLIB_PATH 环境变量优先)
    if let Ok(env_path) = std::env::var("ORT_DYLIB_PATH") {
        if !env_path.is_empty() && std::path::Path::new(&env_path).is_file() {
            tracing::info!(
                "onnxruntime_loader · ORT_DYLIB_PATH 已设 · {} · 跳过装载",
                env_path
            );
            return Ok(PathBuf::from(env_path));
        }
    }

    let install_dir = onnxruntime_install_dir(&spec.version);
    let expected_binary = install_dir.join(&spec.extracted_binary);

    if expected_binary.is_file() {
        tracing::info!(
            "onnxruntime_loader · 已装 · {} · setenv ORT_DYLIB_PATH",
            expected_binary.display()
        );
        std::env::set_var("ORT_DYLIB_PATH", &expected_binary);
        return Ok(expected_binary);
    }

    // 没装 · 下载 + 解压
    install(&install_dir, spec).await
        .with_context(|| format!("装 libonnxruntime v{} 失败", spec.version))?;

    if !expected_binary.is_file() {
        return Err(anyhow!(
            "装完但找不到 {} · archive 内部结构可能跟 spec 不符",
            expected_binary.display()
        ));
    }

    // 在 Linux 上 archive 给的是 libonnxruntime.so.1.18.1 · ort 默认找 libonnxruntime.so
    // 建一个软链让 ort 用默认名也找得到(给两条退路)
    #[cfg(unix)]
    create_unversioned_symlink(&expected_binary);

    std::env::set_var("ORT_DYLIB_PATH", &expected_binary);
    tracing::info!(
        "onnxruntime_loader · 装好 · {} · setenv ORT_DYLIB_PATH",
        expected_binary.display()
    );
    Ok(expected_binary)
}

fn onnxruntime_install_dir(version: &str) -> PathBuf {
    paths::runtime_root()
        .join("onnxruntime")
        .join(format!("v{}", version))
}

async fn install(install_dir: &PathBuf, spec: &OnnxRuntimeSpec) -> Result<()> {
    std::fs::create_dir_all(install_dir).with_context(|| {
        format!("创建 {} 失败", install_dir.display())
    })?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("构建 HTTP client 失败")?;

    // archive 文件名从 url 取最后一段
    let archive_name = spec.url.rsplit('/').next()
        .ok_or_else(|| anyhow!("onnx_runtime.url 不合法 · {}", spec.url))?;
    let archive_path = install_dir.join(archive_name);

    // 已存在合法 archive(sha256 对得上)→ 跳过下载
    if archive_path.exists()
        && std::fs::metadata(&archive_path).map(|m| m.len() > 0).unwrap_or(false)
    {
        if !spec.sha256.is_empty() && verify_sha256(&archive_path, &spec.sha256).unwrap_or(false) {
            tracing::info!(
                "onnxruntime_loader · 复用已下载 archive · {}",
                archive_path.display()
            );
        } else {
            let _ = std::fs::remove_file(&archive_path);
        }
    }

    if !archive_path.exists() {
        download_with_fallback(&client, spec, &archive_path).await
            .context("下载 archive 失败 · 所有源都失败")?;
    }

    if !spec.sha256.is_empty() {
        if !verify_sha256(&archive_path, &spec.sha256)? {
            let _ = std::fs::remove_file(&archive_path);
            return Err(anyhow!("archive sha256 不匹配 · 删档"));
        }
    } else {
        tracing::warn!("onnxruntime_loader · spec.sha256 为空 · 跳过校验(后端 TODO 回填)");
    }

    extract(&archive_path, install_dir, &spec.archive_kind)
        .with_context(|| format!("解压 {} 失败", archive_path.display()))?;

    // 解压成功后 archive 可删(节约磁盘)
    let _ = std::fs::remove_file(&archive_path);
    Ok(())
}

async fn download_with_fallback(
    client: &reqwest::Client,
    spec: &OnnxRuntimeSpec,
    dest: &PathBuf,
) -> Result<()> {
    let mut urls: Vec<String> = vec![spec.url.clone()];
    urls.extend(spec.fallback_urls.iter().cloned());
    let mut last_err: Option<String> = None;
    for (i, url) in urls.iter().enumerate() {
        match download_one(client, url, dest).await {
            Ok(_) => {
                if i > 0 {
                    tracing::warn!(
                        "onnxruntime_loader · 主源失败 · 已用 fallback#{} '{}'",
                        i, url
                    );
                }
                return Ok(());
            }
            Err(e) => {
                last_err = Some(format!("{}: {}", url, e));
                tracing::warn!(
                    "onnxruntime_loader · 源 {} 失败 · 尝试下一个 · {}", url, e
                );
            }
        }
    }
    Err(anyhow!(
        "所有源都失败 · 最后一次: {}",
        last_err.unwrap_or_else(|| "unknown".into())
    ))
}

async fn download_one(
    client: &reqwest::Client,
    url: &str,
    dest: &PathBuf,
) -> Result<()> {
    let resp = client.get(url).send().await
        .map_err(|e| anyhow!("GET 失败 · {}", e))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {} · {}", resp.status().as_u16(), url));
    }
    let bytes = resp.bytes().await
        .map_err(|e| anyhow!("读 body 失败 · {}", e))?;
    if bytes.is_empty() {
        return Err(anyhow!("body 为空"));
    }
    std::fs::write(dest, &bytes).with_context(|| {
        format!("写入 {} 失败", dest.display())
    })?;
    Ok(())
}

fn verify_sha256(path: &PathBuf, expected: &str) -> Result<bool> {
    let data = std::fs::read(path)
        .with_context(|| format!("读 {} 失败", path.display()))?;
    let mut h = Sha256::new();
    h.update(&data);
    let got: String = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    Ok(got.eq_ignore_ascii_case(expected))
}

fn extract(archive: &PathBuf, dest_dir: &PathBuf, kind: &str) -> Result<()> {
    match kind {
        "tar.gz" | "tgz" => extract_tar_gz(archive, dest_dir),
        "zip" => extract_zip(archive, dest_dir),
        other => Err(anyhow!("不支持的 archive_kind: {}", other)),
    }
}

fn extract_tar_gz(archive: &PathBuf, dest_dir: &PathBuf) -> Result<()> {
    let f = std::fs::File::open(archive)
        .with_context(|| format!("打开 {} 失败", archive.display()))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(dest_dir)
        .with_context(|| format!("tar 解压到 {} 失败", dest_dir.display()))?;
    Ok(())
}

fn extract_zip(archive: &PathBuf, dest_dir: &PathBuf) -> Result<()> {
    use std::io::Read;
    let f = std::fs::File::open(archive)
        .with_context(|| format!("打开 {} 失败", archive.display()))?;
    let mut zip = zip::ZipArchive::new(f).context("打开 zip 失败")?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).context("读 zip 条目失败")?;
        let entry_name = entry.name().to_string();
        // 防 zip slip
        if entry_name.contains("..") {
            tracing::warn!("onnxruntime_loader · 跳过可疑 zip 条目 · {}", entry_name);
            continue;
        }
        let out_path = dest_dir.join(&entry_name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).ok();
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)
            .with_context(|| format!("读 zip 条目 {} 失败", entry_name))?;
        std::fs::write(&out_path, &buf)
            .with_context(|| format!("写 {} 失败", out_path.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_unversioned_symlink(versioned: &PathBuf) {
    let parent = match versioned.parent() {
        Some(p) => p,
        None => return,
    };
    let name = match versioned.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return,
    };
    // libonnxruntime.so.1.18.1 → libonnxruntime.so
    // libonnxruntime.dylib       → 不需要(本身就是无版本)
    if !name.starts_with("libonnxruntime.so.") {
        return;
    }
    let unversioned = parent.join("libonnxruntime.so");
    if unversioned.exists() {
        return;
    }
    if let Err(e) = std::os::unix::fs::symlink(name, &unversioned) {
        tracing::warn!(
            "onnxruntime_loader · 建软链失败 · {} → {} · {} (非致命)",
            unversioned.display(), name, e
        );
    } else {
        tracing::info!(
            "onnxruntime_loader · 建无版本软链 · {} → {}",
            unversioned.display(), name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_dir_path() {
        let dir = onnxruntime_install_dir("1.18.1");
        let s = dir.to_string_lossy();
        assert!(s.ends_with("onnxruntime/v1.18.1"), "got {}", s);
    }
}
