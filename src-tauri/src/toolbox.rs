//! 工具箱 —— 本机 bundle 依赖探测与一键安装。
//!
//! 每个 bundle 在后端 /api/v8/bundles 定义。客户端：
//!   - detect_bundle(deps) → 跑 check 命令，返回每项 installed + version
//!   - install_dep(install_cmd) → 异步跑 install 命令，emit("install_log") 实时输出

use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, Deserialize)]
pub struct DepSpec {
    pub name: String,
    pub check: String,
    #[serde(default)]
    pub install: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepStatus {
    pub name: String,
    pub installed: bool,
    pub version: String,
    pub error: String,
}

/// 一次性探测一组依赖。每项跑 check 命令，看 exit code 和首行输出。
#[tauri::command]
pub async fn detect_deps(deps: Vec<DepSpec>) -> Result<Vec<DepStatus>, String> {
    let mut handles = Vec::new();
    for d in deps {
        let h = tokio::spawn(async move { check_dep(&d).await });
        handles.push(h);
    }
    let mut out = Vec::new();
    for h in handles {
        match h.await {
            Ok(s) => out.push(s),
            Err(e) => out.push(DepStatus {
                name: "?".into(),
                installed: false,
                version: String::new(),
                error: format!("join error: {}", e),
            }),
        }
    }
    Ok(out)
}

async fn check_dep(d: &DepSpec) -> DepStatus {
    // 跨平台 shell: Windows cmd /C · Unix /bin/sh -c
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&d.check);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(&d.check);
        c
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.kill_on_drop(true);
    crate::proc_util::hide_window_tokio(&mut cmd);

