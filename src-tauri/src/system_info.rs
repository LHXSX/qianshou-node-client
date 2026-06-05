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
    // 2026-06-05 硬件感知:GPU/加速器画像(工具管理按本机硬件推荐/门控 tier)
    pub has_gpu: bool,
    pub gpu_model: String,
    pub gpu_vram_gb: f32,
    pub supports_cuda: bool,
    pub supports_metal: bool,
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

    // 2026-06-05 硬件感知:复用 hardware_capabilities 探测 GPU/加速器
    let caps = crate::hardware_capabilities::detect();
    let has_gpu = caps.supports_cuda || caps.supports_metal || caps.supports_rocm;
    let gpu_model = if caps.supports_cuda {
        "NVIDIA CUDA GPU".to_string()
    } else if caps.supports_metal {
        "Apple Silicon (Metal)".to_string()
    } else if caps.supports_rocm {
        "AMD ROCm GPU".to_string()
    } else {
        String::new()
    };

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
        has_gpu,
        gpu_model,
        gpu_vram_gb: caps.gpu_vram_gb,
        supports_cuda: caps.supports_cuda,
        supports_metal: caps.supports_metal,
    }
}
