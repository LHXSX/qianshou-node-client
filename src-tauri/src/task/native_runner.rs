//! Native binary 执行器 · V8.2 RFC (2026-06-11) 节点执行层重构
//!
//! 路径:
//!   服务端 task_registry 标记 executor=NATIVE + native_binary + native_args
//!   → 节点不开 Python 解释器 · 直接 tokio::process 调内置/已装 binary
//!
//! 支持的 binary (从 platform_v8/engine/task_registry.required_native_binaries):
//!   ffmpeg / ffprobe       视频/音频转码/取信息 (内置 · prebake-runtime.sh)
//!   pdftotext / pdfinfo    PDF 提文/元信息 (poppler · prebake-natives.sh · WIP)
//!   vips                   图像变换 (libvips · prebake-natives.sh · WIP)
//!   whisper-cli            语音转文字 (whisper.cpp · prebake-natives.sh · WIP)
//!
//! 协议:
//!   task.executor       = "native"
//!   task.native_binary  = "ffmpeg"
//!   task.native_args    = ["-i", "{input}", "-c:v", "libx264", "{output}"]
//!     占位符:
//!       {input}   → 单文件输入路径 (single_file 下载到 tempdir)
//!       {output}  → 输出文件路径 (脚本侧建议 · {tempdir}/output.<ext>)
//!       {tempdir} → 临时工作目录 (multi_file 输入也在此)
//!   task.args.input_kind, input_ref, input_refs, params, slice_meta · 同 script 协议
//!
//! 输出:
//!   binary stdout → JSON {"ok":true,"output":"...","output_url":"oss://..."}
//!   或 binary 写到 {output} 文件 · 客户端读出来 base64 inline 回报(小文件) /
//!     上传 OSS 回报 output_url(大文件 · 未来 · MVP 阶段 inline)
//!
//! 失败处理:
//!   - 找不到 binary 路径 → Err(missing_dep="<binary>") · 节点 fallback python3
//!   - exit_code != 0    → RunOutcome{ok=false, stderr_tail 带 binary 报错}
//!   - 超时              → tokio::time::timeout 触发 · kill 子进程

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::process::Command;

use super::executor::RunOutcome;
use super::resource_limit;
use super::{TaskAssign};

/// 临时目录前缀
const TEMPDIR_PREFIX: &str = "ec-native-";

/// 输入文件最大字节数 (200MB) · 防止单 shard 拖垮节点
const MAX_INPUT_BYTES: u64 = 200 * 1024 * 1024;

