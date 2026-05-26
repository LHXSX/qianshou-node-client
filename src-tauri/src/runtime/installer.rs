//! Runtime installer · venv + pip + 自检 · 镜像源 fallback
//!
//! 流程:
//!   1. 拉 manifest (mirrors + tier packages + smoke_test)
//!   2. 找/选定 host python
//!   3. 创建/重置 venv: `host_python -m venv <venv_dir>`
//!   4. 升级 pip / setuptools / wheel
//!   5. pip install 依赖 · 按 mirrors 顺序 fallback
//!   6. 跑 smoke_test
//!   7. 探测 binaries (imageio_ffmpeg.get_ffmpeg_exe 等)
//!   8. 写 installed.json (原子)
//!   9. 整个过程 emit "runtime_install_log" / "runtime_install_done"

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::detector::{
    check_python, read_installed_meta, write_installed_meta, InstalledTier,
};
use super::manifest::{self, BinarySpec, MirrorSource, PrebuiltVenvSpec, TierSpec};
use super::paths;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const PIP_TIMEOUT_SECS: u64 = 600; // 单源最长 10 分钟
const SMOKE_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize)]
struct InstallLogPayload {
    job_id: String,
    tier: String,
    line: String,
    is_stderr: bool,
}

#[derive(Debug, Clone, Serialize)]
struct InstallDonePayload {
    job_id: String,
    tier: String,
    success: bool,
    error: String,
    used_mirror: String,
    venv_python: String,
}

/// 后台执行: 安装一个 tier · 完成后 emit `runtime_install_done`
pub async fn install_tier(app: AppHandle, tier: String) -> Result<String> {
    let job_id = format!("rt-{}-{}", sanitize_label(&tier), now_ms());
    let app_for_task = app.clone();
    let tier_for_task = tier.clone();
    let job_for_task = job_id.clone();
    tokio::spawn(async move {
        if let Err(e) = run_install(&job_for_task, &tier_for_task, &app_for_task).await {
            emit_log(&app_for_task, &job_for_task, &tier_for_task, &format!("✗ 安装失败: {}", e), true);
            let _ = app_for_task.emit(
                "runtime_install_done",
                &InstallDonePayload {
                    job_id: job_for_task.clone(),
                    tier: tier_for_task.clone(),
                    success: false,
                    error: e.to_string(),
                    used_mirror: String::new(),
                    venv_python: String::new(),
                },
            );
        }
    });
    Ok(job_id)
}

