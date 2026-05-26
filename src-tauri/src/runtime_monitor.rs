//! 客户端运行时采样 (P0 · NCE 算法依赖)
//!
//! 提供:
//!   1. CPU / 内存实时占用率采样 (sysinfo)
//!   2. 进程启动时间 → uptime_sec (服务端 NCE rep_stability 合入 capabilities 用)
//!   3. 重启次数计数 (持久化 · 调试用)
//!
//! GPU/硬件探测 · 由 hardware_capabilities.rs 统一负责 (不在本文件重复)
//! 心跳每 15s 调 `sample()` 拿最新一帧 · 由 v8_ws.rs 灌进 HbPayload.extra

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sysinfo::{Pid, System};

// ────────────────────── 状态 ──────────────────────

#[derive(Debug)]
struct MonitorState {
    sys: System,
    start_instant: Instant,
    pid: Pid,
    restart_count: u32,
}

static STATE: OnceLock<Mutex<MonitorState>> = OnceLock::new();
static RESTART_COUNT_PATH: OnceLock<PathBuf> = OnceLock::new();

// ────────────────────── 公共结构 ──────────────────────

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeSample {
    /// 0-100 当前 CPU 占用率 (全局)
    pub cpu_usage: f32,
    /// 0-100 当前内存占用率
    pub mem_usage: f32,
    /// 0-1 综合负载估计 (max(cpu, mem) / 100)
    pub load_rate: f32,
    /// 进程启动以来秒数
    pub uptime_sec: u64,
    /// 累计重启次数 (持久化)
    pub restart_count: u32,
}

// GpuInfo 删除 · GPU 探测统一由 hardware_capabilities.rs 负责

// ────────────────────── 初始化 ──────────────────────

/// 启动时调一次 (main 里) · 之后 `sample()` 直接取
pub fn init(app_data_dir: &Path) {
    if STATE.get().is_some() {
        return;
    }

    // restart_count 持久化路径
    let p = app_data_dir.join("restart_count.txt");
    let _ = std::fs::create_dir_all(app_data_dir);
    let prev: u32 = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let new_count = prev.saturating_add(1);
    // 原子写
    let tmp = p.with_extension("txt.tmp");
    if std::fs::write(&tmp, new_count.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
    let _ = RESTART_COUNT_PATH.set(p);

    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    let state = MonitorState {
        sys,
        start_instant: Instant::now(),
        pid: Pid::from_u32(std::process::id()),
        restart_count: new_count,
    };
    let _ = STATE.set(Mutex::new(state));

    tracing::info!(
        "runtime_monitor: init done · restart_count={} pid={}",
        new_count, std::process::id(),
    );
}

// ────────────────────── 采样 ──────────────────────

pub fn sample() -> RuntimeSample {
    let Some(m) = STATE.get() else {
        return RuntimeSample::default();
    };
    let Ok(mut guard) = m.lock() else {
        return RuntimeSample::default();
    };

    // CPU: sysinfo 要求两次 refresh_cpu 之间有间隔才有数据
    guard.sys.refresh_cpu_all();
    guard.sys.refresh_memory();
    let cpu_usage = guard.sys.global_cpu_usage();

    let total = guard.sys.total_memory() as f64;
    let used = guard.sys.used_memory() as f64;
    let mem_usage = if total > 0.0 {
        (used / total * 100.0) as f32
    } else {
        0.0
    };

    let load_rate = (cpu_usage.max(mem_usage) / 100.0).clamp(0.0, 1.0);

    RuntimeSample {
        cpu_usage,
        mem_usage,
        load_rate,
        uptime_sec: guard.start_instant.elapsed().as_secs(),
        restart_count: guard.restart_count,
    }
}

// gpu_info() / detect_gpu() / detect_nvidia_smi() 等全部删除
// 原因: 重复 hardware_capabilities.rs · GPU 探测统一走 v8_ws.collect_capabilities()
/* 原代码作历史参考 · 永远不会编译进 binary

fn detect_nvidia_smi() -> Option<GpuInfo> {
    let out = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return None;
    }
    let first = lines[0];
    let parts: Vec<&str> = first.split(',').map(|p| p.trim()).collect();
    let model = parts.get(0).copied().unwrap_or("").to_string();
    let mem: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    Some(GpuInfo {
        count: lines.len() as u32,
        model,
        memory_mb: mem,
        source: "nvidia-smi".into(),
    })
}

#[cfg(target_os = "macos")]
fn detect_macos() -> Option<GpuInfo> {
    // system_profiler SPDisplaysDataType 输出含 "Chipset Model: Apple M2"
    let out = std::process::Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-detailLevel", "mini"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut model = String::new();
    let mut mem_mb: u32 = 0;
    for line in s.lines() {
        let line = line.trim();
        if line.starts_with("Chipset Model:") {
            if model.is_empty() {
                model = line.trim_start_matches("Chipset Model:").trim().to_string();
            }
        } else if line.starts_with("VRAM (Dynamic, Max):") || line.starts_with("VRAM (Total):") {
            // 例: "1536 MB" / "8 GB"
            let rest = line.split(':').nth(1).unwrap_or("").trim();
            mem_mb = parse_memory_mb(rest);
        }
    }
    if model.is_empty() {
        return None;
    }
    Some(GpuInfo {
        count: 1,
        model,
        memory_mb: mem_mb,
        source: "system_profiler".into(),
    })
}

#[cfg(target_os = "windows")]
fn detect_windows() -> Option<GpuInfo> {
    // wmic path win32_VideoController get Name,AdapterRAM
    let out = std::process::Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "Name,AdapterRAM", "/format:csv"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut model = String::new();
    let mut mem_mb: u32 = 0;
    let mut count = 0u32;
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Node") {
            continue;
        }
        // CSV: Node,AdapterRAM,Name
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 3 {
            continue;
        }
        let ram_bytes: u64 = parts[1].trim().parse().unwrap_or(0);
        let name = parts[2].trim();
        if name.is_empty() {
            continue;
        }
        count += 1;
        if model.is_empty() {
            model = name.to_string();
            mem_mb = (ram_bytes / 1024 / 1024) as u32;
        }
    }
    if count == 0 {
        return None;
    }
    Some(GpuInfo {
        count,
        model,
        memory_mb: mem_mb,
        source: "wmic".into(),
    })
}

#[cfg(target_os = "linux")]
fn detect_linux_lspci() -> Option<GpuInfo> {
    // lspci | grep -E "VGA|3D" → "01:00.0 VGA compatible controller: NVIDIA ..."
    let out = std::process::Command::new("sh")
        .args(["-c", "lspci 2>/dev/null | grep -E 'VGA|3D|Display' || true"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return None;
    }
    let first = lines[0];
    // 取 ":" 后面的部分
    let model = first.split(':').nth(2).unwrap_or(first).trim().to_string();
    Some(GpuInfo {
        count: lines.len() as u32,
        model,
        memory_mb: 0, // lspci 不带显存信息
        source: "lspci".into(),
    })
}

*/ // ··· 闭合删除注释块

// ────────────────────── 工具 ──────────────────────

#[allow(dead_code)]
pub fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_and_sample() {
        let tmp = TempDir::new().unwrap();
        init(tmp.path());
        let s = sample();
        assert!(s.cpu_usage >= 0.0 && s.cpu_usage <= 100.0);
        assert!(s.mem_usage >= 0.0 && s.mem_usage <= 100.0);
        assert!(s.restart_count >= 1);
    }

}
