//! 用户名/密码登录 + 注册（M3.5.1）。
//!
//! 后端端点：
//!   POST /api/v8/auth/login    {username, password}     → access_token + agent_token + user
//!   POST /api/v8/auth/register {username, email, password} → 同上
//!
//! 优先使用 agent_token（1 年长效），客户端长期持有，省去 refresh 流程。

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::magic_link::post_json;

#[derive(Serialize)]
struct LoginReq<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(Serialize)]
struct RegisterReq<'a> {
    username: &'a str,
    email: &'a str,
    password: &'a str,
}

#[derive(Deserialize, Debug, Clone)]
pub struct UserPublic {
    /// backend 可能返 i64 或 string ("admin")；统一用 string 接
    #[serde(deserialize_with = "de_any_to_string")]
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
}

fn de_any_to_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    use serde::de::Error;
    let v: serde_json::Value = serde::Deserialize::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Err(D::Error::custom(format!("expect string or number, got {}", other))),
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct LoginResp {
    pub access_token: String,
    #[serde(default)]
    pub agent_token: Option<String>,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub agent_token_expires_in: u64,
    #[serde(default)]
    pub role: String,
    pub user: UserPublic,
}

pub async fn login(username: &str, password: &str) -> Result<LoginResp> {
    post_json("/auth/login", &LoginReq { username, password }).await
}

pub async fn register(username: &str, email: &str, password: &str) -> Result<LoginResp> {
    post_json(
        "/auth/register",
        &RegisterReq { username, email, password },
    )
    .await
}
