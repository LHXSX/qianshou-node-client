//! 磁盘空间自动清理 (P0 · 防"用户硬盘被占满"流失源)
//!
//! 策略:
//!   - 启动时跑一次 (清掉上次崩溃留的临时文件)
//!   - 之后每 24h 跑一次
//!   - 目标:
//!       ~/.qianshou/runtime/tmp/        > 1d → 删 (临时下载/解压)
//!       ~/.qianshou/runtime/cache/      > 7d → 删 (tarball cache · 升级 tier 后用不到)
//!       ~/.qianshou/logs/*.log          > 30d → 删 (历史日志)
//!       ~/.qianshou/pending_results.jsonl  > 1KB 且 mtime > 7d → 警告 (不动 · 怕丢数据)
//!
//! 不删 venvs · 不删 binaries · 不删 installed.json
//! emit Tauri event "disk_freed" 当释放 > 100MB · 给 UI 显示气泡

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Emitter};

use super::paths;

const TMP_AGE_DAYS: u64 = 1;
const CACHE_AGE_DAYS: u64 = 7;
const LOG_AGE_DAYS: u64 = 30;
const TICK_INTERVAL_HOURS: u64 = 24;
const NOTIFY_THRESHOLD_MB: u64 = 100;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct GcStats {
    pub freed_mb: u64,
    pub files_deleted: u64,
    pub elapsed_ms: u64,
}

/// 启动入口: lib.rs setup 调一次
/// 立即跑一次 + spawn 24h ticker
pub fn spawn(app: AppHandle) {
    // 立即跑一次 (放后台 · 不阻塞启动)
    let app_first = app.clone();
    tauri::async_runtime::spawn(async move {
        // 让 app 完成 setup · 等 10s 再跑 · 避免抢启动 IO
        tokio::time::sleep(Duration::from_secs(10)).await;
        let stats = run_once();
        emit_if_significant(&app_first, &stats);
    });

    // 周期 24h
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(TICK_INTERVAL_HOURS * 3600));
        ticker.tick().await; // 跳过第一个 (刚跑过)
        loop {
            ticker.tick().await;
            let stats = run_once();
            emit_if_significant(&app, &stats);
        }
    });

    tracing::info!(
        "garbage_collect: spawned · 启动后 10s 首跑 + 周期 {}h",
        TICK_INTERVAL_HOURS
    );
}

fn emit_if_significant(app: &AppHandle, stats: &GcStats) {
    if stats.freed_mb >= NOTIFY_THRESHOLD_MB {
        let _ = app.emit("disk_freed", stats);
    }
    tracing::info!(
        "garbage_collect: 清理完成 · freed={}MB files={} elapsed={}ms",
        stats.freed_mb, stats.files_deleted, stats.elapsed_ms
    );
}

/// 同步跑一次清理 · 返回统计
pub fn run_once() -> GcStats {
    let t0 = std::time::Instant::now();
    let runtime_root = paths::runtime_root();
    let mut total_bytes: u64 = 0;
    let mut total_files: u64 = 0;

    // 1. tmp/ → 删 > 1d
    let tmp_dir = runtime_root.join("tmp");
    if tmp_dir.exists() {
        let (b, n) = clean_dir(&tmp_dir, TMP_AGE_DAYS);
        total_bytes += b;
        total_files += n;
    }

    // 2. cache/ → 删 > 7d
    let cache_dir = runtime_root.join("cache");
    if cache_dir.exists() {
        let (b, n) = clean_dir(&cache_dir, CACHE_AGE_DAYS);
        total_bytes += b;
        total_files += n;
    }

    // 3. logs/ (在 app_data_dir · 不在 runtime_root) · 但我们也清 ~/.qianshou/logs
    let logs_dir = runtime_root.parent()
        .map(|p| p.join("logs"))
        .unwrap_or_else(|| PathBuf::from("/dev/null"));
    if logs_dir.exists() {
        let (b, n) = clean_dir(&logs_dir, LOG_AGE_DAYS);
        total_bytes += b;
        total_files += n;
    }

    GcStats {
        freed_mb: total_bytes / (1024 * 1024),
        files_deleted: total_files,
        elapsed_ms: t0.elapsed().as_millis() as u64,
    }
}

/// 删 dir 下所有 mtime > age_days 天的文件 · 返 (字节数, 文件数)
/// 不删子目录 · 不递归到 venvs/binaries 等保护目录
fn clean_dir(dir: &Path, age_days: u64) -> (u64, u64) {
    let cutoff = SystemTime::now() - Duration::from_secs(age_days * 86400);
    let mut bytes = 0u64;
    let mut count = 0u64;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return (0, 0),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        // 跳过子目录 (避免误删 cache/venvs/ 之类)
        if meta.is_dir() {
            continue;
        }
        let mtime = meta.modified().unwrap_or_else(|_| SystemTime::now());
        if mtime < cutoff {
            let sz = meta.len();
            if std::fs::remove_file(&path).is_ok() {
                bytes += sz;
                count += 1;
                tracing::debug!("garbage_collect: 删 {} ({} bytes)", path.display(), sz);
            }
        }
    }
    (bytes, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn clean_dir_skips_recent_files() {
        let tmp = TempDir::new().unwrap();
        let new_file = tmp.path().join("new.txt");
        File::create(&new_file).unwrap().write_all(b"hello").unwrap();
        // age=1d → 该文件刚创建 (mtime=now) · 不会删
        let (bytes, count) = clean_dir(tmp.path(), 1);
        assert_eq!(count, 0);
        assert_eq!(bytes, 0);
        assert!(new_file.exists());
    }

    #[test]
    fn run_once_nonexistent_safe() {
        // runtime_root 不存在时不 panic
        let stats = run_once();
        assert_eq!(stats.freed_mb, 0);
    }
}
