//! IP 代理池 · 节点端 (W3-D2 · 2026-05-26)
//!
//! 设计: 接 platform 的 TunnelOpen 帧 (service_type="ip_proxy") · 开 TCP socket · 双向 pipe
//! 详见 docs/开发主计划_2026-05-26_v1.md W1-2 节
//!
//! 模块组成:
//!   - session.rs · 全局 TunnelMap (HashMap<tunnel_id, NodeTunnel>) + 增删改查
//!   - handler.rs · 收 TunnelOpen/Chunk/Close 帧 · 处理 TCP pipe
//!
//! 跟 v8_ws.rs 集成:
//!   1. lib.rs 启动时 · 创建全局 TunnelMap (session::make_tunnel_map())
//!   2. v8_ws 主循环传给 handle_incoming
//!   3. handle_incoming 收 tunnel_open/chunk/close · 路由到 handler 对应 fn
//!
//! 节点 UI:
//!   - tunnel 不进 task_phase event (业务脱敏 · 不占 active_shards)
//!   - 仅 tracing 日志可见 (admin 看 server 端 we_proxy_sessions 审计)

pub mod handler;
pub mod session;

pub use session::{make_tunnel_map, TunnelMap};
