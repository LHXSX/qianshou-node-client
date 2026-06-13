//! 任务执行器。
//!
//! v3.0.0：仅支持 shell（args.cmd → bash -c ...）
//! v3.1 (M3.6)：新增 script 模式
//!   - task_type=script, runtime=python3/bash/sh/node, code_url=远程脚本 URL
//!   - 客户端下载脚本 → 写到 tempfile → 用 runtime 执行
//!   - args.cmd_extra 作为命令行参数
//!   - args.stdin 作为 stdin 输入
//!
//! 共用：
//!   - 超时强制 kill
//!   - 输出截断到 8 KiB
//!
//! v3.2 计划：wasmtime 沙盒，去掉直接 shell。
//!
//! v4 ⭐ 主推：新增 skill_exec 模式（详见 docs/v4/DECISIONS.md ADR-009）
//!   - task_type=skill_exec, skill_id=技能集 id, tool=工具名, args=工具输入 JSON
//!   - 节点零模型：subprocess 直接跑 Python 工具，stdin/stdout JSON 协议
//!   - 回报含 result_sha256（跨副本作弊检测）
//!
//! v7 高级节点保留：llm_infer 模式
//!   - task_type=llm_infer, prompt=推理提示词
//!   - 通过 ollama daemon 调本地 LLM
//!   - V4 阶段不派此类任务

use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::process::Command;

use super::llm_ollama::OllamaRuntime;
use super::llm_runtime::{LlmInferRequest, LLMRuntime};
use super::resource_limit::{self, ThrottleLevel};
use super::tool_caller::{self, ToolCallOutput};
use super::{TaskAssign, TaskResult};
use super::failure_class;

// 2026-05-18 · 8 KB 太小 · 单张图 base64 就超过 → 任务输出 JSON 被截断
// v8 任务输出常含 result_image_b64 (单图 ~50KB-2MB) · multi_file 任务更大
// 提到 16 MB · 大于此值的脚本应该上传 OSS 返 URL (不要 inline)
const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const ALLOWED_RUNTIMES: &[&str] = &["shell", "bash", "sh", "python3", "python", "node"];

/// 8.1.8 (2026-06-03) · 子进程跑完后的完整诊断信息 (带回 stderr/python_used)
/// 为了让 Win 节点 exit 1/103 这种"只有数字没原因"的失败可定位
pub struct RunOutcome {
    pub output: String,
    pub exit_code: i32,
    pub stderr_tail: String,    // 最多 2KB
    pub python_used: String,    // 实际启动的解释器绝对路径
}

impl RunOutcome {
    pub fn plain(output: String, exit_code: i32) -> Self {
        Self { output, exit_code, stderr_tail: String::new(), python_used: String::new() }
    }
}

/// 截 stderr 尾部 · 防 we_shards.error 列爆掉
fn tail_stderr(raw: &[u8], max_bytes: usize) -> String {
    if raw.is_empty() { return String::new(); }
    let s = String::from_utf8_lossy(raw);
    if s.len() <= max_bytes { return s.to_string(); }
    // 从尾部回退到 char 边界
    let start = s.len() - max_bytes;
    let mut start = start;
    while start < s.len() && !s.is_char_boundary(start) { start += 1; }
    s[start..].to_string()
}

pub async fn run_task(task: &TaskAssign) -> TaskResult {
    run_task_with_progress(task, "").await
}

pub async fn run_task_with_progress(task: &TaskAssign, _node_id: &str) -> TaskResult {
    let start = Instant::now();

    // skill_exec 走单独路径（要回报 skill_id / tool / sha256 / stderr_tail）
    if task.task_type == "skill_exec" {
        return run_skill_exec_and_pack(task, start).await;
    }

    match try_run(task).await {
        Ok(outcome) => {
            // P0 NCE · shell/script 也算 result_sha256 (反作弊覆盖 100%)
            // 在未截断的完整 output 上算 · 跨副本可比对
            let sha = super::skill_registry::sha256_hex(outcome.output.as_bytes());
            let ok = outcome.exit_code == 0;
            // 8.1.8 · 失败时分类 (供后端区分对待 + 触发自愈)
            let cls = if ok {
                None
            } else {
                Some(failure_class::classify(
                    Some(outcome.exit_code),
                    &outcome.stderr_tail,
                    &format!("exit code {}", outcome.exit_code),
                ))
            };
            TaskResult {
            task_id: task.task_id.clone(),
            ok,
            elapsed_ms: start.elapsed().as_millis() as u64,
            output: truncate_output(&outcome.output),
            error: if ok {
                None
            } else {
                Some(format!("exit code {}", outcome.exit_code))
            },
            exit_code: Some(outcome.exit_code),
            skill_id: None,
            tool: None,
            result_sha256: Some(sha),
            // 失败时回传 stderr_tail · 成功时不带 (省带宽)
            stderr_tail: if ok || outcome.stderr_tail.is_empty() { None } else { Some(outcome.stderr_tail) },
            python_used: if outcome.python_used.is_empty() { None } else { Some(outcome.python_used) },
            failure_class: cls.as_ref().map(|c| c.class.as_str().to_string()),
            missing_dep: cls.as_ref().and_then(|c| c.missing_dep.clone()),
            }
        },
        Err(e) => {
            let emsg = e.to_string();
            // 8.1.8 · spawn 失败/下载失败/超时等 · 也分类
            let c = failure_class::classify(None, "", &emsg);
            TaskResult {
                task_id: task.task_id.clone(),
                ok: false,
                elapsed_ms: start.elapsed().as_millis() as u64,
                output: String::new(),
                error: Some(emsg),
                exit_code: None,
                skill_id: None,
                tool: None,
                result_sha256: None,
                stderr_tail: None,
                python_used: None,
                failure_class: Some(c.class.as_str().to_string()),
                missing_dep: c.missing_dep.clone(),
            }
        },
    }
    // 进度由 v8_ws 在收到结果后统一上报 ShardProgress / ShardResult
}

