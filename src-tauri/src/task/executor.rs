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

// 2026-05-18 · 8 KB 太小 · 单张图 base64 就超过 → 任务输出 JSON 被截断
// v8 任务输出常含 result_image_b64 (单图 ~50KB-2MB) · multi_file 任务更大
// 提到 16 MB · 大于此值的脚本应该上传 OSS 返 URL (不要 inline)
const OUTPUT_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const ALLOWED_RUNTIMES: &[&str] = &["shell", "bash", "sh", "python3", "python", "node"];

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
        Ok((output, exit_code)) => {
            // P0 NCE · shell/script 也算 result_sha256 (反作弊覆盖 100%)
            // 在未截断的完整 output 上算 · 跨副本可比对
            let sha = super::skill_registry::sha256_hex(output.as_bytes());
            TaskResult {
            task_id: task.task_id.clone(),
            ok: exit_code == 0,
            elapsed_ms: start.elapsed().as_millis() as u64,
            output: truncate_output(&output),
            error: if exit_code == 0 {
                None
            } else {
                Some(format!("exit code {}", exit_code))
            },
            exit_code: Some(exit_code),
            skill_id: None,
            tool: None,
            result_sha256: Some(sha),
            stderr_tail: None,
            }
        },
        Err(e) => TaskResult {
            task_id: task.task_id.clone(),
            ok: false,
            elapsed_ms: start.elapsed().as_millis() as u64,
            output: String::new(),
            error: Some(e.to_string()),
            exit_code: None,
            skill_id: None,
            tool: None,
            result_sha256: None,
            stderr_tail: None,
        },
    }
    // 进度由 v8_ws 在收到结果后统一上报 ShardProgress / ShardResult
}

async fn try_run(task: &TaskAssign) -> Result<(String, i32)> {
    let timeout = Duration::from_secs(task.timeout_s.max(1).min(600));

    // 2026-05-18 v8 收口策略:
    // 节点不内置 task_type · 全部通过 code_url 下载 backend 脚本跑
    // (新增 task 只需后端加 .py · 节点零修改 · 真正可扩展)
    //
    // unknown task_type → 自动当作 script 跑 (v8 submit 时 code_url 会自动指向 /api/v8/scripts/{task_type}.py)
    match task.task_type.as_str() {
        "shell" => run_shell(task, timeout).await,
        "script" => run_script(task, timeout).await,
        "llm_infer" => run_llm_infer(task, timeout).await,
        _ => {
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
            stderr_tail,
        },
        Err(e) => TaskResult {
            task_id: task.task_id.clone(),
            ok: false,
            elapsed_ms: start.elapsed().as_millis() as u64,
            output: String::new(),
            error: Some(e.to_string()),
            exit_code: None,
            skill_id,
            tool: Some(tool_name.to_string()),
            result_sha256: None,
            stderr_tail: None,
        },
    }
}

/// 旧 shell 模式：args.cmd 直接 bash -c
async fn run_shell(task: &TaskAssign, timeout: Duration) -> Result<(String, i32)> {
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
fn current_throttle_level() -> ThrottleLevel {
    super::resource_limit::current_level()
}

/// M3.6 script 模式：拉 code_url → 写到临时文件 → 用 runtime 执行
async fn run_script(task: &TaskAssign, timeout: Duration) -> Result<(String, i32)> {
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
    // 2026-05-23 · 优先用内置预烘焙 Python (cpython python3.11)
    // 这样新装客户端的用户不用本机预装 Python 也能跑 image_resize 等任务
    //
    // 实现:
    //   - bundled_runtime_for(&["image","base"]) → (python_bin, PYTHONPATH 列表)
    //   - 注意不能用 envs/*/bin/python (Tauri bundle 会 deref symlink · venv 失效 · 找不到 stdlib)
    //   - 改用 cpython 真二进制 + PYTHONPATH 喂 envs/*/lib/site-packages
    let (runtime_bin, bundled_pythonpath): (String, Vec<std::path::PathBuf>) =
        if runtime == "python3" || runtime == "python" {
            match crate::runtime::paths::bundled_runtime_for(&["image", "base"]) {
                Some((p, paths)) => (p.to_string_lossy().into_owned(), paths),
                None => (runtime.clone(), Vec::new()),
            }
        } else {
            (runtime.clone(), Vec::new())
        };
    let mut command = Command::new(&runtime_bin);
    crate::proc_util::hide_window_tokio(&mut command);
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

    // 5. 接 stdin（必须先 stdin(piped)）
    use std::process::Stdio;
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .with_context(|| format!("启动 runtime 失败: {}", runtime))?;
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
            if !output.stderr.is_empty() {
                let stderr_str = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(
                    "executor · 子进程 stderr ({} 字节): {}",
                    output.stderr.len(),
                    if stderr_str.len() > 500 { &stderr_str[..500] } else { &stderr_str }
                );
            }
            Ok((stdout, output.status.code().unwrap_or(-1)))
        }
    }
}

async fn run_llm_infer(task: &TaskAssign, timeout: Duration) -> Result<(String, i32)> {
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

    Ok((result.content, 0))
}

async fn run_with_timeout(mut command: Command, timeout: Duration) -> Result<(String, i32)> {
    let child_result = tokio::time::timeout(timeout, async {
        let output = command.output().await?;
        Ok::<_, std::io::Error>(output)
    })
    .await;

    match child_result {
        Err(_timeout) => Err(anyhow!("任务执行超时（{}秒）", timeout.as_secs())),
        Ok(Err(io_err)) => Err(anyhow!("启动失败: {}", io_err)),
        Ok(Ok(output)) => {
            let mut combined = String::new();
            combined.push_str(&String::from_utf8_lossy(&output.stdout));
            if !output.stderr.is_empty() {
                if !combined.is_empty() && !combined.ends_with('\n') {
                    combined.push('\n');
                }
                combined.push_str("--- stderr ---\n");
                combined.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            Ok((combined, output.status.code().unwrap_or(-1)))
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
