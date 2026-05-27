pub mod executor;
pub mod llm_ollama;
pub mod llm_runtime;
pub mod pull_worker;  // W1-7 · 节点端 PULL 模式后台抢任务
pub mod resource_limit;
pub mod skill_pack;
pub mod skill_registry;
pub mod tool_caller;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 服务端 task_assign 消息的 payload。
///
/// 四种模式：
/// - shell：task_type=shell, args.cmd=要跑的命令
/// - script (M3.6)：task_type=script, runtime=python3/bash/node, code_url=远程脚本 URL,
///                  args.cmd_extra=命令行参数, args.stdin=stdin 输入
/// - skill_exec (V4 ⭐ 主推)：task_type=skill_exec, skill_id=技能集 id, tool=工具名,
///                            args=JSON 对象作为工具 stdin
/// - llm_infer (V7 高级节点保留)：task_type=llm_infer, prompt=推理提示词,
///                                  tools=OpenAI function calling 工具定义（可选）,
///                                  output_schema=期望输出 JSON Schema（可选）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskAssign {
    pub task_id: String,
    #[serde(default = "default_task_type")]
    pub task_type: String,
    #[serde(default)]
    pub runner: String,
    /// M3.6 script 模式：python3 / bash / sh / node
    #[serde(default)]
    pub runtime: String,
    /// M3.6 script 模式：远程脚本绝对 URL
    #[serde(default)]
    pub code_url: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    #[serde(default)]
    pub reward: f64,
    /// V4 skill_exec 模式：技能集 id（如 "text-tools-v1"，可带 @version）
    #[serde(default)]
    pub skill_id: Option<String>,
    /// V4 skill_exec 模式：工具名（如 "extract_clauses"）
    #[serde(default)]
    pub tool: Option<String>,
    /// V7 llm_infer 模式：推理提示词
    #[serde(default)]
    pub prompt: Option<String>,
    /// V7 llm_infer 模式：OpenAI function calling 工具定义
    #[serde(default)]
    pub tools: Option<Value>,
    /// V7 llm_infer 模式：期望输出 JSON Schema
    #[serde(default)]
    pub output_schema: Option<Value>,
    /// V8 任务级 fork 派生：服务端在 task 帧带 skill_pack_id 时，
    /// 客户端先 GET /api/v8/skill-packs/{id} 拉 runner_code，sha256 校验通过后
    /// 用这个**任务专用副本**取代原始 bundle 的 runner_code。
    #[serde(default)]
    pub skill_pack_id: Option<String>,
    /// V8.1 (2026-05-27) · 运行时 tier 路由 · executor 按此选 venvs/<tier>/bin/python 跑
    ///   "" (默认) → 用 fallback_tiers · 都没装则用 lite 兜底 · 仍没就系统 python3
    ///   "ocr" / "speech" / "vision-ai" / "lite" / "crawl" → 强制对应 venv
    /// 老服务端 (8.0.x) 不发此字段 · serde::default 取空 · 客户端走老路径 (打包 cpython)
    #[serde(default)]
    pub required_tier: String,
    /// V8.1 · required_tier 没装时的兜底 tier (按顺序 try)
    #[serde(default)]
    pub fallback_tiers: Vec<String>,
}

fn default_task_type() -> String {
    "shell".into()
}
fn default_timeout() -> u64 {
    30
}

/// Client → Server task_ack payload。
#[derive(Debug, Clone, Serialize)]
pub struct TaskAck<'a> {
    pub task_id: &'a str,
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'a str>,
}

/// Client → Server task_result payload。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// V4 skill_exec 回报：来源技能集
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    /// V4 skill_exec 回报：工具名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// V4 skill_exec 回报：stdout 的 sha256（跨副本作弊检测用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_sha256: Option<String>,
    /// V4 skill_exec 回报：stderr 尾部（最多 2 KiB，出错时辅助定位）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
}
