//! ONNX 推理执行器 · V8.2 RFC (2026-06-11) 节点执行层重构
//!
//! 协议:
//!   task.executor   = "onnx"
//!   task.onnx_model = "rapid_ocr_v1" / "clip_vit_b32_v1" / ...
//!   task.args.input_kind = single_file / multi_file / archive (同 native_runner)
//!
//! 路径:
//!   节点用 ort crate 直接 load 模型文件(~/.qianshou/runtime/onnx/<model>/) · 推理 · 不开 Python ·
//!   不依赖 paddleocr/transformers/torch
//!
//! 模型注册表对应到 platform_v8/api/v8/bundles.py::_ONNX_MODELS_REGISTRY:
//!   rapid_ocr_v1     · RapidOCR PP-OCRv4 mobile · 4 个文件 · 16MB · 真实现 ✓
//!   clip_vit_b32_v1  · CLIP · 3 个文件 · 330MB · P2 阶段补
//!
//! Feature gating:
//!   - 默认 default = ["custom-protocol"] · onnx 关 · onnx_runner 返 NotSupported
//!     executor.rs fallback python3 · 跟现状一致 · 零回归
//!   - cargo build --features onnx · 编 ort + ndarray + image · 包体多 ~20MB
//!     (load-dynamic · libonnxruntime 运行时找)
//!
//! 输出 schema 跟 ocr_image.py 兼容:
//!   {
//!     "status": "ok", "schema_version": "v1", "task_type": "ocr_image",
//!     "elapsed_ms": 1234,
//!     "summary": {"pages": 1, "pages_failed": 0, "total_chars": 250,
//!                 "review_pages": 0, "language": "ch", "backend": "rapid_ocr_onnx",
//!                 "avg_confidence": 0.95, "output_path": null},
//!     "result_text": "【第1页:image.jpg】\n识别的文字...",
//!     "pages": [...]
//!   }

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use super::executor::RunOutcome;
use super::TaskAssign;

/// V8.2 · ONNX 推理执行入口
#[cfg(feature = "onnx")]
pub async fn run_onnx(task: &TaskAssign, timeout: Duration) -> Result<RunOutcome> {
    if task.onnx_model.is_empty() {
        return Err(anyhow!("onnx task 缺 onnx_model 字段"));
    }
    let model_dir = crate::runtime::paths::onnx_model_dir(&task.onnx_model);
    if !model_dir.is_dir() {
        tracing::warn!(
            "onnx_runner · 模型 '{}' 未装到 {:?} · 节点 fallback python3",
            task.onnx_model, model_dir
        );
        return Err(anyhow!("missing_onnx_model:{}", task.onnx_model));
    }
    tracing::info!(
        "onnx_runner · task={} model={} dir={:?}",
        task.task_id, task.onnx_model, model_dir
    );

    match task.task_type.as_str() {
        "ocr_image" | "pdf_ocr" => {
            run_rapid_ocr(task, &model_dir, timeout).await
        }
        "image_caption" => {
            // P2 · CLIP 实现待补
            Err(anyhow!("clip_not_implemented_yet · fallback python3"))
        }
        other => {
            Err(anyhow!(
                "unsupported_onnx_task:{} · 当前仅支持 ocr_image/pdf_ocr",
                other
            ))
        }
    }
}

/// Feature 关闭时的 stub · 返 NotSupported · executor.rs 兜底 python3
#[cfg(not(feature = "onnx"))]
pub async fn run_onnx(_task: &TaskAssign, _timeout: Duration) -> Result<RunOutcome> {
    tracing::warn!("onnx_runner · feature 'onnx' 未启用 · 节点 fallback python3");
    Err(anyhow!("onnx_feature_disabled"))
}

// ════════════════════════════════════════════════════════════════
// RapidOCR · 真实推理 · 调用 rapid_ocr::RapidOcrEngine
// ════════════════════════════════════════════════════════════════