async fn try_run(task: &TaskAssign) -> Result<RunOutcome> {
    let timeout = Duration::from_secs(task.timeout_s.max(1).min(600));

    // V8.2 (2026-06-11 RFC 节点执行层重构) · 优先按服务端建议的 executor 走原生路径
    //
    // 路径选择(高 → 低 · 失败自动 fallback):
    //   1. task.executor=="native"  → native_runner (tokio::process 直调 ffmpeg/vips/...)
    //   2. task.executor=="onnx"    → onnx_runner (ort crate 直推 RapidOCR/CLIP/...)
    //   3. task.executor=="http"    → http_runner (P1 · 当前先 fallback script · llm_chat.py 已是 urllib API)
    //   4. fallback (上面任一失败 + 服务端未指定) → 走老 script/shell/skill_exec 路径
    //
    // 关键: native/onnx 失败 (binary 缺 / 模型缺 / feature 关) 时不传播错误 ·
    //       继续走 python3 路径 · 老节点 + 新节点行为完全兼容
    if !task.executor.is_empty() {
        match task.executor.as_str() {
            "native" => {
                match super::native_runner::run_native(task, timeout).await {
                    Ok(outcome) => return Ok(outcome),
                    Err(e) => {
                        tracing::warn!(
                            "executor · native_runner 失败 ({}) · fallback python3 script",
                            e
                        );
                        // continue to legacy path below
                    }
                }
            }
            "onnx" => {
                match super::onnx_runner::run_onnx(task, timeout).await {
                    Ok(outcome) => return Ok(outcome),
                    Err(e) => {
                        tracing::warn!(
                            "executor · onnx_runner 失败 ({}) · fallback python3 script",
                            e
                        );
                        // continue to legacy path below
                    }
                }
            }
            "http" => {
                // P1 · http_runner 暂未实现 · 老脚本(llm_chat.py / embedding.py)已是 urllib API · 走原 script 路径
                tracing::debug!("executor · http executor 暂转 script · llm/embedding 已是 API");
            }
            "python3" | "" => {} // 走老路径
            other => {
                tracing::warn!("executor · 未知 executor='{}' · fallback python3", other);
            }
        }
    }

    // 2026-05-18 v8 收口策略:
    // 节点不内置 task_type · 全部通过 code_url 下载 backend 脚本跑
    // (新增 task 只需后端加 .py · 节点零修改 · 真正可扩展)
    //
    // 2026-05-28 v2 升级 · 增加 v2 本地 entry 快路:
    //   未知 task_type 在进入 script 下载分支前 · 先查本地 skill_registry (装在 ~/.local/lib/edgecompute/skills/)
    //   命中 → 跳过 download · 直接跑 tool.entry_file (低延迟 + 离线可跑 + _runtime 在 sys.path)
    //   未命中 → fall through 到 v1 script 路径 (拉 code_url 跑)
    match task.task_type.as_str() {
        "shell" => run_shell(task, timeout).await,
        "script" => run_script(task, timeout).await,
        "llm_infer" => run_llm_infer(task, timeout).await,
        _ => {
            // V2 本地 entry 优先 (快 · 离线 · 完整 _runtime)
            // 2026-05-28 · global() 现返 Arc<SkillRegistry> · 拆两行让 Arc 跨过 find_tool 借用作用域
            let reg = super::skill_registry::global();
            if let Some((skill, tool)) = reg.find_tool(&task.task_type) {
                let skill_dir = skill.dir.clone();
                let entry_file = tool.entry_file.clone();
                let tool_name = tool.name.clone();
                tracing::info!(
                    "executor.v2 · task_type={} 命中 skill={} entry={:?}",
                    task.task_type, skill.id, entry_file
                );
                return run_v2_skill(task, skill_dir, entry_file, tool_name, timeout).await;
            }
            // 默认走 script 模式 · 适配 v8 提任务 (task_type=dedup_lines/base64_encode/...)
            if !task.code_url.is_empty() {
                run_script(task, timeout).await
            } else {
                Err(anyhow!("unsupported task_type: {} (code_url 也为空)", task.task_type))
            }
        }
    }
}

