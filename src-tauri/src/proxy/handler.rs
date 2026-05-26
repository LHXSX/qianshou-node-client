//! IP 代理池 · 节点端 handler (W3-D2.2 · 2026-05-26)
//!
//! 职责:
//!   1. handle_tunnel_open · 收 TunnelOpenPayload · 开 TCP socket · spawn 双向 pipe task
//!   2. handle_tunnel_chunk · 收 TunnelChunkPayload · 写到对应 tunnel socket
//!   3. handle_tunnel_close · 收 TunnelClosePayload · abort tunnel + 释放 socket
//!
//! 协议:
//!   - service_type="ip_proxy" 才处理 · 其他 (cdn_relay/...) 留扩展
//!   - target = {"host": "...", "port": 443, "use_tls": false}
//!   - 节点 TCP 直连 target (use_tls=true 时 TLS upgrade · MVP 阶段先支持纯 TCP)
//!
//! 上行数据流:
//!   平台 (TunnelChunk 下行) → handler.handle_tunnel_chunk → session.write_chunk → socket.write
//!   socket.read → 节点 → TunnelChunk 上行 → 平台
//!
//! 安全:
//!   - target 风控由平台做 (gateway.is_target_allowed) · 节点信任 server
//!   - 单 tunnel 收发 buffer 限制 (TCP socket 自带背压)
//!   - 节能/电池模式由 v8_ws 主循环外层控制 (不在 handler)

use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

use super::session::{self, NodeTunnel, TunnelMap};
use crate::comm::v8_proto::{TunnelChunkPayload, TunnelClosePayload, TunnelOpenPayload, OutFrame};

/// TCP 连接超时 (避免 DNS / SYN 卡死)
const CONNECT_TIMEOUT_S: u64 = 8;
/// pipe 单次 read buffer (8KB · 适合大多数 HTTP/HTTPS payload)
const READ_BUFFER_BYTES: usize = 8 * 1024;

// ════════════════════════════════════════════════════════════════
// handle_tunnel_open · 收 server 派的开 tunnel 指令
// ════════════════════════════════════════════════════════════════
pub async fn handle_tunnel_open(
    payload: TunnelOpenPayload,
    map: TunnelMap,
    outbound_tx: mpsc::UnboundedSender<String>,
) -> Result<()> {
    if payload.service_type != "ip_proxy" {
        // 其他业务 · 留扩展 · 此 handler 暂只跑 ip_proxy
        tracing::debug!("proxy.handler · 跳过非 ip_proxy service_type={}", payload.service_type);
        return Ok(());
    }

    let tid = payload.tunnel_id.clone();

    // 1. 解 target
    let target = payload.target.as_object()
        .ok_or_else(|| anyhow!("tunnel_open target not object · tid={}", tid))?;
    let host = target.get("host").and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("tunnel_open target.host missing"))?
        .to_string();
    let port = target.get("port").and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("tunnel_open target.port missing"))? as u16;
    let use_tls = target.get("use_tls").and_then(|v| v.as_bool()).unwrap_or(false);

    let label = format!("{}:{}", host, port);

    // 2. 开 TCP socket (带超时)
    tracing::info!("proxy.handler · open tid={} target={} tls={}", &tid[..8.min(tid.len())], label, use_tls);
    let socket = match timeout(
        Duration::from_secs(CONNECT_TIMEOUT_S),
        TcpStream::connect((host.clone(), port)),
    ).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            send_close(&outbound_tx, &tid, "connect_error", 0, 0, &e.to_string());
            return Err(anyhow!("tcp connect fail: {}", e));
        }
        Err(_) => {
            send_close(&outbound_tx, &tid, "connect_timeout", 0, 0, "tcp_connect_timeout");
            return Err(anyhow!("tcp connect timeout {}s", CONNECT_TIMEOUT_S));
        }
    };

    // MVP: use_tls=true 时也走纯 TCP (server 端给的就是 SNI · 这边只搬运字节)
    // 实际 HTTPS/TLS 由客户端 SDK 完成 (节点只搬密文)
    let _ = use_tls;

    // 3. 创建 channel (handler 写 · pipe task 读)
    let (to_socket_tx, to_socket_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // 4. spawn 双向 pipe task
    let tid_for_task = tid.clone();
    let map_for_task = map.clone();
    let outbound_for_task = outbound_tx.clone();
    let initial = payload.initial_data_b64.clone();
    let idle_timeout = payload.timeout_s;
    let deadline_ms = payload.deadline_ms;

    let pipe_handle = tokio::spawn(async move {
        let result = pipe_loop(
            socket, to_socket_rx, initial, &tid_for_task,
            &outbound_for_task, idle_timeout, deadline_ms,
        ).await;
        // pipe 结束 · 清理本 tunnel 记 (forget · 防止双重 close)
        let mut g = map_for_task.lock().await;
        if let Some(t) = g.remove(&tid_for_task) {
            let dur = t.started_at.elapsed().as_secs_f64();
            tracing::info!("proxy.handler · pipe end tid={} target={} duration={:.1}s result={}",
                &tid_for_task[..8.min(tid_for_task.len())], t.target_label, dur,
                result.as_ref().map(|s| s.as_str()).unwrap_or_else(|e| { 
                    tracing::warn!("proxy.handler · pipe end err: {}", e); "error"
                })
            );
        }
    });

    // 5. 注册到 map
    let entry = NodeTunnel {
        service_type: payload.service_type,
        target_label: label,
        to_socket_tx,
        pipe_handle,
        started_at: Instant::now(),
    };
    session::insert(&map, tid, entry).await;
    Ok(())
}