async fn run_install(job_id: &str, tier: &str, app: &AppHandle) -> Result<()> {
    emit_log(app, job_id, tier, "▶ 拉取后端运行时清单…", false);
    let m = manifest::fetch().await?;
    emit_log(
        app,
        job_id,
        tier,
        &format!(
            "✓ 清单: platform={} install_mode={} schema={}",
            m.platform, m.install_mode, m.schema_version
        ),
        false,
    );
    let spec = m
        .tiers
        .get(tier)
        .ok_or_else(|| anyhow!("manifest 没有 tier `{}`", tier))?
        .clone();

    // 2026-05-24 · binaries-only tier (例如 ffmpeg) 不走 venv · 直接下载静态二进制
    if spec.packages.is_empty() && !spec.binaries.is_empty() {
        return run_install_binaries_only(job_id, tier, app, &m, &spec).await;
    }

    // 2026-05-24 · system-command-only tier (例如 render → blender) 不走 venv 也不下二进制
    // 只 which 探测系统是否已装 · 失败则 tier 装不上 (避免 software 撒谎)
    if spec.packages.is_empty() && spec.binaries.is_empty() && !spec.system_commands.is_empty() {
        return run_install_system_check_only(job_id, tier, app, &m, &spec).await;
    }

    // 2026-05-26 · Layer 2 自源 CDN 优先 (Ollama 型 · 跳过 PyPI)
    // 后端 manifest 配了 prebuilt_venv → 优先拉 tarball + 校验 + 解压
    // 成功就直接 done · 失败 emit warn + 走下面老路径 (pip + 4 镜像)
    if let Some(pv) = spec.prebuilt_venv.clone() {
        emit_log(app, job_id, tier,
            &format!("▶ 检测到预打包源 · 优先走 OSS tarball ({} MB · {})",
                pv.size_mb, pv.version), false);
        match install_prebuilt_venv(job_id, tier, app, &m, &spec, &pv).await {
            Ok(()) => {
                emit_log(app, job_id, tier, "✅ 预打包源安装成功 · 跳过 PyPI", false);
                return Ok(());
            }
            Err(e) => {
                emit_log(app, job_id, tier,
                    &format!("⚠ 预打包源失败 · 降级到 PyPI 公共源: {}", e), true);
                // 不 return · 继续走下面的老路径
            }
        }
    }

    if m.install_mode != "public_mirror_venv" {
        return Err(anyhow!(
            "后端 manifest install_mode=`{}` (schema={}) 不是 public_mirror_venv · \
             请把 platform_v8/api/v8/bundles.py 升级到 v2 (mirrors+packages)",
            if m.install_mode.is_empty() { "<empty · 旧后端>" } else { m.install_mode.as_str() },
            m.schema_version
        ));
    }
    if m.mirrors.is_empty() {
        return Err(anyhow!("manifest.mirrors 为空 · 请在后端 manifest 配置至少一个公共源"));
    }

    // ── 1. 引导 uv (打包在 app resources 里 · 没有则 HTTP 下) ──
    emit_log(app, job_id, tier, "▶ 准备 uv (节点自带 · 零依赖运行时)…", false);
    let uv_bin = match super::uv::ensure_uv(app).await {
        Ok(p) => {
            emit_log(app, job_id, tier, &format!("✓ uv 就绪: {}", p.display()), false);
            p
        }
        Err(e) => {
            return Err(anyhow!(
                "准备 uv 失败 · 运行时不可用: {} · 请检查网络后重试",
                e
            ));
        }
    };

    // ── 2. uv python install (本机没 Python 也 OK) ──
    let need_ver = m
        .python
        .as_ref()
        .and_then(|p| p.preferred_versions.first().cloned())
        .or_else(|| m.python.as_ref().and_then(|p| p.min_version.clone()))
        .unwrap_or_else(|| "3.11".to_string());
    emit_log(app, job_id, tier, &format!("▶ 确保 Python {} 可用 (uv 自动拉取)…", need_ver), false);
    let py_for_venv = match super::uv::ensure_python(&uv_bin, &need_ver).await {
        Ok(p) => {
            emit_log(app, job_id, tier, &format!("✓ Python {} 就绪: {}", need_ver, p.display()), false);
            p
        }
        Err(e) => {
            return Err(anyhow!("uv 获取 Python {} 失败: {}", need_ver, e));
        }
    };

    // ── 3. uv venv ──
    let venv_dir = paths::venv_dir(tier);
    if venv_dir.exists() {
        emit_log(
            app,
            job_id,
            tier,
            &format!("▶ 重置旧 venv: {}", venv_dir.display()),
            false,
        );
        let _ = std::fs::remove_dir_all(&venv_dir);
    }
    if let Some(parent) = venv_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| anyhow!("创建 venvs 目录失败: {}", e))?;
    }
    emit_log(app, job_id, tier, "▶ 创建 venv (uv)…", false);
    let mut venv_cmd = Command::new(&uv_bin);
    venv_cmd
        .arg("venv")
        .arg(&*venv_dir.to_string_lossy())
        .arg("--python")
        .arg(py_for_venv.to_string_lossy().as_ref())
        .env("UV_PYTHON_INSTALL_DIR", paths::uv_python_dir());
    run_capture(app, job_id, tier, &mut venv_cmd, Duration::from_secs(120)).await?;

    let venv_py = paths::venv_python(tier);
    if !venv_py.exists() {
        return Err(anyhow!("venv 创建后 python 不存在: {}", venv_py.display()));
    }
    emit_log(
        app,
        job_id,
        tier,
        &format!("✓ venv python: {}", venv_py.display()),
        false,
    );

    // ── 4. uv pip install packages (多源 fallback) ──
    if spec.packages.is_empty() {
        emit_log(app, job_id, tier, "ℹ 此 tier 不含 pip 包 · 跳过安装", false);
    }
    let used_mirror = install_packages(app, job_id, tier, &uv_bin, &venv_py, &spec, &m.mirrors).await?;

    // ── smoke test ──
    if !spec.smoke_test.trim().is_empty() {
        emit_log(app, job_id, tier, "▶ 运行自检 smoke_test…", false);
        let mut sm = Command::new(&venv_py);
        sm.arg("-c").arg(&spec.smoke_test);
        run_capture(app, job_id, tier, &mut sm, Duration::from_secs(SMOKE_TIMEOUT_SECS))
            .await
            .map_err(|e| anyhow!("smoke_test 失败: {}", e))?;
    }
    emit_log(app, job_id, tier, "✓ 自检通过", false);

    // ── binaries 探测 (ffmpeg 等) ──
    let mut binaries = std::collections::BTreeMap::new();
    if spec.packages.iter().any(|p| p.eq_ignore_ascii_case("imageio-ffmpeg")) {
        if let Some(ffmpeg) = detect_imageio_ffmpeg(&venv_py).await {
            binaries.insert("ffmpeg".into(), ffmpeg);
        }
    }

    // ── 5. 拉 skill zip (按 tier.skills · 装到 ~/.local/lib/edgecompute/skills/) ──
    let mut installed_skills: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    if spec.skills.is_empty() {
        emit_log(app, job_id, tier, "ℹ 此 tier 不带 skill 文件 · 跳过下发", false);
    } else {
        emit_log(
            app,
            job_id,
            tier,
            &format!("▶ 下发 skill 文件 ({} 个) · 解压到 {}",
                spec.skills.len(),
                paths::skills_install_dir().display()
            ),
            false,
        );
        for skill_id in &spec.skills {
            match super::skills_fetcher::fetch_and_install(skill_id).await {
                Ok(r) => {
                    installed_skills.insert(r.skill_id.clone(), r.version.clone());
                    emit_log(
                        app,
                        job_id,
                        tier,
                        &format!(
                            "  ✓ {} v{} · {} 个文件 · {:.1} KB · sha {}…",
                            r.skill_id,
                            if r.version.is_empty() { "?" } else { r.version.as_str() },
                            r.file_count,
                            (r.size_bytes as f64) / 1024.0,
                            &r.sha256[..12.min(r.sha256.len())]
                        ),
                        false,
                    );
                }
                Err(e) => {
                    // skill 下载失败不阻塞 tier 安装 · 但要告诉用户
                    emit_log(
                        app,
                        job_id,
                        tier,
                        &format!("  ⚠ {} 下载失败 (可继续): {}", skill_id, e),
                        true,
                    );
                }
            }
        }
    }

    // ── 写 installed.json ──
    let mut meta = read_installed_meta();
    if meta.schema_version.is_empty() {
        meta.schema_version = "2".into();
    }
    if meta.install_mode.is_empty() {
        meta.install_mode = "public_mirror_venv".into();
    }
    if meta.platform.is_empty() {
        meta.platform = m.platform.clone();
    }
    meta.host_python = Some(py_for_venv.to_string_lossy().to_string());

    let pkgs = list_installed_packages(&venv_py).await.unwrap_or_default();
    meta.tiers.insert(
        tier.to_string(),
        InstalledTier {
            ok: true,
            python: venv_py.to_string_lossy().to_string(),
            packages: pkgs,
            software: spec.software.clone(),
            mirror_label: used_mirror.clone(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            last_message: "ok".into(),
            binaries,
            installed_skills,
        },
    );
    write_installed_meta(&meta)?;
    emit_log(
        app,
        job_id,
        tier,
        &format!("✓ 已写 installed.json (mirror={})", used_mirror),
        false,
    );

    let _ = app.emit(
        "runtime_install_done",
        &InstallDonePayload {
            job_id: job_id.to_string(),
            tier: tier.to_string(),
            success: true,
            error: String::new(),
            used_mirror,
            venv_python: venv_py.to_string_lossy().to_string(),
        },
    );
    Ok(())
}

