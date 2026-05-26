//! 技能集下发 · 装完 tier 后按 tier.skills[] 拉 skill zip
//!
//! 后端: GET /api/v8/skills/{id}/download
//!   - body: application/zip
//!   - header X-Skill-Sha256: <hex>  · 我们必须校验防止中间人篡改
//!   - header X-Skill-Version: <ver>
//!
//! 节点流程:
//!   1. HTTP GET 拿 bytes + 读 sha256 header
//!   2. 算实际 sha256 比对 (不一致 → 拒装 · 报错)
//!   3. 解压到 `<skills_install_dir>/{id}/`  (先清旧目录)
//!   4. 让调用方继续装下一个 skill · 失败不阻塞整 tier 安装

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

use super::manifest::api_base;
use super::paths;

/// 单 skill 安装结果 (给 installer.rs 打日志用)
#[derive(Debug, Clone)]
pub struct SkillInstallResult {
    pub skill_id: String,
    pub version: String,
    pub size_bytes: usize,
    pub sha256: String,
    pub install_dir: PathBuf,
    pub file_count: usize,
}

/// 拉单个 skill zip + 校验 sha256 + 解压
pub async fn fetch_and_install(skill_id: &str) -> Result<SkillInstallResult> {
    // 基本防御 · 防止 ./../ 注入
    if skill_id.is_empty()
        || skill_id.contains(['/', '\\', '.'])
        || !skill_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!("非法 skill_id: {}", skill_id));
    }

    let url = format!("{}/api/v8/skills/{}/download", api_base(), skill_id);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(format!("EdgeCompute-Client/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("构建 http client 失败")?;

    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("请求 skill 失败: {}", url))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("HTTP {} from {}", status.as_u16(), url));
    }

    let expected_sha = resp
        .headers()
        .get("x-skill-sha256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let version = resp
        .headers()
        .get("x-skill-version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();

    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("读 skill 字节流失败: {}", url))?;

    let actual_sha = sha256_hex(&bytes);
    if !expected_sha.is_empty() && actual_sha.to_lowercase() != expected_sha.to_lowercase() {
        return Err(anyhow!(
            "skill sha256 校验失败 (expected={} actual={}) · 可能被篡改 · 拒装",
            expected_sha,
            actual_sha
        ));
    }

    // 解压到 <skills_install_dir>/{skill_id}/  · 先清旧
    let install_root = paths::skills_install_dir();
    std::fs::create_dir_all(&install_root)
        .with_context(|| format!("创建 {} 失败", install_root.display()))?;
    let install_dir = install_root.join(skill_id);
    if install_dir.exists() {
        let _ = std::fs::remove_dir_all(&install_dir);
    }
    std::fs::create_dir_all(&install_dir)
        .with_context(|| format!("创建 skill 目录失败: {}", install_dir.display()))?;

    let file_count = extract_zip(&bytes, &install_dir)?;

    Ok(SkillInstallResult {
        skill_id: skill_id.to_string(),
        version,
        size_bytes: bytes.len(),
        sha256: actual_sha,
        install_dir,
        file_count,
    })
}

/// 解压 zip 字节流到目标目录 · 防 zip-slip · 返回写入文件数
fn extract_zip(zip_bytes: &[u8], dest: &Path) -> Result<usize> {
    let reader = Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| anyhow!("解析 zip 失败: {}", e))?;

    let mut count = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| anyhow!("读 zip 条目 {} 失败: {}", i, e))?;
        let raw_name = entry.name().to_string();

        // 防 zip-slip: 拒绝绝对路径 / .. / 反斜杠
        if raw_name.starts_with('/')
            || raw_name.starts_with('\\')
            || raw_name.contains("..")
            || raw_name.contains('\\')
        {
            return Err(anyhow!("不安全的 zip 条目名: {}", raw_name));
        }
        let out_path = dest.join(&raw_name);
        // 二次保险 · canonicalize 后必须仍在 dest 之下
        let dest_canon = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
        let parent = out_path.parent().unwrap_or(dest);
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir {} 失败", parent.display()))?;
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)
                .with_context(|| format!("mkdir {} 失败", out_path.display()))?;
            continue;
        }

        // 路径再次确认 (mkdir 后)
        if let Ok(out_canon) = out_path.canonicalize() {
            if !out_canon.starts_with(&dest_canon) {
                return Err(anyhow!("zip 条目逃逸目标目录: {}", raw_name));
            }
        }

        let mut f = std::fs::File::create(&out_path)
            .with_context(|| format!("创建 {} 失败", out_path.display()))?;
        std::io::copy(&mut entry, &mut f)
            .with_context(|| format!("写 {} 失败", out_path.display()))?;
        count += 1;
    }
    Ok(count)
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}
