//! shard_result 离线缓存 · 防 WS 断连任务结果丢失 (v8)
//!
//! 策略 (P1):
//!   1. WS 断开/发送失败时 → 把 shard_result 写到 app_data/pending_shard_results.jsonl
//!   2. 下次 WS 重连 auth_ok 后 → drain 文件 · 逐行重发
//!   3. 服务端 shard_result 幂等 (按 shard_id 去重) · 重发安全
//!
//! 文件格式: 一行一个 JSON (shard_result OutFrame 的整帧)
//! 直接存帧体 · 重连后丢回 outbound 通道即可

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static QUEUE_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(app_data_dir: &Path) {
    let _ = std::fs::create_dir_all(app_data_dir);
    let p = app_data_dir.join("pending_shard_results.jsonl");
    let _ = QUEUE_PATH.set(p);
}

/// 把一个未发出的 shard_result 帧 JSON 落盘 (append-only)
/// `frame_json` 是已经序列化好的整帧 (含 v / type / payload)
pub fn enqueue_frame(frame_json: &str) {
    let Some(p) = QUEUE_PATH.get() else { return };
    let mut f = match OpenOptions::new().create(true).append(true).open(p) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("result_queue: open fail: {}", e);
            return;
        }
    };
    if let Err(e) = writeln!(f, "{}", frame_json) {
        tracing::warn!("result_queue: write fail: {}", e);
        return;
    }
    tracing::info!("result_queue: enqueued · bytes={}", frame_json.len());
}

/// 读出全部 pending 帧 · 清空文件
/// 返回所有未发的帧 JSON · 调用方按序送进 outbound_tx
pub fn drain() -> Vec<String> {
    let Some(p) = QUEUE_PATH.get() else { return Vec::new() };
    if !p.exists() {
        return Vec::new();
    }
    let f = match File::open(p) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut frames: Vec<String> = Vec::new();
    for line in BufReader::new(f).lines().flatten() {
        let line = line.trim().to_string();
        if !line.is_empty() {
            frames.push(line);
        }
    }
    // 原子清空 (先写空临时文件 · 再 rename)
    let tmp = p.with_extension("jsonl.tmp");
    let _ = std::fs::write(&tmp, b"");
    let _ = std::fs::rename(&tmp, p);
    if !frames.is_empty() {
        tracing::info!("result_queue: drained {} pending frames", frames.len());
    }
    frames
}

/// 当前队列长度 (用于 UI / 诊断)
pub fn pending_count() -> usize {
    let Some(p) = QUEUE_PATH.get() else { return 0 };
    if !p.exists() {
        return 0;
    }
    let Ok(f) = File::open(p) else { return 0 };
    BufReader::new(f)
        .lines()
        .filter(|l| l.as_ref().map(|x| !x.trim().is_empty()).unwrap_or(false))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn enqueue_drain_roundtrip() {
        let tmp = TempDir::new().unwrap();
        init(tmp.path());
        let f1 = r#"{"v":"8.0","type":"shard_result","payload":{"shard_id":"s1","ok":true}}"#;
        let f2 = r#"{"v":"8.0","type":"shard_result","payload":{"shard_id":"s2","ok":false}}"#;
        enqueue_frame(f1);
        enqueue_frame(f2);
        assert_eq!(pending_count(), 2);
        let drained = drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0], f1);
        assert_eq!(drained[1], f2);
        assert_eq!(pending_count(), 0);
    }
}
