//! EdgeCompute Desktop Client v3.0.0
//!
//! - M2.2：Rust WebSocket 长连 + 状态机
//! - M2.3：magic-link 登录 + session.json 持久化
//! - M2.4：node_id 持久化 + graceful exit on close

use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, RunEvent, WindowEvent};

mod api_error;  // 2026-05-26 · 后端错误友好化 (auth/commands 都用)
mod auth;
mod auto_updater;
mod commands;
mod comm;
mod crash_reporter;
mod runtime_monitor;
mod state;
mod system_info;
mod benchmark;
mod hardware_capabilities;
mod task;
mod toolbox;
mod proc_util;
mod runtime;
mod proxy;  // W3 (2026-05-26) · IP 代理池节点端

/// 简单的 tracing Layer：把格式化日志行写入内存 ring buffer。
struct MemLogLayer;

impl<S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>>
    tracing_subscriber::Layer<S> for MemLogLayer
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let meta = event.metadata();
        let level = meta.level();
        let target = meta.target();
        let mut msg = String::new();
        let mut visitor = crate::commands::LogVisitor(&mut msg);
        event.record(&mut visitor);
        let line = format!(
            "[{}] {} {} — {}",
            chrono::Local::now().format("%H:%M:%S"),
            level,
            target,
            msg.trim()
        );
        crate::commands::push_log_line(line);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    commands::init_log_buffer();

    // P0-2: panic hook — 把 panic 信息记到 tracing 而非沉默 abort，
    // 便于排障 + 后续接 Sentry。原 hook 仍然链式调用以保留 Tauri 默认行为。
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // 提取 panic 位置和消息
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };
        tracing::error!(target: "panic", location = %loc, payload = %payload, "panic 触发");
        // 也写入内存 log buffer，方便用户上报
        commands::record_panic(&loc, &payload);
        // P0 NCE · 落盘 · 下次启动 crash_reporter::check_and_report 上报 server
        crash_reporter::write_to_disk(&loc, &payload);
        prev_hook(info);
    }));

    let filter = std::env::var("EDGECOMPUTE_LOG")
        .unwrap_or_else(|_| "info,edgecompute_client_lib=debug".to_string());
    use tracing_subscriber::prelude::*;
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(
            tracing_subscriber::EnvFilter::new(&filter),
        ))
        .with(MemLogLayer.with_filter(tracing_subscriber::EnvFilter::new(&filter)))
        .try_init();

    let app_state: Arc<state::AppState> = Arc::new(state::AppState::new());
    let state_for_exit = app_state.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::app_version,
            commands::get_state,
            commands::ws_connect,
            commands::ws_disconnect,
            commands::auth_send_code,
            commands::auth_verify,
            commands::auth_restore,
            commands::auth_logout,
            commands::auth_login,
            commands::auth_register,
            commands::get_my_account,
            commands::get_my_history,
            commands::get_my_earnings,
            commands::set_mode,
            commands::set_throttle,
            commands::get_throttle_level,
            commands::set_throttle_level,
            commands::check_for_updates,
            commands::install_update,
            auto_updater::check_updates_now,
            commands::get_system_info,
            commands::set_device_name,
            commands::get_diagnostics,
            commands::reset_local_data,
            commands::open_data_dir,
            commands::list_installed_skills,
            commands::get_recent_logs,
            commands::api_get,
            commands::api_post,
            commands::save_capability_consent,
            commands::check_old_versions,
            commands::kill_old_processes,
            commands::clean_old_data_dirs,
            commands::save_remembered_account,
            commands::load_remembered_account,
            commands::clear_remembered_account,
            toolbox::detect_deps,
            toolbox::install_dep,
            runtime::commands::runtime_fetch_manifest,
            runtime::commands::runtime_get_installed,
            runtime::commands::runtime_install_tier,
            runtime::commands::runtime_uninstall_tier,
            runtime::commands::runtime_recheck,
            runtime::commands::runtime_host_python,
        ])
        .setup(move |app| {
            // M2.4：初始化持久化路径（refresh_token + node_id 都落 app_data 目录）
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法解析 app_data 目录: {}", e))?;
            tracing::info!("app_data dir: {}", app_data.display());
            if let Err(e) = auth::token_store::init_session_path(&app_data) {
                tracing::warn!("token_store init failed: {}", e);
            }
            if let Err(e) = auth::node_store::init_node_id_path(&app_data) {
                tracing::warn!("node_store init failed: {}", e);
            }
            if let Err(e) = auth::device_store::init_path(&app_data) {
                tracing::warn!("device_store init failed: {}", e);
            }
            if let Err(e) = auth::consent_store::init_path(&app_data) {
                tracing::warn!("consent_store init failed: {}", e);
            }
            // P0 NCE · 运行时采样器 (CPU/mem/uptime/restart_count + GPU 探测)
            runtime_monitor::init(&app_data);
            // P1 NCE · shard_result 离线缓存初始化 (WS 断时落盘 · 重连后 flush)
            comm::result_queue::init(&app_data);
            // P1 · 热更新 · 后台 6h check + WS push 触发
            auto_updater::spawn_checker(app.handle().clone());
            // P0 NCE · 崩溃上报 · 初始化路径 + 启动检查上次崩溃并上报
            crash_reporter::init(&app_data);
            let api_base = std::env::var("EDGECOMPUTE_API_BASE")
                .unwrap_or_else(|_| "https://www.wujisuanli.com".to_string());
            crash_reporter::check_and_report(api_base);
            // P0 NCE · 磁盘自动清理 · 启动 10s 后首跑 + 24h 周期
            runtime::garbage_collect::spawn(app.handle().clone());

            // 2026-05-23 · 首次启动 · 从 bundle resources/runtime/ 拷贝预烘焙 Python + envs
            // (失败也不阻断 app 启动 · installer 会走 uv install 路径兜底)
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = runtime::bootstrap_bundled::ensure_bundled_runtime(&app_handle).await {
                    tracing::warn!("bootstrap_bundled 失败: {} (走老 uv install 路径)", e);
                }
            });

            // V8.1 (2026-05-27) · 首启自动装 manifest.tiers 标 auto_install=true 的 tier
            // 当前: 仅 lite (基础包 pillow numpy 等) · executor 路由的兜底 venv
            // 老后端不发 auto_install 字段 = false · 此函数等于 no-op (兼容)
            runtime::auto_install_tiers::spawn_auto_install(app.handle().clone());

            // 2026-05-28 · skill_registry 周期性预扫 · 保证全新装机 · auto_install 装完 skill
            //              后无需重启即可被 executor.v2 path 找到 (skill_registry 是可刷新版)
            //              第 1 次 5s 后 (启动期可能还没装好 · 扫到啥算啥)
            //              第 2 次 60s 后 (lite tier venv + 4 v2 skill 通常已就绪)
            //              第 3 次 180s 后 (大 tier 装久的兜底)
            tauri::async_runtime::spawn(async move {
                for (idx, delay_s) in [5u64, 60, 180].iter().enumerate() {
                    tokio::time::sleep(std::time::Duration::from_secs(*delay_s)).await;
                    let n = crate::task::skill_registry::refresh();
                    tracing::info!("skill_registry · 启动期第 {} 次预扫 · {} 个 skill", idx + 1, n);
                }
            });
            // M3.3：系统托盘（菜单 + 关闭主窗口最小化到托盘）
            let show_item = MenuItem::with_id(app, "show", "打开主面板", true, None::<&str>)?;
            let pause_item = MenuItem::with_id(app, "pause", "暂停贡献", true, None::<&str>)?;
            let resume_item = MenuItem::with_id(app, "resume", "恢复贡献", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 EdgeCompute", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[
                &show_item, &pause_item, &resume_item, &sep1, &quit_item,
            ])?;
            // P0-2: 真实危险 unwrap 修复 — default_window_icon 缺失时不应 panic，
            // 退而求其次用 builder 默认（不带 icon 也能工作，只是托盘不好看）。
            let mut tray_builder = TrayIconBuilder::with_id("main");
            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            } else {
                tracing::warn!("default_window_icon() returned None — tray icon will be empty");
            }
            tray_builder
                .tooltip("EdgeCompute v3.0.0")
                .title("EC")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "pause" => {
                            if let Some(s) = app.try_state::<Arc<state::AppState>>() {
                                s.request_mode("paused");
                            }
                        }
                        "resume" => {
                            if let Some(s) = app.try_state::<Arc<state::AppState>>() {
                                s.request_mode("active");
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // v2 pull daemon — 已废弃，全部走 v8 ws

            tracing::info!(
                "EdgeCompute client v{} started",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        })
        // M3.3：关闭窗口 → 隐藏到托盘（不真退出）
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                tracing::info!("window close prevented, hidden to tray");
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(move |_app_handle, event| {
        // 真正退出之前再保险一次：如果 WS 还在跑，触发 shutdown 并等 200ms
        if let RunEvent::ExitRequested { .. } = event {
            let mut g = state_for_exit.shutdown_tx.lock().unwrap();
            if let Some(tx) = g.take() {
                let _ = tx.send(true);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
}
