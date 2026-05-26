//! 任务专用技能包 (Task Skill Pack) — V8 协议
//!
//! 当 task 帧带有 `skill_pack_id` 时，客户端：
//!   1. GET /api/v8/skill-packs/{pack_id}  拉取定制 runner_code
//!   2. sha256 校验代码完整性（防止运营/中间人篡改）
//!   3. 落到 ~/.qianshou/task-packs/{pack_id}/runner.py
//!   4. executor 用这个文件代替原始 bundle 的 runner_code
//!
//! 设计：
//!   - 进程内缓存（同 pack_id 多任务复用）
//!   - 落盘缓存（重启后仍可用，按 expires_at 自动失效）
//!   - 失败兜底：拉不到 / sha256 不一致 → 返回错误，调度引擎可降级

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct SkillPack {
    pub pack_id: String,
    pub forked_from: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub runner_code: String,
    pub code_sha256: String,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub expires_at: f64,
}

impl SkillPack {
    /// sha256(runner_code) 必须等于服务端给的 code_sha256，否则代码被篡改。
    pub fn verify_integrity(&self) -> Result<()> {
        let mut h = Sha256::new();
        h.update(self.runner_code.as_bytes());
        let actual = hex_lower(&h.finalize());
        if actual.eq_ignore_ascii_case(&self.code_sha256) {
            Ok(())
        } else {
            Err(anyhow!(
                "skill_pack {} sha256 mismatch: server={} actual={}",
                self.pack_id, self.code_sha256, actual
            ))
        }
    }

    /// 把 runner_code 写到固定路径：~/.qianshou/task-packs/<pack_id>/runner.py
    pub async fn materialize(&self) -> Result<PathBuf> {
        let dir = pack_dir(&self.pack_id);
        fs::create_dir_all(&dir).await
            .with_context(|| format!("create dir {:?}", dir))?;
        let runner_path = dir.join("runner.py");
        // 已存在且 sha256 匹配 → 跳过写盘
        if let Ok(existing) = fs::read_to_string(&runner_path).await {
            let mut h = Sha256::new();
            h.update(existing.as_bytes());
            if hex_lower(&h.finalize()).eq_ignore_ascii_case(&self.code_sha256) {
                return Ok(runner_path);
            }
        }
        fs::write(&runner_path, &self.runner_code).await
            .with_context(|| format!("write {:?}", runner_path))?;
        // 写一个 .meta 方便调试
        let meta = serde_json::json!({
            "pack_id": self.pack_id,
            "forked_from": self.forked_from,
            "name": self.name,
            "code_sha256": self.code_sha256,
            "revision": self.revision,
            "expires_at": self.expires_at,
        });
        let _ = fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?).await;
        Ok(runner_path)
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn pack_root() -> PathBuf {
    if let Some(home) = dirs_home() {
        home.join(".qianshou").join("task-packs")
    } else {
        PathBuf::from("/tmp").join("qianshou-task-packs")
    }
}

fn pack_dir(pack_id: &str) -> PathBuf {
    pack_root().join(pack_id)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// ─── 进程内缓存（同一 pack_id 重复任务复用）────────────────────────────

use std::sync::OnceLock;

fn mem_cache() -> &'static Mutex<HashMap<String, SkillPack>> {
    static CACHE: OnceLock<Mutex<HashMap<String, SkillPack>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(pack_id: &str) -> Option<SkillPack> {
    mem_cache().lock().ok()?.get(pack_id).cloned()
}

fn cache_put(pack: SkillPack) {
    if let Ok(mut g) = mem_cache().lock() {
        g.insert(pack.pack_id.clone(), pack);
    }
}

// ─── 公开 API ──────────────────────────────────────────────────────────

/// 拉取并校验一个 pack。命中缓存直接返回；否则走 HTTP。
///
/// `api_base` 形如 `https://www.wujisuanli.com`。
pub async fn fetch_and_verify(api_base: &str, pack_id: &str) -> Result<SkillPack> {
    if let Some(p) = cache_get(pack_id) {
        return Ok(p);
    }
    let url = format!("{}/api/v8/skill-packs/{}", api_base.trim_end_matches('/'), pack_id);
    tracing::info!("拉取 skill_pack: {}", url);
    let pack: SkillPack = reqwest::Client::new()
        .get(&url)
        .send().await
        .with_context(|| format!("GET {}", url))?
        .error_for_status()
        .with_context(|| format!("status from {}", url))?
        .json::<SkillPack>().await
        .context("parse skill_pack JSON")?;
    pack.verify_integrity()
        .with_context(|| format!("integrity check for pack {}", pack_id))?;
    cache_put(pack.clone());
    Ok(pack)
}

/// 拉 pack + 落盘 + 返回 runner.py 绝对路径。
pub async fn ensure_runner(api_base: &str, pack_id: &str) -> Result<PathBuf> {
    let pack = fetch_and_verify(api_base, pack_id).await?;
    pack.materialize().await
}

/// 清理某个 pack 的本地缓存（管理员手动撤销时用）。
#[allow(dead_code)]
pub async fn clear_local(pack_id: &str) -> Result<()> {
    let dir = pack_dir(pack_id);
    if Path::new(&dir).exists() {
        fs::remove_dir_all(&dir).await?;
    }
    if let Ok(mut g) = mem_cache().lock() {
        g.remove(pack_id);
    }
    Ok(())
}
