//! OSS 上传客户端 (节点 → OSS)
//!
//! 用途:
//!   节点跑完任务输出 > 64 KB 时，主动调本模块上传到 OSS · 返 object_key
//!   v8_ws 把 object_key 作为 ShardResultPayload.output_ref · 不再 inline
//!
//! 流程:
//!   1. POST /api/v8/files/upload-url  (bearer token)
//!      body: { filename, purpose=result, content_type, size_bytes, task_id }
//!      resp: { object_key, url, method, headers, expires_in }
//!   2. PUT <url> body=bytes headers=...
//!   3. 返 object_key
//!
//! 失败回退: caller (v8_ws.run_shard_task) 截断 output + 走 inline_output

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_API_BASE: &str = "https://www.wujisuanli.com";
const UPLOAD_URL_PATH: &str = "/api/v8/files/upload-url";
const UPLOAD_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Serialize)]
struct UploadUrlReq<'a> {
    filename: &'a str,
    purpose: &'a str,
    content_type: &'a str,
    size_bytes: Option<u64>,
    task_id: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct UploadUrlResp {
    object_key: String,
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
}

fn default_method() -> String { "PUT".to_string() }

/// 上传 bytes 到 OSS · 成功返 object_key (作为 ShardResultPayload.output_ref)
///
/// Args:
///   access_token: 节点已 auth 的 bearer token
///   bytes:        要上传的内容
///   filename:     友好文件名 (e.g. "shard-xxx-output.json")
///   content_type: MIME (e.g. "application/json", "image/png")
///   task_id:      所属 shard_id · 用作 OSS 路径分仓
pub async fn upload_bytes(
    access_token: &str,
    bytes: Vec<u8>,
    filename: &str,
    content_type: &str,
    task_id: Option<&str>,
) -> Result<String> {
    let api_base = std::env::var("EDGECOMPUTE_API_BASE")
        .unwrap_or_else(|_| DEFAULT_API_BASE.to_string());
    let client = reqwest::Client::builder()
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(UPLOAD_TIMEOUT_SECS))
        .danger_accept_invalid_certs(true) // 自签名兼容
        .build()
        .context("build reqwest client")?;

    // ── 1. presign ──
    let presign_url = format!("{}{}", api_base.trim_end_matches('/'), UPLOAD_URL_PATH);
    let body = UploadUrlReq {
        filename,
        purpose: "result",
        content_type,
        size_bytes: Some(bytes.len() as u64),
        task_id,
    };
    let resp = client
        .post(&presign_url)
        .bearer_auth(access_token)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("presign 请求失败: {}", presign_url))?;
    let status = resp.status();
    if !status.is_success() {
        let txt = resp.text().await.unwrap_or_default();
        return Err(anyhow!("presign HTTP {}: {}", status.as_u16(), txt));
    }
    let presign: UploadUrlResp = resp
        .json()
        .await
        .context("presign JSON 解析失败")?;

    // ── 2. PUT 到 OSS ──
    let mut put = match presign.method.to_uppercase().as_str() {
        "POST" => client.post(&presign.url),
        _ => client.put(&presign.url),
    };
    for (k, v) in &presign.headers {
        put = put.header(k.as_str(), v.as_str());
    }
    // Content-Type 兜底 · 部分 OSS 严格校验
    let has_ct = presign.headers.keys().any(|k| k.eq_ignore_ascii_case("content-type"));
    if !has_ct {
        put = put.header("Content-Type", content_type);
    }

    let size = bytes.len();
    let put_resp = put
        .body(bytes)
        .send()
        .await
        .with_context(|| format!("OSS PUT 失败 url={}", presign.url))?;
    let put_status = put_resp.status();
    if !put_status.is_success() {
        let txt = put_resp.text().await.unwrap_or_default();
        return Err(anyhow!("OSS PUT HTTP {}: {}", put_status.as_u16(), txt));
    }

    tracing::info!(
        "oss.upload · ok filename={} size={} → object_key={}",
        filename, size, presign.object_key
    );
    Ok(presign.object_key)
}
