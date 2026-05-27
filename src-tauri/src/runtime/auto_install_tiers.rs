//! V8.1 (2026-05-27) · 启动时自动装 manifest 标 auto_install=true 的 tier
//!
//! 触发: app.setup() · 跟 bootstrap_bundled 并行 (异步 spawn)
//!
//! 流程:
//!   1. 等 5s · 让 UI 起来 + 网络/auth 稳定
//!   2. 拉 manifest (走老 paths · 失败重试 30s · 不阻塞)
//!   3. 遍历 manifest.tiers · 找 auto_install=true 的
//!   4. 对每个 · 检查 venvs/<tier>/bin/python 是否存在
//!   5. 不存在 → spawn installer::install_tier (后台跑 · UI 通过现有 runtime_install_log 事件可见)
//!
//! 双端: paths::venv_python 已 cfg(target_os="windows") 区分 · 无需特殊处理
//!
//! 老后端 (v8.0.x) 不发 auto_install 字段 · TierSpec.auto_install 默认 false · 不触发任何安装 (向后兼容)

use std::time::Duration;

use tauri::AppHandle;

use super::{installer, manifest, paths};

/// 启动时调一次 · 后台跑 · 不阻塞 app 启动
pub fn spawn_auto_install(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // 等 UI 起来 · 避免装 tier 跟 auth 抢资源
        tokio::time::sleep(Duration::from_secs(5)).await;
        if let Err(e) = run_auto_install(&app).await {
            tracing::warn!("auto_install_tiers · 失败 (不致命): {}", e);
        }
    });
}

async fn run_auto_install(app: &AppHandle) -> anyhow::Result<()> {
    // 拉 manifest · 失败重试 (最多 3 次 · 间隔 30s)
    let mut m_opt: Option<manifest::RuntimeManifest> = None;
    for attempt in 1..=3 {
        match manifest::fetch().await {
            Ok(m) => {
                m_opt = Some(m);
                break;
            }
            Err(e) => {
                tracing::warn!(
                    "auto_install_tiers · 拉 manifest 失败 (attempt {}/3): {}",
                    attempt, e
                );
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    }
    let m = match m_opt {
        Some(m) => m,
        None => return Err(anyhow::anyhow!("拉 manifest 失败 3 次 · 放弃 · 用户可手动装")),
    };

    // 找所有 auto_install=true 的 tier
    let auto_tiers: Vec<String> = m
        .tiers
        .iter()
        .filter(|(_, spec)| spec.auto_install)
        .map(|(name, _)| name.clone())
        .collect();

    if auto_tiers.is_empty() {
        tracing::info!("auto_install_tiers · manifest 无 auto_install tier · 跳过");
        return Ok(());
    }

    tracing::info!("auto_install_tiers · 检查 {} 个 auto_install tier", auto_tiers.len());

    for tier in &auto_tiers {
        let venv_py = paths::venv_python(tier);
        if venv_py.exists() {
            tracing::info!("auto_install_tiers · {} 已就绪 ({}) · 跳过", tier, venv_py.display());
            continue;
        }

        // 没装 → 触发 installer · 后台异步跑
        tracing::info!("auto_install_tiers · {} 未装 · 开始自动安装", tier);
        let app_clone = app.clone();
        let tier_clone = tier.clone();
        // installer::install_tier 本身就是异步 spawn · 立即返回 job_id
        match installer::install_tier(app_clone, tier_clone).await {
            Ok(job_id) => {
                tracing::info!("auto_install_tiers · {} 已派发 job={}", tier, job_id);
            }
            Err(e) => {
                tracing::warn!("auto_install_tiers · {} 派发失败: {}", tier, e);
            }
        }

        // 间隔 2s 再启下一个 · 避免同时跑多个 uv 抢网络
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    Ok(())
}
