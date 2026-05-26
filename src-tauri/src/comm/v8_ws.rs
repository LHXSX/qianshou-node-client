//! v8 WebSocket 客户端（跟 v1 ws_client.rs 完全独立）
//!
//! 设计:
//!   - 单一长连 · 不再有 HTTP polling (v8 用 WS push)
//!   - hello → welcome → auth → auth_ok → 主循环
//!   - 主循环: 心跳 (15s) + 接 shard_assign + 跑 + send shard_result
//!   - 任何错误 → close + reconnect (指数退避)
//!
//! 启动: lib.rs 根据 env V8_MODE 或探测结果选 v1 还是 v8

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use http::header::SEC_WEBSOCKET_PROTOCOL;
use native_tls::TlsConnector;
use serde_json::{json, Value};
use tauri::AppHandle;
use tokio::sync::{mpsc, watch, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;

use super::v8_proto::*;
use crate::auth::node_store;
use crate::state::{AppState, ConnectionState};
use crate::task::{executor, TaskAssign};

/// emit connection_state_changed event 到前端 UI (跟 v1 ws_client.rs 同步)
fn emit_state(app: &AppHandle, state: &Arc<AppState>) {
    use tauri::Emitter;
    let snap = state.snapshot();
    let _ = app.emit("connection_state_changed", &snap);
}

const HELLO_TIMEOUT_SECS: u64 = 5;
const AUTH_TIMEOUT_SECS: u64 = 10;
const HB_INTERVAL_SECS: u64 = 15;
const RECONNECT_MAX_SECS: u64 = 60;

/// 公开入口: 由 lib.rs 调用 (在 v8 模式下替代 ws_client::run_loop)
pub async fn run_v8_loop(
    access_token: String,
    persistent_node_id: Option<String>,
    state: Arc<AppState>,
    app: AppHandle,
    shutdown_rx: watch::Receiver<bool>,
) {
    let mut backoff_secs = 1u64;

    loop {
        if *shutdown_rx.borrow() {
            tracing::info!("v8.ws · shutdown · exit loop");
            break;
        }
        let _persistent_id = persistent_node_id.clone(); // 留待 hello.worker_id 用 (从 node_store 直接读)

        match run_v8_session(&state, &app, &access_token).await {
            Ok(()) => {
                tracing::info!("v8.ws · session 正常结束 · 1s 后重连");
                backoff_secs = 1;
            }
            Err(e) => {
                tracing::warn!("v8.ws · session 错: {} · {}s 后重连", e, backoff_secs);
            }
        }

        // session 退出 (正常/错) · 标 Disconnected 让 UI 显示真实状态 + emit
        state.set_state(ConnectionState::Disconnected);
        emit_state(&app, &state);

        // 退避重连间 · 标 Reconnecting + emit
        state.set_state(ConnectionState::Reconnecting);
        emit_state(&app, &state);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
            _ = async {
                let mut rx = shutdown_rx.clone();
                let _ = rx.changed().await;
            } => {
                state.set_state(ConnectionState::Disconnected);
                emit_state(&app, &state);
                break;
            }
        }
        // 重连失败后翻倍 (1 → 2 → 4 → ... → 60s 上限)
        backoff_secs = (backoff_secs * 2).min(RECONNECT_MAX_SECS);
    }
}