async fn install_packages(
    app: &AppHandle,
    job_id: &str,
    tier: &str,
    uv_bin: &PathBuf,
    venv_py: &PathBuf,
    spec: &TierSpec,
    mirrors: &[MirrorSource],
) -> Result<String> {
    if spec.packages.is_empty() {
        return Ok(String::new());
    }
    let mirror_list: Vec<MirrorSource> = if mirrors.is_empty() {
        vec![MirrorSource {
            label: "官方 PyPI".into(),
            index_url: "https://pypi.org/simple".into(),
            trusted_host: None,
        }]
    } else {
        mirrors.to_vec()
    };
    let mut last_err = String::new();
    for (idx, mirror) in mirror_list.iter().enumerate() {
        emit_log(
            app,
            job_id,
            tier,
            &format!(
                "▶ 安装 ({}个包) · 源 {}/{}: {} [{}]",
                spec.packages.len(),
                idx + 1,
                mirror_list.len(),
                mirror.label,
                mirror.index_url
            ),
            false,
        );
        // uv pip install --python <venv_py> --index-url <mirror> [pkgs] [pip_args]
        let mut args: Vec<String> = vec![
            "pip".into(),
            "install".into(),
            "--python".into(),
            venv_py.to_string_lossy().to_string(),
            "--index-url".into(),
            mirror.index_url.clone(),
        ];
        if let Some(host) = &mirror.trusted_host {
            // uv 没有 --trusted-host · 但接受同名 env
            args.push("--allow-insecure-host".into());
            args.push(host.clone());
        }
        // 2026-05-21 兜底过滤: uv 0.11.x 直接拒绝 pip 经典 flag (--prefer-binary / --timeout / --retries)
        // 早期 uv silently ignore · 现在 exit 2 整个安装失败 · 后端 manifest 还在传旧 flag 的话也别让节点死
        // uv 默认就 prefer wheel · timeout 走 UV_HTTP_TIMEOUT env · 这些 flag 全 drop 不影响功能
        let uv_unsupported = ["--prefer-binary", "--timeout", "--retries"];
        let mut i = 0;
        while i < spec.pip_args.len() {
            let a = &spec.pip_args[i];
            if uv_unsupported.iter().any(|f| a == f) {
                // --timeout 90 / --retries 2 这种带值的 flag 要把后一个数字也跳过
                let has_value = a == "--timeout" || a == "--retries";
                i += if has_value { 2 } else { 1 };
                continue;
            }
            args.push(a.clone());
            i += 1;
        }
        for p in &spec.packages {
            args.push(p.clone());
        }
        let mut cmd = Command::new(uv_bin);
        cmd.args(&args)
            .env("UV_PYTHON_INSTALL_DIR", paths::uv_python_dir());
        match run_capture(app, job_id, tier, &mut cmd, Duration::from_secs(PIP_TIMEOUT_SECS)).await {
            Ok(()) => {
                emit_log(
                    app,
                    job_id,
                    tier,
                    &format!("✓ 源 [{}] 安装成功", mirror.label),
                    false,
                );
                return Ok(mirror.label.clone());
            }
            Err(e) => {
                last_err = e.to_string();
                emit_log(
                    app,
                    job_id,
                    tier,
                    &format!("✗ 源 [{}] 失败: {}", mirror.label, last_err),
                    true,
                );
            }
        }
    }
    Err(anyhow!("所有镜像源安装均失败 · 最后错误: {}", last_err))
}

