//! Tauri commands：暴露给 Vue UI 的 invoke 端点。

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::watch;

use crate::auth::{device_store, magic_link, node_store, password_auth, token_store};
use crate::state::{AppState, AppStateSnapshot, UserInfo};

fn emit_state(app: &AppHandle, state: &AppState) {
    let snap = state.snapshot();
    let _ = app.emit("connection_state_changed", &snap);
}

#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub fn get_state(state: State<'_, Arc<AppState>>) -> AppStateSnapshot {
    state.snapshot()
}

#[tauri::command]
pub async fn ws_connect(
    access_token: String,
    node_id: Option<String>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    if access_token.trim().is_empty() {
        return Err("access_token 不能为空".into());
    }

    // 先停掉旧 session（如果在跑）
    {
        let mut g = state.shutdown_tx.lock().unwrap();
        if let Some(tx) = g.take() {
            let _ = tx.send(true);
        }
    }

    // 新 watch channel 控制本次 session
    let (tx, rx) = watch::channel(false);
    {
        let mut g = state.shutdown_tx.lock().unwrap();
        *g = Some(tx);
    }
    {
        let mut g = state.inner.lock().unwrap();
        g.access_token = Some(access_token.clone());
    }

    let app_clone = app.clone();
    let state_clone: Arc<AppState> = (*state).clone();
    tokio::spawn(async move {
        tracing::info!("ws · v8 模式");
        crate::comm::v8_ws::run_v8_loop(access_token, node_id, state_clone, app_clone, rx).await;
    });

    Ok(())
}

#[tauri::command]
pub fn ws_disconnect(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut g = state.shutdown_tx.lock().unwrap();
    if let Some(tx) = g.take() {
        let _ = tx.send(true);
    }
    Ok(())
}

#[tauri::command]
pub fn set_mode(
    mode: String,
    throttle_pct: Option<u8>,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let m = mode.as_str();
    if !matches!(m, "active" | "paused" | "throttled") {
        return Err(format!("invalid mode: {}", m));
    }
    match throttle_pct {
        Some(p) => state.request_mode_with_pct(m, p),
        None => state.request_mode(m),
    }
    Ok(())
}

/// M3.5.2：用 0-100 滑杆驱动 mode
/// - pct = 0 → mode=paused
/// - pct = 100 → mode=active  
/// - 1..=99 → mode=throttled + throttle_pct=该值
#[tauri::command]
pub fn set_throttle(pct: u8, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let p = pct.min(100);
    let mode = match p {
        0 => "paused",
        100 => "active",
        _ => "throttled",
    };
    state.request_mode_with_pct(mode, p);
    Ok(())
}

// ═══════════════════ P0 NCE · 任务资源限制档位 ═══════════════════
//
// 三档 (UI 算力调节页设置):
//   eco       · nice +15 · 笔记本办公时几乎无感
//   balanced  · nice +10 · 默认
//   full      · nice 0   · 服务器/专机 · 全速
//
// 持久化: 当前进程内存中 · 重启重置默认 Balanced
// (UI 可记到 localStorage 跨重启 · 然后启动调一次)

#[tauri::command]
pub fn get_throttle_level() -> String {
    match crate::task::resource_limit::current_level() {
        crate::task::resource_limit::ThrottleLevel::Eco => "eco",
        crate::task::resource_limit::ThrottleLevel::Balanced => "balanced",
        crate::task::resource_limit::ThrottleLevel::Full => "full",
    }
    .to_string()
}

#[tauri::command]
pub fn set_throttle_level(level: String) -> Result<(), String> {
    let lv = crate::task::resource_limit::ThrottleLevel::from_str(&level);
    crate::task::resource_limit::set_level(lv);
    Ok(())
}

// ═══════════════════ M3.4：自动更新检查 ═══════════════════

#[derive(serde::Serialize, Clone, Debug)]
pub struct UpdateInfo {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pub_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
}