/// V4 skill_exec 主路径：派单 → 找工具 → subprocess → 回报含 sha256
async fn run_skill_exec_and_pack(task: &TaskAssign, start: Instant) -> TaskResult {
    let skill_id = task.skill_id.clone();
    let tool_name_opt = task.tool.clone();

    // V8: 任务级 fork 派生 — 拉 + 校验 + 落盘 → 替换 runner.py
    let mut entry_override: Option<std::path::PathBuf> = None;
    if let Some(pack_id) = task.skill_pack_id.as_deref() {
        let api_base = std::env::var("EDGECOMPUTE_API_BASE")
            .unwrap_or_else(|_| "https://www.wujisuanli.com".into());
        match super::skill_pack::ensure_runner(&api_base, pack_id).await {
            Ok(path) => {
                tracing::info!(
                    "task {} 使用 skill_pack {} → runner: {:?}",
                    task.task_id, pack_id, path
                );
                entry_override = Some(path);
            }
            Err(e) => {
                tracing::error!("拉取 skill_pack {} 失败: {}", pack_id, e);
                return TaskResult {
                    task_id: task.task_id.clone(),
                    ok: false,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    output: String::new(),
                    error: Some(format!("skill_pack {} fetch/verify failed: {}", pack_id, e)),
                    exit_code: None,
                    skill_id,
                    tool: tool_name_opt,
                    result_sha256: None,
                    stderr_tail: None,
                    python_used: None,
                    failure_class: Some(failure_class::FailureClass::NetworkError.as_str().to_string()),
                    missing_dep: None,
                };
            }
        }
    }

    // 参数预检
    let tool_name = match tool_name_opt.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => {
            return TaskResult {
                task_id: task.task_id.clone(),
                ok: false,
                elapsed_ms: start.elapsed().as_millis() as u64,
                output: String::new(),
                error: Some("skill_exec 缺少 tool 字段".into()),
                exit_code: None,
                skill_id,
                tool: tool_name_opt,
                result_sha256: None,
                stderr_tail: None,
                python_used: None,
                failure_class: Some(failure_class::FailureClass::ScriptError.as_str().to_string()),
                missing_dep: None,
            };
        }
    };

    // args 序列化成 JSON 字符串作为 stdin
    let args_json = serde_json::to_string(&task.args).unwrap_or_else(|_| "{}".into());
    let timeout = if task.timeout_s == 0 {
        None
    } else {
        Some(Duration::from_secs(task.timeout_s))
    };

    let outcome = tool_caller::call_tool_with_override(
        skill_id.as_deref(),
        tool_name,
        &args_json,
        timeout,
        entry_override,
    )
    .await;

    match outcome {
        Ok(ToolCallOutput {
            stdout,
            stderr_tail,
            exit_code,
            result_sha256,
        }) => TaskResult {
            task_id: task.task_id.clone(),
            ok: exit_code == 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
            output: stdout,
            error: if exit_code == 0 {
                None
            } else {
                Some(format!("tool exit {}", exit_code))
            },
            exit_code: Some(exit_code),
            skill_id,
            tool: Some(tool_name.to_string()),
            result_sha256: Some(result_sha256),
            stderr_tail: stderr_tail.clone(),
            python_used: None,
            // 8.1.8 · tool 非零退出时分类
            failure_class: if exit_code == 0 {
                None
            } else {
                Some(failure_class::classify(Some(exit_code), stderr_tail.as_deref().unwrap_or(""), &format!("tool exit {}", exit_code)).class.as_str().to_string())
            },
            missing_dep: if exit_code == 0 {
                None
            } else {
                failure_class::classify(Some(exit_code), stderr_tail.as_deref().unwrap_or(""), "").missing_dep
            },
        },
        Err(e) => {
            let emsg = e.to_string();
            let c = failure_class::classify(None, "", &emsg);
            TaskResult {
            task_id: task.task_id.clone(),
            ok: false,
            elapsed_ms: start.elapsed().as_millis() as u64,
            output: String::new(),
            error: Some(emsg),
            exit_code: None,
            skill_id,
            tool: Some(tool_name.to_string()),
            result_sha256: None,
            stderr_tail: None,
            python_used: None,
            failure_class: Some(c.class.as_str().to_string()),
            missing_dep: c.missing_dep,
        }},
    }
}

/// 旧 shell 模式：args.cmd 直接 bash -c
async fn run_shell(task: &TaskAssign, timeout: Duration) -> Result<RunOutcome> {
    let cmd = task
        .args
        .get("cmd")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing args.cmd"))?;

    #[cfg(unix)]
    let mut command = {
        let mut c = Command::new("/bin/bash");
        c.arg("-c").arg(cmd);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    };

    command.kill_on_drop(true);
    crate::proc_util::hide_window_tokio(&mut command);
    // P0 NCE · 资源限制 (防笔记本卡死) · 从 state 读档位 · 默认 Balanced (nice+10)
    resource_limit::apply(&mut command, current_throttle_level());
    run_with_timeout(command, timeout).await
}

/// 读全局 throttle 档位 (从 state.rs 同步 · UI 可改)
/// AppState 不能被 executor.rs 直读 (避免循环引用) · 用 OnceLock cache
/// commands.rs::set_throttle_level 会调 update_throttle_level() 同步
/// 2026-06-11 · 改 pub(super) · native_runner/onnx_runner 共用 throttle 档位
pub(super) fn current_throttle_level() -> ThrottleLevel {
    super::resource_limit::current_level()
}