#[cfg(feature = "onnx")]
async fn run_rapid_ocr(
    task: &TaskAssign,
    model_dir: &std::path::Path,
    timeout: Duration,
) -> Result<RunOutcome> {
    // ── 1. 获取输入图像 bytes(支持 single_file / inline_input · multi_file 暂走单图循环)──
    let images = fetch_input_images(task).await?;
    if images.is_empty() {
        return Err(anyhow!("rapid_ocr · 未拿到任何输入图像"));
    }

    // ── 2. 跑推理 · spawn_blocking 把 CPU 密集的 ORT 调用扔到 blocking 线程 ──
    let model_dir = model_dir.to_path_buf();
    let task_id = task.task_id.clone();
    let inference = tokio::task::spawn_blocking(move || -> Result<RapidOcrBatchResult> {
        let mut engine = super::rapid_ocr::RapidOcrEngine::load(&model_dir)
            .context("加载 RapidOcr engine 失败")?;
        let mut pages: Vec<PageResult> = Vec::with_capacity(images.len());
        let mut errors: Vec<PageError> = Vec::new();
        let mut total_elapsed: u64 = 0;
        for (page_no, (fname, bytes)) in images.iter().enumerate() {
            match engine.run(bytes) {
                Ok(r) => {
                    total_elapsed += r.elapsed_ms;
                    pages.push(PageResult {
                        page: page_no + 1,
                        filename: fname.clone(),
                        text: r.text.clone(),
                        line_detail: r.lines.into_iter().map(|l| LineDetail {
                            text: l.text,
                            confidence: l.confidence,
                        }).collect(),
                    });
                }
                Err(e) => {
                    errors.push(PageError {
                        page: page_no + 1,
                        filename: fname.clone(),
                        error: e.to_string().chars().take(200).collect(),
                    });
                }
            }
        }
        Ok(RapidOcrBatchResult { pages, errors, elapsed_ms: total_elapsed })
    });

    // ── 3. 超时控制(rec 多框时单图可能 5-10s · 给一倍裕量)──
    let result = match tokio::time::timeout(timeout, inference).await {
        Ok(Ok(r)) => r?,
        Ok(Err(join_err)) => return Err(anyhow!("rapid_ocr blocking 任务异常: {}", join_err)),
        Err(_) => return Err(anyhow!("rapid_ocr 超时 (timeout={:?})", timeout)),
    };

    // ── 4. 拼装 JSON · 兼容 ocr_image.py schema ──
    let combined_text = result.pages.iter()
        .map(|p| format!("【第{}页:{}】\n{}", p.page, p.filename, p.text))
        .collect::<Vec<_>>()
        .join("\n\n");

    let total_chars: usize = result.pages.iter().map(|p| p.text.chars().count()).sum();
    let total_lines: usize = result.pages.iter().map(|p| p.line_detail.len()).sum();
    let avg_conf: f32 = if total_lines == 0 {
        0.0
    } else {
        let sum: f32 = result.pages.iter()
            .flat_map(|p| p.line_detail.iter().map(|l| l.confidence))
            .sum();
        sum / total_lines as f32
    };

    let output_json = serde_json::json!({
        "status": "ok",
        "schema_version": "v1",
        "task_type": task.task_type,
        "elapsed_ms": result.elapsed_ms,
        "summary": {
            "pages": result.pages.len(),
            "pages_failed": result.errors.len(),
            "total_chars": total_chars,
            "review_pages": 0,
            "language": "ch",
            "backend": "rapid_ocr_onnx",
            "avg_confidence": (avg_conf * 1000.0).round() / 1000.0,
            "output_path": serde_json::Value::Null,
        },
        "result_text": combined_text,
        "pages": result.pages.iter().map(|p| {
            let page_avg = if p.line_detail.is_empty() {
                0.0_f32
            } else {
                let s: f32 = p.line_detail.iter().map(|l| l.confidence).sum();
                (s / p.line_detail.len() as f32 * 1000.0).round() / 1000.0
            };
            serde_json::json!({
                "page": p.page,
                "filename": p.filename,
                "chars": p.text.chars().count(),
                "lines": p.line_detail.len(),
                "avg_confidence": page_avg,
                "need_review": false,
                "line_detail": p.line_detail.iter().map(|l| serde_json::json!({
                    "text": l.text,
                    "confidence": (l.confidence * 1000.0).round() / 1000.0,
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
        "errors": result.errors.iter().map(|e| serde_json::json!({
            "page": e.page,
            "filename": e.filename,
            "error": e.error,
        })).collect::<Vec<_>>(),
    });

    let output = serde_json::to_string(&output_json)
        .unwrap_or_else(|_| "{}".to_string());

    tracing::info!(
        "rapid_ocr · task={} ✓ pages={}/{}  chars={}  conf={:.3}  elapsed={}ms",
        task_id, result.pages.len(),
        result.pages.len() + result.errors.len(),
        total_chars, avg_conf, result.elapsed_ms
    );

    Ok(RunOutcome {
        output,
        exit_code: 0,
        stderr_tail: String::new(),
        python_used: String::new(),
    })
}

// ════════════════════════════════════════════════════════════════
// 输入获取 · 支持 single_file / multi_file / archive / inline_input
// ════════════════════════════════════════════════════════════════

#[cfg(feature = "onnx")]
async fn fetch_input_images(task: &TaskAssign) -> Result<Vec<(String, Vec<u8>)>> {
    use base64::Engine as _;
    let args = &task.args;
    let input_kind = args.get("input_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("single_file");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("构建 HTTP client 失败")?;

    match input_kind {
        "inline_input" => {
            // base64 inline
            let b64 = args.get("inline_input")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("inline_input 字段缺失或非 string"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("inline_input base64 解码失败")?;
            Ok(vec![("inline.bin".to_string(), bytes)])
        }
        "multi_file" => {
            let urls: Vec<String> = args.get("input_refs")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            if urls.is_empty() {
                return Err(anyhow!("multi_file 模式 input_refs 为空"));
            }
            let mut out = Vec::with_capacity(urls.len());
            for url in urls {
                let bytes = download_bytes(&client, &url).await
                    .with_context(|| format!("下载 {} 失败", url))?;
                let fname = url.rsplit('/').next().unwrap_or("img.bin").to_string();
                out.push((fname, bytes));
            }
            Ok(out)
        }
        _ => {
            // single_file (default)
            let url = args.get("input_ref")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("single_file 模式 input_ref 字段缺失"))?;
            let bytes = download_bytes(&client, url).await?;
            let fname = url.rsplit('/').next().unwrap_or("img.bin").to_string();
            Ok(vec![(fname, bytes)])
        }
    }
}

#[cfg(feature = "onnx")]
async fn download_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client.get(url).send().await
        .map_err(|e| anyhow!("GET {} 失败: {}", url, e))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {} {}", resp.status().as_u16(), url));
    }
    let bytes = resp.bytes().await
        .map_err(|e| anyhow!("读 body 失败: {}", e))?;
    Ok(bytes.to_vec())
}

// ════════════════════════════════════════════════════════════════
// 内部结构
// ════════════════════════════════════════════════════════════════

#[cfg(feature = "onnx")]
struct RapidOcrBatchResult {
    pages: Vec<PageResult>,
    errors: Vec<PageError>,
    elapsed_ms: u64,
}

#[cfg(feature = "onnx")]
struct PageResult {
    page: usize,
    filename: String,
    text: String,
    line_detail: Vec<LineDetail>,
}

#[cfg(feature = "onnx")]
struct LineDetail {
    text: String,
    confidence: f32,
}

#[cfg(feature = "onnx")]
struct PageError {
    page: usize,
    filename: String,
    error: String,
}