/// V8.2 · native binary 执行入口
///
/// 流程:
///   1. paths::find_native_binary 找 binary 绝对路径(内置/tier_root/system PATH)
///   2. 准备 tempdir + 下载 input(按 input_kind)
///   3. 渲染 native_args 占位符 ({input} / {output} / {tempdir})
///   4. tokio::process::Command 跑 · 带超时 · UTF-8 IO 锁定
///   5. exit_code == 0 + output 文件存在 → 读取 → 写入 stdout JSON 回报
pub async fn run_native(task: &TaskAssign, timeout: Duration) -> Result<RunOutcome> {
    // ── 1. 找 binary 路径 ─────────────────────────────────────────
    if task.native_binary.is_empty() {
        return Err(anyhow!("native task 缺 native_binary 字段"));
    }
    let binary_path = match crate::runtime::paths::find_native_binary(&task.native_binary) {
        Some(p) => p,
        None => {
            tracing::warn!(
                "native_runner · binary '{}' 找不到 · 节点 fallback python3",
                task.native_binary
            );
            // 用 anyhow err · executor.rs 接住后 try_run 会重试 python3 路径
            return Err(anyhow!(
                "missing_native_binary:{}",
                task.native_binary
            ));
        }
    };
    tracing::info!(
        "native_runner · task={} binary={} path={:?}",
        task.task_id,
        task.native_binary,
        binary_path
    );

    // ── 2. 准备 tempdir + 下载 input ───────────────────────────────
    let tempdir = tempfile::Builder::new()
        .prefix(TEMPDIR_PREFIX)
        .tempdir()
        .context("创建 native_runner 临时目录失败")?;
    let tempdir_path = tempdir.path().to_path_buf();

    let input_path = prepare_input(task, &tempdir_path).await?;

    // 输出文件 · 客户端预定一个路径 · 由 binary 写入
    let output_path = tempdir_path.join("output");

    // ── 3. 渲染 native_args 占位符 ────────────────────────────────
    let rendered_args = render_args(
        &task.native_args,
        input_path.as_deref(),
        Some(&output_path),
        &tempdir_path,
    );
    tracing::info!(
        "native_runner · args={:?}",
        rendered_args
    );

    // ── 4. 跑 binary ───────────────────────────────────────────────
    let mut cmd = Command::new(&binary_path);
    crate::proc_util::hide_window_tokio(&mut cmd);
    cmd.env("LC_ALL", "C.UTF-8");
    cmd.env("LANG", "C.UTF-8");
    resource_limit::apply(&mut cmd, super::executor::current_throttle_level());
    cmd.args(&rendered_args);
    cmd.current_dir(&tempdir_path);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()
        .with_context(|| format!("启动 {} 失败", task.native_binary))?;

    let wait = child.wait_with_output();
    let output = match tokio::time::timeout(timeout, wait).await {
        Ok(out) => out.with_context(|| format!("{} 等待输出失败", task.native_binary))?,
        Err(_) => {
            return Err(anyhow!(
                "{} 超时 ({}s)",
                task.native_binary,
                timeout.as_secs()
            ));
        }
    };

    let exit_code = output.status.code().unwrap_or(-1);
    let stderr_tail = tail_stderr(&output.stderr, 2048);
    let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();

    // ── 5. 收集 output 文件 ────────────────────────────────────────
    // 模板可能用 {output} (无扩展) 或 {output}.mp4 / .mp3 / .txt 等
    // 这里扫 tempdir 找 output 或 output.<ext> · 优先取带扩展名的(避免 ffmpeg 报错)
    // 后期 P1 · 大文件改 OSS 上传 · 输出 output_url
    let resolved_output = if exit_code == 0 {
        resolve_output_artifact(&tempdir_path, &output_path)
    } else {
        None
    };

    let final_output = if exit_code == 0 && resolved_output.is_some() {
        let resolved = resolved_output.as_ref().expect("just checked");
        let bytes = std::fs::read(resolved)
            .with_context(|| format!("读取 binary 输出失败: {:?}", resolved))?;
        let result_json = serde_json::json!({
            "ok": true,
            "executor": "native",
            "binary": task.native_binary,
            "output_file": resolved.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            "output_size": bytes.len(),
            "output_base64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &bytes,
            ),
        });
        serde_json::to_string(&result_json).unwrap_or_default()
    } else if exit_code == 0 {
        // 没写 output 文件 · stdout 即结果(pdftotext 默认输出到 stdout · ffprobe -of json · 等)
        stdout_str.clone()
    } else {
        // 失败 · output 字段返 stdout (后端 aggregator 看 ok=false + stderr_tail)
        stdout_str.clone()
    };

    Ok(RunOutcome {
        output: final_output,
        exit_code,
        stderr_tail,
        python_used: String::new(),  // native 路径 · 不开 python
    })
}

