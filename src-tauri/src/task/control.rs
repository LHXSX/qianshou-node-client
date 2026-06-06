//! 后端下发 control 指令执行器 (白名单自愈 · 2026-06-05)
//!
//! 闭环: 后端 decider/admin 下发 control 帧 → 本模块按*白名单 action* 执行修复
//!       (复用 installer/fix_venv/detector) → 回报 ControlResult。
//!
//! 安全铁律:
//!   - action 严格白名单 (CONTROL_ACTIONS) · 未知 action 直接拒绝 (绝不执行任意命令)
//!   - control_id 幂等去重 · 防重放
//!   - expires_at_ms 过期校验 · 防过期指令重放
//!   - 是客户端本地自愈 (self_heal.rs) 的增强 · 后端主导针对性修复

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Instant;

use tauri::AppHandle;

use crate::comm::v8_proto::{ControlPayload, ControlResultPayload};

/// 白名单动作 · 必须与后端 ws_schema.CONTROL_ACTIONS 一致
pub const CONTROL_ACTIONS: &[&str] = &[
    "reinstall_tier",
    "fix_venv_cfg",
    "clear_cache",
    "switch_mirror",
    "reprobe",
    "prefetch_tier",
];

/// 已执行过的 control_id (幂等去重 · 防重放) · 进程级
static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 记录并判断 control_id 是否已处理过 (幂等)。返回 true = 首次 (应执行)。
fn mark_seen(control_id: &str) -> bool {
    let mut guard = SEEN.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    if set.contains(control_id) {
        return false;
    }
    // 防无限增长 · 简单上限
    if set.len() > 512 {
        set.clear();
    }
    set.insert(control_id.to_string());
    true
}

fn result(ctrl: &ControlPayload, ok: bool, detail: &str, t0: Instant) -> ControlResultPayload {
    ControlResultPayload {
        control_id: ctrl.control_id.clone(),
        action: ctrl.action.clone(),
        ok,
        detail: detail.chars().take(500).collect(),
        elapsed_ms: Some(t0.elapsed().as_millis() as i64),
    }
}

/// 从 params 定位要操作的 tier:params.tier 优先 · 否则按 missing_dep 在 installed.json 找。
fn pick_tier(ctrl: &ControlPayload) -> Option<String> {
    if let Some(t) = ctrl.params.get("tier").and_then(|v| v.as_str()) {
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let dep = ctrl.params.get("missing_dep").and_then(|v| v.as_str())?;
    if dep.is_empty() {
        return None;
    }
    // 在 installed.json 里找声称含该包的 tier
    let installed = crate::runtime::detector::read_installed_meta();
    for (tier_name, tier) in installed.tiers.iter() {
        if tier.software.iter().any(|s| s == dep) {
            return Some(tier_name.clone());
        }
    }
    None
}

/// 执行一条 control 指令 · 返回回报 payload。不 panic (任何错误都转成 ok=false)。
pub async fn execute(app: AppHandle, ctrl: ControlPayload) -> ControlResultPayload {
    let t0 = Instant::now();

    // 1. 过期校验 (防重放过期指令)
    if ctrl.expires_at_ms > 0 && now_ms() > ctrl.expires_at_ms {
        tracing::warn!("control · {} 已过期 · 拒绝执行", ctrl.action);
        return result(&ctrl, false, "control 已过期 · 拒绝执行", t0);
    }

    // 2. 白名单校验 (绝不执行未知/任意动作)
    if !CONTROL_ACTIONS.contains(&ctrl.action.as_str()) {
        tracing::warn!("control · 未知 action={} · 拒绝", ctrl.action);
        return result(&ctrl, false, &format!("未知 action: {} · 拒绝执行", ctrl.action), t0);
    }

    // 3. 幂等去重 (control_id 已处理过 → 跳过)
    if !mark_seen(&ctrl.control_id) {
        tracing::info!("control · {} 重复 control_id · 跳过", ctrl.control_id);
        return result(&ctrl, true, "重复 control_id · 已跳过 (幂等)", t0);
    }

    tracing::warn!(
        "control · 执行 action={} params={:?} reason={}",
        ctrl.action, ctrl.params, ctrl.reason
    );

    let outcome: Result<String, String> = match ctrl.action.as_str() {
        "reprobe" => {
            crate::runtime::detector::invalidate_probe_cache();
            Ok("探针缓存已失效 · 下次心跳重报真实能力".into())
        }
        "fix_venv_cfg" => {
            let dest = crate::runtime::paths::runtime_root();
            crate::runtime::bootstrap_bundled::fix_venv_pyvenv_cfg(&dest);
            Ok("已修复 venv pyvenv.cfg (CI 路径 → 本地 CPython)".into())
        }
        // reinstall / prefetch / switch_mirror 都走整 tier 重装 (installer 自带多镜像源 fallback)
        "reinstall_tier" | "prefetch_tier" | "switch_mirror" => {
            match pick_tier(&ctrl) {
                Some(tier) => crate::runtime::installer::install_tier(app.clone(), tier.clone())
                    .await
                    .map(|m| format!("tier={} 重装成功 · {}", tier, m))
                    .map_err(|e| format!("tier 重装失败: {}", e)),
                None => Err("无 params.tier / missing_dep · 无法定位要修复的 tier".into()),
            }
        }
        "clear_cache" => clear_tier_cache(&ctrl),
        _ => Err("unreachable".into()),
    };

    match outcome {
        Ok(msg) => {
            // 修复成功 → 失效探针 → 下次心跳重报真实能力 (调度器随之重新认为合格)
            crate::runtime::detector::invalidate_probe_cache();
            tracing::warn!("control · action={} 成功 · {}", ctrl.action, msg);
            result(&ctrl, true, &msg, t0)
        }
        Err(e) => {
            tracing::error!("control · action={} 失败 · {}", ctrl.action, e);
            result(&ctrl, false, &e, t0)
        }
    }
}

/// 清指定 tier 的 venv 目录 (下次安装重建) · params.tier 必填。
fn clear_tier_cache(ctrl: &ControlPayload) -> Result<String, String> {
    let tier = match ctrl.params.get("tier").and_then(|v| v.as_str()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return Err("clear_cache 需 params.tier".into()),
    };
    let venv_dir = crate::runtime::paths::venv_dir(&tier);
    if venv_dir.exists() {
        std::fs::remove_dir_all(&venv_dir)
            .map_err(|e| format!("删 venv {} 失败: {}", venv_dir.display(), e))?;
        Ok(format!("已清 venv {} (下次安装重建)", tier))
    } else {
        Ok(format!("venv {} 不存在 · 无需清理", tier))
    }
}
