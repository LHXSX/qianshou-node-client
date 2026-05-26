//! Magic-link 登录 REST 客户端。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub const API_BASE: &str = "https://www.wujisuanli.com/api/v8";

#[derive(Serialize)]
struct StartReq<'a> {
    email: &'a str,
}

#[derive(Serialize)]
struct VerifyReq<'a> {
    email: &'a str,
    code: &'a str,
}

#[derive(Serialize)]
struct RefreshReq<'a> {
    refresh_token: &'a str,
}

#[derive(Deserialize, Debug)]
pub struct StartResp {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub expires_in: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub email: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct VerifyResp {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: u64,
    pub user: UserInfo,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RefreshResp {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: u64,
}

pub(crate) async fn post_json<TBody: Serialize, TResp: for<'de> Deserialize<'de>>(
    path: &str,
    body: &TBody,
) -> Result<TResp> {
    let client = reqwest::Client::builder()
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .danger_accept_invalid_certs(true) // www.wujisuanli.com 自签名证书
        .build()?;
    let url = format!("{}{}", API_BASE, path);
    // 2026-05-26 · 用 api_error 模块统一友好化网络/响应错误
    let resp = client.post(&url).json(body).send().await
        .map_err(|e| anyhow!("{}", crate::api_error::parse_request_error(&e)))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // 优先级: backend code (友好映射) → backend message → HTTP 状态码兜底
        return Err(anyhow!("{}", crate::api_error::parse_api_error(status.as_u16(), &text)));
    }
    serde_json::from_str::<TResp>(&text).map_err(|e| anyhow!("响应解析失败: {}", e))
}

pub async fn send_code(email: &str) -> Result<StartResp> {
    post_json("/auth/magic-link/start", &StartReq { email }).await
}

pub async fn verify(email: &str, code: &str) -> Result<VerifyResp> {
    post_json("/auth/magic-link/verify", &VerifyReq { email, code }).await
}

pub async fn refresh(refresh_token: &str) -> Result<RefreshResp> {
    post_json("/auth/refresh", &RefreshReq { refresh_token }).await
}