/// 2026-05-28 · V2 本地 entry 路径 · 不下载 code_url · 直接跑 ~/.local/lib/edgecompute/skills/<pack>/<file>.py
///
/// 调用前提:
///   - skill_registry::global().find_tool(task_type) 命中
///   - skill_dir = skill 装入目录 (含 _runtime/ 子目录)
///   - entry_file = tool 的 .py 绝对路径
///
/// stdin 协议 (v2 _runtime/context.py SkillContext.from_stdin_and_env):
///   { params, input_kind, inline_input, input_file, input_refs,
///     slice_meta, task_id, shard_id, workload_id, tool_name }
///
/// 节点 task.args 已含后端扣出的这些字段 · 直接透传 · 补上 task_id/tool_name 上下文
async fn run_v2_skill(
    task: &TaskAssign,
    skill_dir: std::path::PathBuf,
    entry_file: std::path::PathBuf,
    tool_name: String,
    timeout: Duration,
) -> Result<RunOutcome> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    // 1. 选 python 候选 (tier 路由 · 逐个 try-spawn)
    let rt_hint = if task.required_tier.is_empty() {
        None
    } else {
        Some(task.required_tier.as_str())
    };
    let candidates = crate::runtime::paths::python_candidates(rt_hint, &task.fallback_tiers);

    // 2. 构造 stdin JSON · 透传 task.args + 补上上下文
    let mut stdin_data = if task.args.is_object() {
        task.args.clone()
    } else {
        serde_json::json!({})
    };
    if let Some(obj) = stdin_data.as_object_mut() {
        obj.entry("task_id".to_string())
            .or_insert(serde_json::Value::String(task.task_id.clone()));
        obj.entry("tool_name".to_string())
            .or_insert(serde_json::Value::String(tool_name.clone()));
    }
    let stdin_bytes = serde_json::to_vec(&stdin_data).context("序列化 v2 stdin 失败")?;

    // 3. spawn python <entry_file> · cwd = skill_dir (sys.path 默认含当前目录 → _runtime 可 import)
    let build_cmd = |python_bin: &str, bundled_pythonpath: &[std::path::PathBuf]| -> Command {
        let mut command = Command::new(python_bin);
        crate::proc_util::hide_window_tokio(&mut command);
        // 8.1.9 · 强制子进程 UTF-8 IO (修 Windows python 默认 GBK → 输出 emoji/中文 exit 1)
        command.env("PYTHONIOENCODING", "utf-8");
        command.env("PYTHONUTF8", "1");
        resource_limit::apply(&mut command, current_throttle_level());
        command.arg(&entry_file);
        command.current_dir(&skill_dir);
        command.kill_on_drop(true);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

        // 4. PYTHONPATH = bundled_pythonpath + skill_dir (多一层保险 _runtime 能 import)
        let sep = if cfg!(windows) { ";" } else { ":" };
        let mut path_parts: Vec<String> = bundled_pythonpath
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        path_parts.push(skill_dir.to_string_lossy().into_owned());
        if let Ok(existing) = std::env::var("PYTHONPATH") {
            if !existing.is_empty() {
                path_parts.push(existing);
            }
        }
        command.env("PYTHONPATH", path_parts.join(sep));

        // 5. 上下文 env vars (v2 context.py 支持 stdin / env 双通道)
        command.env("EC_TASK_ID", &task.task_id);
        command.env("EC_TOOL_NAME", &tool_name);
        if let Some(params) = stdin_data.get("params") {
            command.env("EC_PARAMS", serde_json::to_string(params).unwrap_or_default());
        }
        if let Some(ik) = stdin_data.get("input_kind").and_then(|v| v.as_str()) {
            command.env("EC_INPUT_KIND", ik);
        }
        command
    };

    // 6. 逐个候选 try-spawn (Win venv 起不来自动退下一个) + 喉 stdin
    let mut child_opt = None;
    let mut used_py = String::new();
    let mut last_err = String::new();
    let total = candidates.len();
    for (i, (python_bin, bundled_pythonpath)) in candidates.iter().enumerate() {
        match build_cmd(python_bin, bundled_pythonpath).spawn() {
            Ok(c) => {
                if i > 0 {
                    tracing::warn!(
                        "v2 skill · python 候选 #{} 启动成功 (前 {} 个失败) → {}",
                        i, i, python_bin
                    );
                }
                used_py = python_bin.clone();
                child_opt = Some(c);
                break;
            }
            Err(e) => {
                last_err = format!("{} ({})", python_bin, e);
                tracing::warn!(
                    "v2 skill · python 候选启动失败 [{}/{}]: {}",
                    i + 1, total, last_err
                );
            }
        }
    }
    let mut child = match child_opt {
        Some(c) => c,
        None => {
            return Err(anyhow!(
                "v2 skill 无可用 Python:{} 个候选全部启动失败 (最后: {})",
                total, last_err
            ))
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&stdin_bytes).await;
        drop(stdin);
    }

    // 7. 等结果 · 带 timeout
    let wait = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match wait {
        Err(_) => Err(anyhow!("v2 skill 超时（{}秒）", timeout.as_secs())),
        Ok(Err(e)) => Err(anyhow!("v2 skill 等待子进程失败: {}", e)),
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            // stderr 走 tracing · 不污染业务输出
            let stderr_tail = tail_stderr(&output.stderr, 2048);
            if !output.stderr.is_empty() {
                let s = String::from_utf8_lossy(&output.stderr);
                let preview: String = s.chars().take(500).collect();
                tracing::warn!("v2 skill stderr (task={}): {}", task.task_id, preview);
            }
            Ok(RunOutcome { output: stdout, exit_code, stderr_tail, python_used: used_py })
        }
    }
}