/// 自定义更新检查 —— 用 reqwest 绕过自签名证书问题
/// 服务端未实现时返回空/非 JSON 均视为"暂无更新"，不报错
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION");
    let target = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let url = format!(
        "https://www.wujisuanli.com/api/v8/client/updates/{target}/{arch}/{current}"
    );
    let client = reqwest::Client::builder()
        .user_agent(format!("EdgeCompute-Client/{}", current))
        .timeout(std::time::Duration::from_secs(15))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!("update check HTTP {}: {}", status.as_u16(), body);
        return Ok(UpdateInfo { available: false, version: None, notes: None, pub_date: None, download_url: None });
    }
    #[derive(serde::Deserialize)]
    struct UpdateResponse {
        version: Option<String>,
        notes: Option<String>,
        pub_date: Option<String>,
        download_url: Option<String>,
    }
    let ur: UpdateResponse = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("update check parse error (server may not implement yet): {}", e);
            return Ok(UpdateInfo { available: false, version: None, notes: None, pub_date: None, download_url: None });
        }
    };
    let available = ur.version.as_ref().map_or(false, |v| v != current);
    Ok(UpdateInfo {
        available,
        version: ur.version,
        notes: ur.notes,
        pub_date: ur.pub_date,
        download_url: ur.download_url,
    })
}

/// 自定义更新安装 —— 下载 DMG 并用系统打开
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let info = check_for_updates().await?;
    if !info.available {
        return Err("没有可用更新".to_string());
    }
    let download_url = info.download_url.ok_or("缺少下载地址")?;
    let filename = download_url
        .split('/')
        .last()
        .unwrap_or("EdgeCompute-Update.dmg");
    let tmp_dir = std::env::temp_dir();
    let dest = tmp_dir.join(filename);
    let client = reqwest::Client::builder()
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&download_url).send().await.map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    // macOS: 用 open 命令打开 DMG
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dest)
            .spawn()
            .map_err(|e| format!("打开失败: {}", e))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        opener::open(&dest).map_err(|e| format!("打开失败: {}", e))?;
    }
    Ok(())
}

/// 内部 helper：spawn 一个新的 WS session loop。
/// 调用前会先 cancel 旧 session（如有）。
fn spawn_ws_loop(
    access_token: String,
    node_id: Option<String>,
    state: Arc<AppState>,
    app: AppHandle,
) {
    {
        let mut g = state.shutdown_tx.lock().unwrap();
        if let Some(tx) = g.take() {
            let _ = tx.send(true);
        }
    }
    let (tx, rx) = watch::channel(false);
    {
        let mut g = state.shutdown_tx.lock().unwrap();
        *g = Some(tx);
    }
    state.set_access_token(Some(access_token.clone()));
    let st = state.clone();
    let ah = app.clone();
    tokio::spawn(async move {
        tracing::info!("ws (resume) · v8 模式");
        crate::comm::v8_ws::run_v8_loop(access_token, node_id, st, ah, rx).await;
    });
}

// ═══════════════════ M3.5.5：设置 + 诊断 ═══════════════════

#[derive(serde::Serialize, Clone, Debug)]
pub struct Diagnostics {
    pub client_version: &'static str,
    pub platform: &'static str,
    pub arch: &'static str,
    pub api_base: &'static str,
    pub ws_url: &'static str,
    pub session_kind: String,
    pub session_username: String,
    pub session_email: String,
    pub has_session: bool,
    pub node_id: Option<String>,
    pub connection_state: String,
    pub last_error: Option<String>,
    pub mode: String,
    pub throttle_pct: u8,
    pub data_dir: String,
}

