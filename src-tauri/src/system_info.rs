//! 本机系统信息（hostname / CPU / 内存 / OS）— M3.5.3。

use serde::Serialize;
use sysinfo::System;

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub device_name: String,
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    pub total_memory_mb: u64,
    pub arch: &'static str,
}

pub fn collect() -> SystemInfo {
    let mut sys = System::new_all();
    sys.refresh_all();

    let hostname = System::host_name().unwrap_or_else(|| "unknown".to_string());
    let os_name = System::name().unwrap_or_else(|| "unknown".to_string());
    let os_version = System::os_version().unwrap_or_default();
    let kernel_version = System::kernel_version().unwrap_or_default();

    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    let cpu_threads = sys.cpus().len();
    let cpu_cores = sys.physical_core_count().unwrap_or(cpu_threads);
    let total_memory_mb = sys.total_memory() / 1024 / 1024;

    let arch = std::env::consts::ARCH;

    SystemInfo {
        device_name: hostname.clone(),
        hostname,
        os_name,
        os_version,
        kernel_version,
        cpu_brand,
        cpu_cores,
        cpu_threads,
        total_memory_mb,
        arch,
    }
}