/// M3.6 script 模式：拉 code_url → 写到临时文件 → 用 runtime 执行
async fn run_script(task: &TaskAssign, timeout: Duration) -> Result<RunOutcome> {
    let runtime = if task.runtime.is_empty() {
        "python3".to_string()
    } else {
        task.runtime.to_lowercase()
    };
    if !ALLOWED_RUNTIMES.contains(&runtime.as_str()) {
        return Err(anyhow!("不支持的 runtime: {}", runtime));
    }
    if task.code_url.is_empty() {
        return Err(anyhow!("script 模式缺少 code_url"));
    }

    // 1. 下载脚本（用 reqwest，TLS 用 rustls；超时一半给下载，剩下给执行）
    let dl_timeout = (timeout / 2).max(Duration::from_secs(5));
    let client = reqwest::Client::builder()
        .timeout(dl_timeout)
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("构建 HTTP client 失败")?;
    let resp = client
        .get(&task.code_url)
        .send()
        .await
        .with_context(|| format!("下载脚本失败: {}", task.code_url))?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "下载脚本 HTTP {}: {}",
            resp.status().as_u16(),
            task.code_url
        ));
    }
    let code_bytes = resp.bytes().await.context("读取脚本响应失败")?;
    if code_bytes.is_empty() {
        return Err(anyhow!("脚本内容为空: {}", task.code_url));
    }

    // 2. 写到临时文件（按 runtime 决定后缀）
    let suffix = match runtime.as_str() {
        "python3" | "python" => ".py",
        "node" => ".js",
        "bash" | "sh" => ".sh",
        _ => ".txt",
    };
    let mut tmp = tempfile::Builder::new()
        .prefix("edgec-task-")
        .suffix(suffix)
        .tempfile()
        .context("创建临时文件失败")?;
    tmp.write_all(&code_bytes).context("写脚本内容失败")?;
    tmp.flush().ok();
    let script_path = tmp.path().to_path_buf();
    // 持有 _tmp 直到执行结束
    let _tmp_guard = tmp;

    // 3. 解析参数
    let cmd_extra = task
        .args
        .get("cmd_extra")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut stdin_text = task
        .args
        .get("stdin")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // 2026-05-20 · 二进制安全 stdin · 优先 stdin_bytes (single_file 下载的 binary)
    // 若空 · fallback 用 stdin_text.as_bytes() (老路径文本)
    let mut stdin_bytes: Vec<u8> = Vec::new();

    // 3b. v8 输入准备 · 按 input_kind 选 fetch 策略
    //
    // 协议 (跟 backend platform_v8/engine/task_registry.py 对齐):
    //   inline       → args["stdin"] 直接喂 (已上面处理)
    //   single_file  → fetch args["input_ref"] → stdin
    //   multi_file   → 下载 args["input_refs"] (list) 到临时目录 → ENV EC_INPUT_DIR
    //   archive      → 下载 args["input_ref"] (zip) → 解压 → ENV EC_INPUT_DIR
    //   params_only  → 啥都不喂 · 只用 ENV EC_PARAMS / EC_SLICE_META
    //   stream       → MVP 暂不实现
    //
    // 暴露给脚本的 env vars (统一接口):
    //   EC_INPUT_KIND   = input_kind
    //   EC_INPUT_REF    = single_file URL (调试用)
    //   EC_INPUT_DIR    = multi_file/archive 临时目录路径 (脚本读这个目录处理所有文件)
    //   EC_SLICE_META   = JSON · 切片元数据 (page 范围 / 时段 / 等)
    //   EC_PARAMS       = JSON · 用户传的 params
    let input_kind = task
        .args
        .get("input_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("single_file")
        .to_string();
    let mut env_kv: Vec<(String, String)> = Vec::new();
    env_kv.push(("EC_INPUT_KIND".into(), input_kind.clone()));
    // params / slice_meta 转 JSON 透传
    if let Some(v) = task.args.get("params") {
        env_kv.push(("EC_PARAMS".into(), serde_json::to_string(v).unwrap_or_default()));
    }
    if let Some(v) = task.args.get("slice_meta") {
        env_kv.push(("EC_SLICE_META".into(), serde_json::to_string(v).unwrap_or_default()));
    }
    // _input_dir_guard 持有 TempDir 直到执行结束 · drop 时自动清理
    let mut _input_dir_guard: Option<tempfile::TempDir> = None;

    if stdin_text.is_empty() && stdin_bytes.is_empty() {
        match input_kind.as_str() {
            "single_file" | "" => {
                // 2026-05-20 · 二进制安全 · 用 .bytes() 替代 .text()
                // .text() 会把图片/音频/视频等 binary 强转 UTF-8 · 编码彻底损坏 (PNG/JPG/WAV 失效)
                // 改用 stdin_bytes (Vec<u8>) 直传 stdin · 文本任务一样能跑 (从 stdin 读字符串)
                if let Some(input_ref) = task.args.get("input_ref").and_then(|v| v.as_str()) {
                    if !input_ref.is_empty()
                        && (input_ref.starts_with("http://") || input_ref.starts_with("https://"))
                    {
                        tracing::info!("executor.fetch_input_ref · url={}...", &input_ref[..input_ref.len().min(80)]);
                        let iresp = client
                            .get(input_ref)
                            .send()
                            .await
                            .with_context(|| format!("下载 input_ref 失败: {}", input_ref))?;
                        if !iresp.status().is_success() {
                            return Err(anyhow!(
                                "input_ref HTTP {}: {}",
                                iresp.status().as_u16(),
                                input_ref
                            ));
                        }
                        let bytes = iresp.bytes().await.context("读取 input_ref 响应失败")?;
                        stdin_bytes = bytes.to_vec();
                        env_kv.push(("EC_INPUT_REF".into(), input_ref.to_string()));
                        env_kv.push(("EC_INPUT_BYTES".into(), stdin_bytes.len().to_string()));
                        tracing::info!("executor.fetch_input_ref · OK · {} bytes (binary safe)", stdin_bytes.len());
                    }
                }
            }
            "multi_file" => {
                // 下载多个 URL 到临时目录 · 通过 EC_INPUT_DIR 暴露给脚本
                let urls: Vec<String> = task
                    .args
                    .get("input_refs")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                if urls.is_empty() {
                    return Err(anyhow!("multi_file 任务没有 input_refs"));
                }
                let dir = tempfile::Builder::new()
                    .prefix("ec-multi-")
                    .tempdir()
                    .context("创建 multi_file 临时目录失败")?;
                tracing::info!("executor.fetch_multi · {} files → {:?}", urls.len(), dir.path());
                for (i, url) in urls.iter().enumerate() {
                    let fname = url
                        .split('?').next().unwrap_or(url)
                        .rsplit('/').next().unwrap_or(&format!("file-{}", i))
                        .to_string();
                    // 防止文件名冲突 · 加 index 前缀
                    let safe = format!("{:03}-{}", i, fname);
                    let dst = dir.path().join(&safe);
                    let iresp = client
                        .get(url)
                        .send()
                        .await
                        .with_context(|| format!("multi_file 下载 #{} 失败: {}", i, url))?;
                    if !iresp.status().is_success() {
                        return Err(anyhow!(
                            "multi_file #{} HTTP {}: {}",
                            i, iresp.status().as_u16(), url
                        ));
                    }
                    let bytes = iresp.bytes().await.context("multi_file 读响应失败")?;
                    std::fs::write(&dst, &bytes).with_context(|| format!("multi_file 写入 {:?}", dst))?;
                }
                env_kv.push(("EC_INPUT_DIR".into(), dir.path().to_string_lossy().to_string()));
                _input_dir_guard = Some(dir);
            }
            "archive" => {
                // 下载 zip → 解压到临时目录 → EC_INPUT_DIR
                let url = task
                    .args
                    .get("input_ref")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("archive 任务缺 input_ref"))?
                    .to_string();
                tracing::info!("executor.fetch_archive · {}", &url[..url.len().min(80)]);
                let iresp = client
                    .get(&url)
                    .send()
                    .await
                    .with_context(|| format!("archive 下载失败: {}", url))?;
                if !iresp.status().is_success() {
                    return Err(anyhow!("archive HTTP {}: {}", iresp.status().as_u16(), url));
                }
                let bytes = iresp.bytes().await.context("archive 读响应失败")?;
                let dir = tempfile::Builder::new()
                    .prefix("ec-archive-")
                    .tempdir()
                    .context("创建 archive 临时目录失败")?;
                // 用 zip crate 解压 (Cargo.toml 已有 zip = "2")
                let cursor = std::io::Cursor::new(bytes.as_ref());
                let mut zip = zip::ZipArchive::new(cursor).context("解压 zip 打开失败")?;
                for i in 0..zip.len() {
                    let mut entry = zip.by_index(i).context("zip entry 读失败")?;
                    if entry.is_dir() {
                        continue;
                    }
                    let dst = dir.path().join(entry.mangled_name());
                    if let Some(p) = dst.parent() {
                        std::fs::create_dir_all(p).ok();
                    }
                    let mut f = std::fs::File::create(&dst)
                        .with_context(|| format!("zip 写入 {:?}", dst))?;
                    std::io::copy(&mut entry, &mut f).context("zip 写入流失败")?;
                }
                tracing::info!("executor.fetch_archive · 解压 {} 个 entry → {:?}", zip.len(), dir.path());
                env_kv.push(("EC_INPUT_DIR".into(), dir.path().to_string_lossy().to_string()));
                _input_dir_guard = Some(dir);
            }
            "params_only" => {
                // 不喂 stdin · 只 env vars
                tracing::info!("executor · params_only 任务 · stdin 留空");
            }
            other => {
                tracing::warn!("executor · 未知 input_kind={} · 当 single_file 处理", other);
            }
        }
    }

    // 4. 构造 command：runtime <script> <cmd_extra...>
    //
    // V8.1 (2026-05-27) · 按 task.required_tier 路由到对应 venv
    //
    // 路由优先级 (高 → 低):
    //   1. task.required_tier (后端 v8.1+ 发) → venvs/<tier>/bin/python
    //   2. task.fallback_tiers · 按顺序 try → venvs/<tier>/bin/python
    //   3. venvs/lite (auto-install · 大概率装好) → venvs/lite/bin/python
    //   4. 老路径: bundled_runtime_for(["image","base"]) (旧客户端打包内置 cpython)
    //   5. 最终兜底: 系统 python3
    //
    // 老后端 (v8.0.x) 不发 required_tier · serde 取空 · 直接进 step 3 (venvs/lite)
    // 新客户端首次启动会自动装 lite · 所以 step 3 几乎一定命中
    // 4a. python 候选 (tier 路由 · 逐个 try-spawn · Win venv 起不来自动退下一个)
    let rt_hint = if task.required_tier.is_empty() {
        None
    } else {
        Some(task.required_tier.as_str())
    };
    let candidates: Vec<(String, Vec<std::path::PathBuf>)> =
        if runtime == "python3" || runtime == "python" {
            crate::runtime::paths::python_candidates(rt_hint, &task.fallback_tiers)
        } else {
            vec![(runtime.clone(), Vec::new())]
        };
    use std::process::Stdio;
    // 同一套配置 · 对每个候选 python 重建 Command (env/PATH/PYTHONPATH 一致)
    let build_cmd = |runtime_bin: &str, bundled_pythonpath: &[std::path::PathBuf]| -> Command {
        let mut command = Command::new(runtime_bin);
    crate::proc_util::hide_window_tokio(&mut command);
    // 8.1.9 · 强制子进程 UTF-8 IO —— 修 Windows python stdout 默认 GBK/cp936,
    // 脚本输出 emoji/中文(几乎所有脚本的 summary_text)时 UnicodeEncodeError → exit 1。
    // 这是 "Mac 全成功 · Win 全失败 · 跨版本" 的真正根因。Mac/Linux 默认 UTF-8 不受影响。
    command.env("PYTHONIOENCODING", "utf-8");
    command.env("PYTHONUTF8", "1");
    // P0 NCE · 资源限制 (同 run_shell)
    resource_limit::apply(&mut command, current_throttle_level());
    command.arg(&script_path);
    if !cmd_extra.is_empty() {
        // 简单按空格切（不解析 quote 等复杂场景，dev 阶段足够）
        for arg in cmd_extra.split_whitespace() {
            command.arg(arg);
        }
    }
    // 4b. 注入 v8 统一 env vars (EC_INPUT_KIND / EC_INPUT_DIR / EC_SLICE_META / ...)
    for (k, v) in &env_kv {
        command.env(k, v);
    }
    // 4b.5 注入 PATH · 把所有已装 tier 的 binaries 目录加进子进程 PATH
    //   ~/.qianshou/runtime/tiers/ffmpeg/bin · 让脚本 subprocess.run(["ffmpeg", ...]) 能找到
    //   也支持脚本通过 EC_FFMPEG / EC_TIER_BINARIES_JSON 拿绝对路径
    {
        let installed = crate::runtime::detector::read_installed_meta();
        let mut extra_bin_dirs: Vec<std::path::PathBuf> = Vec::new();
        let mut all_binaries: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
        for (tier_name, tier) in installed.tiers.iter() {
            if !tier.ok { continue; }
            for (bin_name, abs_path) in tier.binaries.iter() {
                all_binaries.insert(bin_name.clone(), abs_path.clone());
                if let Some(parent) = std::path::Path::new(abs_path).parent() {
                    if !extra_bin_dirs.iter().any(|p| p == parent) {
                        extra_bin_dirs.push(parent.to_path_buf());
                    }
                }
            }
            // tier_root/bin 兜底 (二进制 tier 安装时把 bin/ffmpeg 解到这里)
            let tier_bin = crate::runtime::paths::tier_root(tier_name).join("bin");
            if tier_bin.exists() && !extra_bin_dirs.iter().any(|p| p == &tier_bin) {
                extra_bin_dirs.push(tier_bin);
            }
        }
        if !extra_bin_dirs.is_empty() {
            let mut parts: Vec<std::ffi::OsString> = extra_bin_dirs
                .into_iter()
                .map(|p| p.into_os_string())
                .collect();
            if let Some(existing) = std::env::var_os("PATH") {
                parts.push(existing);
            }
            if let Ok(joined) = std::env::join_paths(parts) {
                command.env("PATH", joined);
            }
        }
        // 暴露 EC_FFMPEG 等单点路径 · 老脚本可直接读
        if let Some(p) = all_binaries.get("ffmpeg") {
            command.env("EC_FFMPEG", p);
        }
        if let Some(p) = all_binaries.get("ffprobe") {
            command.env("EC_FFPROBE", p);
        }
        if !all_binaries.is_empty() {
            if let Ok(j) = serde_json::to_string(&all_binaries) {
                command.env("EC_TIER_BINARIES_JSON", j);
            }
        }
        // 8.1.6 · 内置 OCR(双端)· ocr tier 的 tesseract 目录已随上面 binaries→PATH 注入
        //   (pytesseract 默认即可找到 tesseract) · 这里补 TESSDATA_PREFIX 让 tesseract
        //   找到 chi_sim/eng 语言包,并暴露 EC_TESSERACT 绝对路径兜底。
        if let Some(tess) = all_binaries.get("tesseract") {
            command.env("EC_TESSERACT", tess);
            let ocr_root = crate::runtime::paths::tier_root("ocr");
            if ocr_root.join("tessdata").is_dir() {
                command.env("TESSDATA_PREFIX", &ocr_root);
            }
        }
    }
    // 4c. 内置 runtime 用 PYTHONPATH 喂第三方包 (envs/*/lib/python3.11/site-packages)
    //     合并已有 PYTHONPATH (用户/系统级) · 用 OS path separator
    if !bundled_pythonpath.is_empty() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let mut parts: Vec<String> = bundled_pythonpath
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if let Ok(existing) = std::env::var("PYTHONPATH") {
            if !existing.is_empty() {
                parts.push(existing);
            }
        }
        let joined = parts.join(sep);
        command.env("PYTHONPATH", joined);
    }
        command.kill_on_drop(true);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        command
    };

    // 8.2.1 · 4c.5 preflight: 跑任务前先验证候选 python 能否启动
    // 修 "子进程秒崩 + stderr 为空 · 后端只看到 exit code 1" 问题。
    // 用 `python --version` 跑一次 · 失败时拿到完整 OS 错误 / stderr ·
    // 拼进 RunOutcome.stderr_tail 上报。
    let mut preflight_diagnostics: Vec<String> = Vec::new();
    let mut viable: Vec<(String, Vec<std::path::PathBuf>)> = Vec::new();
    for (runtime_bin, bundled_pythonpath) in candidates.iter() {
        use std::process::{Command as StdCommand, Stdio};
        let mut check = StdCommand::new(runtime_bin);
        check.arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        crate::proc_util::hide_window_std(&mut check);
        match check.spawn() {
            Ok(mut child) => {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut ok = false;
                loop {
                    match child.try_wait() {
                        Ok(Some(st)) => {
                            if st.success() {
                                ok = true;
                            } else {
                                let stderr = child.wait_with_output().ok()
                                    .map(|o| String::from_utf8_lossy(&o.stderr).chars().take(300).collect::<String>())
                                    .unwrap_or_default();
                                preflight_diagnostics.push(format!(
                                    "{} → --version exit={} stderr={}",
                                    runtime_bin, st.code().unwrap_or(-1), stderr
                                ));
                            }
                            break;
                        }
                        Ok(None) => {
                            if std::time::Instant::now() > deadline {
                                let _ = child.kill();
                                preflight_diagnostics.push(format!(
                                    "{} → --version 超时 5s · venv 严重损坏", runtime_bin
                                ));
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(30));
                        }
                        Err(e) => {
                            preflight_diagnostics.push(format!("{} → wait 失败: {}", runtime_bin, e));
                            break;
                        }
                    }
                }
                if ok {
                    viable.push((runtime_bin.clone(), bundled_pythonpath.clone()));
                }
            }
            Err(e) => {
                preflight_diagnostics.push(format!("{} → spawn 失败: {}", runtime_bin, e));
            }
        }
    }
    if viable.is_empty() {
        return Err(anyhow!(
            "节点无可用 Python:{} 个候选 preflight 全部失败 (任务前自检拿到的诊断 · 已上报后端):\n{}",
            candidates.len(),
            preflight_diagnostics.join("\n")
        ));
    }
    if !preflight_diagnostics.is_empty() {
        tracing::warn!(
            "executor · preflight 剔除 {} 个不可用候选: {}",
            preflight_diagnostics.len(),
            preflight_diagnostics.join(" | ")
        );
    }

    // 4d. 逐个候选 try-spawn · 第一个能起来的就用 (Win: venv 坏/缺则自动退到下一个)
    let mut child_opt = None;
    let mut used_py = String::new();
    let mut last_err = String::new();
    let total = viable.len();
    for (i, (runtime_bin, bundled_pythonpath)) in viable.iter().enumerate() {
        match build_cmd(runtime_bin, bundled_pythonpath).spawn() {
            Ok(c) => {
                if i > 0 {
                    tracing::warn!(
                        "executor · python 候选 #{} 启动成功 (前 {} 个失败) → {}",
                        i, i, runtime_bin
                    );
                }
                used_py = runtime_bin.clone();
                child_opt = Some(c);
                break;
            }
            Err(e) => {
                last_err = format!("{} ({})", runtime_bin, e);
                tracing::warn!(
                    "executor · python 候选启动失败 [{}/{}]: {}",
                    i + 1, total, last_err
                );
            }
        }
    }
    let mut child = match child_opt {
        Some(c) => c,
        None => {
            return Err(anyhow!(
                "节点无可用 Python:preflight 通过 {} 个候选但 spawn 全失败 (最后: {} | preflight 诊断: {})",
                total, last_err, preflight_diagnostics.join(" | ")
            ))
        }
    };
    tracing::info!("executor · run_script 用 python={} task_type={}", used_py, task.task_type);
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        // 二进制优先 (single_file 下载的 PNG/JPG/WAV 等)
        if !stdin_bytes.is_empty() {
            let _ = stdin.write_all(&stdin_bytes).await;
        } else if !stdin_text.is_empty() {
            // 文本 fallback (dedup_lines 等纯文本任务)
            let _ = stdin.write_all(stdin_text.as_bytes()).await;
        }
        // close stdin → 让脚本能 EOF
        drop(stdin);
    }

    let wait = tokio::time::timeout(timeout, child.wait_with_output()).await;
    match wait {
        Err(_) => Err(anyhow!("任务执行超时（{}秒）", timeout.as_secs())),
        Ok(Err(e)) => Err(anyhow!("等待子进程失败: {}", e)),
        Ok(Ok(output)) => {
            // 2026-05-18 · 只返 stdout (脚本约定 stdout = JSON 结果)
            // stderr 走本地 tracing (PIL warnings 等不污染 output_ref · 避免 aggregator parse 失败)
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let mut stderr_tail = tail_stderr(&output.stderr, 2048);
            let exit_code = output.status.code().unwrap_or(-1);
            if !output.stderr.is_empty() {
                let stderr_str = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    "executor · 子进程 stderr ({} 字节): {}",
                    output.stderr.len(),
                    if stderr_str.len() > 500 { &stderr_str[..500] } else { &stderr_str }
                );
            }
            // 8.2.1 · 失败但 stderr 为空 (Windows venv 启动期 OS 层崩) ·
            // 拼上诊断信息让后端能定位 · 否则后端只看到 "exit code 1" 无从下手
            if exit_code != 0 && output.stderr.is_empty() {
                let mut diag = String::new();
                diag.push_str(&format!(
                    "[client diagnostic · 子进程 exit={} 但 stderr 为空 · 可能 venv 启动期 OS 层崩]\n",
                    exit_code
                ));
                diag.push_str(&format!("python_used={}\n", used_py));
                let pyp = std::path::Path::new(&used_py);
                diag.push_str(&format!("exists={} ", pyp.exists()));
                if let Some(parent) = pyp.parent() {
                    diag.push_str(&format!("parent_exists={} ", parent.exists()));
                }
                if let Some(venv_root) = pyp.parent().and_then(|p| p.parent()) {
                    let cfg = venv_root.join("pyvenv.cfg");
                    diag.push_str(&format!("pyvenv.cfg_exists={}", cfg.exists()));
                    if cfg.exists() {
                        if let Ok(s) = std::fs::read_to_string(&cfg) {
                            let preview: String = s.chars().take(400).collect();
                            diag.push_str(&format!("\npyvenv.cfg={}", preview));
                        }
                    }
                }
                if !stdout.is_empty() {
                    let preview: String = stdout.chars().take(500).collect();
                    diag.push_str(&format!("\nstdout_preview={}", preview));
                }
                stderr_tail = diag;
            }
            Ok(RunOutcome {
                output: stdout,
                exit_code,
                stderr_tail,
                python_used: used_py,
            })
        }
    }
}

