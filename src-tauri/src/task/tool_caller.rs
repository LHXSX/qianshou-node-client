//! V4 零模型工具调用器（单 shot 版）。
//!
//! 设计原则（详见 docs/v4/DECISIONS.md ADR-009）：
//! - 节点不装 LLM，只是个"subprocess 调度器"
//! - 中央 Hermes 已经在 plan 阶段决定好 (skill_id, tool, args)
//! - 本模块只负责：找到 entry_file → spawn python3 → 喂 stdin → 拿 stdout
//! - 工具确定性 → 同输入同输出 → result_sha256 = 跨副本作弊检测的命根子
//!
//! 协议：
//! - stdin：args_json（UTF-8）
//! - stdout：result_json（UTF-8，由工具保证）
//! - stderr：诊断信息（保留尾部最多 2 KiB）
//! - exit_code：0 = 成功；非 0 = 工具内部错误

use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::skill_registry::{self, sha256_hex, Skill, Tool};

/// stdout 最多保留 16 KiB（防止工具不当输出爆内存）。
pub const STDOUT_LIMIT_BYTES: usize = 16 * 1024;
/// stderr 尾部最多 2 KiB（出错时辅助定位）。
pub const STDERR_TAIL_BYTES: usize = 2 * 1024;
/// 全局硬上限：工具单次执行不超过 5 分钟。
const HARD_TIMEOUT_S: u64 = 300;

/// 工具调用结果（成功路径）。
#[derive(Debug, Clone)]
pub struct ToolCallOutput {
    pub stdout: String,
    pub stderr_tail: Option<String>,
    pub exit_code: i32,
    pub result_sha256: String,
}

/// 在全局 SkillRegistry 中找工具并执行（无任务级派生）。
///
/// `skill_id_hint`：可选过滤（如 "text-tools-v1" 或 "text-tools-v1@1.0.0"）。
/// 若为 None，按 tool_name 全局查找（要求 tool 名跨技能集唯一）。
pub async fn call_tool(
    skill_id_hint: Option<&str>,
    tool_name: &str,
    args_json: &str,
    timeout_override: Option<Duration>,
) -> Result<ToolCallOutput> {
    call_tool_with_override(skill_id_hint, tool_name, args_json, timeout_override, None).await
}

/// V8: 在全局 SkillRegistry 中找工具，但用 `entry_file_override` 替换 entry_file 来跑。
///
/// 用途：任务级 fork 派生（task_skill_pack）。executor 先拉到 fork 的 runner.py，
/// 再用此函数让它真跑起来；current_dir 仍是原 skill bundle 目录（保留 import 路径）。
pub async fn call_tool_with_override(
    skill_id_hint: Option<&str>,
    tool_name: &str,
    args_json: &str,
    timeout_override: Option<Duration>,
    entry_file_override: Option<PathBuf>,
) -> Result<ToolCallOutput> {
    // 2026-05-28 · global() 现返 Arc<SkillRegistry> · 通过 &*reg 拿 &SkillRegistry 给 locate_tool
    //              reg 持续到函数返回 · skill/tool 借用安全跨 await
    let reg = skill_registry::global();
    let (skill, tool) = locate_tool(&reg, skill_id_hint, tool_name)
        .ok_or_else(|| anyhow!("找不到工具: skill={:?}, tool={}", skill_id_hint, tool_name))?;

    let timeout = timeout_override
        .unwrap_or_else(|| Duration::from_secs(tool.timeout_s))
        .min(Duration::from_secs(HARD_TIMEOUT_S));

    let entry: &Path = entry_file_override.as_deref().unwrap_or(&tool.entry_file);
    spawn_python_tool(skill, tool, entry, args_json, timeout).await
}

/// 把 (skill_id_hint, tool_name) 解析为 (Skill, Tool)。
fn locate_tool<'a>(
    reg: &'a skill_registry::SkillRegistry,
    skill_id_hint: Option<&str>,
    tool_name: &str,
) -> Option<(&'a Skill, &'a Tool)> {
    if let Some(hint) = skill_id_hint {
        // 支持 "text-tools-v1" 或 "text-tools-v1@1.0.0"
        let id = hint.split('@').next().unwrap_or(hint);
        let skill = reg.get(id)?;
        let tool = skill.tools.iter().find(|t| t.name == tool_name)?;
        Some((skill, tool))
    } else {
        reg.find_tool(tool_name)
    }
}