// ════════════════════════════════════════════════════════════════
// handle_tunnel_chunk · 收 server 下行数据 → 写本地 socket
// ════════════════════════════════════════════════════════════════
pub async fn handle_tunnel_chunk(payload: TunnelChunkPayload, map: TunnelMap) -> Result<()> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(&payload.data_b64)
        .with_context(|| format!("tunnel_chunk b64 decode fail · tid={}", payload.tunnel_id))?;
    if data.is_empty() {
        return Ok(());
    }
    session::write_chunk(&map, &payload.tunnel_id, data).await;
    Ok(())
}


// ════════════════════════════════════════════════════════════════
// handle_tunnel_close · server 主动关 tunnel
// ════════════════════════════════════════════════════════════════
pub async fn handle_tunnel_close(payload: TunnelClosePayload, map: TunnelMap) -> Result<()> {
    if let Some((label, dur)) = session::remove_and_abort(&map, &payload.tunnel_id).await {
        tracing::info!("proxy.handler · server close tid={} target={} dur={:.1}s reason={}",
            &payload.tunnel_id[..8.min(payload.tunnel_id.len())], label, dur, payload.reason);
    }
    Ok(())
}


// ════════════════════════════════════════════════════════════════
// pipe_loop · 双向 pipe (socket ↔ channel) 直到 EOF / error / timeout
// ════════════════════════════════════════════════════════════════
async fn pipe_loop(
    mut socket: TcpStream,
    mut to_socket_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    initial_b64: String,
    tunnel_id: &str,
    outbound_tx: &mpsc::UnboundedSender<String>,
    idle_timeout_s: u64,
    deadline_ms: u64,
) -> Result<String> {
    let mut bytes_up: u64 = 0;
    let mut bytes_down: u64 = 0;
    let mut seq: u64 = 0;
    let mut buf = vec![0u8; READ_BUFFER_BYTES];
    let started = Instant::now();

    // 1. 写 initial data (客户首批数据)
    if !initial_b64.is_empty() {
        match base64::engine::general_purpose::STANDARD.decode(&initial_b64) {
            Ok(data) if !data.is_empty() => {
                if let Err(e) = socket.write_all(&data).await {
                    send_close(outbound_tx, tunnel_id, "write_error", bytes_up, bytes_down, &e.to_string());
                    return Ok("initial_write_err".to_string());
                }
                bytes_up = bytes_up.saturating_add(data.len() as u64);
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("proxy.handler · tid={} initial b64 decode fail: {}",
                    &tunnel_id[..8.min(tunnel_id.len())], e);
            }
        }
    }

    let idle = Duration::from_secs(idle_timeout_s.max(5));

    loop {
        // deadline 检查
        if deadline_ms > 0 && started.elapsed().as_millis() as u64 >= deadline_ms {
            send_close(outbound_tx, tunnel_id, "deadline_reached", bytes_up, bytes_down, "");
            return Ok("deadline".to_string());
        }

        tokio::select! {
            // 平台下行数据 (handler.write_chunk push)
            maybe_data = to_socket_rx.recv() => {
                match maybe_data {
                    Some(data) => {
                        if let Err(e) = socket.write_all(&data).await {
                            send_close(outbound_tx, tunnel_id, "write_error", bytes_up, bytes_down, &e.to_string());
                            return Ok("write_err".to_string());
                        }
                        bytes_up = bytes_up.saturating_add(data.len() as u64);
                    }
                    None => {
                        // sender drop · 平台不再下行 (通常 server 主动关)
                        send_close(outbound_tx, tunnel_id, "channel_closed", bytes_up, bytes_down, "");
                        return Ok("channel_closed".to_string());
                    }
                }
            }
            // 目标 socket 数据上行
            read_result = timeout(idle, socket.read(&mut buf)) => {
                match read_result {
                    Ok(Ok(0)) => {
                        // EOF · 目标关连接
                        send_close(outbound_tx, tunnel_id, "target_eof", bytes_up, bytes_down, "");
                        return Ok("eof".to_string());
                    }
                    Ok(Ok(n)) => {
                        let data = &buf[..n];
                        bytes_down = bytes_down.saturating_add(n as u64);
                        seq = seq.saturating_add(1);
                        send_chunk(outbound_tx, tunnel_id, data, seq);
                    }
                    Ok(Err(e)) => {
                        send_close(outbound_tx, tunnel_id, "read_error", bytes_up, bytes_down, &e.to_string());
                        return Ok("read_err".to_string());
                    }
                    Err(_) => {
                        // idle timeout
                        send_close(outbound_tx, tunnel_id, "idle_timeout", bytes_up, bytes_down, "");
                        return Ok("idle".to_string());
                    }
                }
            }
        }
    }
}


// ════════════════════════════════════════════════════════════════
// helpers · 发 TunnelChunk / TunnelClose 给 server
// ════════════════════════════════════════════════════════════════
fn send_chunk(tx: &mpsc::UnboundedSender<String>, tunnel_id: &str, data: &[u8], seq: u64) {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let frame = OutFrame::new("tunnel_chunk", TunnelChunkPayload {
        tunnel_id: tunnel_id.to_string(),
        data_b64: b64,
        seq,
    });
    if let Ok(s) = serde_json::to_string(&frame) {
        let _ = tx.send(s);  // outbound channel · 主循环负责 send
    }
}


fn send_close(
    tx: &mpsc::UnboundedSender<String>, tunnel_id: &str,
    reason: &str, bytes_up: u64, bytes_down: u64, error: &str,
) {
    let stats = serde_json::json!({
        "bytes_up": bytes_up,
        "bytes_down": bytes_down,
    });
    let frame = OutFrame::new("tunnel_close", TunnelClosePayload {
        tunnel_id: tunnel_id.to_string(),
        reason: reason.to_string(),
        stats,
        error: error.to_string(),
    });
    if let Ok(s) = serde_json::to_string(&frame) {
        let _ = tx.send(s);
    }
}