async fn list_installed_packages(venv_py: &PathBuf) -> Result<Vec<String>> {
    let mut cmd = Command::new(venv_py);
    cmd.args(["-m", "pip", "list", "--format=freeze", "--disable-pip-version-check"]);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    crate::proc_util::hide_window_tokio(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| anyhow!("pip list 超时"))?
        .map_err(|e| anyhow!("pip list 失败: {}", e))?;
    if !out.status.success() {
        return Err(anyhow!("pip list 退出 {}", out.status.code().unwrap_or(-1)));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut pkgs = Vec::new();
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        pkgs.push(line.to_string());
    }
    Ok(pkgs)
}

// ════════════════════════════════════════════════════════════════════
// 2026-05-24 · 静态二进制 tier 安装 (ffmpeg 等)
// ════════════════════════════════════════════════════════════════════
// 2026-05-26 · Layer 2 自源 CDN · 预打包 venv 安装路径 (Ollama 型)
// ════════════════════════════════════════════════════════════════════

/// 流程: 下载 tarball → sha256 校验 → 解压到 venvs/<tier> → smoke test → 写 installed.json
/// 失败任一步都 propagate Err · 上层 run_install 会 fallback 到 pip install
async fn install_prebuilt_venv(
    job_id: &str,
    tier: &str,
    app: &AppHandle,
    m: &manifest::RuntimeManifest,
    spec: &TierSpec,
    pv: &PrebuiltVenvSpec,
) -> Result<()> {
    // 1. 决定解压目标 · 默认 venvs/<tier>
    let extract_name = if pv.extract_to.trim().is_empty() {
        tier.to_string()
    } else {
        pv.extract_to.trim().to_string()
    };
    let venvs_root = paths::venv_dir(tier).parent()
        .ok_or_else(|| anyhow!("venv_dir parent 为 None"))?
        .to_path_buf();
    let venv_dest = venvs_root.join(&extract_name);
    std::fs::create_dir_all(&venvs_root).map_err(|e| anyhow!("创建 venvs 目录失败: {}", e))?;

    // 2. 旧 venv 存在 → 先清掉 (避免半残)
    if venv_dest.exists() {
        emit_log(app, job_id, tier,
            &format!("▶ 清除旧 venv: {}", venv_dest.display()), false);
        let _ = std::fs::remove_dir_all(&venv_dest);
    }

    // 3. 下载 tarball 到临时文件
    emit_log(app, job_id, tier,
        &format!("▶ 下载 prebuilt venv · {}", pv.url), false);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1800))  // 30 min · 大包 (vision-ai 可能 3GB)
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| anyhow!("reqwest client 失败: {}", e))?;
    let tmpdir = tempfile::tempdir().map_err(|e| anyhow!("tempdir 失败: {}", e))?;
    let archive_path = tmpdir.path().join("venv.tar.gz");
    download_to_file(&client, &pv.url, &archive_path).await
        .map_err(|e| anyhow!("下载 prebuilt venv 失败: {}", e))?;
    let bytes = std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0);
    emit_log(app, job_id, tier,
        &format!("✓ 下载完成 · {:.1} MB", bytes as f64 / 1024.0 / 1024.0), false);

    // 4. sha256 校验 (TBD 跳过 · 给 warn)
    if pv.sha256.trim().is_empty() || pv.sha256.eq_ignore_ascii_case("TBD") {
        emit_log(app, job_id, tier,
            "⚠ sha256 未配置 · 跳过校验 (生产前务必回填)", true);
    } else {
        let got = sha256_of_file(&archive_path)?;
        if !got.eq_ignore_ascii_case(&pv.sha256) {
            return Err(anyhow!("sha256 校验失败 · 期望 {} 实际 {}", pv.sha256, got));
        }
        emit_log(app, job_id, tier,
            &format!("✓ sha256 校验通过 {}…", &got[..12.min(got.len())]), false);
    }

    // 5. 解压
    emit_log(app, job_id, tier,
        &format!("▶ 解压 → {}", venv_dest.display()), false);
    std::fs::create_dir_all(&venv_dest).map_err(|e| anyhow!("创建解压目录失败: {}", e))?;
    extract_tar_gz(&archive_path, &venv_dest)?;
    emit_log(app, job_id, tier, "✓ 解压完成", false);

    // 6. 校验 python 存在
    let venv_py = venv_dest.join(&pv.python_rel);
    if !venv_py.exists() {
        return Err(anyhow!(
            "解压后 python 不存在: {} · tarball 内部布局可能不对 (期望 {})",
            venv_py.display(), pv.python_rel
        ));
    }
    emit_log(app, job_id, tier,
        &format!("✓ venv python: {}", venv_py.display()), false);

    // 7. smoke test (verify_cmd 优先 · 否则用 spec.smoke_test)
    let verify = if !pv.verify_cmd.trim().is_empty() {
        pv.verify_cmd.clone()
    } else {
        spec.smoke_test.clone()
    };
    if !verify.trim().is_empty() {
        emit_log(app, job_id, tier, "▶ 运行 verify_cmd / smoke_test…", false);
        let mut sm = Command::new(&venv_py);
        sm.arg("-c").arg(&verify);
        run_capture(app, job_id, tier, &mut sm, Duration::from_secs(SMOKE_TIMEOUT_SECS))
            .await
            .map_err(|e| anyhow!("verify 失败 (解压可能损坏): {}", e))?;
        emit_log(app, job_id, tier, "✓ 验证通过", false);
    }

    // 8. 写 installed.json
    let mut meta = read_installed_meta();
    if meta.schema_version.is_empty() {
        meta.schema_version = "2".into();
    }
    if meta.install_mode.is_empty() {
        meta.install_mode = "prebuilt_venv".into();
    }
    if meta.platform.is_empty() {
        meta.platform = m.platform.clone();
    }
    meta.host_python = Some(venv_py.to_string_lossy().to_string());

    let pkgs = list_installed_packages(&venv_py.to_path_buf()).await.unwrap_or_default();
    meta.tiers.insert(
        tier.to_string(),
        InstalledTier {
            ok: true,
            python: venv_py.to_string_lossy().to_string(),
            packages: pkgs,
            software: spec.software.clone(),
            mirror_label: format!("prebuilt_oss ({})", pv.version),
            installed_at: chrono::Utc::now().to_rfc3339(),
            last_message: format!("从 OSS 预打包源安装 · 版本 {} · {} MB", pv.version, pv.size_mb),
            binaries: BTreeMap::new(),
            installed_skills: BTreeMap::new(),
        },
    );
    write_installed_meta(&meta)?;

    let _ = app.emit(
        "runtime_install_done",
        &InstallDonePayload {
            job_id: job_id.to_string(),
            tier: tier.to_string(),
            success: true,
            error: String::new(),
            used_mirror: format!("prebuilt_oss ({})", pv.version),
            venv_python: venv_py.to_string_lossy().to_string(),
        },
    );
    Ok(())
}