async fn run_v8_session(state: &Arc<AppState>, app: &AppHandle, token: &str) -> Result<()> {
    let ws_url = build_ws_url(state);
    tracing::info!("v8.ws · 连接 {}", ws_url);

    state.set_state(ConnectionState::Connecting);
    emit_state(app, state);

    let mut req = ws_url.into_client_request().context("invalid ws url")?;
    req.headers_mut().insert(SEC_WEBSOCKET_PROTOCOL, SUBPROTOCOL.parse().unwrap());

    let connector = TlsConnector::builder().build().context("tls builder")?;
    let (mut ws, _) = tokio_tungstenite::connect_async_tls_with_config(
        req,
        None,
        false,
        Some(Connector::NativeTls(connector)),
    )
    .await
    .context("ws connect failed")?;

    // ── 1. send hello ──
    let worker_id = node_store::load_node_id();
    let host_caps = collect_capabilities();
    // 2026-05-23 · 从磁盘读 5 能力授权矩阵 (webview 之前通过 save_capability_consent 写入)
    let consent_payload = crate::auth::consent_store::load_as_ws_payload();
    let hello = OutFrame::new(
        "hello",
        HelloPayload {
            client_version: env!("CARGO_PKG_VERSION").into(),
            os: OS_STR.into(),
            arch: ARCH_STR.into(),
            worker_id: worker_id.clone(),
            capabilities: host_caps,
            capability_consent: consent_payload,
        },
    );
    ws.send(Message::Text(serde_json::to_string(&hello)?)).await?;

    // ── 2. recv welcome (5s) ──
    let welcome_raw = recv_with_timeout(&mut ws, HELLO_TIMEOUT_SECS).await?;
    let welcome_frame: InFrame = serde_json::from_str(&welcome_raw)?;
    if welcome_frame.type_ != "welcome" {
        return Err(anyhow!("expected welcome · got {}", welcome_frame.type_));
    }
    let welcome: WelcomePayload = serde_json::from_value(welcome_frame.payload).unwrap_or_default();
    tracing::info!(
        "v8.ws · welcome server={} hb_interval={}s",
        welcome.server_version, welcome.hb_interval_s
    );

    // ── 3. send auth ──
    let display_name = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| worker_id.clone().unwrap_or_else(|| "v8-node".to_string()));
    let auth = OutFrame::new(
        "auth",
        AuthPayload {
            access_token: token.into(),
            name: display_name,
        },
    );
    ws.send(Message::Text(serde_json::to_string(&auth)?)).await?;

    // ── 4. recv auth_ok (10s) ──
    let auth_raw = recv_with_timeout(&mut ws, AUTH_TIMEOUT_SECS).await?;
    let auth_frame: InFrame = serde_json::from_str(&auth_raw)?;
    if auth_frame.type_ == "err" {
        let err: ErrPayload = serde_json::from_value(auth_frame.payload).unwrap_or(ErrPayload {
            code: 0, message: "unknown".into(), fatal: true,
        });
        return Err(anyhow!("v8 auth failed: code={} {}", err.code, err.message));
    }
    if auth_frame.type_ != "auth_ok" {
        return Err(anyhow!("expected auth_ok · got {}", auth_frame.type_));
    }
    let auth_ok: AuthOkPayload = serde_json::from_value(auth_frame.payload)?;
    let assigned_worker_id = auth_ok.worker_id.clone();
    let _ = node_store::save_node_id(&assigned_worker_id);
    state.set_node_owner(assigned_worker_id.clone(), auth_ok.owner_id);
    emit_state(app, state);  // node_id 变 · 也 emit

    tracing::info!(
        "v8.ws · auth_ok worker={} owner={} welcome_back={}",
        assigned_worker_id, auth_ok.owner_id, auth_ok.welcome_back
    );
    state.set_state(ConnectionState::Registered);
    emit_state(app, state);  // UI 显示 已连接

    // ── 4.5. P1 NCE · auth_ok 后立刻 flush 离线缓存的 shard_result ──
    // 上次 WS 断连时累积的任务结果 · 服务端按 shard_id 幂等去重安全
    let pending = super::result_queue::drain();
    if !pending.is_empty() {
        tracing::info!("v8.ws · flushing {} pending shard_results from offline cache", pending.len());
        for frame_json in pending {
            if let Err(e) = ws.send(Message::Text(frame_json.clone())).await {
                tracing::warn!("v8.ws · flush fail: {} · re-enqueue", e);
                super::result_queue::enqueue_frame(&frame_json);
                return Err(anyhow!("flush pending fail: {}", e));
            }
        }
    }

    // ── 5. 主循环 (心跳 + 接 shard) ──
    // 2026-05-20 · 关键修复: shard 执行用 tokio::spawn 后台跑
    // 结果帧通过 channel 回传到主循环 ws.send · 心跳不再被阻塞
    let hb_interval = welcome.hb_interval_s.max(5) as u64;
    let mut last_hb = Instant::now();
    let mut hb_timer = tokio::time::interval(Duration::from_secs(1));
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<String>();
    let active_shards: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // W3 (2026-05-26) · IP 代理池节点端 · TunnelMap 跟 session 同寿命
    let tunnel_map = crate::proxy::make_tunnel_map();

    // W1-7 (2026-05-26) · spawn pull_worker (节点主动抢 PULL 模式 shard · 默认 OFF)
    // pull_worker 跟 session 同寿命 · session 退出时 outbound_tx 关 · pull_loop 自动退出
    let _pull_task = {
        let tx = outbound_tx.clone();
        let state_clone = state.clone();
        let active_clone = active_shards.clone();
        tokio::spawn(async move {
            crate::task::pull_worker::pull_loop(tx, state_clone, active_clone).await;
        })
    };

    loop {
        tokio::select! {
            _ = hb_timer.tick() => {
                if last_hb.elapsed().as_secs() >= hb_interval {
                    // P0 NCE · 真采样 (一套对齐引擎实际消费的字段)
                    let rt = crate::runtime_monitor::sample();
                    let active = active_shards.lock().await.len() as i32;
                    let mut extra: HashMap<String, Value> = HashMap::new();
                    // uptime_sec · 服务端 heartbeat.py 会合入 capabilities · rep_stability 子分用
                    // (其他原扩展字段 cpu_usage/mem_usage/restart_count/pending_queue 服务端不消费 · 删)
                    extra.insert("uptime_sec".into(), json!(rt.uptime_sec));
                    let hb = OutFrame::new("hb", HbPayload {
                        load: rt.load_rate,
                        active_shards: active,
                        // 2026-05-21 P0-2 · 上报节能档位 + 模式 · 让服务端 planner 排除暂停节点
                        throttle_pct: Some(state.current_throttle_pct()),
                        mode: Some(state.current_mode()),
                        extra,
                    });
                    if let Err(e) = ws.send(Message::Text(serde_json::to_string(&hb)?)).await {
                        return Err(anyhow!("hb send failed: {}", e));
                    }
                    last_hb = Instant::now();
                }
            }
            Some(frame_json) = outbound_rx.recv() => {
                // 后台 shard 执行完毕 · 回传帧 · 主循环负责 ws.send
                if let Err(e) = ws.send(Message::Text(frame_json.clone())).await {
                    tracing::warn!("v8.ws · outbound send failed: {} · 落盘缓存 · 重连后 flush", e);
                    // P1 NCE · send fail → 落盘 · 不丢 shard_result
                    super::result_queue::enqueue_frame(&frame_json);
                    return Err(anyhow!("outbound send failed: {}", e));
                }
            }
            msg = ws.next() => {
                let msg = msg.ok_or_else(|| anyhow!("ws stream closed"))?;
                let msg = msg?;
                match msg {
                    Message::Text(raw) => {
                        if let Err(e) = handle_incoming(&raw, &outbound_tx, &active_shards, &tunnel_map, state, app, &assigned_worker_id).await {
                            tracing::warn!("v8.handle_incoming · {}", e);
                        }
                    }
                    Message::Close(_) => {
                        tracing::info!("v8.ws · server closed");
                        return Ok(());
                    }
                    Message::Ping(p) => {
                        let _ = ws.send(Message::Pong(p)).await;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn handle_incoming(
    raw: &str,
    outbound_tx: &mpsc::UnboundedSender<String>,
    active_shards: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    tunnel_map: &crate::proxy::TunnelMap,
    state: &Arc<AppState>,
    app: &AppHandle,
    worker_id: &str,
) -> Result<()> {
    let frame: InFrame = serde_json::from_str(raw).context("parse incoming")?;

    match frame.type_.as_str() {
        "hb_ack" => {
            // 心跳 ack · ignore
            Ok(())
        }
        "shard_assign" => {
            let assign: ShardAssignPayload = serde_json::from_value(frame.payload)?;
            tracing::info!(
                "v8.ws · shard_assign id={} task_type={} reward={:.4}",
                assign.shard_id, assign.task_type, assign.reward
            );
            spawn_shard_task(assign, outbound_tx, active_shards, state, app, worker_id).await;
            Ok(())
        }
        // W3 (2026-05-26) · IP 代理池 · 节点收 TunnelOpen → 开 TCP socket · spawn pipe
        "tunnel_open" => {
            let p: TunnelOpenPayload = serde_json::from_value(frame.payload)?;
            tracing::info!(
                "v8.ws · tunnel_open tid={} service={} display={}",
                p.tunnel_id, p.service_type, p.display_task_type,
            );
            let map = tunnel_map.clone();
            let tx = outbound_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::proxy::handler::handle_tunnel_open(p, map, tx).await {
                    tracing::warn!("v8.proxy.open · {}", e);
                }
            });
            Ok(())
        }
        // W3 · 服务端下行数据 · 写到对应 tunnel socket
        "tunnel_chunk" => {
            let p: TunnelChunkPayload = serde_json::from_value(frame.payload)?;
            let map = tunnel_map.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::proxy::handler::handle_tunnel_chunk(p, map).await {
                    tracing::warn!("v8.proxy.chunk · {}", e);
                }
            });
            Ok(())
        }
        // W3 · 服务端主动关 tunnel
        "tunnel_close" => {
            let p: TunnelClosePayload = serde_json::from_value(frame.payload)?;
            let map = tunnel_map.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::proxy::handler::handle_tunnel_close(p, map).await {
                    tracing::warn!("v8.proxy.close · {}", e);
                }
            });
            Ok(())
        }
        // W1-7 (2026-05-26) · PULL 模式 · server 回的 N 个 shard 跟 ShardAssign 一样 spawn
        "pull_assign" => {
            let assign: PullAssignPayload = serde_json::from_value(frame.payload)?;
            tracing::info!(
                "v8.ws · pull_assign count={} next_pull_after_ms={} load={:.2}",
                assign.shards.len(), assign.next_pull_after_ms, assign.server_load_hint
            );
            for sh in assign.shards {
                spawn_shard_task(sh, outbound_tx, active_shards, state, app, worker_id).await;
            }
            // TODO P2: 接 next_pull_after_ms · 让 pull_worker 动态调节间隔
            Ok(())
        }
        "shard_cancel" => {
            let cancel: ShardCancelPayload = serde_json::from_value(frame.payload)?;
            tracing::info!("v8.ws · shard_cancel id={} reason={}", cancel.shard_id, cancel.reason);
            if let Some(handle) = active_shards.lock().await.remove(&cancel.shard_id) {
                handle.abort();
                if state.current_task().as_deref() == Some(cancel.shard_id.as_str()) {
                    state.set_current_task(None);
                    emit_state(app, state);
                }
                {
                    use tauri::Emitter;
                    let _ = app.emit("task_phase", &json!({
                        "task_id": cancel.shard_id,
                        "phase": "cancelled",
                        "ok": false,
                        "error": cancel.reason,
                    }));
                }
            } else {
                tracing::info!("v8.ws · shard_cancel id={} 无本地运行任务", cancel.shard_id);
            }
            Ok(())
        }
        "err" => {
            let err: ErrPayload = serde_json::from_value(frame.payload).unwrap_or(ErrPayload {
                code: 0, message: "unknown".into(), fatal: false,
            });
            tracing::warn!("v8.ws ← err code={} {} (fatal={})", err.code, err.message, err.fatal);
            if err.fatal {
                Err(anyhow!("server sent fatal err: {}", err.message))
            } else {
                Ok(())
            }
        }
        // P1 · 服务端推送 · 强制立即检查更新
        // payload 可选: {min_version, reason}
        "update_required" => {
            tracing::info!("v8.ws · update_required received from server");
            crate::auto_updater::trigger_check_now();
            Ok(())
        }
        // 2026-05-23 · 运营位热更新推送 (broker.broadcast_to_all_workers 触发)
        "op_slots_changed" => {
            use tauri::Emitter;
            let payload: super::v8_proto::OpSlotsChangedPayload =
                match serde_json::from_value(frame.payload) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("v8.ws · op_slots_changed payload 解析失败: {}", e);
                        return Ok(());
                    }
                };
            tracing::info!(
                "v8.ws ← op_slots_changed keys={:?} action={}",
                payload.affected_keys, payload.action,
            );
            // emit 给 webview · Vue useOpSlots 监听后重拉
            let _ = app.emit("op_slots:changed", &payload);
            Ok(())
        }
        other => {
            tracing::warn!("v8.ws · 未知帧 type={}", other);
            Ok(())
        }
    }
}

