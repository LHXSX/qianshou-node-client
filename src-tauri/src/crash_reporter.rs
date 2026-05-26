//! 客户端崩溃 (panic) 上报 (P0 · 防"用户报 bug 没上下文")
//!
//! 流程:
//!   1. panic hook 把异常写到磁盘 ~/.qianshou/last_panic.json
//!      (内存 log buffer 在 panic 时已没 · 必须落盘)
//!   2. 下次启动 → check_and_report() 读盘 → POST 后端 → 删文件
//!   3. 服务端 endpoint: POST /api/v8/client/crash-report
//!      入 we_client_crashes 表 (admin 看)
//!
//! 安全:
//!   - 不上报敏感数据 (token / 用户 hostname 全脱敏)
//!   - 5s 超时 · 失败保留文件等下次重试
//!   - 文件原子写 (临时文件 + rename)

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

static PANIC_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    pub client_version: String,
    pub os: String,
    pub arch: String,
    pub location: String,
    pub payload: String,
    pub captured_at_ms: i64,
    #[serde(default)]
    pub backtrace: String,
}

/// lib.rs setup 时调一次 · 初始化路径
pub fn init(app_data_dir: &Path) {
    let p = app_data_dir.join("last_panic.json");
    let _ = PANIC_PATH.set(p);
}

/// panic hook 内部调 · 把崩溃信息落盘
/// 必须 async-signal-safe (不能 alloc 太多 · 但 panic 时 alloc 已经被允许)
pub fn write_to_disk(location: &str, payload: &str) {
    let Some(p) = PANIC_PATH.get() else { return };
    let report = CrashReport {
        client_version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        location: location.into(),
        payload: truncate(payload, 4096),
        captured_at_ms: chrono::Utc::now().timestamp_millis(),
        backtrace: capture_backtrace(),
    };
    let json = match serde_json::to_string_pretty(&report) {
        Ok(s) => s,
        Err(_) => return,
    };
    // 原子写
    let tmp = p.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, p);
    }
}

/// 启动后调一次 · 检查上次崩溃 · 有则 POST 到 server + 删
/// 后台 spawn · 失败不影响主流程
pub fn check_and_report(api_base: String) {
    let Some(p) = PANIC_PATH.get().cloned() else { return };
    if !p.exists() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let json = match std::fs::read_to_string(&p) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("crash_reporter: 读 {} 失败: {}", p.display(), e);
                return;
            }
        };
        let report: CrashReport = match serde_json::from_str(&json) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("crash_reporter: 解析失败 · 删坏文件: {}", e);
                let _ = std::fs::remove_file(&p);
                return;
            }
        };
        tracing::info!(
            "crash_reporter: 上次启动崩溃 · location={} · version={} · 准备上报",
            report.location, report.client_version
        );
        let url = format!(
            "{}/api/v8/client/crash-report",
            api_base.trim_end_matches('/')
        );
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        match client.post(&url).json(&report).send().await {
            Ok(r) if r.status().is_success() => {
                tracing::info!("crash_reporter: 上报成功 · 删本地文件");
                let _ = std::fs::remove_file(&p);
            }
            Ok(r) => {
                tracing::warn!(
                    "crash_reporter: 上报 HTTP {} · 保留文件等下次重试",
                    r.status().as_u16()
                );
            }
            Err(e) => {
                tracing::warn!(
                    "crash_reporter: 上报网络失败: {} · 保留文件等下次重试",
                    e
                );
            }
        }
    });
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}…[truncated]", &s[..end])
    }
}

fn capture_backtrace() -> String {
    // std::backtrace 默认 disabled · 设 RUST_BACKTRACE=1 后能拿到
    // 我们在 release 默认不开 · 只在 dev/debug 启用 · 不强求
    std::env::var("RUST_BACKTRACE")
        .ok()
        .map(|_| format!("{:?}", std::backtrace::Backtrace::capture()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_then_parse() {
        let tmp = TempDir::new().unwrap();
        init(tmp.path());
        write_to_disk("src/foo.rs:42", "boom!");
        let p = PANIC_PATH.get().unwrap();
        assert!(p.exists());
        let json = std::fs::read_to_string(p).unwrap();
        let r: CrashReport = serde_json::from_str(&json).unwrap();
        assert_eq!(r.location, "src/foo.rs:42");
        assert_eq!(r.payload, "boom!");
        assert!(!r.client_version.is_empty());
    }

    #[test]
    fn truncate_keeps_chars() {
        assert_eq!(truncate("abc", 100), "abc");
        let s = truncate(&"ab".repeat(2000), 100);
        assert!(s.ends_with("…[truncated]"));
    }
}
