//! v8 WebSocket 协议消息类型（对照 platform_v8/protocol/ws_schema.py）
//!
//! 跟 v1 (protocol.rs) 完全不同的协议:
//!   - subprotocol: "edgecompute.v8" (v1 是 "edgecompute.v1")
//!   - 帧格式: {"type": "...", "v": "8.0", "payload": {...}}  (v1 多了 id/ts 字段)
//!   - 帧名:   hello/welcome/auth/auth_ok/hb/hb_ack/shard_assign/shard_result/err
//!   - 路径:   /api/v8/ws/worker  (v1 是 /api/v8/ws/agent)
//!
//! 编译开关: 默认编译进来 · 启动时探测 v8 可用才用 · 否则 fallback v1

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub const PROTOCOL_VERSION: &str = "8.0";
pub const SUBPROTOCOL: &str = "edgecompute.v8";
pub const WS_PATH: &str = "/api/v8/ws/worker";

// ════════════════════════════════════════════════════════════════════
// Client → Server payloads
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize)]
pub struct HelloPayload {
    pub client_version: String,
    pub os: String,
    pub arch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    pub capabilities: HashMap<String, Value>,
    /// 2026-05-23 · 5 能力同意矩阵快照 (老客户端 / 未授权时 None · 服务端兼容)
    /// 结构跟 platform_v8.protocol.ws_schema.HelloPayload.capability_consent 对齐
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_consent: Option<Value>,
}