/// P1 NCE · 任务跳过 outbound channel · 直接落盘
/// 场景: outbound_tx 已 closed (session 退出中) → 丢、改落盘 → 重连时 flush
fn enqueue_result_to_disk(payload: &ShardResultPayload) {
    let out = OutFrame::new("shard_result", payload.clone());
    if let Ok(s) = serde_json::to_string(&out) {
        super::result_queue::enqueue_frame(&s);
    }
}

/// W1-7 · 抽出 spawn 逻辑 · shard_assign 和 pull_assign 共用
/// (跟原 shard_assign 处理一致 · 只是抽成函数复用)
async fn spawn_shard_task(
    assign: ShardAssignPayload,
    outbound_tx: &mpsc::UnboundedSender<String>,
    active_shards: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    state: &Arc<AppState>,
    app: &AppHandle,
    worker_id: &str,
) {
    let tx = outbound_tx.clone();
    let state_clone = state.clone();
    let app_clone = app.clone();
    let wid = worker_id.to_string();
    let shard_id = assign.shard_id.clone();
    let active_for_task = active_shards.clone();
    let shard_id_for_task = shard_id.clone();

    let handle = tokio::spawn(async move {
        if let Err(e) = run_shard_task(assign, &tx, &state_clone, &app_clone, &wid).await {
            tracing::warn!("v8.shard_task · {}", e);
        }
        active_for_task.lock().await.remove(&shard_id_for_task);
    });
    if let Some(old) = active_shards.lock().await.insert(shard_id.clone(), handle) {
        tracing::warn!("v8.ws · shard={} 重复派发 · abort old handle", shard_id);
        old.abort();
    }
}