// ════════════════════════════════════════════════════════════════════

/// binaries-only tier 安装入口 · 不走 venv · 直接下载/校验/解压
async fn run_install_binaries_only(
    job_id: &str,
    tier: &str,
    app: &AppHandle,
    m: &manifest::RuntimeManifest,
    spec: &TierSpec,
) -> Result<()> {
    let dest = paths::tier_root(tier);
    if dest.exists() {
        emit_log(app, job_id, tier, &format!("▶ 重置旧 tier 目录: {}", dest.display()), false);
        let _ = std::fs::remove_dir_all(&dest);
    }
    std::fs::create_dir_all(&dest).map_err(|e| anyhow!("创建 tier 目录失败: {}", e))?;
    emit_log(app, job_id, tier, &format!("✓ tier 目录: {}", dest.display()), false);

    let binaries = install_binaries(app, job_id, tier, &dest, &spec.binaries).await?;

    // smoke_test · 拼临时 PATH 跑 (例如 `ffmpeg -version`)
    if !spec.smoke_test.trim().is_empty() {
        emit_log(app, job_id, tier, "▶ 运行自检 smoke_test…", false);
        let bin_dirs: Vec<PathBuf> = spec.binaries.iter()
            .map(|b| dest.join(&b.extract_to).join(&b.bin_dir))
            .collect();
        let combined_path = build_path_with(&bin_dirs);
        // 2026-05-25 8.0.9 · macOS GUI app 启动时 PATH 可能不含 /bin · spawn("sh") 失败
        // 修复: 用绝对路径 /bin/sh · combined_path 已含系统标准路径兜底
        let shell_cmd = if cfg!(target_os = "windows") { "cmd" } else { "/bin/sh" };
        let shell_flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };
        let mut sm = Command::new(shell_cmd);
        sm.arg(shell_flag).arg(&spec.smoke_test).env("PATH", &combined_path);
        run_capture(app, job_id, tier, &mut sm, Duration::from_secs(SMOKE_TIMEOUT_SECS))
            .await
            .map_err(|e| anyhow!("smoke_test 失败: {} · 请检查 ffmpeg 可执行性", e))?;
        emit_log(app, job_id, tier, "✓ 自检通过", false);
    }

    // 写 installed.json
    let mut meta = read_installed_meta();
    if meta.schema_version.is_empty() { meta.schema_version = "2".into(); }
    if meta.install_mode.is_empty() { meta.install_mode = "public_mirror_venv".into(); }
    if meta.platform.is_empty() { meta.platform = m.platform.clone(); }
    meta.tiers.insert(
        tier.to_string(),
        InstalledTier {
            ok: true,
            python: String::new(), // 不依赖 python
            packages: vec![],
            software: spec.software.clone(),
            mirror_label: "binary_oss".into(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            last_message: "ok".into(),
            binaries,
            installed_skills: BTreeMap::new(),
        },
    );
    write_installed_meta(&meta)?;
    emit_log(app, job_id, tier, "✓ 已写 installed.json", false);

    let _ = app.emit(
        "runtime_install_done",
        &InstallDonePayload {
            job_id: job_id.to_string(),
            tier: tier.to_string(),
            success: true,
            error: String::new(),
            used_mirror: "binary_oss".into(),
            venv_python: String::new(),
        },
    );
    Ok(())
}

