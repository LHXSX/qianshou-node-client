//! 硬件能力探测 (V8) — 节点启动时探测真实加速器能力
//! 跨平台兼容: macOS Apple Silicon · Linux NVIDIA/AMD · Windows NVIDIA

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareCapabilities {
    // ── GPU 加速器 (boolean) ──
    pub supports_cuda: bool,          // NVIDIA CUDA
    pub supports_metal: bool,         // Apple Metal
    pub supports_mlx: bool,           // Apple MLX (AI 优化)
    pub supports_rocm: bool,          // AMD ROCm

    // ── 视频/AI 硬编硬解 ──
    pub supports_nvenc: bool,         // NVIDIA 视频硬编
    pub supports_nvdec: bool,         // NVIDIA 视频硬解
    pub supports_videotoolbox: bool,  // Apple 视频硬编硬解
    pub supports_qsv: bool,           // Intel Quick Sync Video
    pub supports_tensor_cores: bool,  // NVIDIA Tensor Cores
    pub supports_neural_engine: bool, // Apple ANE

    // ── 内存架构 ──
    pub unified_memory: bool,         // Apple UMA · 大模型友好

    // ── 显存 ──
    pub gpu_vram_gb: f32,             // 独立显存大小

    // ── 已安装软件 (节点运维拼装大模型/视频任务时用)──
    pub installed_software: Vec<String>,  // ["ffmpeg", "python.PIL", "blender>=4.0", ...]
}

/// 启动时一次性探测节点硬件能力
pub fn detect() -> HardwareCapabilities {
    let mut caps = HardwareCapabilities::default();

    // ─── 平台特定探测 ───
    #[cfg(target_os = "macos")]
    detect_macos(&mut caps);

    #[cfg(target_os = "linux")]
    detect_linux(&mut caps);

    #[cfg(target_os = "windows")]
    detect_windows(&mut caps);

    // ─── 软件依赖探测 (所有平台) ───
    detect_software(&mut caps);

    caps
}

#[cfg(target_os = "macos")]
fn detect_macos(caps: &mut HardwareCapabilities) {
    // Apple Silicon 都有 Metal / VideoToolbox / Neural Engine
    let is_apple_silicon = cfg!(target_arch = "aarch64");
    if is_apple_silicon {
        caps.supports_metal = true;
        caps.supports_mlx = true;
        caps.supports_videotoolbox = true;
        caps.supports_neural_engine = true;
        caps.unified_memory = true;
        // Apple M 系列共享系统内存的一部分作为 VRAM
        // 实际可用 ≈ 总内存 × 0.75 (macOS 默认 GPU memory limit)
        let mut c = Command::new("sysctl"); c.args(&["-n", "hw.memsize"]); crate::proc_util::hide_window_std(&mut c);
        if let Ok(out) = c.output() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                if let Ok(bytes) = s.trim().parse::<u64>() {
                    caps.gpu_vram_gb = (bytes as f32 / 1024.0 / 1024.0 / 1024.0) * 0.75;
                }
            }
        }
    } else {
        // Intel Mac 也有 VideoToolbox + 可能 Metal
        caps.supports_videotoolbox = true;
        caps.supports_metal = true;
    }
}

#[cfg(target_os = "linux")]
fn detect_linux(caps: &mut HardwareCapabilities) {
    // NVIDIA: 用 nvidia-smi 探测
    let mut nv = Command::new("nvidia-smi");
    nv.args(&["--query-gpu=name,memory.total,compute_cap", "--format=csv,noheader,nounits"]);
    crate::proc_util::hide_window_std(&mut nv);
    if let Ok(out) = nv.output()
    {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let first_line = s.lines().next().unwrap_or("");
                let parts: Vec<&str> = first_line.split(',').map(|x| x.trim()).collect();
                if parts.len() >= 3 {
                    caps.supports_cuda = true;
                    caps.supports_nvenc = true;
                    caps.supports_nvdec = true;
                    // VRAM (MiB → GB)
                    if let Ok(mib) = parts[1].parse::<f32>() {
                        caps.gpu_vram_gb = mib / 1024.0;
                    }
                    // Tensor Cores: compute capability >= 7.0
                    if let Ok(cc) = parts[2].parse::<f32>() {
                        caps.supports_tensor_cores = cc >= 7.0;
                    }
                }
            }
        }
    }

    // AMD: 用 rocm-smi 探测
    let mut rs = Command::new("rocm-smi"); rs.arg("--version"); crate::proc_util::hide_window_std(&mut rs);
    if rs.output().is_ok() {
        caps.supports_rocm = true;
    }

    // Intel QSV: 看 /dev/dri/render*
    if std::path::Path::new("/dev/dri/renderD128").exists() {
        // 进一步可用 vainfo 验证，简化版直接标 true
        caps.supports_qsv = true;
    }
}

#[cfg(target_os = "windows")]
fn detect_windows(caps: &mut HardwareCapabilities) {
    // NVIDIA: 同 Linux 用 nvidia-smi
    let mut nv = Command::new("nvidia-smi");
    nv.args(&["--query-gpu=name,memory.total,compute_cap", "--format=csv,noheader,nounits"]);
    crate::proc_util::hide_window_std(&mut nv);
    if let Ok(out) = nv.output()
    {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let first_line = s.lines().next().unwrap_or("");
                let parts: Vec<&str> = first_line.split(',').map(|x| x.trim()).collect();
                if parts.len() >= 3 {
                    caps.supports_cuda = true;
                    caps.supports_nvenc = true;
                    caps.supports_nvdec = true;
                    if let Ok(mib) = parts[1].parse::<f32>() {
                        caps.gpu_vram_gb = mib / 1024.0;
                    }
                    if let Ok(cc) = parts[2].parse::<f32>() {
                        caps.supports_tensor_cores = cc >= 7.0;
                    }
                }
            }
        }
    }
    // Intel QSV: Windows 默认带 Intel 集成显卡 → 假设 OK (生产中可用 wmic 精确检测)
    caps.supports_qsv = true;
}

/// 探测节点已安装的软件 (节点能跑哪些技能的关键)
fn detect_software(caps: &mut HardwareCapabilities) {
    let candidates: &[(&str, &[&str])] = &[
        // (上报名, 探测命令)
        ("ffmpeg",   &["ffmpeg", "-version"]),
        ("blender",  &["blender", "--version"]),
        ("imagemagick", &["convert", "--version"]),
    ];
    for (name, cmd) in candidates {
        let mut c = Command::new(cmd[0]);
        c.args(&cmd[1..]);
        crate::proc_util::hide_window_std(&mut c);
        if c.output().is_ok() {
            caps.installed_software.push(name.to_string());
        }
    }

    // Python 包探测 (Pillow / numpy / paddleocr / faster-whisper)
    let py_packages: &[&str] = &["PIL", "numpy", "paddleocr", "faster_whisper", "fitz", "transformers", "torch", "onnxruntime"];
    for pkg in py_packages {
        // python3 -c "import {pkg}" 成功 = 装了
        let mut c = Command::new("python3");
        c.args(&["-c", &format!("import {}", pkg)]);
        crate::proc_util::hide_window_std(&mut c);
        if c.output().map(|o| o.status.success()).unwrap_or(false) {
            caps.installed_software.push(format!("python.{}", pkg));
        }
    }
}