/// 后台执行 shard 任务 · 通过 channel 回传 shard_result 帧
/// 不持有 ws · 不阻塞主循环
async fn run_shard_task(
    assign: ShardAssignPayload,
    outbound_tx: &mpsc::UnboundedSender<String>,
    state: &Arc<AppState>,
    app: &AppHandle,
    worker_id: &str,
) -> Result<()> {
    use tauri::Emitter;

    // 2026-05-18 · v8 收口 · 发 task_phase 事件给 UI (Kanban 看板用)
    // 4 阶段: queued → running → verifying → done
    let started_at = chrono::Utc::now().timestamp_millis();
    let phase_base = serde_json::json!({
        "task_id": assign.shard_id,
        "workload_id": assign.workload_id,
        "task_type": assign.task_type,
        "runtime": assign.runtime,
        "cmd": format!("v8/{} (shard {}/{})", assign.task_type, assign.index + 1, assign.total),
        "reward": assign.reward,
        "timeout_s": assign.timeout_s,
        "started_at_ms": started_at,
        // 2026-05-21 · UI 展示字段
        "workload_name": assign.workload_name,
        "requester_name": assign.requester_name,
        "requester_avatar": assign.requester_avatar,
        "created_at_ms": assign.created_at_ms,
        "index": assign.index,
        "total": assign.total,
    });

    // emit running
    let mut running = phase_base.clone();
    running["phase"] = serde_json::json!("running");
    let _ = app.emit("task_phase", &running);

    // 转 TaskAssign · 复用现有 executor
    let task = shard_to_task_assign(&assign);
    let result = executor::run_task_with_progress(&task, worker_id).await;

    // emit verifying
    let mut verifying = phase_base.clone();
    verifying["phase"] = serde_json::json!("verifying");
    verifying["elapsed_ms"] = serde_json::json!(result.elapsed_ms);
    let _ = app.emit("task_phase", &verifying);

    // 2026-05-21 P0-3 · 大输出走 OSS · 小输出 inline 走 ws
    // 阈值 64 KB · 超过就 PUT presign 上 OSS · 拿 object_key 作 output_ref
    const INLINE_LIMIT_BYTES: usize = 64 * 1024;
    let mut output_ref: Option<String> = None;
    let mut inline_output: Option<String> = None;
    if !result.output.is_empty() {
        if result.output.len() > INLINE_LIMIT_BYTES {
            // 尝试 OSS 上传 · 失败回退到 inline (截断由 server 端的 aggregator 决定)
            if let Some(token) = state.access_token() {
                let bytes = result.output.clone().into_bytes();
                let filename = format!("shard-{}-output.json", &assign.shard_id);
                let ct = if result.output.starts_with('{') || result.output.starts_with('[')
                    { "application/json" } else { "text/plain; charset=utf-8" };
                match super::oss_upload::upload_bytes(
                    &token, bytes, &filename, ct, Some(&assign.shard_id)
                ).await {
                    Ok(key) => {
                        tracing::info!(
                            "v8.shard_task · large output ({} bytes) → OSS key={}",
                            result.output.len(), key
                        );
                        output_ref = Some(key);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "v8.shard_task · OSS upload 失败 ({} bytes) · 回退 inline: {}",
                            result.output.len(), e
                        );
                        inline_output = Some(result.output.clone());
                    }
                }
            } else {
                // 没 token (理论上不可能 · 已 auth) · 直接 inline
                inline_output = Some(result.output.clone());
            }
        } else {
            inline_output = Some(result.output.clone());
        }
    }

    // 回 shard_result · 通过 channel 送到主循环 ws.send
    let payload = ShardResultPayload {
        shard_id: assign.shard_id.clone(),
        ok: result.ok,
        output_ref,
        inline_output,
        elapsed_ms: Some(result.elapsed_ms as i64),
        error: result.error.clone().unwrap_or_default(),
    };
    let out = OutFrame::new("shard_result", payload.clone());
    let frame_json = serde_json::to_string(&out)?;
    if let Err(send_err) = outbound_tx.send(frame_json) {
        // P1 NCE · channel closed (session 退出了) · 落盘 · 重连后 flush
        tracing::warn!("v8.shard_task · outbound 闭了 · 落盘 shard={} · raw_len={}",
            payload.shard_id, send_err.0.len());
        enqueue_result_to_disk(&payload);
        anyhow::bail!("outbound channel closed");
    }
    tracing::info!(
        "v8.shard_task · done shard={} ok={} elapsed={}ms",
        assign.shard_id, result.ok, result.elapsed_ms
    );

    // emit done
    let mut done = phase_base;
    done["phase"] = serde_json::json!("done");
    done["elapsed_ms"] = serde_json::json!(result.elapsed_ms);
    done["ok"] = serde_json::json!(result.ok);
    if let Some(err) = &result.error {
        done["error"] = serde_json::json!(err);
    }
    let _ = app.emit("task_phase", &done);

    Ok(())
}