/// 实际 spawn python3 跑工具。
///
/// `entry_file` 可能是 bundle 的（无 fork）或 task_skill_pack 的（有 fork）。
/// `current_dir` 始终是 bundle 目录，保留 import 路径。
async fn spawn_python_tool(
    skill: &Skill,
    tool: &Tool,
    entry_file: &Path,
    args_json: &str,
    timeout: Duration,
) -> Result<ToolCallOutput> {
    if !entry_file.exists() {
        return Err(anyhow!(
            "entry_file 不存在: {} (skill {} tool {})",
            entry_file.display(),
            skill.id_versioned(),
            tool.name,
        ));
    }

    // V8.1 (2026-05-27) · 跟 executor.rs 共用路由 · 优先用 venvs/<tier>/bin/python
    // skill_exec 不带 required_tier (skill 元数据未来可扩展) · 走默认 lite 兜底
    let (python_bin, bundled_pp) = crate::runtime::paths::pick_python_with_hint(None, &[]);
    tracing::info!(
        "tool_caller · skill={} tool={} python={}",
        skill.id, tool.name, python_bin
    );
    let mut command = Command::new(&python_bin);
    crate::proc_util::hide_window_tokio(&mut command);
    command
        .arg(entry_file)
        .current_dir(&skill.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // 老路径 fallback 时塞 PYTHONPATH (cpython + envs/{image,base})
    if !bundled_pp.is_empty() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let mut parts: Vec<String> = bundled_pp
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if let Ok(existing) = std::env::var("PYTHONPATH") {
            if !existing.is_empty() {
                parts.push(existing);
            }
        }
        command.env("PYTHONPATH", parts.join(sep));
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("启动 python ({}) 失败 (skill={} tool={})", python_bin, skill.id, tool.name))?;

    // 喂 stdin
    if let Some(mut stdin) = child.stdin.take() {
        let payload = args_json.as_bytes();
        stdin
            .write_all(payload)
            .await
            .context("写工具 stdin 失败")?;
        drop(stdin); // close → 工具能 EOF
    }

    let wait = tokio::time::timeout(timeout, child.wait_with_output()).await;
    let output = match wait {
        Err(_) => {
            return Err(anyhow!(
                "工具执行超时（{} 秒）: skill={} tool={}",
                timeout.as_secs(),
                skill.id_versioned(),
                tool.name
            ));
        }
        Ok(Err(io_err)) => return Err(anyhow!("等待子进程失败: {}", io_err)),
        Ok(Ok(o)) => o,
    };

    // 截 stdout 到 16 KiB（按 char boundary 安全切）
    let stdout = clamp_utf8(&output.stdout, STDOUT_LIMIT_BYTES);
    let exit_code = output.status.code().unwrap_or(-1);
    let stderr_tail = if output.stderr.is_empty() {
        None
    } else {
        Some(clamp_utf8_tail(&output.stderr, STDERR_TAIL_BYTES))
    };

    // 在原始（未截断）stdout 上算 sha256，跨副本可比
    let result_sha256 = sha256_hex(&output.stdout);

    Ok(ToolCallOutput {
        stdout,
        stderr_tail,
        exit_code,
        result_sha256,
    })
}

/// 取字节前缀，沿 UTF-8 char boundary 截到 limit 字节以内。
fn clamp_utf8(data: &[u8], limit: usize) -> String {
    if data.len() <= limit {
        return String::from_utf8_lossy(data).into_owned();
    }
    let mut end = limit;
    while end > 0 && (data[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    let mut s = String::from_utf8_lossy(&data[..end]).into_owned();
    s.push_str("\n... [stdout truncated]");
    s
}

/// 取字节尾部，沿 UTF-8 char boundary 截到 limit 字节以内。
fn clamp_utf8_tail(data: &[u8], limit: usize) -> String {
    if data.len() <= limit {
        return String::from_utf8_lossy(data).into_owned();
    }
    let mut start = data.len().saturating_sub(limit);
    while start < data.len() && (data[start] & 0b1100_0000) == 0b1000_0000 {
        start += 1;
    }
    let mut s = String::from("... [stderr head truncated]\n");
    s.push_str(&String::from_utf8_lossy(&data[start..]));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_utf8_short_unchanged() {
        let s = "hello";
        assert_eq!(clamp_utf8(s.as_bytes(), 100), "hello");
    }

    #[test]
    fn clamp_utf8_truncates_at_boundary() {
        // "你好" 是 6 字节（每字 3 字节）
        let s = "你好世界".repeat(100);
        let out = clamp_utf8(s.as_bytes(), 100);
        assert!(out.len() <= 130); // 100 + truncation tag
        assert!(out.ends_with("[stdout truncated]"));
        // 不应有半个 utf-8 字符
        assert!(out.contains("..."));
    }

    #[test]
    fn clamp_utf8_tail_keeps_end() {
        let s = "X".repeat(5000);
        let out = clamp_utf8_tail(s.as_bytes(), 500);
        assert!(out.starts_with("..."));
        assert!(out.ends_with("X"));
    }
}