#[tauri::command]
pub fn get_diagnostics(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Diagnostics {
    let snap = state.snapshot();
    let session = token_store::load_session_v2().ok().flatten();
    let data_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    Diagnostics {
        client_version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        api_base: "https://www.wujisuanli.com/api/v8",
        ws_url: "wss://www.wujisuanli.com/api/v8/ws/worker",
        session_kind: session.as_ref().map(|s| s.kind.clone()).unwrap_or_default(),
        session_username: session.as_ref().map(|s| s.username.clone()).unwrap_or_default(),
        session_email: session.as_ref().map(|s| s.email.clone()).unwrap_or_default(),
        has_session: session.is_some(),
        node_id: snap.node_id.clone(),
        connection_state: format!("{:?}", snap.connection_state).to_lowercase(),
        last_error: snap.last_error.clone(),
        mode: snap.mode.clone(),
        throttle_pct: snap.throttle_pct,
        data_dir,
    }
}

/// 清空所有本地数据（session.json / node_id.txt / device_name.txt）
/// 主要用于"切换账号"或"完全重置"
#[tauri::command]
pub fn reset_local_data(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // 先断 WS
    {
        let mut g = state.shutdown_tx.lock().unwrap();
        if let Some(tx) = g.take() {
            let _ = tx.send(true);
        }
    }
    let _ = token_store::clear_session();
    // node_id.txt + device_name.txt
    if let Ok(dir) = app.path().app_data_dir() {
        for f in &["node_id.txt", "device_name.txt"] {
            let p = dir.join(f);
            if p.exists() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    // 清 user state
    state.set_user(None);
    state.set_access_token(None);
    emit_state(&app, &state);
    Ok(())
}

#[tauri::command]
pub async fn open_data_dir(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let path_str = dir.display().to_string();
    app.shell()
        .open(&path_str, None)
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ═══════════════════ M3.5.3：系统信息 + 机器名 ═══════════════════

#[derive(serde::Serialize, Clone, Debug)]
pub struct DeviceOverview {
    pub device_name: String,
    pub system: crate::system_info::SystemInfo,
}

#[tauri::command]
pub fn get_system_info() -> DeviceOverview {
    let mut info = crate::system_info::collect();
    // 用户自定义名称优先
    if let Some(custom) = device_store::load() {
        info.device_name = custom;
    }
    DeviceOverview {
        device_name: info.device_name.clone(),
        system: info,
    }
}

#[tauri::command]
pub fn set_device_name(name: String) -> Result<DeviceOverview, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("机器名不能为空".into());
    }
    if trimmed.len() > 64 {
        return Err("机器名最多 64 个字符".into());
    }
    device_store::save(trimmed).map_err(|e| e.to_string())?;
    Ok(get_system_info())
}

// ═══════════════════ M3.5.1：用户名/密码登录 + 注册 ═══════════════════

fn user_info_from_password_resp(resp: &password_auth::LoginResp) -> UserInfo {
    UserInfo {
        id: resp.user.id.parse::<i64>().unwrap_or(0),
        username: resp.user.username.clone(),
        email: resp.user.email.clone(),
    }
}

fn pick_long_token(resp: &password_auth::LoginResp) -> String {
    // 优先用 agent_token（1 年长效），fallback access_token（15 min）
    resp.agent_token
        .clone()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| resp.access_token.clone())
}

#[tauri::command]
pub async fn auth_login(
    username: String,
    password: String,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UserInfo, String> {
    let u = username.trim();
    let p = password.as_str();
    if u.is_empty() || p.is_empty() {
        return Err("账号或密码不能为空".into());
    }
    let resp = password_auth::login(u, p)
        .await
        .map_err(|e| e.to_string())?;
    let info = user_info_from_password_resp(&resp);
    let long_token = pick_long_token(&resp);
    if let Err(e) = token_store::save_session_v2(
        &long_token,
        &info.email,
        "agent",
        &info.username,
    ) {
        tracing::warn!("save session (login) failed: {}", e);
    }
    state.set_user(Some(info.clone()));
    emit_state(&app, &state);
    let pnode = node_store::load_node_id();
    spawn_ws_loop(long_token, pnode, (*state).clone(), app);
    Ok(info)
}

#[tauri::command]
pub async fn auth_register(
    username: String,
    email: String,
    password: String,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UserInfo, String> {
    let u = username.trim();
    let e_in = email.trim();
    let p = password.as_str();
    if u.len() < 3 {
        return Err("账号至少 3 个字符".into());
    }
    if p.len() < 6 {
        return Err("密码至少 6 位".into());
    }
    let resp = password_auth::register(u, e_in, p)
        .await
        .map_err(|e| e.to_string())?;
    let info = user_info_from_password_resp(&resp);
    let long_token = pick_long_token(&resp);
    if let Err(e) = token_store::save_session_v2(
        &long_token,
        &info.email,
        "agent",
        &info.username,
    ) {
        tracing::warn!("save session (register) failed: {}", e);
    }
    state.set_user(Some(info.clone()));
    emit_state(&app, &state);
    let pnode = node_store::load_node_id();
    spawn_ws_loop(long_token, pnode, (*state).clone(), app);
    Ok(info)
}

// ═══════════════════ Magic-link 登录（兼容保留） ═══════════════════

#[tauri::command]
pub async fn auth_send_code(email: String) -> Result<u64, String> {
    let email = email.trim();
    if email.is_empty() {
        return Err("邮箱不能为空".into());
    }
    let r = magic_link::send_code(email).await.map_err(|e| e.to_string())?;
    Ok(r.expires_in)
}

#[tauri::command]
pub async fn auth_verify(
    email: String,
    code: String,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UserInfo, String> {
    let resp = magic_link::verify(email.trim(), code.trim())
        .await
        .map_err(|e| e.to_string())?;

    // 持久化：refresh_token + email 落 OS Keyring
    if let Err(e) = token_store::save_session(&resp.refresh_token, &resp.user.email) {
        tracing::warn!("save_session to keyring failed: {}", e);
    }

    let user_info = UserInfo {
        id: resp.user.id,
        username: resp.user.username.clone(),
        email: resp.user.email.clone(),
    };
    state.set_user(Some(user_info.clone()));
    emit_state(&app, &state);

    // 自动启动 WS 会话；复用持久化 node_id（如有）
    let pnode = node_store::load_node_id();
    spawn_ws_loop(resp.access_token, pnode, (*state).clone(), app);

    Ok(user_info)
}

#[tauri::command]
pub async fn auth_restore(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<UserInfo>, String> {
    let Some(sess) = token_store::load_session_v2().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if sess.token.is_empty() {
        return Ok(None);
    }
    let access_token = match sess.kind.as_str() {
        // M3.5.1：密码登录的 agent_token 1 年长效，直接使用
        "agent" => sess.token.clone(),
        // M2.3：magic-link 流程，refresh_token 换新 access
        _ => match magic_link::refresh(&sess.token).await {
            Ok(r) => r.access_token,
            Err(e) => {
                tracing::warn!("auth_restore refresh failed: {}", e);
                let _ = token_store::clear_session();
                return Ok(None);
            }
        },
    };
    let username = if !sess.username.is_empty() {
        sess.username.clone()
    } else {
        sess.email.split('@').next().unwrap_or("").to_string()
    };
    let user_info = UserInfo {
        id: 0,
        username,
        email: sess.email.clone(),
    };
    state.set_user(Some(user_info.clone()));
    emit_state(&app, &state);
    let pnode = node_store::load_node_id();
    spawn_ws_loop(access_token, pnode, (*state).clone(), app);
    Ok(Some(user_info))
}

// ═══════════════════ M3.2：账户/历史拉取 ═══════════════════

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct AccountSummary {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub balance: f64,
    pub total_earnings: f64,
    pub completed_tasks: u64,
    #[serde(default)]
    pub status: String,
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct MyHistoryItem {
    pub task_id: String,
    #[serde(default)]
    pub node_id: String,
    pub owner_id: i64,
    #[serde(default)]
    pub cmd: Option<String>,
    pub reward: f64,
    #[serde(default)]
    pub assigned_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    pub status: String,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub output: String,
    #[serde(default)]
    pub error: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct MyHistory {
    pub total: usize,
    pub items: Vec<MyHistoryItem>,
}

const API_BASE: &str = "https://www.wujisuanli.com/api/v8";

async fn authed_get<T: for<'de> serde::Deserialize<'de>>(
    state: &AppState,
    path: &str,
) -> Result<T, String> {
    let token = state
        .access_token()
        .ok_or_else(|| "未登录（无 access_token）".to_string())?;
    let client = reqwest::Client::builder()
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(8))
        .danger_accept_invalid_certs(true) // www.wujisuanli.com 自签名证书
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}{}", API_BASE, path);
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| crate::api_error::parse_request_error(&e))?;
    let status = resp.status();
    let txt = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::api_error::parse_api_error(status.as_u16(), &txt));
    }
    serde_json::from_str::<T>(&txt).map_err(|e| format!("响应解析失败: {}", e))
}

#[tauri::command]
pub async fn get_my_account(
    state: State<'_, Arc<AppState>>,
) -> Result<AccountSummary, String> {
    authed_get(state.inner().as_ref(), "/users/me").await
}

#[tauri::command]
pub async fn get_my_history(
    state: State<'_, Arc<AppState>>,
    limit: Option<u32>,
) -> Result<MyHistory, String> {
    let n = limit.unwrap_or(20).min(200);
    let path = format!("/users/me/v3-history?limit={}", n);
    authed_get(state.inner().as_ref(), &path).await
}

#[derive(serde::Deserialize, serde::Serialize, Clone, Debug)]
pub struct EarningPoint {
    pub date: String,
    pub earnings: f64,
    pub count: u64,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct EarningSeries {
    pub days: u32,
    pub series: Vec<EarningPoint>,
}

#[tauri::command]
pub async fn get_my_earnings(
    state: State<'_, Arc<AppState>>,
    days: Option<u32>,
) -> Result<EarningSeries, String> {
    let d = days.unwrap_or(7).clamp(1, 90);
    let path = format!("/users/me/v3-earnings?days={}", d);
    authed_get(state.inner().as_ref(), &path).await
}


/// 通用 HTTP GET 代理 —— 绕过 WebKit 自签名证书限制
/// Vue 前端用 invoke("api_get", { url, token? }) 替代原生 fetch
/// 如果不传 token，自动从 AppState 取当前 access_token
#[tauri::command]
pub async fn api_get(
    url: String,
    token: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let effective_token = token
        .filter(|t| !t.is_empty())
        .or_else(|| state.access_token());
    let client = reqwest::Client::builder()
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .danger_accept_invalid_certs(true)
        // 2026-05-25 8.0.15: 强制 HTTP/1.1 · 避免 H2 stream reset / chunked 解码错
        .http1_only()
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client
        .get(&url)
        // 拒绝任何 content-encoding · 不让中间代理 gzip/br 压缩 · reqwest 无 gzip feature 时解压会炸
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if let Some(t) = effective_token {
        if !t.is_empty() {
            req = req.bearer_auth(t);
        }
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    // 8.0.15: 用 chunk() 流式收集 · 即使中途断流也能拿到 partial body 不致全空
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp;
    let mut chunk_err: Option<String> = None;
    loop {
        match stream.chunk().await {
            Ok(Some(chunk)) => buf.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(e) => {
                chunk_err = Some(format!("chunk read · 已收 {} bytes · err: {}", buf.len(), e));
                tracing::warn!("api_get · {}", chunk_err.as_ref().unwrap());
                break;
            }
        }
    }
    let body = String::from_utf8_lossy(&buf).to_string();
    tracing::debug!("api_get · status={} bytes={} url={}", status.as_u16(), buf.len(), url);
    if !status.is_success() {
        return Err(crate::api_error::parse_api_error(status.as_u16(), &body));
    }
    // 若 chunk 异常但已收到一些 body · 仍返回 (Vue 那边 JSON.parse 会自己判断)
    if buf.is_empty() {
        if let Some(e) = chunk_err {
            return Err(format!("响应体读取失败 · {}", e));
        }
    }
    Ok(body)
}

/// 通用 HTTP POST JSON 代理 —— 配合 api_get 使用
/// Vue 前端: invoke("api_post", { url, body: <object>, token? })
/// body 在 Rust 侧用 serde_json::Value 接 · 透传 JSON 对象到服务端
#[tauri::command]
pub async fn api_post(
    url: String,
    body: serde_json::Value,
    token: Option<String>,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    let effective_token = token
        .filter(|t| !t.is_empty())
        .or_else(|| state.access_token());
    let client = reqwest::Client::builder()
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.post(&url).json(&body);
    if let Some(t) = effective_token {
        if !t.is_empty() {
            req = req.bearer_auth(t);
        }
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    // 2026-05-25 8.0.14 · 同 api_get bug 修复: 用 bytes() + utf8_lossy 取代 text()
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取响应体失败: {}", e))?;
    let body_text = String::from_utf8_lossy(&bytes).to_string();
    if !status.is_success() {
        return Err(crate::api_error::parse_api_error(status.as_u16(), &body_text));
    }
    Ok(body_text)
}

/// 持久化 5 能力同意状态到磁盘 · webview 在 setConsents/toggleConsent 后调用
/// data 透传前端 ConsentState 结构 ({ consents, agreedToS, agreedPrivacy, confirmedAtMs })
/// 下次 v8_ws 启动时 load 进 hello payload 上报后端
#[tauri::command]
pub fn save_capability_consent(data: serde_json::Value) -> Result<(), String> {
    crate::auth::consent_store::save(&data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn auth_logout(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    // 断 WS
    {
        let mut g = state.shutdown_tx.lock().unwrap();
        if let Some(tx) = g.take() {
            let _ = tx.send(true);
        }
    }
    // 清凭证
    let _ = token_store::clear_session();
    state.set_user(None);
    state.set_access_token(None);
    {
        let mut g = state.inner.lock().unwrap();
        g.node_id = None;
        g.owner_id = None;
        g.server_version = None;
        g.last_error = None;
    }
    emit_state(&app, &state);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// V4 P1 · 「我的 AI 能力」面板 — 暴露 skill_registry 给 Vue UI
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub dir: String,
    pub verified: bool,
    pub tools: Vec<ToolInfo>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub entry_file: String,
    pub timeout_s: u64,
    pub deterministic: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillRegistrySnapshot {
    pub skills_count: usize,
    pub tools_count: usize,
    pub scan_root_hint: String,
    pub skills: Vec<SkillInfo>,
}

/// 列出本机已安装的全部技能集（驱动 AICapabilityPage.vue）。
#[tauri::command]
pub fn list_installed_skills() -> SkillRegistrySnapshot {
    use crate::task::skill_registry;

    let reg = skill_registry::global();
    let mut skills: Vec<SkillInfo> = Vec::new();

    // 注：reg 没有公开 iter，借助 list_ids_versioned 拿 ID 列表，逐个 get
    for id_versioned in reg.list_ids_versioned() {
        // "text-tools-v1@1.0.0" → "text-tools-v1"
        let id = id_versioned.split('@').next().unwrap_or(&id_versioned).to_string();
        let Some(skill) = reg.get(&id) else { continue };

        let tools: Vec<ToolInfo> = skill
            .tools
            .iter()
            .map(|t| ToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
                entry_file: t.entry_file.to_string_lossy().to_string(),
                timeout_s: t.timeout_s,
                deterministic: t.deterministic,
            })
            .collect();

        skills.push(SkillInfo {
            id: skill.id.clone(),
            name: skill.name.clone(),
            version: skill.version.clone(),
            // Skill 运行时结构没保留 description/category/tags（被 Manifest 吸收了），
            // 先留空字符串，未来如果要展示再扩 skill_registry 公开字段。
            description: String::new(),
            category: String::new(),
            tags: Vec::new(),
            dir: skill.dir.to_string_lossy().to_string(),
            verified: skill.verified,
            tools,
        });
    }

    let scan_root_hint = default_skill_root_hint();

    SkillRegistrySnapshot {
        skills_count: reg.skills_count(),
        tools_count: reg.tools_count(),
        scan_root_hint,
        skills,
    }
}

fn default_skill_root_hint() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .map(|p| format!("{}\\EdgeCompute\\skills\\", p))
            .unwrap_or_else(|_| "%LOCALAPPDATA%\\EdgeCompute\\skills\\".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(|p| format!("{}/.local/lib/edgecompute/skills/", p))
            .unwrap_or_else(|_| "~/.local/lib/edgecompute/skills/".to_string())
    }
}

// ── tracing 日志字段访问器 ────────────────────────────────────────────────

/// 从 tracing Event 中提取 message 字段。
pub struct LogVisitor<'a>(pub &'a mut String);

impl tracing::field::Visit for LogVisitor<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            let _ = write!(self.0, "{:?}", value);
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}

// ── 全局日志 ring buffer（最近 500 条）────────────────────────────────────
use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;

static LOG_BUFFER: StdMutex<Option<VecDeque<String>>> = StdMutex::new(None);
const LOG_BUFFER_CAP: usize = 500;

/// 初始化日志缓冲（在 lib.rs 启动时调用一次）。
pub fn init_log_buffer() {
    let mut g = LOG_BUFFER.lock().unwrap();
    *g = Some(VecDeque::with_capacity(LOG_BUFFER_CAP));
}

/// 向日志缓冲追加一行（由自定义 tracing layer 调用）。
pub fn push_log_line(line: String) {
    if let Ok(mut g) = LOG_BUFFER.lock() {
        if let Some(buf) = g.as_mut() {
            if buf.len() >= LOG_BUFFER_CAP {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    }
}

/// P0-2: 记录 panic 到日志缓冲（由 panic hook 调用，确保崩溃可排查）。
/// 用 lock() 兜底而不是 unwrap，因为 panic hook 自身不能再 panic。
pub fn record_panic(location: &str, payload: &str) {
    let line = format!(
        "[{}] PANIC at {} — {}",
        chrono::Utc::now().to_rfc3339(),
        location,
        payload,
    );
    if let Ok(mut g) = LOG_BUFFER.lock() {
        if let Some(buf) = g.as_mut() {
            if buf.len() >= LOG_BUFFER_CAP {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    }
}

/// 返回最近 `limit` 条日志（供诊断页展示）。
#[tauri::command]
pub fn get_recent_logs(limit: Option<usize>) -> Vec<String> {
    let n = limit.unwrap_or(200).min(LOG_BUFFER_CAP);
    if let Ok(g) = LOG_BUFFER.lock() {
        if let Some(buf) = g.as_ref() {
            let start = buf.len().saturating_sub(n);
            return buf.iter().skip(start).cloned().collect();
        }
    }
    vec![]
}

// ═══════════════════ 老版本检测 ═══════════════════

#[derive(Debug, Clone, serde::Serialize)]
pub struct OldVersionInfo {
    pub found: bool,
    pub old_processes: Vec<String>,
    pub old_data_dirs: Vec<String>,
    pub current_version: String,
}

// 2026-05-25 · P0 修复: 只匹配真正老 brand (v1/v2 EdgeCompute)
// 不再匹配 qianshou/千手 · 因为这是当前 brand · 会误命中自己 spawn 的 worker/python 子进程
// → 误判老进程 → kill_old_processes 把自己 worker -9 → 节点离线
// 同版本间残留进程不靠 process 检测 (无法可靠区分自己子进程 vs 旧版本子进程)
// 而是靠用户启动新版前正常 quit 旧版 · 或 macOS 单实例锁
const OLD_PROCESS_PATTERNS: &[&str] = &[
    "EdgeCompute", "edgecompute",
    "com.edgecompute",
];
const OLD_DIR_PATTERNS: &[&str] = &[
    "edgecompute", "EdgeCompute",
    "qianshou", "QianShou", "Qianshou",
    "com.edgecompute", "com.qianshou",
    "千手",
];

/// 检测是否有旧版本千手 / EdgeCompute 进程在运行，或旧数据目录残留。
/// 用于启动时（或诊断页）提示用户清理，避免新旧客户端抢登录。
#[tauri::command]
pub fn check_old_versions(app: AppHandle) -> OldVersionInfo {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let self_pid = std::process::id();
    let mut old_processes: Vec<String> = Vec::new();
    let mut old_data_dirs: Vec<String> = Vec::new();

    // 检测旧进程（macOS · 用 pgrep -fl 拿到 PID + 命令行）
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for pat in OLD_PROCESS_PATTERNS {
            if let Ok(output) = Command::new("pgrep").args(["-fl", pat]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    // 跳过自己
                    if let Some(pid_str) = line.split_whitespace().next() {
                        if pid_str.parse::<u32>().ok() == Some(self_pid) { continue; }
                    }
                    // 跳过当前版本（兼顾 QianShou 1.2.0 老进程 vs 1.2.0 新进程：版本号相同则视为非旧）
                    if line.contains(&current_version) { continue; }
                    if seen.insert(line.to_string()) {
                        old_processes.push(line.to_string());
                    }
                }
            }
        }
    }

    // 检测旧进程（Windows · 用 tasklist）
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut tasklist = Command::new("tasklist");
        tasklist.args(["/FO", "CSV", "/NH"]);
        crate::proc_util::hide_window_std(&mut tasklist);
        if let Ok(output) = tasklist.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let lower = line.to_lowercase();
                let matched = OLD_PROCESS_PATTERNS.iter().any(|p| lower.contains(&p.to_lowercase()));
                if !matched { continue; }
                // CSV 第一段是 image name，第二段是 PID
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 2 {
                    let pid_str = parts[1].trim_matches('"');
                    if pid_str.parse::<u32>().ok() == Some(self_pid) { continue; }
                }
                let entry = line.trim().to_string();
                if !entry.is_empty() && seen.insert(entry.clone()) {
                    old_processes.push(entry);
                }
            }
        }
    }

    // 检测旧数据目录
    if let Ok(data_dir) = app.path().app_data_dir() {
        if let Some(parent) = data_dir.parent() {
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let matched = OLD_DIR_PATTERNS.iter().any(|p| name.contains(p));
                    if !matched { continue; }
                    let path = entry.path();
                    if path != data_dir && path.is_dir() {
                        old_data_dirs.push(path.display().to_string());
                    }
                }
            }
        }
    }

    OldVersionInfo {
        found: !old_processes.is_empty() || !old_data_dirs.is_empty(),
        old_processes,
        old_data_dirs,
        current_version,
    }
}

/// 杀掉旧版本进程（macOS + Windows）
#[tauri::command]
pub fn kill_old_processes() -> Result<Vec<String>, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let self_pid = std::process::id();
    let mut killed: Vec<String> = Vec::new();
    let mut killed_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        for pat in OLD_PROCESS_PATTERNS {
            if let Ok(output) = Command::new("pgrep").args(["-fl", pat]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.is_empty() { continue; }
                    let Some(pid_str) = line.split_whitespace().next() else { continue; };
                    let Ok(pid) = pid_str.parse::<u32>() else { continue; };
                    if pid == self_pid { continue; }
                    if line.contains(&current_version) { continue; }
                    if !killed_pids.insert(pid) { continue; }
                    let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
                    killed.push(format!("PID {} 已终止 ({})", pid, line));
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let mut tasklist = Command::new("tasklist");
        tasklist.args(["/FO", "CSV", "/NH"]);
        crate::proc_util::hide_window_std(&mut tasklist);
        if let Ok(output) = tasklist.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let lower = line.to_lowercase();
                let matched = OLD_PROCESS_PATTERNS.iter().any(|p| lower.contains(&p.to_lowercase()));
                if !matched { continue; }
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() < 2 { continue; }
                let pid_str = parts[1].trim_matches('"');
                let Ok(pid) = pid_str.parse::<u32>() else { continue; };
                if pid == self_pid { continue; }
                if !killed_pids.insert(pid) { continue; }
                let mut tk = Command::new("taskkill");
                tk.args(["/F", "/PID", &pid.to_string()]);
                crate::proc_util::hide_window_std(&mut tk);
                let _ = tk.output();
                killed.push(format!("PID {} 已终止", pid));
            }
        }
    }

    Ok(killed)
}