fn shard_to_task_assign(shard: &ShardAssignPayload) -> TaskAssign {
    // 把 v8 shard_assign 映射到现有 TaskAssign (复用 v1 executor)
    // v8 协议字段全塞进 args · executor 根据 input_kind 选 fetch 策略
    let mut args_map = shard.params.clone();

    // 2026-05-23 B6 修复 · 把整个 params 对象也额外存一份到 args["params"]
    // 上游 executor.rs 用 task.args.get("params") 整体取 → 转成 EC_PARAMS=<JSON>
    // 之前只 spread 顶层字段 (width/height/quality 等) · executor 拿不到 params 整体 → EC_PARAMS 始终为空 → 脚本全用默认值
    if !shard.params.is_empty() {
        args_map.insert(
            "params".into(),
            serde_json::to_value(&shard.params).unwrap_or(Value::Object(Default::default())),
        );
    }

    // input_kind (single_file / multi_file / archive / inline / params_only / stream)
    args_map.insert("input_kind".into(), Value::String(shard.input_kind.clone()));

    if let Some(inp) = &shard.inline_input {
        args_map.insert("inline_input".into(), Value::String(inp.clone()));
        // 走 args["stdin"] · 让脚本能从 stdin 读
        args_map.insert("stdin".into(), Value::String(inp.clone()));
    }
    if !shard.input_ref.is_empty() {
        args_map.insert("input_ref".into(), Value::String(shard.input_ref.clone()));
    }
    // 2026-05-18 · multi_file · 把 URL 列表传给 executor
    if !shard.input_refs.is_empty() {
        args_map.insert(
            "input_refs".into(),
            Value::Array(shard.input_refs.iter().map(|u| Value::String(u.clone())).collect()),
        );
    }
    // 2026-05-18 · slice_meta · 切片元数据 (page 范围 / 时段)
    // 脚本通过 ENV var (e.g. SLICE_META_PAGE_START=0) 或 args["slice_meta"] 读取
    if !shard.slice_meta.is_empty() {
        args_map.insert(
            "slice_meta".into(),
            Value::Object(serde_json::Map::from_iter(shard.slice_meta.clone())),
        );
    }

    TaskAssign {
        task_id: shard.shard_id.clone(),
        task_type: shard.task_type.clone(),
        runner: String::new(),
        runtime: shard.runtime.clone(),
        code_url: shard.code_url.clone(),
        args: serde_json::to_value(&args_map).unwrap_or(Value::Object(Default::default())),
        timeout_s: shard.timeout_s,
        reward: shard.reward,
        skill_id: shard.params.get("skill_id").and_then(|v| v.as_str().map(String::from)),
        tool: shard.params.get("tool").and_then(|v| v.as_str().map(String::from)),
        prompt: shard.params.get("prompt").and_then(|v| v.as_str().map(String::from)),
        tools: shard.params.get("tools").cloned(),
        output_schema: shard.params.get("output_schema").cloned(),
        skill_pack_id: shard.params.get("skill_pack_id").and_then(|v| v.as_str().map(String::from)),
    }
}

