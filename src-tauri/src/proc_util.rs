//! Cross-platform process spawning helpers.
//!
//! Windows 默认 spawn 子进程会闪 console 窗口 (CMD/PowerShell)。
//! 本模块给 `std::process::Command` 和 `tokio::process::Command` 都加上
//! `CREATE_NO_WINDOW` flag · 让节点端跑 python 子进程时不再闪烁。

/// Windows `CREATE_NO_WINDOW` process creation flag.
/// 见 https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 给 `std::process::Command` 加 no-window flag (Windows-only · 其他平台 no-op)
pub fn hide_window_std(_cmd: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        _cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// 给 `tokio::process::Command` 加 no-window flag (Windows-only · 其他平台 no-op)
///
/// 用 tokio 1.7+ 内置的 `creation_flags` 方法 (Windows 专属 · cfg 隔离)
pub fn hide_window_tokio(_cmd: &mut tokio::process::Command) {
    #[cfg(target_os = "windows")]
    {
        _cmd.creation_flags(CREATE_NO_WINDOW);
    }
}