/// 2026-05-24 · system-command-only tier 安装入口
/// 例: render tier 依赖 blender CLI · 节点 which 探测 · 缺则 fail · 不撒谎
async fn run_install_system_check_only(
    job_id: &str,
    tier: &str,
    app: &AppHandle,
    m: &manifest::RuntimeManifest,
    spec: &TierSpec,
) -> Result<()> {
    emit_log(app, job_id, tier, &format!("▶ 检查系统命令依赖 ({} 个)…", spec.system_commands.len()), false);
    let mut found_paths: BTreeMap<String, String> = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();
    for cmd in &spec.system_commands {
        match which::which(cmd) {
            Ok(p) => {
                emit_log(app, job_id, tier, &format!("  ✓ {} → {}", cmd, p.display()), false);
                found_paths.insert(cmd.clone(), p.to_string_lossy().to_string());
            }
            Err(_) => {
                emit_log(app, job_id, tier, &format!("  ✗ {} · 系统未安装", cmd), true);
                missing.push(cmd.clone());
            }
        }
    }
    if !missing.is_empty() {
        // 给用户复制粘贴指引 · 按 OS 取
        let os_key = if cfg!(target_os = "macos") { "macos" }
                     else if cfg!(target_os = "windows") { "windows" }
                     else { "linux" };
        let hint = spec.install_hint.get(os_key).cloned()
            .unwrap_or_else(|| format!("请先安装 {} CLI 后重试", missing.join(", ")));
        return Err(anyhow!(
            "tier `{}` 需要系统命令 [{}] 但未找到 · 安装指引:\n  {}\n安装完成后请重新点击 '安装 {}' 按钮",
            tier, missing.join(", "), hint, tier
        ));
    }

    // smoke_test (例如 `blender --version`)
    if !spec.smoke_test.trim().is_empty() {
        emit_log(app, job_id, tier, &format!("▶ 运行自检: {}", spec.smoke_test), false);
        // 2026-05-25 8.0.9 · macOS GUI app PATH 可能空 · /bin/sh 绝对路径稳
        let shell_cmd = if cfg!(target_os = "windows") { "cmd" } else { "/bin/sh" };
        let shell_flag = if cfg!(target_os = "windows") { "/C" } else { "-c" };
        let mut sm = Command::new(shell_cmd);
        sm.arg(shell_flag).arg(&spec.smoke_test);
        run_capture(app, job_id, tier, &mut sm, Duration::from_secs(SMOKE_TIMEOUT_SECS))
            .await
            .map_err(|e| anyhow!("smoke_test 失败: {} · 请检查 {} 命令可用性", e, spec.system_commands.join(",")))?;
        emit_log(app, job_id, tier, "✓ 自检通过", false);
    }

    // 写 installed.json · software 真实可信
    let mut meta = read_installed_meta();
    if meta.schema_version.is_empty() { meta.schema_version = "2".into(); }
    if meta.install_mode.is_empty() { meta.install_mode = "public_mirror_venv".into(); }
    if meta.platform.is_empty() { meta.platform = m.platform.clone(); }
    meta.tiers.insert(
        tier.to_string(),
        InstalledTier {
            ok: true,
            python: String::new(),
            packages: vec![],
            software: spec.software.clone(),
            mirror_label: "system_command".into(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            last_message: "ok".into(),
            binaries: found_paths,
            installed_skills: BTreeMap::new(),
        },
    );
    write_installed_meta(&meta)?;
    emit_log(app, job_id, tier, "✓ 已写 installed.json (来源: 系统命令真实探测)", false);

    let _ = app.emit(
        "runtime_install_done",
        &InstallDonePayload {
            job_id: job_id.to_string(),
            tier: tier.to_string(),
            success: true,
            error: String::new(),
            used_mirror: "system_command".into(),
            venv_python: String::new(),
        },
    );
    Ok(())
}

/// 下载 + 校验 + 解压所有 BinarySpec · 返回 name -> 绝对可执行路径
async fn install_binaries(
    app: &AppHandle,
    job_id: &str,
    tier: &str,
    dest: &PathBuf,
    specs: &[BinarySpec],
) -> Result<BTreeMap<String, String>> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    if specs.is_empty() {
        return Ok(out);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| anyhow!("reqwest client 构建失败: {}", e))?;
    for spec in specs {
        emit_log(app, job_id, tier, &format!("▶ 下载二进制 {} · {}", spec.name, spec.url), false);
        let tmpdir = tempfile::tempdir().map_err(|e| anyhow!("tempdir 失败: {}", e))?;
        let archive_path = tmpdir.path().join(match spec.archive.as_str() {
            "zip" => "binary.zip",
            _ => "binary.tar.gz",
        });
        download_to_file(&client, &spec.url, &archive_path).await
            .map_err(|e| anyhow!("下载 {} 失败: {}", spec.url, e))?;
        let bytes = std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0);
        emit_log(app, job_id, tier, &format!("✓ 下载完成 · {:.1} MB", bytes as f64 / 1024.0 / 1024.0), false);
        if !spec.sha256.trim().is_empty() && !spec.sha256.starts_with("TBD") {
            let got = sha256_of_file(&archive_path)?;
            if !got.eq_ignore_ascii_case(&spec.sha256) {
                return Err(anyhow!("sha256 校验失败 · 期望 {} 实际 {}", spec.sha256, got));
            }
            emit_log(app, job_id, tier, &format!("✓ sha256 校验通过 {}…", &got[..12.min(got.len())]), false);
        } else {
            emit_log(app, job_id, tier, "⚠ sha256 未配置 · 跳过校验 (生产前务必回填)", true);
        }
        let extract_dir = dest.join(&spec.extract_to);
        std::fs::create_dir_all(&extract_dir).map_err(|e| anyhow!("创建解压目录失败: {}", e))?;
        emit_log(app, job_id, tier, &format!("▶ 解压 → {}", extract_dir.display()), false);
        if spec.archive.eq_ignore_ascii_case("zip") {
            extract_zip(&archive_path, &extract_dir)?;
        } else {
            extract_tar_gz(&archive_path, &extract_dir)?;
        }
        // 校验可执行文件就位 + chmod +x
        let bin_dir = extract_dir.join(&spec.bin_dir);
        for exe in &spec.executables {
            let exe_name = if cfg!(target_os = "windows") && !exe.ends_with(".exe") {
                format!("{}.exe", exe)
            } else {
                exe.clone()
            };
            let exe_path = bin_dir.join(&exe_name);
            if !exe_path.exists() {
                return Err(anyhow!("解压后找不到可执行 {} (期望路径 {})", exe_name, exe_path.display()));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&exe_path) {
                    let mut perms = meta.permissions();
                    perms.set_mode(perms.mode() | 0o111);
                    let _ = std::fs::set_permissions(&exe_path, perms);
                }
            }
            out.insert(exe.clone(), exe_path.to_string_lossy().to_string());
            emit_log(app, job_id, tier, &format!("✓ {} → {}", exe, exe_path.display()), false);
        }
    }
    Ok(out)
}

