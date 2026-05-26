//! IP 代理池 · 节点端 session 表 (W3-D2.1 · 2026-05-26)
//!
//! 设计:
//!   - 全局 HashMap<tunnel_id, NodeTunnel>
//!   - 每个 NodeTunnel 持: sender(向 TCP socket 写下行数据) + spawn task handle + stats
//!   - 平台收到 TunnelChunk (下行) → handler.write_chunk(tid, data) → sender.send(data)
//!   - 平台收到 TunnelClose                                    → handler.close_tunnel(tid)
//!   - socket 自然关闭 → spawn task 自我清理 + 发 TunnelClose 给平台
//!
//! 并发安全: 全局 Mutex 保护表 · sender 是 mpsc 自己线程安全

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

/// 单个节点端代理隧道
pub struct NodeTunnel {
    /// 业务标识 ("ip_proxy" / "cdn_relay" / ...)
    pub service_type: String,
    /// 目标 host:port (用于日志)
    pub target_label: String,
    /// 节点 → TCP socket 的 sender (handler 写入 · TCP task 读出)
    pub to_socket_tx: mpsc::UnboundedSender<Vec<u8>>,
    /// pipe task handle (用于 close 时 abort)
    pub pipe_handle: JoinHandle<()>,
    /// 起始时间 (用于 close 时算 duration)
    pub started_at: Instant,
}

/// 全局节点端隧道表
pub type TunnelMap = Arc<Mutex<HashMap<String, NodeTunnel>>>;

/// 创建空表 (lib.rs 启动时调一次 · 传给 v8_ws session)
pub fn make_tunnel_map() -> TunnelMap {
    Arc::new(Mutex::new(HashMap::new()))
}

/// 插入新 tunnel (handler.handle_tunnel_open 调)
pub async fn insert(map: &TunnelMap, tunnel_id: String, t: NodeTunnel) {
    let mut g = map.lock().await;
    if let Some(old) = g.insert(tunnel_id.clone(), t) {
        // 极少见 · 同 tid 重派 · abort 老 pipe task
        tracing::warn!("proxy.session · 重复 tid={} · abort old pipe", tunnel_id);
        old.pipe_handle.abort();
    }
}

/// 写下行数据 (server → node → socket)
/// 返 false 表示 tunnel 不在 (已关 / 没建)
pub async fn write_chunk(map: &TunnelMap, tunnel_id: &str, data: Vec<u8>) -> bool {
    let g = map.lock().await;
    match g.get(tunnel_id) {
        Some(t) => {
            // 不阻塞 · channel 满了直接丢 (背压 TODO P2)
            if t.to_socket_tx.send(data).is_err() {
                tracing::debug!("proxy.session · tid={} sender closed · drop chunk", tunnel_id);
                false
            } else {
                true
            }
        }
        None => {
            tracing::debug!("proxy.session · tid={} 不存在 · drop chunk", tunnel_id);
            false
        }
    }
}

/// 关闭 tunnel (server 主动关 或 socket 自身关)
/// 返 (existed, duration_s)
pub async fn remove_and_abort(map: &TunnelMap, tunnel_id: &str) -> Option<(String, f64)> {
    let mut g = map.lock().await;
    if let Some(t) = g.remove(tunnel_id) {
        let dur = t.started_at.elapsed().as_secs_f64();
        let label = t.target_label.clone();
        // socket task 自己检测 sender drop · 会自然退出 · 这里防御 abort
        t.pipe_handle.abort();
        // sender 跟着 NodeTunnel drop · 自然关 socket task
        Some((label, dur))
    } else {
        None
    }
}

/// 当前活跃 tunnel 数 (admin/状态用)
pub async fn active_count(map: &TunnelMap) -> usize {
    map.lock().await.len()
}
