//! 5 能力同意矩阵持久化到 app_data 目录。
//!
//! 为什么不用 keychain：同意状态非敏感凭证，纯配置数据。
//! 为什么需要它：webview 的 localStorage 跟 Rust 后台进程不共享，
//! v8_ws.rs 启动 ws 时需要在 hello payload 带 capability_consent，
//! 必须能从磁盘读到 webview 写的最新同意状态。
//!
//! 路径示例（macOS）：
//!   ~/Library/Application Support/com.edgecompute.client/capability_consent.json
//!
//! 文件格式（兼容前端 useCapabilities.ConsentState）:
//!   { "consents": { "compute": true, ... },
//!     "agreedToS": true, "agreedPrivacy": true,
//!     "confirmedAtMs": 1716500000000 }

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static CONSENT_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init_path(app_data_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(app_data_dir)?;
    let p = app_data_dir.join("capability_consent.json");
    let _ = CONSENT_PATH.set(p);
    Ok(())
}

/// 读磁盘 · 返回原始 JSON Value · 失败 / 文件不存在 → None
pub fn load() -> Option<Value> {
    let p = CONSENT_PATH.get()?;
    let s = fs::read_to_string(p).ok()?;
    if s.trim().is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(&s).ok()
}

/// 写磁盘 · 原子写 (.tmp + rename)
pub fn save(data: &Value) -> std::io::Result<()> {
    let p = match CONSENT_PATH.get() {
        Some(p) => p,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "consent_store path not initialized",
            ))
        }
    };
    let serialized = serde_json::to_string(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serialized)?;
    fs::rename(&tmp, p)?;
    Ok(())
}

/// 转换为 WS hello payload 用的 capability_consent dict
/// 把前端 ConsentState 的 camelCase 字段名映射为后端 snake_case
/// 如果磁盘没数据 / 格式损坏 · 返回 None (老用户首次启动)
pub fn load_as_ws_payload() -> Option<Value> {
    let raw = load()?;
    let obj = raw.as_object()?;

    let mut payload = serde_json::Map::new();
    if let Some(v) = obj.get("consents") {
        payload.insert("consents".into(), v.clone());
    }
    if let Some(v) = obj.get("agreedToS") {
        payload.insert("agreed_tos".into(), v.clone());
    }
    if let Some(v) = obj.get("agreedPrivacy") {
        payload.insert("agreed_privacy".into(), v.clone());
    }
    if let Some(v) = obj.get("confirmedAtMs").and_then(|x| x.as_i64()) {
        // 毫秒时间戳 → ISO8601 (UTC) · 兼容后端 user_consent.confirmed_at
        let secs = v / 1000;
        let nanos = ((v % 1000) * 1_000_000) as u32;
        if let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos) {
            payload.insert(
                "confirmed_at".into(),
                Value::String(dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            );
        }
    }
    payload.insert("tos_version".into(), Value::String("v1.0".into()));
    payload.insert("privacy_version".into(), Value::String("v1.0".into()));

    if payload.is_empty() {
        None
    } else {
        Some(Value::Object(payload))
    }
}
