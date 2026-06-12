pub mod control;       // 2026-06-05 · 后端下发 control 白名单执行器 (自愈)
pub mod executor;
pub mod failure_class;
pub mod llm_ollama;
pub mod llm_runtime;
pub mod native_runner;  // V8.2 (2026-06-11 RFC) · native binary 直调 (ffmpeg / vips / poppler / ...)
pub mod onnx_runner;    // V8.2 (2026-06-11 RFC) · ONNX 推理直推 (RapidOCR / CLIP / bge)
#[cfg(feature = "onnx")]
pub mod rapid_ocr;      // V8.2 (2026-06-11 RFC) · RapidOCR PP-OCRv4 真实推理实现
pub mod pull_worker;  // W1-7 · 节点端 PULL 模式后台抢任务
pub mod resource_limit;
pub mod self_heal;
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

    /// V8.2 (2026-06-11 RFC 节点执行层重构) · 推荐执行器
    ///   "" / "python3" (默认) → code_url 拉脚本 + venv 跑 (现状)
    ///   "native"  → tokio::process 直调 native_binary (ffmpeg/vips/...)
    ///   "onnx"    → ort crate 直推 onnx_model (RapidOCR/CLIP/...)
    ///   "http"    → reqwest 调平台 API (llm_chat/embedding/crawl)
    /// 老服务端不发 · 客户端 ignore · 走老 python3 路径 · 完全向后兼容
    /// 客户端若不支持(supported_executors 不含此值) · 也走 python3 兜底
    #[serde(default)]
    pub executor: String,
    /// V8.2 · executor=native 时指明该 binary 逻辑名 (ffmpeg / vips / pdftotext / ...)
    /// 客户端 paths::find_native_binary 按此查找
    #[serde(default)]
    pub native_binary: String,
    /// V8.2 · executor=native 时的命令行参数列表 (服务端预渲染好 · 客户端不二次替换 · 防注入)
    /// 支持占位符: {input} {output} {tempdir} (客户端 native_runner 仅在此 3 个占位符上做替换)
    #[serde(default)]
    pub native_args: Vec<String>,
    /// V8.2 · executor=onnx 时指明该 model 逻辑名 (rapid_ocr_v1 / clip_vit_b32_v1 / ...)
    /// 客户端 onnx_runner 从 ~/.qianshou/runtime/onnx/<onnx_model>/ load 模型
    #[serde(default)]
    pub onnx_model: String,
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
    /// 8.1.8 (2026-06-03) · 实际启动的 Python 解释器绝对路径 (Win 103 关键证据)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python_used: Option<String>,
    /// 8.1.8 · 失败分类 (env_missing_pkg/env_broken_venv/resource_oom/script_error/...)
    /// 后端 aggregator 据此区分对待:环境问题不判节点"不胜任",硬件不足才降级
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    /// 8.1.8 · 缺失依赖名 (pip 包名 / 系统工具名) · 供后端记录 + 节点自愈
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing_dep: Option<String>,
}
