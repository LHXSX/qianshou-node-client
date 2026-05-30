//! Tauri commands · 暴露给前端 UI
//!
//!  - runtime_fetch_manifest()            UI 展示当前 tiers / mirrors
//!  - runtime_get_installed()             UI 展示本机已装情况
//!  - runtime_install_tier(tier)          一键安装
//!  - runtime_uninstall_tier(tier)        删除
//!  - runtime_recheck()                   重新自检
//!  - runtime_host_python()               诊断: 当前用的 host python

use anyhow::Result;
use serde::Serialize;
use tauri::AppHandle;

use super::{detector, installer, manifest};
use crate::hardware_capabilities;

#[tauri::command]
pub async fn runtime_fetch_manifest() -> Result<manifest::RuntimeManifest, String> {
    let caps = hardware_capabilities::detect();
    let hw = manifest::HardwareSnapshot {
        metal: caps.supports_metal,
        cuda: caps.supports_cuda,
        gpu: caps.supports_metal || caps.supports_cuda || caps.supports_rocm,
        vram_gb: caps.gpu_vram_gb,
        ram_gb: 0.0,  // TODO: 跨平台总内存探测 · sysctl/meminfo
    };
    manifest::fetch(&hw).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn runtime_get_installed() -> detector::InstalledMeta {
    detector::read_installed_meta()
}

#[tauri::command]
pub async fn runtime_install_tier(tier: String, app: AppHandle) -> Result<String, String> {
    installer::install_tier(app, tier).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn runtime_uninstall_tier(tier: String) -> Result<(), String> {
    installer::uninstall_tier(&tier).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn runtime_recheck(tier: String) -> detector::InstalledTier {
    installer::recheck_tier(&tier).await
}

#[derive(Debug, Clone, Serialize)]
pub struct HostPythonInfo {
    pub path: String,
    pub version: String,
    pub ok: bool,
    pub message: String,
}

#[tauri::command]
pub async fn runtime_host_python() -> HostPythonInfo {
    match detector::detect_host_python().await {
        Some(p) => match detector::check_python(&p).await {
            Some(v) => HostPythonInfo {
                path: p.to_string_lossy().to_string(),
                version: format!("{}.{}.{}", v.0, v.1, v.2),
                ok: true,
                message: "ok".into(),
            },
            None => HostPythonInfo {
                path: p.to_string_lossy().to_string(),
                version: String::new(),
                ok: false,
                message: "Python 版本探测失败".into(),
            },
        },
        None => HostPythonInfo {
            path: String::new(),
            version: String::new(),
            ok: false,
            message: "本机未找到可用 Python (>=3.9) · 请先安装 Python 3.11".into(),
        },
    }
}
