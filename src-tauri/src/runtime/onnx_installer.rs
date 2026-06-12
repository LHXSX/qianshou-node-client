//! V8.2 (2026-06-11 RFC) · ONNX 模型按需下载器
//!
//! 设计:
//!   - 不打进 .app · 不打进 tier venv tarball
//!   - 节点首次收到 executor=onnx 任务 + 本地无模型 → 触发本模块下载
//!   - 跨平台共享同一份模型(ONNX 平台无关)
//!   - 文件直接放 ~/.qianshou/runtime/onnx/<model>/<file_name>
//!
//! 跟 installer.rs::install_binaries 的区别:
//!   - 单个 ONNX 模型 = 多个文件(det/cls/rec/keys)· 但没 archive 解压
//!   - 主源 by.wujisuanli.com · 主源失败按 fallback_urls 顺序尝试(GitHub raw / HuggingFace)
//!   - sha256 校验 · 空串跳过(给 warn)
//!
//! 调用入口:
//!   ensure_onnx_model(&manifest, "rapid_ocr_v1") → 已装/装好 → Ok(model_dir)
//!                                                 → 装失败  → Err(...)

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

use super::manifest::{OnnxFileSpec, OnnxModelSpec, RuntimeManifest};
use super::paths;

/// 单文件下载超时(慢源也得给机会)
const DOWNLOAD_TIMEOUT_SECS: u64 = 300;

/// 节点启动时 manifest 拉一次后调用 · 按 task_executors 表中所有 onnx_model 预装
///
/// 返回已装好的模型数量(失败的不计入 · 但日志会打)
pub async fn auto_install_required_models(m: &RuntimeManifest) -> usize {
    let mut needed: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for (_task, spec) in &m.task_executors {
        if spec.executor == "onnx" && !spec.onnx_model.is_empty() {
            needed.insert(spec.onnx_model.clone());
        }
    }
    if needed.is_empty() {
        tracing::info!("onnx_installer · manifest 无 onnx 任务 · 跳过");
        return 0;
    }
    let mut ok_count = 0usize;
    for model_name in &needed {
        match ensure_onnx_model(m, model_name).await {
            Ok(dir) => {
                tracing::info!(
                    "onnx_installer · 模型 '{}' 就绪 · {}",
                    model_name, dir.display()
                );
                ok_count += 1;
            }
            Err(e) => {
                tracing::warn!(
                    "onnx_installer · 模型 '{}' 安装失败: {} · 节点暂不接 onnx 任务 · 走 python3 fallback",
                    model_name, e
                );
            }
        }
    }
    ok_count
}

/// 确保指定模型已装 · 已装直接返回路径 · 未装下载安装
pub async fn ensure_onnx_model(m: &RuntimeManifest, model_name: &str) -> Result<PathBuf> {
    let spec = m
        .onnx_models
        .get(model_name)
        .ok_or_else(|| anyhow!("manifest 中找不到 onnx 模型 '{}'", model_name))?;
    let dir = paths::onnx_model_dir(model_name);

    // 已装快路径: smoke_test 文件存在且非空 → 跳过下载
    if is_installed(&dir, spec) {
        return Ok(dir);
    }
    install(model_name, &dir, spec).await?;
    if !is_installed(&dir, spec) {
        return Err(anyhow!(
            "模型 '{}' 装完但 smoke_test 失败 · {}",
            model_name, spec.smoke_test
        ));
    }
    Ok(dir)
}

fn is_installed(dir: &PathBuf, spec: &OnnxModelSpec) -> bool {
    if !dir.is_dir() {
        return false;
    }
    if spec.smoke_test.is_empty() {
        // 没指定 smoke_test 但所有 file 都在 → 算装好
        return spec.files.iter().all(|f| {
            let p = dir.join(&f.name);
            p.exists() && std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false)
        });
    }
    let smoke = dir.join(&spec.smoke_test);
    smoke.exists() && std::fs::metadata(&smoke).map(|m| m.len() > 0).unwrap_or(false)
}