async fn recv_with_timeout(
    ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
    secs: u64,
) -> Result<String> {
    let msg = tokio::time::timeout(Duration::from_secs(secs), ws.next())
        .await
        .context("recv timeout")?
        .ok_or_else(|| anyhow!("ws stream closed"))?
        .context("ws msg")?;
    match msg {
        Message::Text(t) => Ok(t),
        other => Err(anyhow!("expected text frame · got {:?}", other)),
    }
}

fn build_ws_url(_state: &Arc<AppState>) -> String {
    // 从 env 拿 v8 WS base · 默认生产
    let base = std::env::var("V8_WS_BASE").unwrap_or_else(|_| {
        "wss://www.wujisuanli.com".to_string()
    });
    format!("{}{}", base.trim_end_matches('/'), WS_PATH)
}

fn collect_capabilities() -> HashMap<String, Value> {
    // v8 完整硬件信息 · 复用 system_info::collect() 拿真实数据
    // 上报字段:
    //   cpu_brand, cpu_cores, cpu_threads, total_memory_mb,
    //   hostname, device_name, os_name, os_version, kernel_version, arch,
    //   runtimes, software (探测), tier (业务字段)
    let mut caps = HashMap::new();
    let sys = crate::system_info::collect();

    caps.insert("cpu_brand".into(), json!(sys.cpu_brand));
    caps.insert("cpu_cores".into(), json!(sys.cpu_cores));
    caps.insert("cpu_threads".into(), json!(sys.cpu_threads));
    caps.insert("total_memory_mb".into(), json!(sys.total_memory_mb));
    caps.insert("hostname".into(), json!(sys.hostname));
    caps.insert("device_name".into(), json!(sys.device_name));
    caps.insert("os_name".into(), json!(sys.os_name));
    caps.insert("os_version".into(), json!(sys.os_version));
    caps.insert("kernel_version".into(), json!(sys.kernel_version));
    caps.insert("arch".into(), json!(sys.arch));

    // 业务字段
    caps.insert("os".into(), json!(OS_STR));
    caps.insert("tier".into(), json!("basic"));
    caps.insert("runtimes".into(), json!(["python3", "bash", "node"]));

    // 2026-05-20 · GPU / 硬件加速器上报 (修复 capabilities.gpu_count 永远 0 的 bug)
    // 复用 hardware_capabilities::detect() 探测结果 · server 端 _row_to_worker 会消费这些字段
    let hw = crate::hardware_capabilities::detect();
    let has_gpu = hw.supports_cuda || hw.supports_metal || hw.supports_rocm;
    let gpu_model = if hw.supports_cuda {
        "NVIDIA (CUDA)"
    } else if hw.supports_metal {
        if hw.supports_mlx { "Apple Silicon (Metal + MLX)" } else { "Apple Metal" }
    } else if hw.supports_rocm {
        "AMD (ROCm)"
    } else {
        ""
    };
    caps.insert("gpu_count".into(), json!(if has_gpu { 1 } else { 0 }));
    caps.insert("gpu_model".into(), json!(gpu_model));
    caps.insert("gpu_vram_gb".into(), json!(hw.gpu_vram_gb));
    caps.insert("vram_mb".into(), json!((hw.gpu_vram_gb * 1024.0) as i64));
    // boolean flags · server WorkerCapabilities 不全用但保留 · 方便 admin 看
    caps.insert("supports_cuda".into(), json!(hw.supports_cuda));
    caps.insert("supports_metal".into(), json!(hw.supports_metal));
    caps.insert("supports_mlx".into(), json!(hw.supports_mlx));
    caps.insert("supports_rocm".into(), json!(hw.supports_rocm));
    caps.insert("supports_nvenc".into(), json!(hw.supports_nvenc));
    caps.insert("supports_nvdec".into(), json!(hw.supports_nvdec));
    caps.insert("supports_videotoolbox".into(), json!(hw.supports_videotoolbox));
    caps.insert("supports_qsv".into(), json!(hw.supports_qsv));
    caps.insert("supports_tensor_cores".into(), json!(hw.supports_tensor_cores));
    caps.insert("supports_neural_engine".into(), json!(hw.supports_neural_engine));
    caps.insert("unified_memory".into(), json!(hw.unified_memory));
    // accelerators 字符串列表 · server WorkerCapabilities.accelerators 字段
    let mut accelerators: Vec<&str> = Vec::new();
    if hw.supports_cuda { accelerators.push("cuda"); }
    if hw.supports_metal { accelerators.push("metal"); }
    if hw.supports_mlx { accelerators.push("mlx"); }
    if hw.supports_rocm { accelerators.push("rocm"); }
    if hw.supports_nvenc { accelerators.push("nvenc"); }
    if hw.supports_videotoolbox { accelerators.push("videotoolbox"); }
    if hw.supports_neural_engine { accelerators.push("ane"); }
    caps.insert("accelerators".into(), json!(accelerators));
    tracing::info!(
        "hardware caps · gpu={} gpu_model='{}' vram={:.1}GB cuda={} metal={} mlx={} accel={:?}",
        has_gpu, gpu_model, hw.gpu_vram_gb,
        hw.supports_cuda, hw.supports_metal, hw.supports_mlx, accelerators,
    );

    // 2026-05-18 · software 探测 (planner 调度匹配用)
    // 检测每个常用 software 是否在 PATH · 或 python 模块是否能 import
    let mut sw_set: std::collections::BTreeSet<String> = detect_softwares()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // 2026-05-20 · 叠加 ~/.qianshou/runtime/installed.json 提供的 software
    // 调度器 planner.py 用 capabilities.software 匹配 task_registry.required_software
    // 用户在客户端工具页点"一键安装运行环境"装了 lite/ocr/speech/vision-ai 后,
    // 这里就会上报 ["pillow","numpy","onnxruntime","pymupdf","pdfplumber","ffmpeg",...]
    let installed = crate::runtime::detector::read_installed_meta();
    let mut tier_names: Vec<String> = Vec::new();
    for (tier_name, tier) in installed.tiers.iter() {
        if !tier.ok {
            continue;
        }
        tier_names.push(tier_name.clone());
        for s in &tier.software {
            sw_set.insert(s.clone());
        }
    }
    let sw_list: Vec<String> = sw_set.into_iter().collect();
    caps.insert("software".into(), json!(sw_list));
    caps.insert("runtime_tiers".into(), json!(tier_names));
    caps.insert("runtime_install_mode".into(), json!(installed.install_mode));
    if let Some(hp) = installed.host_python.as_ref() {
        caps.insert("runtime_host_python".into(), json!(hp));
    }
    tracing::info!(
        "capabilities · software={:?} · runtime_tiers={:?}",
        sw_list, tier_names
    );

    // 2026-05-18 · benchmark 探针 (CPU/内存/磁盘 + 综合算力分)
    // 启动时跑一次 · ~2-3s · 然后塞 capabilities 上报
    let bench = crate::benchmark::run_bench();
    caps.insert("bench_cpu_mb_per_sec".into(), json!(bench.cpu_sha256_mb_per_sec));
    caps.insert("bench_memory_gb_per_sec".into(), json!(bench.memory_gb_per_sec));
    caps.insert("bench_disk_mb_per_sec".into(), json!(bench.disk_write_mb_per_sec));
    caps.insert("bench_capability_score".into(), json!(bench.capability_score));
    tracing::info!(
        "benchmark · CPU={} MB/s · MEM={} GB/s · DISK={} MB/s · SCORE={:.1} · {}ms",
        bench.cpu_sha256_mb_per_sec, bench.memory_gb_per_sec,
        bench.disk_write_mb_per_sec, bench.capability_score, bench.bench_elapsed_ms
    );

    caps
}

fn detect_softwares() -> Vec<&'static str> {
    let mut found = Vec::new();
    // 命令行工具 · which <cmd> 检测
    for (cmd, name) in [
        ("ffmpeg", "ffmpeg"),
        ("ffprobe", "ffprobe"),
        ("blender", "blender"),
        ("ollama", "ollama"),
        ("convert", "imagemagick"),
        ("unzip", "unzip"),
        ("git", "git"),
    ] {
        if which::which(cmd).is_ok() {
            found.push(name);
        }
    }
    // Python 模块 · python3 -c "import X" 退出码 0 表示有
    for (module, name) in [
        ("PIL", "pillow"),
        ("numpy", "numpy"),
        ("fitz", "pymupdf"),
        ("paddleocr", "paddleocr"),
        ("onnxruntime", "onnxruntime"),
        ("whisper", "whisper"),
        ("faster_whisper", "faster_whisper"),
        ("torch", "torch"),
        ("transformers", "transformers"),
    ] {
        let mut c = std::process::Command::new("python3");
        c.args(["-c", &format!("import {}", module)])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::proc_util::hide_window_std(&mut c);
        if c.status().map(|s| s.success()).unwrap_or(false) {
            found.push(name);
        }
    }
    found
}
