//! 客户端全局共享状态。
//!
//! 由 Rust 主进程持有，UI 通过 `invoke("get_state")` 拉，
//! 状态变化时通过 `emit("connection_state_changed", ...)` 推。

use serde::Serialize;
use std::sync::Mutex;
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Registered, // hello+auth 走完
    Reconnecting,
}

impl ConnectionState {
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionState::Disconnected => "未连接",
            ConnectionState::Connecting => "正在连接...",
            ConnectionState::Authenticating => "正在认证...",
            ConnectionState::Registered => "已连接",
            ConnectionState::Reconnecting => "重连中...",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppStateSnapshot {
    pub connection_state: ConnectionState,
    pub state_label: &'static str,
    pub node_id: Option<String>,
    pub owner_id: Option<i64>,
    pub server_version: Option<String>,
    pub last_error: Option<String>,
    pub client_version: &'static str,
    pub user: Option<UserInfo>,
    pub is_authenticated: bool,
    pub current_task_id: Option<String>,
    pub mode: String,
    pub throttle_pct: u8,
    /// 心跳 RTT 毫秒，None 表示尚未测量
    pub latency_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub email: String,
}

#[derive(Debug)]
pub struct AppState {
    pub inner: Mutex<AppStateInner>,
    /// shutdown signal：cancel WS session 时使用
    pub shutdown_tx: Mutex<Option<watch::Sender<bool>>>,
    /// 模式变更广播：commands 写入，ws_client 订阅然后发到 server
    pub mode_tx: broadcast::Sender<ModeChange>,
}

#[derive(Debug, Default)]
pub struct AppStateInner {
    pub connection_state: ConnectionStateField,
    pub node_id: Option<String>,
    pub owner_id: Option<i64>,
    pub server_version: Option<String>,
    pub last_error: Option<String>,
    pub access_token: Option<String>,
    pub user: Option<UserInfo>,
    pub current_task_id: Option<String>,
    pub mode: String,
    pub throttle_pct: u8,
    pub latency_ms: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ConnectionStateField(pub ConnectionState);

impl Default for ConnectionStateField {
    fn default() -> Self {
        ConnectionStateField(ConnectionState::Disconnected)
    }
}

/// 模式变更请求：mode + 可选 throttle_pct（0-100）
#[derive(Debug, Clone)]
pub struct ModeChange {
    pub mode: String,
    pub throttle_pct: u8,
}

impl AppState {
    pub fn new() -> Self {
        let (mode_tx, _) = broadcast::channel::<ModeChange>(16);
        let mut inner = AppStateInner::default();
        inner.mode = "active".to_string();
        inner.throttle_pct = 100;
        Self {
            inner: Mutex::new(inner),
            shutdown_tx: Mutex::new(None),
            mode_tx,
        }
    }

    pub fn subscribe_mode(&self) -> broadcast::Receiver<ModeChange> {
        self.mode_tx.subscribe()
    }

    pub fn request_mode(&self, mode: &str) {
        // 兼容旧接口：默认 throttle_pct 由 mode 推导
        let pct = match mode {
            "active" => 100,
            "paused" => 0,
            _ => 50,
        };
        let _ = self.mode_tx.send(ModeChange { mode: mode.to_string(), throttle_pct: pct });
    }

    pub fn request_mode_with_pct(&self, mode: &str, throttle_pct: u8) {
        let _ = self.mode_tx.send(ModeChange {
            mode: mode.to_string(),
            throttle_pct: throttle_pct.min(100),
        });
    }

    pub fn set_mode(&self, mode: &str) {
        let mut g = self.inner.lock().unwrap();
        g.mode = mode.to_string();
    }

    pub fn set_mode_with_pct(&self, mode: &str, throttle_pct: u8) {
        let mut g = self.inner.lock().unwrap();
        g.mode = mode.to_string();
        g.throttle_pct = throttle_pct.min(100);
    }

    pub fn current_mode(&self) -> String {
        let g = self.inner.lock().unwrap();
        g.mode.clone()
    }

    /// 当前 throttle 百分比 (0-100) · 0 = 完全暂停 · 100 = 满负载
    pub fn current_throttle_pct(&self) -> u8 {
        let g = self.inner.lock().unwrap();
        g.throttle_pct
    }

    pub fn snapshot(&self) -> AppStateSnapshot {
        let g = self.inner.lock().unwrap();
        AppStateSnapshot {
            connection_state: g.connection_state.0,
            state_label: g.connection_state.0.label(),
            node_id: g.node_id.clone(),
            owner_id: g.owner_id,
            server_version: g.server_version.clone(),
            last_error: g.last_error.clone(),
            client_version: env!("CARGO_PKG_VERSION"),
            user: g.user.clone(),
            is_authenticated: g.user.is_some(),
            current_task_id: g.current_task_id.clone(),
            mode: g.mode.clone(),
            throttle_pct: g.throttle_pct,
            latency_ms: g.latency_ms,
        }
    }

    pub fn set_latency(&self, ms: u32) {
        let mut g = self.inner.lock().unwrap();
        g.latency_ms = Some(ms);
    }

    pub fn set_current_task(&self, t: Option<String>) {
        let mut g = self.inner.lock().unwrap();
        g.current_task_id = t;
    }

    pub fn current_task(&self) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.current_task_id.clone()
    }

    pub fn set_user(&self, u: Option<UserInfo>) {
        let mut g = self.inner.lock().unwrap();
        g.user = u;
    }

    pub fn set_access_token(&self, t: Option<String>) {
        let mut g = self.inner.lock().unwrap();
        g.access_token = t;
    }

    pub fn access_token(&self) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.access_token.clone()
    }

    pub fn set_state(&self, s: ConnectionState) {
        let mut g = self.inner.lock().unwrap();
        g.connection_state = ConnectionStateField(s);
    }

    pub fn node_id(&self) -> Option<String> {
        self.inner.lock().unwrap().node_id.clone()
    }

    pub fn set_node_owner(&self, node_id: String, owner_id: i64) {
        let mut g = self.inner.lock().unwrap();
        g.node_id = Some(node_id);
        g.owner_id = Some(owner_id);
    }

    pub fn set_server_version(&self, v: String) {
        let mut g = self.inner.lock().unwrap();
        g.server_version = Some(v);
    }

    pub fn set_error(&self, msg: Option<String>) {
        let mut g = self.inner.lock().unwrap();
        g.last_error = msg;
    }
}