/// 准备输入 · 简化版只处理 single_file / multi_file / archive / params_only
/// 返回: 单文件输入路径(single_file/archive 解压后选第一个)或 None(multi_file/params_only)
async fn prepare_input(task: &TaskAssign, tempdir: &PathBuf) -> Result<Option<PathBuf>> {
    let input_kind = task
        .args
        .get("input_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("single_file")
        .to_string();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("构建 reqwest 失败")?;

    match input_kind.as_str() {
        "single_file" | "" => {
            let input_ref = task
                .args
                .get("input_ref")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if input_ref.is_empty() {
                return Ok(None);
            }
            if !input_ref.starts_with("http") {
                // 不是 URL · 当成本地路径(罕见 · 内部测试)
                return Ok(Some(PathBuf::from(input_ref)));
            }
            // 推断扩展名
            let ext = input_ref
                .split('?').next().unwrap_or(input_ref)
                .rsplit('.').next().unwrap_or("bin")
                .to_lowercase();
            let safe_ext = if ext.len() > 5 || !ext.chars().all(|c| c.is_alphanumeric()) {
                "bin".to_string()
            } else {
                ext
            };
            let input_path = tempdir.join(format!("input.{}", safe_ext));
            let resp = client
                .get(input_ref)
                .send()
                .await
                .with_context(|| format!("下载 input_ref 失败: {}", input_ref))?;
            if !resp.status().is_success() {
                return Err(anyhow!(
                    "input_ref HTTP {}: {}",
                    resp.status().as_u16(),
                    input_ref
                ));
            }
            if let Some(len) = resp.content_length() {
                if len > MAX_INPUT_BYTES {
                    return Err(anyhow!(
                        "input_ref 太大 ({} bytes > {} MB limit)",
                        len, MAX_INPUT_BYTES / 1024 / 1024
                    ));
                }
            }
            let bytes = resp.bytes().await.context("读取 input_ref 响应失败")?;
            if (bytes.len() as u64) > MAX_INPUT_BYTES {
                return Err(anyhow!(
                    "input_ref 太大 ({} bytes 解压后 > {} MB limit)",
                    bytes.len(), MAX_INPUT_BYTES / 1024 / 1024
                ));
            }
            std::fs::write(&input_path, &bytes)
                .with_context(|| format!("写入 input 到 {:?}", input_path))?;
            tracing::info!(
                "native_runner · single_file input · {} bytes → {:?}",
                bytes.len(), input_path
            );
            Ok(Some(input_path))
        }
        "multi_file" => {
            // 下载所有 URL 到 tempdir · binary 用 {tempdir} 占位符指向目录
            let urls: Vec<String> = task
                .args
                .get("input_refs")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if urls.is_empty() {
                return Err(anyhow!("multi_file 任务没有 input_refs"));
            }
            for (i, url) in urls.iter().enumerate() {
                let fname = url
                    .split('?').next().unwrap_or(url)
                    .rsplit('/').next().unwrap_or("file")
                    .to_string();
                let safe = format!("{:03}-{}", i, fname);
                let dst = tempdir.join(&safe);
                let resp = client
                    .get(url)
                    .send()
                    .await
                    .with_context(|| format!("multi_file #{} 下载失败: {}", i, url))?;
                if !resp.status().is_success() {
                    return Err(anyhow!(
                        "multi_file #{} HTTP {}",
                        i, resp.status().as_u16()
                    ));
                }
                let bytes = resp.bytes().await.context("multi_file 读响应失败")?;
                std::fs::write(&dst, &bytes)
                    .with_context(|| format!("multi_file 写入 {:?}", dst))?;
            }
            tracing::info!(
                "native_runner · multi_file input · {} 文件 → {:?}",
                urls.len(), tempdir
            );
            Ok(None)  // {tempdir} 占位符指向目录 · 不用 {input}
        }
        "archive" => {
            // 跟 run_script 同逻辑 · 简化版
            let url = task
                .args
                .get("input_ref")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("archive 任务缺 input_ref"))?
                .to_string();
            let resp = client
                .get(&url)
                .send()
                .await
                .with_context(|| format!("archive 下载失败: {}", url))?;
            if !resp.status().is_success() {
                return Err(anyhow!("archive HTTP {}", resp.status().as_u16()));
            }
            let bytes = resp.bytes().await.context("archive 读响应失败")?;
            let cursor = std::io::Cursor::new(bytes.as_ref());
            let mut zip = zip::ZipArchive::new(cursor)
                .context("archive 解压打开失败")?;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).context("zip entry 读失败")?;
                if entry.is_dir() {
                    continue;
                }
                let dst = tempdir.join(entry.mangled_name());
                if let Some(p) = dst.parent() {
                    std::fs::create_dir_all(p).ok();
                }
                let mut f = std::fs::File::create(&dst)
                    .with_context(|| format!("zip 写入 {:?}", dst))?;
                std::io::copy(&mut entry, &mut f).context("zip 写入流失败")?;
            }
            tracing::info!(
                "native_runner · archive input · {} entries → {:?}",
                zip.len(), tempdir
            );
            Ok(None)
        }
        "params_only" | "inline" => Ok(None),
        other => {
            tracing::warn!(
                "native_runner · 未知 input_kind={} · 当 single_file",
                other
            );
            Ok(None)
        }
    }
}