/// 删除旧数据目录（匹配所有品牌名 · 跳过当前数据目录自身）
#[tauri::command]
pub fn clean_old_data_dirs(app: AppHandle) -> Result<Vec<String>, String> {
    let mut cleaned: Vec<String> = Vec::new();
    if let Ok(data_dir) = app.path().app_data_dir() {
        if let Some(parent) = data_dir.parent() {
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let matched = OLD_DIR_PATTERNS.iter().any(|p| name.contains(p));
                    if !matched { continue; }
                    let path = entry.path();
                    if path != data_dir && path.is_dir() {
                        if let Err(e) = std::fs::remove_dir_all(&path) {
                            tracing::warn!("无法删除旧数据目录 {}: {}", path.display(), e);
                        } else {
                            cleaned.push(path.display().to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(cleaned)
}

// ═══════════════════ 记住账号 ═══════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RememberedAccount {
    username: String,
}

fn remembered_account_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("remembered_account.json"))
}

/// 保存记住的账号名
#[tauri::command]
pub fn save_remembered_account(app: AppHandle, username: String) -> Result<(), String> {
    let path = remembered_account_path(&app).ok_or("无法获取数据目录")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let blob = RememberedAccount { username };
    let json = serde_json::to_string_pretty(&blob).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取记住的账号名
#[tauri::command]
pub fn load_remembered_account(app: AppHandle) -> Result<Option<String>, String> {
    let path = remembered_account_path(&app).ok_or("无法获取数据目录")?;
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let blob: RememberedAccount = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(Some(blob.username))
}

/// 清除记住的账号
#[tauri::command]
pub fn clear_remembered_account(app: AppHandle) -> Result<(), String> {
    let path = remembered_account_path(&app).ok_or("无法获取数据目录")?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