// 2026-05-23 · 运营位热更新推送 (服务器 → 客户端)
// 后台 admin 改运营位 → broker.broadcast_to_all_workers
// 客户端收到 · emit Tauri event "op_slots:changed" · Vue useOpSlots 重拉
#[derive(Debug, Clone, serde::Deserialize, Serialize)]
pub struct OpSlotsChangedPayload {
    pub affected_keys: Vec<String>,    // ['splash','banner']
    pub action: String,                 // 'created'/'updated'/'deleted'/'toggled'
    pub ts: String,                     // ISO 时间戳
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthPayload {
    pub access_token: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HbPayload {
    pub load: f32,
    pub active_shards: i32,
    /// 节点节能档位 0-100 · 0 = 暂停接单 · 服务端 planner 据此过滤
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle_pct: Option<u8>,
    /// 节点工作模式: running / paused / battery / scheduled / sleeping
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShardResultPayload {
    pub shard_id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShardProgressPayload {
    pub shard_id: String,
    pub pct: f32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
}

// ════════════════════════════════════════════════════════════════════
// Server → Client payloads
// ════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize, Default)]
pub struct WelcomePayload {
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub proto_version: String,
    #[serde(default)]
    pub server_clock: String,
    #[serde(default)]
    pub min_client_version: String,
    #[serde(default = "default_hb_interval")]
    pub hb_interval_s: u32,
    #[serde(default = "default_hb_timeout")]
    pub hb_timeout_s: u32,
}

fn default_hb_interval() -> u32 { 15 }
fn default_hb_timeout() -> u32 { 45 }

#[derive(Debug, Clone, Deserialize)]
pub struct AuthOkPayload {
    pub worker_id: String,
    pub owner_id: i64,
    #[serde(default)]
    pub welcome_back: bool,
    #[serde(default)]
    pub server_clock: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HbAckPayload {
    #[serde(default)]
    pub server_clock: String,
    #[serde(default)]
    pub server_load: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShardAssignPayload {
    pub shard_id: String,
    pub workload_id: String,
    #[serde(default)]
    pub index: i32,
    #[serde(default = "default_total")]
    pub total: i32,
    pub task_type: String,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default)]
    pub code_url: String,
    // 2026-05-18 · 扩展 input 协议 (跟 backend ShardAssignPayload 对齐)
    #[serde(default = "default_input_kind")]
    pub input_kind: String,                       // "single_file" / "multi_file" / "archive" / "inline" / "params_only" / "stream"
    #[serde(default)]
    pub input_ref: String,
    #[serde(default)]
    pub input_refs: Vec<String>,                  // multi_file 用
    #[serde(default)]
    pub inline_input: Option<String>,
    #[serde(default)]
    pub slice_meta: HashMap<String, Value>,       // 切片元数据 (page 范围 / 时段)
    #[serde(default)]
    pub params: HashMap<String, Value>,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    #[serde(default)]
    pub reward: f64,
    #[serde(default)]
    pub deadline: Option<String>,
    // 2026-05-21 · UI 展示用 (谁发的 / 任务叫什么)
    #[serde(default)]
    pub workload_name: String,
    #[serde(default)]
    pub requester_name: String,
    #[serde(default)]
    pub requester_avatar: String,
    #[serde(default)]
    pub created_at_ms: i64,
    // 2026-05-27 V8.1 · 客户端 venv 路由 (后端 v8.1+ 才发 · 老后端没此字段 · serde default 取空)
    // executor 用 required_tier 选 venvs/<tier>/bin/python 跑脚本
    // fallback_tiers 是兜底列表 (主 tier 没装时按顺序 try)
    #[serde(default)]
    pub required_tier: String,
    #[serde(default)]
    pub fallback_tiers: Vec<String>,
}

fn default_input_kind() -> String { "single_file".into() }

fn default_total() -> i32 { 1 }
fn default_runtime() -> String { "python3".into() }
fn default_timeout() -> u64 { 300 }

#[derive(Debug, Clone, Deserialize)]
pub struct ShardCancelPayload {
    pub shard_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrPayload {
    pub code: i32,
    pub message: String,
    #[serde(default)]
    pub fatal: bool,
}

// ════════════════════════════════════════════════════════════════════
// 通用 Tunnel 帧 (2026-05-26 W0-1 · 业务无关)
// service_type 区分业务 ("ip_proxy" / "cdn_relay" / ...)
// target 业务自定义 · stats 业务自定义
// 节点端静默处理 · UI 走 display_task_type/display_name 脱敏字段
// ════════════════════════════════════════════════════════════════════
#[derive(Debug, Clone, Deserialize)]
pub struct TunnelOpenPayload {
    pub tunnel_id: String,
    pub service_type: String,                          // "ip_proxy" / "cdn_relay" / ...
    #[serde(default)]
    pub target: serde_json::Value,                     // 业务自定义 · 协议透传
    #[serde(default)]
    pub initial_data_b64: String,
    #[serde(default = "default_tunnel_timeout")]
    pub timeout_s: u64,
    #[serde(default)]
    pub deadline_ms: u64,
    // 节点 UI 用 (业务脱敏 + 奖励预告)
    #[serde(default = "default_display_task_type")]
    pub display_task_type: String,
    #[serde(default = "default_display_name")]
    pub display_name: String,
    #[serde(default)]
    pub estimated_reward_edg: f64,
}

fn default_tunnel_timeout() -> u64 { 60 }
fn default_display_task_type() -> String { "system_session".to_string() }
fn default_display_name() -> String { "系统任务".to_string() }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TunnelChunkPayload {
    pub tunnel_id: String,
    pub data_b64: String,
    #[serde(default)]
    pub seq: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TunnelClosePayload {
    pub tunnel_id: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub stats: serde_json::Value,                      // 业务自定义统计 ({"bytes_up":..., "bytes_down":..., ...})
    #[serde(default)]
    pub error: String,
}

// ════════════════════════════════════════════════════════════════════
// 统一引擎 PULL 模式帧 (W1-2 · 2026-05-26 · 跟 backend ws_schema 对齐)
//
// 流向:
//   node → server (PullRequest · 我空闲想接活)
//   server → node (PullAssign · 给你 N 个 shard · shards 复用 ShardAssignPayload)
//
// 节点用法:
//   - 后台 task/pull_worker.rs 定时发 PullRequest (节点空闲 + flag ON)
//   - 收到 PullAssign · 取 shards 列表 · 跟收到 ShardAssign 一样 spawn executor
// ════════════════════════════════════════════════════════════════════
#[derive(Debug, Clone, Serialize)]
pub struct PullRequestPayload {
    pub max_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_type_filter: Vec<String>,                 // 节点想要的 task_type (空=任意)
    #[serde(default)]
    pub free_capacity: serde_json::Value,              // {"cpu_pct": 80, "ram_free_mb": 4096, ...}
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullAssignPayload {
    pub shards: Vec<ShardAssignPayload>,               // 复用 ShardAssignPayload · executor 完全复用
    #[serde(default = "default_pull_next_after")]
    pub next_pull_after_ms: u64,                       // 下次 pull 等多久 (server 限速提示)
    #[serde(default)]
    pub server_load_hint: f32,                         // 0-1 · server 负载提示
}

fn default_pull_next_after() -> u64 { 5000 }

// ════════════════════════════════════════════════════════════════════
// 帧 envelope (跟 v1 不同 · v8 用扁平结构)
// ════════════════════════════════════════════════════════════════════

/// 出站帧 (client → server)
#[derive(Debug, Clone, Serialize)]
pub struct OutFrame<P: Serialize> {
    pub v: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub payload: P,
}

impl<P: Serialize> OutFrame<P> {
    pub fn new(type_: &str, payload: P) -> Self {
        Self {
            v: PROTOCOL_VERSION.into(),
            type_: type_.into(),
            payload,
        }
    }
}

/// 入站帧 (server → client · raw 解析)
#[derive(Debug, Clone, Deserialize)]
pub struct InFrame {
    #[serde(default)]
    pub v: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub payload: Value,
}

// ════════════════════════════════════════════════════════════════════
// 平台探测
// ════════════════════════════════════════════════════════════════════

pub const OS_STR: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "windows") {
    "windows"
} else if cfg!(target_os = "linux") {
    "linux"
} else {
    "unknown"
};

pub const ARCH_STR: &str = if cfg!(target_arch = "aarch64") {
    "aarch64"
} else if cfg!(target_arch = "x86_64") {
    "x86_64"
} else {
    "unknown"
};