/// 解析实际产出的 output 文件路径
///
/// 模板里可能写 `{output}` (无扩展名) 或 `{output}.mp4` / `{output}.mp3` 等
/// → 解析顺序:
///   1. 精确路径 (无扩展名 · 老脚本兼容) — 命中则返
///   2. 同目录 prefix=output. 的文件 — 取最近修改的那个 (ffmpeg / vips 等带扩展名的)
fn resolve_output_artifact(
    tempdir: &std::path::Path,
    output_base: &std::path::Path,
) -> Option<std::path::PathBuf> {
    if output_base.is_file() {
        return Some(output_base.to_path_buf());
    }
    let base_name = output_base.file_name()?.to_string_lossy().to_string();
    // 扫描 tempdir 找 base_name.<ext>
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    let rd = std::fs::read_dir(tempdir).ok()?;
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let fname = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // 接受 output / output.<ext>
        if fname != base_name && !fname.starts_with(&format!("{}.", base_name)) {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &best {
            Some((t, _)) if *t >= mtime => {}
            _ => best = Some((mtime, path)),
        }
    }
    best.map(|(_, p)| p)
}

/// 渲染 native_args 占位符 · 仅在 3 个固定位置替换 · 防注入
fn render_args(
    args: &[String],
    input: Option<&std::path::Path>,
    output: Option<&std::path::Path>,
    tempdir: &std::path::Path,
) -> Vec<String> {
    let tempdir_s = tempdir.to_string_lossy().to_string();
    let input_s = input.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let output_s = output.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    args.iter()
        .map(|a| {
            a.replace("{tempdir}", &tempdir_s)
                .replace("{input}", &input_s)
                .replace("{output}", &output_s)
        })
        .collect()
}

/// 截 stderr 尾部 (同 executor.rs 的 tail_stderr)
fn tail_stderr(raw: &[u8], max_bytes: usize) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let s = String::from_utf8_lossy(raw);
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let start = s.len() - max_bytes;
    let mut start = start;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

// base64 编码用 · executor.rs 已 import 过 · 这里 mod 内重新引一下
use base64::Engine as _;