async fn install(model_name: &str, dir: &PathBuf, spec: &OnnxModelSpec) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| {
        format!("创建模型目录失败 · {}", dir.display())
    })?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("构建 HTTP client 失败")?;

    tracing::info!(
        "onnx_installer · 开始装 '{}' · {} 文件 · 总 ~{}MB",
        model_name, spec.files.len(), spec.size_mb
    );

    for file in &spec.files {
        let dest = dir.join(&file.name);
        if dest.exists() {
            if let Ok(meta) = std::fs::metadata(&dest) {
                if meta.len() > 0 {
                    if check_sha256_if_set(&dest, file).unwrap_or(true) {
                        tracing::info!(
                            "onnx_installer · 文件已存在 · 跳过 · {}",
                            file.name
                        );
                        continue;
                    }
                    tracing::warn!(
                        "onnx_installer · 已存在文件 sha256 不匹配 · 重新下载 · {}",
                        file.name
                    );
                    let _ = std::fs::remove_file(&dest);
                }
            }
        }
        download_with_fallback(&client, file, &dest).await.with_context(|| {
            format!("下载 {} 失败 · 全部源都失败", file.name)
        })?;
    }
    tracing::info!("onnx_installer · '{}' 装好 · {}", model_name, dir.display());
    Ok(())
}

async fn download_with_fallback(
    client: &reqwest::Client,
    file: &OnnxFileSpec,
    dest: &PathBuf,
) -> Result<()> {
    let mut urls: Vec<String> = vec![file.url.clone()];
    urls.extend(file.fallback_urls.iter().cloned());
    let mut last_err: Option<String> = None;
    for (i, url) in urls.iter().enumerate() {
        match download_one(client, url, dest).await {
            Ok(_) => {
                if let Err(e) = check_sha256_if_set_strict(dest, file) {
                    last_err = Some(format!(
                        "{} 下载完但 sha256 校验失败 · {}", url, e
                    ));
                    let _ = std::fs::remove_file(dest);
                    continue;
                }
                if i > 0 {
                    tracing::warn!(
                        "onnx_installer · 主源失败 · 已从 fallback#{} '{}' 装好 {}",
                        i, url, file.name
                    );
                }
                return Ok(());
            }
            Err(e) => {
                last_err = Some(format!("{}: {}", url, e));
                tracing::warn!(
                    "onnx_installer · 源 {} 失败 · 尝试下一个 · {}", url, e
                );
            }
        }
    }
    Err(anyhow!(
        "所有源都失败 · 最后一次: {}",
        last_err.unwrap_or_else(|| "unknown".into())
    ))
}

async fn download_one(client: &reqwest::Client, url: &str, dest: &PathBuf) -> Result<()> {
    let resp = client.get(url).send().await
        .map_err(|e| anyhow!("GET 失败 · {}", e))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {} · {}", resp.status().as_u16(), url));
    }
    let bytes = resp.bytes().await
        .map_err(|e| anyhow!("读 body 失败 · {}", e))?;
    if bytes.is_empty() {
        return Err(anyhow!("响应 body 为空"));
    }
    std::fs::write(dest, &bytes).with_context(|| {
        format!("写入 {} 失败", dest.display())
    })?;
    Ok(())
}

/// 严格 sha256 检查 · 若 spec.sha256 为空返回 Ok (跳过) · 否则不匹配 Err
fn check_sha256_if_set_strict(path: &PathBuf, file: &OnnxFileSpec) -> Result<()> {
    if file.sha256.trim().is_empty() {
        tracing::warn!(
            "onnx_installer · {} sha256 未填(后端 TODO)· 跳过校验",
            file.name
        );
        return Ok(());
    }
    let data = std::fs::read(path)
        .with_context(|| format!("读 {} 失败", path.display()))?;
    let mut h = Sha256::new();
    h.update(&data);
    let got: String = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    if !got.eq_ignore_ascii_case(&file.sha256) {
        return Err(anyhow!(
            "sha256 不匹配 · 期望 {} 实际 {}", file.sha256, got
        ));
    }
    Ok(())
}

/// 宽松 sha256 检查 · 用于"已存在文件是否需要重下" · 出错返 Ok(false)
fn check_sha256_if_set(path: &PathBuf, file: &OnnxFileSpec) -> Result<bool> {
    if file.sha256.trim().is_empty() {
        return Ok(true);
    }
    let data = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&data);
    let got: String = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    Ok(got.eq_ignore_ascii_case(&file.sha256))
}