async fn download_to_file(client: &reqwest::Client, url: &str, dest: &PathBuf) -> Result<()> {
    let resp = client.get(url).send().await.map_err(|e| anyhow!("GET 失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().await.map_err(|e| anyhow!("读 body 失败: {}", e))?;
    std::fs::write(dest, &bytes).map_err(|e| anyhow!("写入 {} 失败: {}", dest.display(), e))?;
    Ok(())
}

fn sha256_of_file(p: &PathBuf) -> Result<String> {
    let data = std::fs::read(p).map_err(|e| anyhow!("读 {} 失败: {}", p.display(), e))?;
    let mut h = Sha256::new();
    h.update(&data);
    Ok(h.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

fn extract_tar_gz(archive: &PathBuf, dest: &PathBuf) -> Result<()> {
    let file = std::fs::File::open(archive).map_err(|e| anyhow!("打开 archive 失败: {}", e))?;
    let dec = flate2::read::GzDecoder::new(file);
    let mut ar = tar::Archive::new(dec);
    ar.set_preserve_permissions(true);
    ar.unpack(dest).map_err(|e| anyhow!("tar.gz 解压失败: {}", e))?;
    Ok(())
}

fn extract_zip(archive: &PathBuf, dest: &PathBuf) -> Result<()> {
    let file = std::fs::File::open(archive).map_err(|e| anyhow!("打开 zip 失败: {}", e))?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| anyhow!("zip 解析失败: {}", e))?;
    z.extract(dest).map_err(|e| anyhow!("zip 解压失败: {}", e))?;
    Ok(())
}

fn build_path_with(extra_dirs: &[PathBuf]) -> std::ffi::OsString {
    let mut parts: Vec<std::ffi::OsString> = extra_dirs.iter()
        .filter(|p| p.exists())
        .map(|p| p.clone().into_os_string())
        .collect();
    if let Some(existing) = std::env::var_os("PATH") {
        parts.push(existing);
    }
    // 2026-05-25 8.0.9 · macOS GUI app (Finder/Launchpad 启动) PATH 通常不含 /usr/local/bin /opt/homebrew/bin
    // 这里兜底加系统标准路径 · 确保 sh / sleep / chmod / 用户装的 brew 工具可见
    #[cfg(unix)]
    {
        for p in ["/usr/bin", "/bin", "/usr/sbin", "/sbin", "/usr/local/bin", "/opt/homebrew/bin"] {
            parts.push(p.into());
        }
    }
    std::env::join_paths(parts).unwrap_or_default()
}

async fn detect_imageio_ffmpeg(venv_py: &PathBuf) -> Option<String> {
    let mut cmd = Command::new(venv_py);
    cmd.arg("-c")
        .arg("import imageio_ffmpeg, sys; sys.stdout.write(imageio_ffmpeg.get_ffmpeg_exe())");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    crate::proc_util::hide_window_tokio(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(15), cmd.output()).await.ok()?.ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

async fn run_capture(
    app: &AppHandle,
    job_id: &str,
    tier: &str,
    cmd: &mut Command,
    timeout: Duration,
) -> Result<()> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    cmd.kill_on_drop(true);
    crate::proc_util::hide_window_tokio(cmd);

    let mut child = cmd.spawn().map_err(|e| anyhow!("spawn 失败: {}", e))?;
    if let Some(stdout) = child.stdout.take() {
        let app_c = app.clone();
        let jid = job_id.to_string();
        let tier_c = tier.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = app_c.emit(
                    "runtime_install_log",
                    &InstallLogPayload {
                        job_id: jid.clone(),
                        tier: tier_c.clone(),
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
        let tier_c = tier.to_string();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = app_c.emit(
                    "runtime_install_log",
                    &InstallLogPayload {
                        job_id: jid.clone(),
                        tier: tier_c.clone(),
                        line,
                        is_stderr: true,
                    },
                );
            }
        });
    }

    let status = tokio::time::timeout(timeout, child.wait())
        .await
        .map_err(|_| anyhow!("命令超时 ({}s)", timeout.as_secs()))?
        .map_err(|e| anyhow!("wait 失败: {}", e))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("命令退出码 {}", status.code().unwrap_or(-1)))
    }
}

