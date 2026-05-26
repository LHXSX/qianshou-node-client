//! 凭证持久化：refresh_token + 关联邮箱。
//!
//! 设计权衡（v3.0.0）：
//!   - dev 阶段用文件（`session.json`）落 app_data 目录，避免 macOS Keychain
//!     在每次 cargo 增量编译换二进制时反复弹"始终允许"授权窗。
//!   - 生产签名稳定后可切回 Keyring（OS 才弹一次）。
//!   - access_token 永远只在内存，不落盘。
//!   - refresh_token 30 天 TTL + 服务端可吊销，文件丢了影响有限。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

static SESSION_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize)]
struct PersistedSession {
    /// 长效 token：magic-link 流程是 refresh_token；password 流程是 agent_token
    #[serde(alias = "refresh_token")]
    token: String,
    email: String,
    /// "refresh"（magic-link）或 "agent"（password login），缺省按 refresh（兼容旧版）
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    username: String,
}

fn default_kind() -> String {
    "refresh".to_string()
}

pub fn init_session_path(app_data_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(app_data_dir)?;
    let p = app_data_dir.join("session.json");
    let _ = SESSION_PATH.set(p);
    Ok(())
}

fn path() -> Result<&'static PathBuf, std::io::Error> {
    SESSION_PATH.get().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "session path not initialized",
        )
    })
}

pub fn save_session(refresh_token: &str, email: &str) -> std::io::Result<()> {
    save_session_v2(refresh_token, email, "refresh", "")
}

/// 新的 save：支持 token kind + username（agent_token 或 refresh_token）
pub fn save_session_v2(
    token: &str,
    email: &str,
    kind: &str,
    username: &str,
) -> std::io::Result<()> {
    let p = path()?;
    let blob = PersistedSession {
        token: token.to_string(),
        email: email.to_string(),
        kind: kind.to_string(),
        username: username.to_string(),
    };
    // 原子写：tmp → rename，避免崩溃留半成品文件
    let tmp = p.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(&blob)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(&tmp, json)?;
    // 收紧权限到仅本人可读（防多用户机器其他账号偷看）
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&tmp, perms)?;
    }
    fs::rename(&tmp, p)?;
    Ok(())
}

pub fn load_session() -> std::io::Result<Option<(String, String)>> {
    Ok(load_session_v2()?.map(|s| (s.token, s.email)))
}

#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub token: String,
    pub email: String,
    pub kind: String, // "refresh" 或 "agent"
    pub username: String,
}

pub fn load_session_v2() -> std::io::Result<Option<LoadedSession>> {
    let p = path()?;
    if !p.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(p)?;
    let blob: PersistedSession = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if blob.token.is_empty() {
        return Ok(None);
    }
    Ok(Some(LoadedSession {
        token: blob.token,
        email: blob.email,
        kind: blob.kind,
        username: blob.username,
    }))
}

pub fn clear_session() -> std::io::Result<()> {
    let p = match SESSION_PATH.get() {
        Some(p) => p,
        None => return Ok(()),
    };
    if p.exists() {
        let _ = fs::remove_file(p);
    }
    Ok(())
}