async fn run_llm_infer(task: &TaskAssign, timeout: Duration) -> Result<RunOutcome> {
    let prompt = task
        .args
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing args.prompt"))?
        .to_string();

    let tools = task
        .args
        .get("tools")
        .and_then(|v| v.as_str())
        .map(|s| serde_json::from_str(s).context("解析 tools JSON 失败"))
        .transpose()?;

    let output_schema = task
        .args
        .get("output_schema")
        .and_then(|v| v.as_str())
        .map(|s| serde_json::from_str(s).context("解析 output_schema JSON 失败"))
        .transpose()?;

    let request = LlmInferRequest {
        prompt,
        tools,
        output_schema,
        temperature: None,
        max_tokens: None,
    };

    let runtime = OllamaRuntime::with_env();

    let infer_future = runtime.infer(&request);
    let result = tokio::time::timeout(timeout, infer_future)
        .await
        .map_err(|_| anyhow!("LLM 推理超时（{}秒）", timeout.as_secs()))?
        .map_err(|e| anyhow!("LLM 推理失败: {}", e))?;

    Ok(RunOutcome::plain(result.content, 0))
}

async fn run_with_timeout(mut command: Command, timeout: Duration) -> Result<RunOutcome> {
    let child_result = tokio::time::timeout(timeout, async {
        let output = command.output().await?;
        Ok::<_, std::io::Error>(output)
    })
    .await;

    match child_result {
        Err(_timeout) => Err(anyhow!("任务执行超时（{}秒）", timeout.as_secs())),
        Ok(Err(io_err)) => Err(anyhow!("启动失败: {}", io_err)),
        Ok(Ok(output)) => {
            // 8.1.8 · 把 stderr 单独保留到 RunOutcome (output 仍保留 inline 兼容老逻辑)
            let stderr_tail = tail_stderr(&output.stderr, 2048);
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&output.stdout));
            if !output.stderr.is_empty() {
                if !combined.is_empty() && !combined.ends_with('\n') {
                    combined.push('\n');
                }
                combined.push_str("--- stderr ---\n");
                combined.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            Ok(RunOutcome {
                output: combined,
                exit_code: output.status.code().unwrap_or(-1),
                stderr_tail,
                python_used: String::new(),
            })
        }
    }
}

fn truncate_output(s: &str) -> String {
    if s.len() <= OUTPUT_LIMIT_BYTES {
        return s.to_string();
    }
    let mut buf = String::with_capacity(OUTPUT_LIMIT_BYTES + 64);
    // 找到 OUTPUT_LIMIT_BYTES 之内的最后一个 char boundary
    let mut end = OUTPUT_LIMIT_BYTES;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    buf.push_str(&s[..end]);
    buf.push_str("\n... [output truncated]");
    buf
}