    match tokio::time::timeout(std::time::Duration::from_secs(8), cmd.output()).await {
        Ok(Ok(output)) => {
            if output.status.success() {
                let v = String::from_utf8_lossy(&output.stdout).to_string();
                let v = if v.is_empty() {
                    String::from_utf8_lossy(&output.stderr).to_string()
                } else {
                    v
                };
                let first_line = v.lines().next().unwrap_or("").trim().to_string();
                DepStatus {
                    name: d.name.clone(),
                    installed: true,
                    version: first_line,
                    error: String::new(),
                }
            } else {
                DepStatus {
                    name: d.name.clone(),
                    installed: false,
                    version: String::new(),
                    error: format!("exit {}", output.status.code().unwrap_or(-1)),
                }
            }
        }
        Ok(Err(e)) => DepStatus {
            name: d.name.clone(),
            installed: false,
            version: String::new(),
            error: format!("spawn error: {}", e),
        },
        Err(_) => DepStatus {
            name: d.name.clone(),
            installed: false,
            version: String::new(),
            error: "timeout".into(),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
struct InstallLogPayload {
    job_id: String,
    line: String,
    is_stderr: bool,
}

#[derive(Debug, Clone, Serialize)]
struct InstallDonePayload {
    job_id: String,
    success: bool,
    exit_code: i32,
    error: String,
    used_source: String, // 最终成功（或最后失败）使用的源 label
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallSource {
    pub label: String,
    pub cmd: String,
}

const INSTALL_TIMEOUT_SECS: u64 = 600; // 单个源最多 10 分钟

/// 异步执行安装命令。stdout/stderr 实时通过 emit("install_log") 推送。
/// 完成后 emit("install_done")。返回 job_id 立即让前端关联。
///
/// **多源 fallback**：按顺序尝试每个源；任一成功即停止；全部失败才报错。
/// 兼容旧调用：如果传 install_cmd（单条命令），自动包装成单源 sources。
#[tauri::command]
pub async fn install_dep(
    bundle_id: String,
    dep_name: String,
    install_cmd: Option<String>,
    install_sources: Option<Vec<InstallSource>>,
    app: AppHandle,
) -> Result<String, String> {
    // 归一化：优先 install_sources；若空则用 install_cmd
    let sources: Vec<InstallSource> = match install_sources {
        Some(v) if !v.is_empty() => v,
        _ => {
            let cmd = install_cmd.unwrap_or_default();
            if cmd.trim().is_empty() {
                return Err("install_sources 和 install_cmd 都为空".into());
            }
            vec![InstallSource {
                label: "默认".into(),
                cmd,
            }]
        }
    };

    let job_id = format!("{}-{}-{}", bundle_id, dep_name, chrono_now_ms());
    let job_id_for_task = job_id.clone();
    let app_for_task = app.clone();

    tokio::spawn(async move {
        run_multi_source_install(&job_id_for_task, sources, &app_for_task).await;
    });

    Ok(job_id)
}

/// 按顺序尝试每个源；一个成功即停；全部失败才标记 done(success=false)。
async fn run_multi_source_install(job_id: &str, sources: Vec<InstallSource>, app: &AppHandle) {
    let total = sources.len();
    let mut last_source_label = String::new();
    let mut last_exit_code = -1i32;
    let mut last_error = String::new();

    for (idx, src) in sources.iter().enumerate() {
        last_source_label = src.label.clone();
        let _ = app.emit(
            "install_log",
            &InstallLogPayload {
                job_id: job_id.into(),
                line: format!(
                    "▶ 尝试源 {}/{}：{}",
                    idx + 1,
                    total,
                    src.label
                ),
                is_stderr: false,
            },
        );
        let _ = app.emit(
            "install_log",
            &InstallLogPayload {
                job_id: job_id.into(),
                line: format!("$ {}", src.cmd),
                is_stderr: false,
            },
        );

        let result = run_single_install(job_id, &src.cmd, app).await;
        match result {
            Ok(()) => {
                // 成功 —— done(success=true) 然后退出
                let _ = app.emit(
                    "install_log",
                    &InstallLogPayload {
                        job_id: job_id.into(),
                        line: format!("✓ 源 [{}] 安装成功", src.label),
                        is_stderr: false,
                    },
                );
                let _ = app.emit(
                    "install_done",
                    &InstallDonePayload {
                        job_id: job_id.into(),
                        success: true,
                        exit_code: 0,
                        error: String::new(),
                        used_source: src.label.clone(),
                    },
                );
                return;
            }
            Err((code, err)) => {
                last_exit_code = code;
                last_error = err.clone();
                let _ = app.emit(
                    "install_log",
                    &InstallLogPayload {
                        job_id: job_id.into(),
                        line: format!(
                            "✗ 源 [{}] 失败 (exit {})，{}",
                            src.label,
                            code,
                            if idx + 1 < total {
                                "尝试下一个..."
                            } else {
                                "已无更多源"
                            }
                        ),
                        is_stderr: true,
                    },
                );
            }
        }
    }

    // 全部失败
    let _ = app.emit(
        "install_done",
        &InstallDonePayload {
            job_id: job_id.into(),
            success: false,
            exit_code: last_exit_code,
            error: format!("全部 {} 个源都失败 - 最后错误: {}", total, last_error),
            used_source: last_source_label,
        },
    );
}

/// 跑单个 install 命令；stdout/stderr stream 给 emit；返回 Ok 或 (exit_code, error_msg)。
async fn run_single_install(
    job_id: &str,
    cmd: &str,
    app: &AppHandle,
) -> Result<(), (i32, String)> {
    // 跨平台 install shell: Windows cmd /C · Unix /bin/bash -lc
    #[cfg(target_os = "windows")]
    let mut install_cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    };
    #[cfg(not(target_os = "windows"))]
    let mut install_cmd = {
        let mut c = Command::new("/bin/bash");
        c.arg("-lc").arg(cmd);
        c
    };
    install_cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    crate::proc_util::hide_window_tokio(&mut install_cmd);
    let mut child = match install_cmd.spawn()
    {
        Ok(c) => c,
        Err(e) => return Err((-1, format!("spawn 失败: {}", e))),
    };

    if let Some(stdout) = child.stdout.take() {
        let app_c = app.clone();
        let jid = job_id.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = app_c.emit(
                    "install_log",
                    &InstallLogPayload {
                        job_id: jid.clone(),
                        line,
                        is_stderr: false,
                    },
                );
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let app_c = app.clone();
        let jid = job_id.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = app_c.emit(
                    "install_log",
                    &InstallLogPayload {
                        job_id: jid.clone(),
                        line,
                        is_stderr: true,
                    },
                );
            }
        });
    }

    let status_fut = child.wait();
    let status = match tokio::time::timeout(
        std::time::Duration::from_secs(INSTALL_TIMEOUT_SECS),
        status_fut,
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err((-1, format!("wait 失败: {}", e))),
        Err(_) => {
            return Err((
                -1,
                format!("超时（{}s）", INSTALL_TIMEOUT_SECS),
            ))
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err((status.code().unwrap_or(-1), "exit non-zero".into()))
    }
}

fn chrono_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
