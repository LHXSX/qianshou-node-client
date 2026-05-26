//! 后端 API 错误友好化 · 2026-05-26
//!
//! 把后端响应 (v8 新格式: {ok, code, message, trace_id} · v1 旧格式: {detail})
//! 和 reqwest 网络错误统一翻译成 *用户看得懂* 的中文.
//!
//! 用法:
//! ```rust
//! match resp.status() {
//!     s if s.is_success() => { ... }
//!     s => Err(crate::api_error::parse_api_error(s.as_u16(), &body)),
//! }
//! ```

use serde::Deserialize;

#[derive(Deserialize, Default)]
struct V8ErrBody {
    #[serde(default)] code: String,
    #[serde(default)] message: String,
    /// v1 兼容 (老 endpoint 用 FastAPI 默认 {detail: "..."})
    #[serde(default)] detail: serde_json::Value,
}

/// HTTP 错误响应 → 友好中文
pub fn parse_api_error(status: u16, body: &str) -> String {
    // 1. 尝试解析 v8 标准错误体
    if let Ok(b) = serde_json::from_str::<V8ErrBody>(body) {
        // 优先 code 映射
        if let Some(friendly) = code_to_friendly(&b.code) {
            return friendly.to_string();
        }
        // 其次 backend message
        if !b.message.is_empty() {
            return b.message;
        }
        // v1 detail (字符串)
        if let serde_json::Value::String(s) = &b.detail {
            if !s.is_empty() {
                return s.clone();
            }
        }
        // v1 detail (数组 / 对象 · validation error)
        if !b.detail.is_null() {
            return format!("参数错误: {}", b.detail);
        }
    }
    // 2. fallback: 按 HTTP 状态码兜底
    status_default_message(status).to_string()
}

/// reqwest 网络错误 → 友好中文
pub fn parse_request_error(err: &reqwest::Error) -> String {
    if err.is_timeout() {
        "连接超时 · 请检查网络或稍后重试".into()
    } else if err.is_connect() {
        "无法连接服务器 · 请检查网络".into()
    } else if err.is_decode() {
        "服务器响应格式异常".into()
    } else if err.is_request() {
        format!("请求异常: {}", err)
    } else {
        format!("网络错误: {}", err)
    }
}

/// 后端 error code → 中文
fn code_to_friendly(code: &str) -> Option<&'static str> {
    Some(match code {
        // auth
        "AUTH_TOKEN_INVALID" | "AUTH_BAD_CREDENTIALS" => "账号或密码错误",
        "AUTH_ACCOUNT_BANNED" | "AUTH_ACCOUNT_DISABLED" => "账号已被禁用 · 请联系客服",
        "AUTH_ACCOUNT_LOCKED" => "账号已锁定 · 请稍后再试",
        "AUTH_2FA_REQUIRED" => "需要二次验证",
        "AUTH_USER_NOT_FOUND" => "账号不存在",
        "AUTH_PASSWORD_TOO_WEAK" => "密码太简单 · 至少 6 位",
        "AUTH_USERNAME_TAKEN" => "用户名已被占用",
        "AUTH_EMAIL_TAKEN" => "邮箱已注册",
        "AUTH_EMAIL_INVALID" => "邮箱格式不正确",
        "AUTH_USERNAME_INVALID" => "用户名只能含字母/数字/下划线",
        "AUTH_TOKEN_EXPIRED" => "登录已过期 · 请重新登录",
        "AUTH_FORBIDDEN" => "权限不足",
        // 通用
        "RATE_LIMITED" => "操作过于频繁 · 请稍后再试",
        "RESOURCE_NOT_FOUND" => "请求的资源不存在",
        "INVALID_PARAMS" | "VALIDATION_ERROR" => "参数错误 · 请检查输入",
        "INSUFFICIENT_BALANCE" => "余额不足",
        "INTERNAL_ERROR" | "INTERNAL_SERVER_ERROR" => "服务器内部错误 · 请稍后重试",
        "SERVICE_UNAVAILABLE" => "服务暂时不可用",
        // worker
        "WORKER_OFFLINE" => "节点离线",
        "WORKER_BUSY" => "节点繁忙",
        "WORKER_VERSION_TOO_OLD" => "客户端版本过旧 · 请升级",
        // workload
        "WORKLOAD_NOT_FOUND" => "任务不存在",
        "WORKLOAD_ALREADY_CANCELLED" => "任务已取消",
        "WORKLOAD_QUOTA_EXCEEDED" => "已超出本月配额",
        _ => return None,
    })
}

/// HTTP 状态码兜底
fn status_default_message(status: u16) -> &'static str {
    match status {
        400 => "请求格式错误",
        401 => "未登录或登录已过期 · 请重新登录",
        403 => "无权访问",
        404 => "请求的资源不存在",
        408 => "请求超时",
        409 => "操作冲突 · 请刷新后重试",
        413 => "上传内容太大",
        422 => "参数验证失败 · 请检查输入",
        429 => "操作过于频繁 · 请稍后再试",
        500 => "服务器内部错误 · 请稍后重试",
        502 => "网关错误 · 服务器暂时不可达",
        503 => "服务暂不可用 · 请稍后重试",
        504 => "服务器响应超时",
        _ if status >= 500 => "服务器错误 · 请稍后重试",
        _ if status >= 400 => "请求失败",
        _ => "未知错误",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v8_code_takes_priority() {
        let body = r#"{"ok":false,"code":"AUTH_TOKEN_INVALID","message":"用户名或密码错误","trace_id":"abc"}"#;
        assert_eq!(parse_api_error(401, body), "账号或密码错误");
    }

    #[test]
    fn unknown_code_falls_back_to_message() {
        let body = r#"{"ok":false,"code":"UNKNOWN_CODE","message":"实际原因 xx","trace_id":""}"#;
        assert_eq!(parse_api_error(400, body), "实际原因 xx");
    }

    #[test]
    fn v1_detail_string_works() {
        let body = r#"{"detail":"详细错误"}"#;
        assert_eq!(parse_api_error(400, body), "详细错误");
    }

    #[test]
    fn non_json_falls_back_to_status() {
        assert_eq!(parse_api_error(500, "<html>"), "服务器内部错误 · 请稍后重试");
        assert_eq!(parse_api_error(401, ""), "未登录或登录已过期 · 请重新登录");
        assert_eq!(parse_api_error(429, ""), "操作过于频繁 · 请稍后再试");
    }
}
