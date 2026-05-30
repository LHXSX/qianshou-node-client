//! Runtime manifest 拉取 + 解析
//!
//! 协议:
//!   GET https://www.wujisuanli.com/api/v8/runtime/manifest?os=macos&arch=arm64
//!
//! 后端可以随时调整 mirrors / packages / smoke_test · 客户端无需发版

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MirrorSource {
    pub label: String,
    pub index_url: String,
    #[serde(default)]
    pub trusted_host: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PythonRequirement {
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub preferred_versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BinarySpec {
    /// 逻辑名 (如 "ffmpeg") · 也是 installed.tier.binaries 的 key
    pub name: String,
    /// 下载 URL · 通常 OSS 上的 tar.gz / zip
    pub url: String,
    /// 校验和 · 16 进制 lowercase · 空串表示后端尚未填 (允许放行 + 警告)
    #[serde(default)]
    pub sha256: String,
    /// 归档类型 · "tar.gz" / "tar" / "zip"
    #[serde(default = "default_archive")]
    pub archive: String,
    /// 解压目标 · 相对 tier_root(tier) · 默认 "." 表示直接解到根
    #[serde(default = "default_extract_to")]
    pub extract_to: String,
    /// 可执行文件所在子目录 (相对 tier_root/extract_to) · 用于注入 PATH
    #[serde(default = "default_bin_dir")]
    pub bin_dir: String,
    /// 解压后期望存在的可执行文件名列表 (探测装好没)
    #[serde(default)]
    pub executables: Vec<String>,
}

fn default_archive() -> String { "tar.gz".to_string() }
fn default_extract_to() -> String { ".".to_string() }
fn default_bin_dir() -> String { "bin".to_string() }

/// 2026-05-26 · Layer 2 自源 CDN (Ollama 型)
/// 后端在 OSS 预打了 venv tarball · 客户端拉 + 校验 + 解压 · 跳过 pip install
/// 失败时 fallback 到老的 pip 路径 (TierSpec.packages + mirrors)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrebuiltVenvSpec {
    /// 版本号 · "2026.05.26" 之类 · 用于打 cache key (未来支持增量)
    #[serde(default)]
    pub version: String,
    /// tarball 直链 (阿里 OSS / 腾讯 COS)
    pub url: String,
    /// sha256 · "TBD" 时跳过校验 + 打 warn (生产环境必须填实)
    #[serde(default)]
    pub sha256: String,
    /// 预估解压后大小 MB (UI 展示)
    #[serde(default)]
    pub size_mb: u32,
    /// 解压到 venvs/ 下的目录名 · 默认就是 tier 名
    #[serde(default = "default_extract_to_tier")]
    pub extract_to: String,
    /// venv 内 python 相对路径 · unix 通常 "bin/python" · win "Scripts/python.exe"
    #[serde(default = "default_python_rel")]
    pub python_rel: String,
    /// 装好后跑这个验证 · 跟 smoke_test 同语义
    #[serde(default)]
    pub verify_cmd: String,
}

fn default_extract_to_tier() -> String { String::new() }  // 空 = 用 tier 名兜底
fn default_python_rel() -> String {
    if cfg!(target_os = "windows") {
        "Scripts/python.exe".to_string()
    } else {
        "bin/python".to_string()
    }
}

/// 2026-05-30 · 多下载源镜像 (后台热管理)
/// 每个 tier 可以有多个下载源 · 客户端按序尝试 · 任一成功即停止
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrebuiltMirror {
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size_mb: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TierSpec {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub auto_install: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub pip_args: Vec<String>,
    #[serde(default)]
    pub smoke_test: String,
    #[serde(default)]
    pub smoke_timeout_secs: u64,
    #[serde(default)]
    pub software: Vec<String>,
    #[serde(default)]
    pub task_types: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub binaries: Vec<BinarySpec>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub system_commands: Vec<String>,
    #[serde(default)]
    pub install_hint: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub prebuilt_venv: Option<PrebuiltVenvSpec>,
    /// 2026-05-30 · 多下载源 · 后台热管理动态配置
    #[serde(default)]
    pub prebuilt_mirrors: Vec<PrebuiltMirror>,
    /// 2026-05-30 · 下载源类型: self_mirror / public_mirror
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub system_binaries: std::collections::BTreeMap<String, SystemBinarySpec>,
}

/// V8.1 (2026-05-27) · 单平台系统二进制下载规范 · 绕开 brew/winget
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemBinarySpec {
    /// 主下载 URL (官方源)
    pub url: String,
    /// 镜像 URL 列表 · 主源失败按顺序 fallback (国内镜像优先后端按 region 重排)
    #[serde(default)]
    pub mirrors: Vec<String>,
    /// 包格式 · "dmg" | "zip" | "tarxz" | "targz"
    pub kind: String,
    /// 解压后 binary 相对路径 · 例: "Blender.app/Contents/MacOS/Blender"
    pub binary: String,
    /// 暴露给系统 PATH 的命令名 · 例: "blender" / "blender.exe"
    #[serde(default)]
    pub exposes: String,
    /// 预估大小 MB (UI 进度展示)
    #[serde(default)]
    pub size_mb: u32,
    /// 可选 sha256 (留空跳过校验)
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeManifest {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub install_mode: String,
    #[serde(default)]
    pub python: Option<PythonRequirement>,
    #[serde(default)]
    pub mirrors: Vec<MirrorSource>,
    #[serde(default)]
    pub tiers: std::collections::BTreeMap<String, TierSpec>,
}

/// API base · 可被 EDGECOMPUTE_API_BASE 覆盖
pub fn api_base() -> String {
    std::env::var("EDGECOMPUTE_API_BASE")
        .unwrap_or_else(|_| "https://www.wujisuanli.com".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// 探测当前 OS/Arch (跟后端约定一致)
pub fn detect_platform() -> (String, String) {
    let os = std::env::consts::OS.to_string();
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64".to_string(),
        other => other.to_string(),
    };
    (os, arch)
}

/// 2026-05-30 · 硬件能力快照 · 传给后端做 tier 过滤
#[derive(Debug, Clone, Default)]
pub struct HardwareSnapshot {
    pub metal: bool,
    pub cuda: bool,
    pub gpu: bool,
    pub vram_gb: f32,
    pub ram_gb: f32,
}

/// 从后端拉 manifest · 传入硬件参数用于过滤不适配的 tier
pub async fn fetch(hw: &HardwareSnapshot) -> Result<RuntimeManifest> {
    let (os, arch) = detect_platform();
    let mut url = format!(
        "{}/api/v8/runtime/manifest?os={}&arch={}&region=auto",
        api_base(),
        os,
        arch
    );
    // 2026-05-30 · 附加硬件参数 · 后端据此过滤不适合本机的 tier
    if hw.metal {
        url.push_str("&metal=true");
    }
    if hw.cuda {
        url.push_str("&cuda=true");
    }
    if hw.gpu {
        url.push_str("&gpu=true");
    }
    if hw.vram_gb > 0.0 {
        url.push_str(&format!("&vram_gb={:.1}", hw.vram_gb));
    }
    if hw.ram_gb > 0.0 {
        url.push_str(&format!("&ram_gb={:.1}", hw.ram_gb));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("构建 HTTP client 失败")?;
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("请求 manifest 失败: {}", url))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("manifest HTTP {} · body={}", status.as_u16(), body));
    }
    let m: RuntimeManifest = serde_json::from_str(&body)
        .with_context(|| format!("解析 manifest 失败: {}", body))?;
    Ok(m)
}