fn emit_log(app: &AppHandle, job_id: &str, tier: &str, line: &str, is_stderr: bool) {
    let _ = app.emit(
        "runtime_install_log",
        &InstallLogPayload {
            job_id: job_id.to_string(),
            tier: tier.to_string(),
            line: line.to_string(),
            is_stderr,
        },
    );
}

fn first_mirror(mirrors: &[MirrorSource]) -> Option<MirrorSource> {
    mirrors.first().cloned()
}

fn sanitize_label(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn version_ge(v: (u32, u32, u32), min: &str) -> bool {
    let parts: Vec<u32> = min.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let m0 = parts.first().copied().unwrap_or(0);
    let m1 = parts.get(1).copied().unwrap_or(0);
    let m2 = parts.get(2).copied().unwrap_or(0);
    (v.0, v.1, v.2) >= (m0, m1, m2)
}

/// 删除一个 tier · UI 用
pub fn uninstall_tier(tier: &str) -> Result<()> {
    let dir = paths::venv_dir(tier);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| anyhow!("删除 venv 失败: {}", e))?;
    }
    let mut meta = read_installed_meta();
    meta.tiers.remove(tier);
    write_installed_meta(&meta)?;
    Ok(())
}

/// 重新探测一次已装 tier (UI 刷新用)
pub async fn recheck_tier(tier: &str) -> InstalledTier {
    let meta = read_installed_meta();
    let mut t = meta.tiers.get(tier).cloned().unwrap_or_default();
    let venv_py = paths::venv_python(tier);
    if !venv_py.exists() {
        t.ok = false;
        t.last_message = "venv 不存在".into();
        return t;
    }
    if let Some(v) = check_python(&venv_py).await {
        t.python = venv_py.to_string_lossy().to_string();
        t.last_message = format!("python v{}.{}.{}", v.0, v.1, v.2);
        t.ok = true;
    } else {
        t.ok = false;
        t.last_message = "venv python 探测失败".into();
    }
    t
}
