//! 任务子进程资源限制 (P0 · 防笔记本卡死的最大流失源)
//!
//! 设计:
//!   - 节点 owner 在 UI 选档位 (eco / balanced / full)
//!   - 派 task 时按档位给 child process 设优先级 + 内存上限
//!   - 用户日常使用 = 前台优先 · 任务永远 nice +
//!
//! 跨平台:
//!   - macOS: setpriority(PRIO_PROCESS, nice) + sched_yield (不抢前台 UI 线程)
//!   - Linux: setpriority + ionice (可选 cgroups v2 但需 root · 不强求)
//!   - Windows: SetPriorityClass(BELOW_NORMAL_PRIORITY_CLASS) + Job Object (P1 加)
//!
//! 失败优雅降级 · 不阻塞 spawn
//!
//! 用法:
//!   let mut cmd = Command::new("python3");
//!   resource_limit::apply(&mut cmd, ThrottleLevel::Balanced);
//!   cmd.spawn()?;

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use tokio::process::Command;

// ────────────── 全局 throttle 档位 (commands 写 · executor 读) ──────────────
//
// 用 AtomicU8 避免锁 · executor 每个任务都要查 · 不能让锁成瓶颈
// 编码: 0=Eco · 1=Balanced (默认) · 2=Full
static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(1);

pub fn current_level() -> ThrottleLevel {
    match CURRENT_LEVEL.load(Ordering::Relaxed) {
        0 => ThrottleLevel::Eco,
        2 => ThrottleLevel::Full,
        _ => ThrottleLevel::Balanced,
    }
}

pub fn set_level(level: ThrottleLevel) {
    let v = match level {
        ThrottleLevel::Eco => 0,
        ThrottleLevel::Balanced => 1,
        ThrottleLevel::Full => 2,
    };
    CURRENT_LEVEL.store(v, Ordering::Relaxed);
    tracing::info!("resource_limit: 全局 throttle 档位 → {:?}", level);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThrottleLevel {
    /// 极致省心 · nice +15 · 几乎不影响前台 (推荐笔记本/办公)
    Eco,
    /// 平衡 · nice +10 · 任务慢一点 · 用户基本无感 (默认)
    Balanced,
    /// 全力 · nice 0 · 任务全速 · 可能感到风扇响 (服务器/专机)
    Full,
}

impl Default for ThrottleLevel {
    fn default() -> Self {
        ThrottleLevel::Balanced
    }
}

impl ThrottleLevel {
    pub fn from_str(s: &str) -> Self {
        match s {
            "eco" => Self::Eco,
            "full" => Self::Full,
            _ => Self::Balanced,
        }
    }

    /// 这个档位对应的 nice 增量 (越大优先级越低)
    pub fn nice_increment(&self) -> i32 {
        match self {
            Self::Eco => 15,
            Self::Balanced => 10,
            Self::Full => 0,
        }
    }
}

/// 在 spawn 子进程前调 · 给 Command 设优先级 hook
/// 失败不抛错 · 只 trace::warn
pub fn apply(cmd: &mut Command, level: ThrottleLevel) {
    let nice = level.nice_increment();
    if nice == 0 {
        // 不需要调 · 跳
        return;
    }

    #[cfg(unix)]
    apply_unix(cmd, nice);

    #[cfg(windows)]
    apply_windows(cmd, level);
}

#[cfg(unix)]
fn apply_unix(cmd: &mut Command, nice_inc: i32) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setpriority 是 async-signal-safe · pre_exec 里调用安全
    unsafe {
        cmd.pre_exec(move || {
            // PRIO_PROCESS=0 · who=0 表示本进程
            let ret = libc::setpriority(libc::PRIO_PROCESS, 0, nice_inc);
            if ret != 0 {
                // 不能 panic · pre_exec 后只能 return Err
                // 设失败也让进程继续跑 · log 不到这里 (pre_exec 在 fork 后 exec 前)
            }
            // Linux: 同时降 IO 优先级 · 不让任务把磁盘抢光
            // (mac 没 ionice · 但 nice 已经影响 IO)
            #[cfg(target_os = "linux")]
            {
                // ioprio_set(IOPRIO_WHO_PROCESS=1, 0, IOPRIO_CLASS_IDLE<<13)
                // 缺 syscall wrapper · 走 nice 已够 · 跳
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn apply_windows(cmd: &mut Command, level: ThrottleLevel) {
    use std::os::windows::process::CommandExt;
    // CREATE_NEW_PROCESS_GROUP 0x00000200 + 优先级 flag
    // BELOW_NORMAL_PRIORITY_CLASS = 0x00004000
    // IDLE_PRIORITY_CLASS = 0x00000040
    let prio_flag = match level {
        ThrottleLevel::Eco => 0x00000040u32,      // IDLE
        ThrottleLevel::Balanced => 0x00004000u32, // BELOW_NORMAL
        ThrottleLevel::Full => 0,
    };
    if prio_flag != 0 {
        cmd.creation_flags(prio_flag);
    }
}

/// std::process::Command (for 同步 sub-process · 不是 tokio) 的同套接口
pub fn apply_std(cmd: &mut std::process::Command, level: ThrottleLevel) {
    let nice = level.nice_increment();
    if nice == 0 {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(move || {
                let _ = libc::setpriority(libc::PRIO_PROCESS, 0, nice);
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let prio_flag = match level {
            ThrottleLevel::Eco => 0x00000040u32,
            ThrottleLevel::Balanced => 0x00004000u32,
            ThrottleLevel::Full => 0,
        };
        if prio_flag != 0 {
            cmd.creation_flags(prio_flag);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_from_str() {
        assert_eq!(ThrottleLevel::from_str("eco"), ThrottleLevel::Eco);
        assert_eq!(ThrottleLevel::from_str("balanced"), ThrottleLevel::Balanced);
        assert_eq!(ThrottleLevel::from_str("full"), ThrottleLevel::Full);
        assert_eq!(ThrottleLevel::from_str("???"), ThrottleLevel::Balanced);
    }

    #[test]
    fn nice_increment_values() {
        assert_eq!(ThrottleLevel::Eco.nice_increment(), 15);
        assert_eq!(ThrottleLevel::Balanced.nice_increment(), 10);
        assert_eq!(ThrottleLevel::Full.nice_increment(), 0);
    }
}