// ════════════════════════════════════════════════════════════════════
// 单元测试 + 真 ffmpeg e2e
// ════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn make_task(
        task_id: &str,
        task_type: &str,
        binary: &str,
        args: Vec<&str>,
        input_path: &str,
    ) -> TaskAssign {
        TaskAssign {
            task_id: task_id.to_string(),
            task_type: task_type.to_string(),
            runner: String::new(),
            runtime: String::new(),
            code_url: String::new(),
            args: serde_json::json!({
                "input_kind": "single_file",
                "input_ref": input_path,
            }),
            timeout_s: 60,
            reward: 0.0,
            skill_id: None,
            tool: None,
            prompt: None,
            tools: None,
            output_schema: None,
            skill_pack_id: None,
            required_tier: String::new(),
            fallback_tiers: Vec::new(),
            executor: "native".to_string(),
            native_binary: binary.to_string(),
            native_args: args.into_iter().map(String::from).collect(),
            onnx_model: String::new(),
        }
    }

    #[test]
    fn test_render_args_basic() {
        let tempdir = Path::new("/tmp/x");
        let input = Path::new("/tmp/x/input.mp4");
        let output = Path::new("/tmp/x/output");
        let raw = vec![
            "-i".to_string(),
            "{input}".to_string(),
            "{output}.mp4".to_string(),
        ];
        let rendered = render_args(&raw, Some(input), Some(output), tempdir);
        assert_eq!(rendered[0], "-i");
        assert_eq!(rendered[1], "/tmp/x/input.mp4");
        assert_eq!(rendered[2], "/tmp/x/output.mp4");
    }

    #[test]
    fn test_resolve_output_artifact_exact() {
        let td = tempfile::tempdir().unwrap();
        let exact = td.path().join("output");
        std::fs::write(&exact, b"hi").unwrap();
        let found = resolve_output_artifact(td.path(), &exact).unwrap();
        assert_eq!(found, exact);
    }

    #[test]
    fn test_resolve_output_artifact_with_extension() {
        // 模板里 {output}.mp4 → ffmpeg 实际写到 output.mp4
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("output");
        let actual = td.path().join("output.mp4");
        std::fs::write(&actual, b"fake mp4").unwrap();
        let found = resolve_output_artifact(td.path(), &base).unwrap();
        assert_eq!(found, actual);
    }

    #[test]
    fn test_resolve_output_artifact_none() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("output");
        std::fs::write(td.path().join("other.txt"), b"x").unwrap();
        assert!(resolve_output_artifact(td.path(), &base).is_none());
    }

    // ─────────────────────────────────────────────────────────────
    // E2E · 真 ffmpeg + 真 mp4 跑 video_compress
    //
    // 跑法:
    //   FFMPEG_BIN_DIR=$PWD/.local_models/ffmpeg \
    //   VIDEO_NATIVE_TEST_INPUT=$PWD/.local_models/test_input.mp4 \
    //   cargo test --no-default-features --features onnx --lib \
    //              task::native_runner::tests::test_video_compress_e2e_real -- --nocapture
    //
    // 两个 ENV 任一缺失 → 测试 skip · CI 不强求
    // ─────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn test_video_compress_e2e_real() {
        let ffmpeg_dir = match std::env::var("FFMPEG_BIN_DIR") {
            Ok(d) if Path::new(&d).is_dir() => d,
            _ => {
                eprintln!("⏭ FFMPEG_BIN_DIR 未配置或目录不存在 · skip e2e");
                return;
            }
        };
        let test_video = match std::env::var("VIDEO_NATIVE_TEST_INPUT") {
            Ok(p) if Path::new(&p).is_file() => p,
            _ => {
                eprintln!("⏭ VIDEO_NATIVE_TEST_INPUT 未配置或不存在 · skip e2e");
                return;
            }
        };

        // 临时把 ffmpeg_dir 加到 PATH · which::which 能找到
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", ffmpeg_dir, old_path),
        );

        eprintln!("\n═══════ video_compress e2e ═══════");
        eprintln!("ffmpeg_dir = {}", ffmpeg_dir);
        eprintln!("test_video = {}", test_video);
        let original_size = std::fs::metadata(&test_video).unwrap().len();
        eprintln!("input size = {} bytes ({:.1} KB)", original_size, original_size as f64 / 1024.0);

        // 模拟 task_registry → native_args_templates 渲染后的实际 args
        // (跟 _args_video_compress(crf=32, preset=ultrafast) 一致)
        let args = vec![
            "-y", "-i", "{input}",
            "-c:v", "libx264", "-preset", "ultrafast", "-crf", "32",
            "-c:a", "aac", "-b:a", "96k",
            "{output}.mp4",
        ];
        let task = make_task("e2e-vc-001", "video_compress", "ffmpeg", args, &test_video);

        let t0 = std::time::Instant::now();
        let result = run_native(&task, Duration::from_secs(60)).await
            .expect("run_native 返 Err");
        let elapsed_ms = t0.elapsed().as_millis();

        // 还原 PATH(其他测试用)
        std::env::set_var("PATH", &old_path);

        eprintln!("✓ exit_code = {}", result.exit_code);
        eprintln!("✓ 总耗时 {} ms", elapsed_ms);
        if !result.stderr_tail.is_empty() {
            let preview = if result.stderr_tail.len() > 500 {
                &result.stderr_tail[result.stderr_tail.len() - 500..]
            } else {
                result.stderr_tail.as_str()
            };
            eprintln!("─── stderr tail (500B) ───\n{}\n──────────────────────", preview);
        }

        assert_eq!(result.exit_code, 0, "ffmpeg 失败 · stderr: {}", result.stderr_tail);

        // 输出应是 JSON · 含 output_base64 + output_size
        let parsed: serde_json::Value = serde_json::from_str(&result.output)
            .expect("output 不是合法 JSON");
        assert_eq!(parsed["ok"], serde_json::json!(true));
        assert_eq!(parsed["executor"], serde_json::json!("native"));
        assert_eq!(parsed["binary"], serde_json::json!("ffmpeg"));
        let output_file = parsed["output_file"].as_str().expect("output_file 字段缺");
        let output_size = parsed["output_size"].as_u64().expect("output_size 字段缺");
        let b64_data = parsed["output_base64"].as_str().expect("output_base64 字段缺");

        eprintln!("✓ output_file = {}", output_file);
        eprintln!("✓ output_size = {} bytes ({:.1} KB)",
            output_size, output_size as f64 / 1024.0);
        eprintln!("✓ 压缩率 = {:.1}%",
            100.0 - (output_size as f64 / original_size as f64) * 100.0);

        // 断言:
        //   1. 输出文件名是 output.mp4
        //   2. 输出 base64 解码后是合法 mp4(开头 4 字节 ftyp 之类)
        assert_eq!(output_file, "output.mp4", "expected output.mp4 · got {}", output_file);
        let decoded = base64::engine::general_purpose::STANDARD.decode(b64_data)
            .expect("base64 解码失败");
        assert_eq!(decoded.len() as u64, output_size);
        // MP4 magic: 字节 4-7 是 "ftyp"
        let magic = &decoded[4..8];
        assert_eq!(magic, b"ftyp", "输出不是合法 MP4 · 前 8 字节: {:?}", &decoded[..8]);

        eprintln!("✓ 输出是合法 MP4 (ftyp magic)");
    }

    /// 同 e2e · audio_extract · ffmpeg -vn -c:a libmp3lame
    #[tokio::test]
    async fn test_audio_extract_e2e_real() {
        let ffmpeg_dir = match std::env::var("FFMPEG_BIN_DIR") {
            Ok(d) if Path::new(&d).is_dir() => d,
            _ => { eprintln!("⏭ skip"); return; }
        };
        let test_video = match std::env::var("VIDEO_NATIVE_TEST_INPUT") {
            Ok(p) if Path::new(&p).is_file() => p,
            _ => { eprintln!("⏭ skip"); return; }
        };
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", ffmpeg_dir, old_path));

        eprintln!("\n═══════ audio_extract e2e ═══════");
        let args = vec![
            "-y", "-i", "{input}", "-vn",
            "-acodec", "libmp3lame", "-ab", "192k",
            "{output}.mp3",
        ];
        let task = make_task("e2e-ae-001", "audio_extract", "ffmpeg", args, &test_video);
        let result = run_native(&task, Duration::from_secs(60)).await.unwrap();
        std::env::set_var("PATH", &old_path);

        assert_eq!(result.exit_code, 0, "ffmpeg 失败: {}", result.stderr_tail);
        let parsed: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        let output_file = parsed["output_file"].as_str().unwrap();
        let output_size = parsed["output_size"].as_u64().unwrap();
        eprintln!("✓ output_file = {} · size = {} bytes ({:.1} KB)",
            output_file, output_size, output_size as f64 / 1024.0);
        assert_eq!(output_file, "output.mp3");
        // MP3 magic: ID3 头(ID3v2 tag)或 0xFFEx (帧头)
        let b64 = parsed["output_base64"].as_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let is_mp3 = decoded.starts_with(b"ID3") || (decoded.len() >= 2 && decoded[0] == 0xFF && (decoded[1] & 0xE0) == 0xE0);
        assert!(is_mp3, "输出不是合法 MP3 · 前 8 字节: {:?}", &decoded[..8.min(decoded.len())]);
        eprintln!("✓ 输出是合法 MP3");
    }

    /// 同 e2e · video_info · ffprobe -of json (stdout 直接是 JSON)
    #[tokio::test]
    async fn test_video_info_e2e_real() {
        let ffmpeg_dir = match std::env::var("FFMPEG_BIN_DIR") {
            Ok(d) if Path::new(&d).is_dir() => d,
            _ => { eprintln!("⏭ skip"); return; }
        };
        let test_video = match std::env::var("VIDEO_NATIVE_TEST_INPUT") {
            Ok(p) if Path::new(&p).is_file() => p,
            _ => { eprintln!("⏭ skip"); return; }
        };
        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", ffmpeg_dir, old_path));

        eprintln!("\n═══════ video_info e2e (ffprobe) ═══════");
        let args = vec![
            "-v", "error", "-show_format", "-show_streams",
            "-of", "json", "{input}",
        ];
        let task = make_task("e2e-vi-001", "video_info", "ffprobe", args, &test_video);
        let result = run_native(&task, Duration::from_secs(30)).await.unwrap();
        std::env::set_var("PATH", &old_path);

        assert_eq!(result.exit_code, 0, "ffprobe 失败: {}", result.stderr_tail);
        // ffprobe 没写 output 文件 · stdout 即结果 JSON
        let probe: serde_json::Value = serde_json::from_str(&result.output)
            .expect("ffprobe 输出不是 JSON");
        assert!(probe["format"].is_object(), "缺 format 字段");
        assert!(probe["streams"].is_array(), "缺 streams 字段");
        let duration = probe["format"]["duration"].as_str().unwrap_or("0.0");
        let streams: Vec<String> = probe["streams"].as_array().unwrap()
            .iter()
            .map(|s| s["codec_name"].as_str().unwrap_or("?").to_string())
            .collect();
        eprintln!("✓ duration = {}s · streams = {:?}", duration, streams);
        assert!(streams.contains(&"h264".to_string()));
        assert!(streams.contains(&"aac".to_string()));
    }
}
