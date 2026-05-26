//! 自动更新检查器 (P1 · 热更新机制)
//!
//! 策略:
//!   1. 启动 30s 后第一次 check (避开启动峰值)
//!   2. 之后每 6h check 一次
//!   3. WS 收到 `update_required` 帧时 → 立即 check + emit
//!   4. 发现更新 → emit `update_available` 事件 · 前端弹提示
//!   5. 用户点"立即更新"由前端调 `install_update` (走老路径 · 下载 DMG)
//!
//! 不做静默自动安装 (避免突然杀进程影响在跑任务)

use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;
use tokio::time::{interval, sleep};

use crate::commands::{check_for_updates, UpdateInfo};

const FIRST_CHECK_DELAY_S: u64 = 30;
const PERIODIC_CHECK_HOURS: u64 = 6;

/// 全局触发通道 · WS push 时 sender.send(()) 唤醒立即 check
static TRIGGER_TX: std::sync::OnceLock<broadcast::Sender<()>> = std::sync::OnceLock::new();

/// 启动后台 updater · 在 main.rs setup hook 调一次
pub fn spawn_checker(app: AppHandle) {
    if TRIGGER_TX.get().is_some() {
        return;
    }
    let (tx, _) = broadcast::channel::<()>(4);
    let _ = TRIGGER_TX.set(tx.clone());

    // Fix (2026-05-26): Tauri setup hook 不在 tokio runtime context 内 ·
    // 直接 tokio::spawn 会 panic "there is no reactor running" ·
    // 改用 tauri::async_runtime::spawn (Tauri 自带的 tokio runtime · 全局可用)
    tauri::async_runtime::spawn(async move {
        // 首次延迟 (避开启动峰值)
        sleep(Duration::from_secs(FIRST_CHECK_DELAY_S)).await;
        let _ = check_and_emit(&app).await;

        // 周期 + 触发双轨
        let mut tick = interval(Duration::from_secs(PERIODIC_CHECK_HOURS * 3600));
        tick.tick().await; // 跳过第一个 (刚 check 过)
        let mut rx = tx.subscribe();
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let _ = check_and_emit(&app).await;
                }
                _ = rx.recv() => {
                    tracing::info!("auto_updater: triggered by WS push");
                    let _ = check_and_emit(&app).await;
                }
            }
        }
    });
    tracing::info!(
        "auto_updater: spawned · first={}s interval={}h",
        FIRST_CHECK_DELAY_S, PERIODIC_CHECK_HOURS
    );
}

/// 外部触发立即 check (供 ws_client 收到 update_required 帧时调)
pub fn trigger_check_now() {
    if let Some(tx) = TRIGGER_TX.get() {
        let _ = tx.send(());
    }
}

async fn check_and_emit(app: &AppHandle) -> Result<(), String> {
    match check_for_updates().await {
        Ok(info) if info.available => {
            tracing::info!(
                "auto_updater: update available · version={:?}",
                info.version
            );
            let _ = app.emit("update_available", &info);
            Ok(())
        }
        Ok(_) => {
            tracing::debug!("auto_updater: no updates");
            Ok(())
        }
        Err(e) => {
            tracing::warn!("auto_updater: check failed: {}", e);
            Err(e)
        }
    }
}

/// 给前端用的 manual 触发 (Tauri command 透传)
#[tauri::command]
pub async fn check_updates_now(app: AppHandle) -> Result<UpdateInfo, String> {
    let info = check_for_updates().await?;
    if info.available {
        let _ = app.emit("update_available", &info);
    }
    Ok(info)
}
