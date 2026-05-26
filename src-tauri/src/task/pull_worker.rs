//! Pull Worker · 节点端后台主动抢任务 (W1-7 · 2026-05-26)
//!
//! 设计:
//!   - tokio 后台 task · 跟 v8_session 同寿命 (重连时重启)
//!   - 默认 OFF · 由 env `V8_PULL_ENABLED=1` 启用 (节点 setting 后期接)
//!   - 节点 CPU/RAM 富余时 · 每 30s 发一次 PullRequest 帧
//!   - 收到 PullAssign 后 · v8_ws handle_incoming 路由到 run_shard_task (复用)
//!
//! flag 控制:
//!   - env V8_PULL_ENABLED=1   · 启用 pull (默认 OFF)
//!   - env V8_PULL_INTERVAL_S=N · 间隔秒数 (默认 30)
//!   - env V8_PULL_MAX_COUNT=N · 单次拉几条 (默认 1)
//!   - env V8_PULL_TASK_FILTER=t1,t2 · 节点只想要的 task_type 列表 (空=任意)
//!
//! 流向 (跟 v8_ws.rs 主 session 同存活):
//!   v8_session::run_v8_session
//!     ├─ spawn pull_worker::pull_loop  (后台)
//!     │   ├─ 每 30s · 检查 active_shards 数量
//!     │   ├─ 空闲 (active < threshold) · 发 PullRequest 到 outbound_tx
//!     │   └─ ws 关闭 · outbound_tx 关闭 · loop 退出
//!     └─ 主循环 handle_incoming("pull_assign") · 遍历 shards · spawn run_shard_task
//!
//! 跟 push 模式区别:
//!   - push: server 主动派给指定 worker (planner 选)
//!   - pull: node 主动来抢 (适合爬虫 / GEO 等"我有空就接"业务)
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use super::super::comm::v8_proto::{OutFrame, PullRequestPayload};
use super::super::state::AppState;

/// 默认配置 (env 可覆盖)
const DEFAULT_INTERVAL_SECS: u64 = 30;
const DEFAULT_MAX_COUNT: u32 = 1;
const DEFAULT_MAX_ACTIVE_SHARDS: usize = 1;  // 节点同时只跑 1 个 shard (跟 push 模式对齐 P4.15 "一节点一片")

/// pull 是否启用 (从 env 读 · 后期接 setting UI)
fn is_pull_enabled() -> bool {
    std::env::var("V8_PULL_ENABLED").ok().as_deref() == Some("1")
}

/// 间隔秒数 (默认 30s)
fn pull_interval_secs() -> u64 {
    std::env::var("V8_PULL_INTERVAL_S")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n >= 5 && n <= 600)
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

/// 单次拉几条 (默认 1 · 跟 P4.15 一节点一片对齐)
fn pull_max_count() -> u32 {
    std::env::var("V8_PULL_MAX_COUNT")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&n| n >= 1 && n <= 10)
        .unwrap_or(DEFAULT_MAX_COUNT)
}

/// 节点想要的 task_type 过滤 (env 逗号分隔 · 空=任意)
fn pull_task_type_filter() -> Vec<String> {
    std::env::var("V8_PULL_TASK_FILTER")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// pull_loop · 跟 ws session 同寿命 · session 关闭时自动退出
///
/// Args:
///   outbound_tx · ws 的发送 channel (跟 shard_result 同一个 · 主循环管 ws.send)
///   state       · AppState · 用于读 current_throttle_pct + 估算空闲资源
///   active_shards · 用于判 "节点是否空闲 · 决定是否拉新任务"
pub async fn pull_loop(
    outbound_tx: mpsc::UnboundedSender<String>,
    state: Arc<AppState>,
    active_shards: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
) {
    // 启动延迟 5s · 给 v8_session 完成 hello/auth 时间
    tokio::time::sleep(Duration::from_secs(5)).await;

    if !is_pull_enabled() {
        tracing::debug!("v8.pull_worker · 禁用 (V8_PULL_ENABLED != 1) · 退出");
        return;
    }

    let interval_s = pull_interval_secs();
    let max_count = pull_max_count();
    let task_filter = pull_task_type_filter();
    tracing::info!(
        "v8.pull_worker · 启动 · interval={}s max_count={} filter={:?}",
        interval_s, max_count, task_filter,
    );

    let mut ticker = tokio::time::interval(Duration::from_secs(interval_s));
    // 第一次立即 tick (本次跳过 · 让节点有 5+15s 完成第一次 hb)
    ticker.tick().await;

    loop {
        ticker.tick().await;

        // 1. 检查节点是否空闲 · 不空闲跳过 (跟 push 模式互让)
        let active_count = active_shards.lock().await.len();
        if active_count >= DEFAULT_MAX_ACTIVE_SHARDS {
            tracing::debug!("v8.pull_worker · 节点忙 · active={} · 跳过", active_count);
            continue;
        }

        // 2. 节能档位 < 30% 时也不拉 (节点用户不愿意干活)
        let throttle = state.current_throttle_pct();
        if throttle < 30 {
            tracing::debug!("v8.pull_worker · throttle={}% 太低 · 跳过", throttle);
            continue;
        }

        // 3. 构造 free_capacity (server 端 NCE 可能用 · 也方便调试)
        let rt = crate::runtime_monitor::sample();
        let free_capacity = json!({
            "cpu_idle_pct": ((1.0 - rt.load_rate.min(1.0).max(0.0)) * 100.0) as u32,
            "throttle_pct": throttle,
            "uptime_sec": rt.uptime_sec,
        });

        // 4. 发 PullRequest
        let payload = PullRequestPayload {
            max_count,
            task_type_filter: task_filter.clone(),
            free_capacity,
        };
        let frame = OutFrame::new("pull_request", payload);
        let frame_json = match serde_json::to_string(&frame) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("v8.pull_worker · 序列化失败: {}", e);
                continue;
            }
        };

        if let Err(_) = outbound_tx.send(frame_json) {
            // outbound 关闭 (session 退出) · 退出 loop · session 重连时会重新 spawn
            tracing::info!("v8.pull_worker · outbound 关闭 · 退出 loop");
            return;
        }

        tracing::debug!("v8.pull_worker · 发了 PullRequest · max_count={} filter={:?}",
            max_count, task_filter);
    }
}
