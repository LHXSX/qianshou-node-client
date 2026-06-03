//! 节点自愈 (self-healing)
//!
//! 任务因**环境问题**失败时 (缺 Python 包 / venv 损坏 / 缺系统工具),节点尝试
//! 自动修复到"硬件级别能直接匹配任务"的状态:重装对应 runtime tier。
//!
//! 闭环:
//!   任务失败 → failure_class 分类 → 可自愈? → try_heal(重装 tier)
//!     → 成功: invalidate 探针缓存 → 下次心跳重报**真实**能力 → 调度器重新认为该节点合格
//!     → 失败: 记一次,连续失败到阈值就放弃 (防死循环空耗)
//!
//! 防失控硬约束 (内测期默认全自动,但必须有底线):
//!   - 全局每天最多 MAX_PER_DAY 次自愈
//!   - 同一 tier 连续失败 MAX_CONSEC_FAIL 次 → 当天不再试该 tier
//!   - 单飞: 同时只允许一个自愈在跑
//!   - 只重装**已知 tier** (不执行任意命令 · 不装黑名单)
//!   - OOM / 磁盘满 / 脚本错 / 超时 → 不在这里处理 (failure_class.is_self_healable 已挡掉)

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::failure_class::FailureClass;

const MAX_PER_DAY: u32 = 10;
const MAX_CONSEC_FAIL: u32 = 3;
const DAY: Duration = Duration::from_secs(24 * 3600);

struct HealState {
    window_start: Instant,
    count_in_window: u32,
    /// tier → 当窗口内连续失败次数
    consec_fail: HashMap<String, u32>,
    in_progress: bool,
}

impl Default for HealState {
    fn default() -> Self {
        HealState {
            window_start: Instant::now(),
            count_in_window: 0,
            consec_fail: HashMap::new(),
            in_progress: false,
        }
    }
}

static STATE: Mutex<Option<HealState>> = Mutex::new(None);

#[derive(Debug, Clone)]
pub struct HealOutcome {
    pub attempted: bool,
    pub success: bool,
    /// 修复类型 (供 RepairReport 上报): reinstall_tier / skipped_ratelimit / skipped_no_target ...
    pub kind: String,
    pub tier: String,
    pub detail: String,
}

impl HealOutcome {
    fn skip(kind: &str, detail: &str) -> Self {
        HealOutcome {
            attempted: false,
            success: false,
            kind: kind.to_string(),
            tier: String::new(),
            detail: detail.to_string(),
        }
    }
}

/// 决定要修哪个 tier:
///   1. 任务带的 required_tier 优先
///   2. 否则按 missing_dep 在 installed.json 里找"声称包含它"的 tier
fn pick_target_tier(tier_hint: &str, missing_dep: Option<&str>) -> Option<String> {
    if !tier_hint.is_empty() {
        return Some(tier_hint.to_string());
    }
    let dep = missing_dep?;
    let installed = crate::runtime::detector::read_installed_meta();
    // 找声称含该包的 tier (优先已装过的 · 它最可能是"装了但坏了")
    for (tier_name, tier) in installed.tiers.iter() {
        if tier.software.iter().any(|s| s == dep) {
            return Some(tier_name.clone());
        }
    }
    None
}

/// 限流闸门:能不能对这个 tier 发起一次自愈
fn admission(tier: &str) -> Result<(), HealOutcome> {
    let mut guard = STATE.lock().unwrap();
    let st = guard.get_or_insert_with(HealState::default);

    // 滚动窗口重置
    if st.window_start.elapsed() > DAY {
        *st = HealState::default();
    }
    if st.in_progress {
        return Err(HealOutcome::skip("skipped_busy", "已有自愈在进行"));
    }
    if st.count_in_window >= MAX_PER_DAY {
        return Err(HealOutcome::skip(
            "skipped_ratelimit",
            "今日自愈次数已达上限",
        ));
    }
    if st.consec_fail.get(tier).copied().unwrap_or(0) >= MAX_CONSEC_FAIL {
        return Err(HealOutcome::skip(
            "skipped_giveup",
            "该 tier 连续自愈失败过多,今日放弃",
        ));
    }
    st.in_progress = true;
    st.count_in_window += 1;
    Ok(())
}

fn record_result(tier: &str, success: bool) {
    let mut guard = STATE.lock().unwrap();
    if let Some(st) = guard.as_mut() {
        st.in_progress = false;
        let c = st.consec_fail.entry(tier.to_string()).or_insert(0);
        if success {
            *c = 0;
        } else {
            *c += 1;
        }
    }
}

/// 尝试自愈。返回结果供上层上报 RepairReport (后端据此知道节点为啥失败 + 修没修好)。
///
/// `class` 必须是 is_self_healable 的;否则直接 skip。
pub async fn try_heal(
    app: AppHandle,
    class: FailureClass,
    missing_dep: Option<String>,
    tier_hint: String,
) -> HealOutcome {
    if !class.is_self_healable() {
        return HealOutcome::skip("skipped_unhealable", "该失败类别不自愈");
    }

    let target = match pick_target_tier(&tier_hint, missing_dep.as_deref()) {
        Some(t) => t,
        None => {
            return HealOutcome::skip(
                "skipped_no_target",
                "无法确定要修复哪个 tier (无 required_tier 且未匹配 missing_dep)",
            )
        }
    };

    if let Err(skip) = admission(&target) {
        tracing::info!("self_heal · 跳过 tier={} · {}", target, skip.detail);
        return skip;
    }

    tracing::warn!(
        "self_heal · 触发自愈 · class={} tier={} missing_dep={:?} → 重装 tier",
        class.as_str(),
        target,
        missing_dep
    );

    // 复用 installer 的整 tier 重装 (uv + 镜像源 fallback · 装完自检 ok)
    let result = crate::runtime::installer::install_tier(app, target.clone()).await;
    let success = result.is_ok();
    record_result(&target, success);

    match result {
        Ok(msg) => {
            // 关键:重装成功 → 让探针缓存失效 → 下次心跳重新 probe → 上报真实能力
            // 调度器随之认为该节点重新合格,之前因缺包跳过的任务可再派
            crate::runtime::detector::invalidate_probe_cache();
            tracing::warn!("self_heal · tier={} 重装成功 · {} · 已刷新能力探针", target, msg);
            HealOutcome {
                attempted: true,
                success: true,
                kind: "reinstall_tier".into(),
                tier: target,
                detail: msg,
            }
        }
        Err(e) => {
            tracing::error!("self_heal · tier={} 重装失败: {}", target, e);
            HealOutcome {
                attempted: true,
                success: false,
                kind: "reinstall_tier".into(),
                tier: target,
                detail: e.to_string(),
            }
        }
    }
}
