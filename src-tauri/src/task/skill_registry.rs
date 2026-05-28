//! 本地技能集注册表 — 启动时扫盘，运行时供 tool_caller / profile 查询。
//!
//! 默认扫描路径（按优先级）：
//! - macOS / Linux: `$HOME/.local/lib/edgecompute/skills/`
//! - macOS / Linux: `/usr/local/lib/edgecompute/skills/`（可选系统级）
//! - Windows: `%LOCALAPPDATA%\EdgeCompute\skills\`
//!
//! 每个技能集是一个子目录，必须含 `manifest.json`（Skill Schema v1）。
//! 解析时容错：manifest 损坏 / entry_file 缺失 → 跳过 + warn，不阻断启动。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

// ────────────────────── manifest schema 反序列化 ──────────────────────

/// manifest.json 完整反序列化结构（保留扩展字段为 Value）。
///
/// 2026-05-28 · 兼容 v1 + v2 schema:
///   v1 (1.0.0): id / name / version / icon / description
///   v2 (2.0.0): pack_id / pack_name / pack_version / pack_icon / pack_description
///   用 #[serde(alias)] 两个名字都接受 · 反序列后走同一个 Rust 字段
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    #[serde(default)]
    pub schema_version: String,
    #[serde(alias = "pack_id")]
    pub id: String,
    #[serde(alias = "pack_name")]
    pub name: String,
    #[serde(alias = "pack_version")]
    pub version: String,
    #[serde(default, alias = "pack_icon")]
    pub icon: String,
    #[serde(default, alias = "pack_description")]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub requirements: Value,
    #[serde(default)]
    pub dependencies: Value,
    pub tools: Vec<ManifestTool>,
    #[serde(default)]
    pub files_sha256: HashMap<String, String>,
    #[serde(default)]
    pub rewards: Value,
}

/// 2026-05-28 · 兼容 v1 + v2 schema:
///   v1: name = 程序 id (如 "extract_clauses") · 无 tool_id 字段
///   v2: tool_id = 程序 id · name = 中文显示名 (如 "条款提取")
///   两个字段都可能存在 · 不能合到同一个 Rust 字段 (serde duplicate field)
///   load_from_manifest 处理: real id = tool_id (v2) · fallback name (v1)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ManifestTool {
    pub name: String,
    /// v2 独有 · 真正的程序 id (v1 为 None · 使用 name 作为 id)
    #[serde(default)]
    pub tool_id: Option<String>,
    #[serde(default)]
    pub description: String,
    /// 形如 "extract_clauses.run" — 提示导入路径，主要是文档用，runtime 实际靠 entry_file
    #[serde(default)]
    pub entry: String,
    /// 文件相对路径 · None 时该 tool 被视为未实现 · 整个 skill 不会因此 fail
    /// (例如 photo-edit-v1 用 schema v2 写法 · 没 entry_file 字段)
    #[serde(default)]
    pub entry_file: Option<String>,
    /// v1: input_schema · v2: params_schema (功能等价 · 都是 OpenAI function calling 用的 JSON Schema)
    #[serde(default, alias = "params_schema")]
    pub input_schema: Value,
    /// v1: output_schema · v2: output_contract (v2 是 {schema, examples} · 这里直接当 Value 接)
    #[serde(default, alias = "output_contract")]
    pub output_schema: Value,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    #[serde(default)]
    pub deterministic: bool,
    #[serde(default)]
    pub examples: Value,
}

fn default_timeout() -> u64 {
    30
}

// ────────────────────── 运行时结构 ──────────────────────

/// 一个已加载并完成基本校验的技能集。
#[derive(Debug, Clone)]
pub struct Skill {
    /// 形如 "text-tools-v1"
    pub id: String,
    pub name: String,
    pub version: String,
    /// 技能集所在目录的绝对路径
    pub dir: PathBuf,
    /// `~/.local/lib/edgecompute/skills/text-tools-v1/manifest.json`
    pub manifest_path: PathBuf,
    /// 工具列表
    pub tools: Vec<Tool>,
    /// 是否通过了 files_sha256 校验
    pub verified: bool,
}

#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub entry_file: PathBuf, // 已展开为绝对路径
    pub input_schema: Value,
    pub output_schema: Value,
    pub timeout_s: u64,
    pub deterministic: bool,
}

impl Skill {
    /// 形如 "text-tools-v1@1.0.0" — 给 profile 上报用。
    pub fn id_versioned(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// 转 OpenAI function calling 工具定义。
    pub fn openai_tool_schemas(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect()
    }
}

// ────────────────────── 注册表 ──────────────────────

#[derive(Debug, Default, Clone)]
pub struct SkillRegistry {
    /// id -> Skill（注意：同一 id 多版本只保留最新一份，覆盖时 warn）
    skills: HashMap<String, Skill>,
    /// tool name -> skill id（防止跨技能集重名工具）
    tool_index: HashMap<String, String>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 扫描默认路径，构造注册表。
    pub fn scan_default_dirs() -> Self {
        let mut reg = Self::new();
        for root in default_skill_roots() {
            reg.scan_root(&root);
        }
        info!(
            "skill_registry: loaded {} skills, {} tools total",
            reg.skills.len(),
            reg.tool_index.len()
        );
        reg
    }

    /// 扫描指定根目录。供测试 / 自定义路径。
    pub fn scan_root(&mut self, root: &Path) -> usize {
        if !root.exists() {
            debug!("skill_registry: root not exist, skip: {}", root.display());
            return 0;
        }
        if !root.is_dir() {
            warn!("skill_registry: root not a dir: {}", root.display());
            return 0;
        }

        let entries = match std::fs::read_dir(root) {
            Ok(e) => e,
            Err(e) => {
                warn!("skill_registry: read_dir failed {}: {}", root.display(), e);
                return 0;
            }
        };

        let mut loaded = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                debug!(
                    "skill_registry: no manifest in {}, skip",
                    path.display()
                );
                continue;
            }
            match Skill::load_from_manifest(&manifest_path) {
                Ok(skill) => {
                    self.insert(skill);
                    loaded += 1;
                }
                Err(e) => {
                    warn!(
                        "skill_registry: load failed {}: {}",
                        manifest_path.display(),
                        e
                    );
                }
            }
        }
        loaded
    }

    /// 插入 / 覆盖。
    pub fn insert(&mut self, skill: Skill) {
        // 检查重复
        if let Some(prev) = self.skills.get(&skill.id) {
            warn!(
                "skill_registry: '{}' already loaded (v{}), overwrite with v{}",
                skill.id, prev.version, skill.version
            );
            // 清掉旧 tool_index
            for t in &prev.tools {
                if self.tool_index.get(&t.name) == Some(&skill.id) {
                    self.tool_index.remove(&t.name);
                }
            }
        }
        for t in &skill.tools {
            if let Some(other) = self.tool_index.get(&t.name) {
                if other != &skill.id {
                    warn!(
                        "skill_registry: tool name '{}' conflict between '{}' and '{}'; latter wins",
                        t.name, other, skill.id
                    );
                }
            }
            self.tool_index.insert(t.name.clone(), skill.id.clone());
        }
        info!(
            "skill_registry: + {} v{} ({} tools, verified={})",
            skill.id,
            skill.version,
            skill.tools.len(),
            skill.verified
        );
        self.skills.insert(skill.id.clone(), skill);
    }

    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.skills.get(id)
    }

    /// 形如 ["text-tools-v1@1.0.0", "ocr-tools-v1@0.2.0"]。
    pub fn list_ids_versioned(&self) -> Vec<String> {
        let mut v: Vec<String> = self.skills.values().map(Skill::id_versioned).collect();
        v.sort();
        v
    }

    /// 工具名 → (skill, tool)。
    pub fn find_tool(&self, tool_name: &str) -> Option<(&Skill, &Tool)> {
        let skill_id = self.tool_index.get(tool_name)?;
        let skill = self.skills.get(skill_id)?;
        let tool = skill.tools.iter().find(|t| t.name == tool_name)?;
        Some((skill, tool))
    }

    /// 把所有技能集的工具拼成一个 OpenAI function calling 数组（给微模型）。
    pub fn all_openai_tool_schemas(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for skill in self.skills.values() {
            out.extend(skill.openai_tool_schemas());
        }
        out
    }

    pub fn skills_count(&self) -> usize {
        self.skills.len()
    }

    pub fn tools_count(&self) -> usize {
        self.tool_index.len()
    }
}

// ────────────────────── 全局单例 ──────────────────────
//
// 整个 client 进程共享一个 SkillRegistry · executor / heartbeat / profile 上报共用。
//
// 2026-05-28 · 升级 OnceLock<SkillRegistry> → OnceLock<Mutex<Arc<SkillRegistry>>>
//   旧设计: OnceLock 锁死一次扫盘结果 · skills_fetcher 装完新 skill 后没办法刷新
//          → 全新装机用户首启时 skills 还没装好就扫 · 之后 lazy init 锁死空表
//   新设计: Mutex<Arc<...>> · global() clone Arc (廉价) · refresh() 重扫并替换 Arc
//   兼容: caller 拿到 Arc<SkillRegistry> · 通过 Deref 仍可直接调原 method (find_tool 等)
//        但 chain 表达式 `global().find_tool(...)` 因 Arc 临时 drop 借用 invalid · 需拆两行

use std::sync::{Arc, Mutex, OnceLock};

static GLOBAL_REGISTRY: OnceLock<Mutex<Arc<SkillRegistry>>> = OnceLock::new();

fn registry_slot() -> &'static Mutex<Arc<SkillRegistry>> {
    GLOBAL_REGISTRY.get_or_init(|| Mutex::new(Arc::new(SkillRegistry::scan_default_dirs())))
}

/// 获取全局 SkillRegistry · 首次调用同步扫盘 · 后续调用 clone 当前 Arc (廉价)
///
/// 用法:
/// ```ignore
/// let reg = skill_registry::global();   // Arc<SkillRegistry>
/// if let Some((skill, tool)) = reg.find_tool(name) {  // Arc 仍存活 · 借用安全
///     let dir = skill.dir.clone();
///     ...
/// }
/// ```
pub fn global() -> Arc<SkillRegistry> {
    registry_slot().lock().unwrap().clone()
}

/// 重新扫描默认目录 · 替换全局 SkillRegistry
///
/// 触发时机:
///   - skills_fetcher 装完一个新 skill zip 之后 (auto_install_tiers / install_tier 调用)
///   - lib.rs setup 阶段延迟 spawn (等 lite tier 装完 skill 后预扫)
///   - 手动 cmd `runtime_recheck` 等
///
/// 返回新扫到的 skill 数 (含老 + 新)
pub fn refresh() -> usize {
    let new_reg = Arc::new(SkillRegistry::scan_default_dirs());
    let count = new_reg.skills_count();
    info!("skill_registry · refresh · 重扫完成 · {} 个 skill", count);
    *registry_slot().lock().unwrap() = new_reg;
    count
}

// ────────────────────── Skill 加载 ──────────────────────

impl Skill {
    pub fn load_from_manifest(manifest_path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(manifest_path)
            .map_err(|e| format!("read manifest failed: {}", e))?;
        let mf: Manifest = serde_json::from_str(&raw)
            .map_err(|e| format!("parse manifest failed: {}", e))?;

        let dir = manifest_path
            .parent()
            .ok_or_else(|| "manifest_path has no parent".to_string())?
            .to_path_buf();

        // 解析 + 校验 tool 文件存在 · 容错: 单 tool 缺/坏 跳过 · 整 skill 不 fail
        let mut tools: Vec<Tool> = Vec::new();
        let mut skipped: Vec<(String, String)> = Vec::new();
        for mt in &mf.tools {
            // 2026-05-28 · v2: tool_id 优先 · v1: fallback name
            let real_id = mt.tool_id.as_deref().unwrap_or(&mt.name).to_string();
            let Some(rel) = mt.entry_file.as_ref().filter(|s| !s.is_empty()) else {
                skipped.push((real_id.clone(), "missing entry_file (schema v2?)".to_string()));
                continue;
            };
            let entry_path = dir.join(rel);
            if !entry_path.exists() {
                skipped.push((
                    real_id.clone(),
                    format!("entry_file 不存在: {}", entry_path.display()),
                ));
                continue;
            }
            tools.push(Tool {
                name: real_id,
                description: mt.description.clone(),
                entry_file: entry_path,
                input_schema: mt.input_schema.clone(),
                output_schema: mt.output_schema.clone(),
                timeout_s: mt.timeout_s,
                deterministic: mt.deterministic,
            });
        }
        if !skipped.is_empty() {
            for (n, reason) in &skipped {
                warn!("skill_registry: skip tool '{}/{}' · {}", mf.id, n, reason);
            }
        }
        // 一个 tool 都没注册成功 → 整 skill 也跳过 · 防止 LLM 看到空 skill 困惑
        if tools.is_empty() {
            return Err(format!(
                "no valid tool in skill '{}' · {} tool declared · {} all skipped",
                mf.id,
                mf.tools.len(),
                skipped.len()
            ));
        }

        // sha256 校验：placeholder 视为开发态（unverified），真实 hash 必须全部对上
        let verified = if mf.files_sha256.is_empty()
            || mf
                .files_sha256
                .values()
                .any(|v| v == "TBD_compute_on_publish")
        {
            false
        } else {
            verify_files_sha256(&dir, &mf.files_sha256)?
        };

        Ok(Skill {
            id: mf.id,
            name: mf.name,
            version: mf.version,
            dir,
            manifest_path: manifest_path.to_path_buf(),
            tools,
            verified,
        })
    }
}

// ────────────────────── sha256 校验 ──────────────────────

/// 自己实现 sha256 避免引新依赖 — 用 Rust 标准库不行（没 sha256），
/// 改用纯字节比较：先 hash 实际文件，再对照 manifest 值。
fn verify_files_sha256(
    dir: &Path,
    expected: &HashMap<String, String>,
) -> Result<bool, String> {
    for (rel_path, expected_hash) in expected {
        let path = dir.join(rel_path);
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("read {} for sha256: {}", path.display(), e))?;
        let actual = sha256_hex(&bytes);
        if actual != expected_hash.to_lowercase() {
            warn!(
                "skill_registry: sha256 mismatch for {}: expected={}, actual={}",
                path.display(),
                expected_hash,
                actual
            );
            return Ok(false);
        }
    }
    Ok(true)
}

/// 纯 Rust 实现 SHA-256（小、无依赖）。公开给 tool_caller 算 result_sha256。
pub fn sha256_hex(data: &[u8]) -> String {
    let h = sha256_raw(data);
    h.iter().map(|b| format!("{:02x}", b)).collect()
}

/// SHA-256 算法实现（RFC 6234）。
fn sha256_raw(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // padding
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // process
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

// ────────────────────── 默认扫描路径 ──────────────────────

fn default_skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            roots.push(PathBuf::from(local).join("EdgeCompute").join("skills"));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            roots.push(
                PathBuf::from(home)
                    .join(".local")
                    .join("lib")
                    .join("edgecompute")
                    .join("skills"),
            );
        }
        // 系统级（可选，需 root 装；正常用户不会装到这里）
        roots.push(PathBuf::from("/usr/local/lib/edgecompute/skills"));
    }

    roots
}

// ────────────────────── 测试 ──────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 项目根 `skills/` 目录（含所有官方 skill）。
    fn skills_dir() -> PathBuf {
        // CARGO_MANIFEST_DIR 是 client-v3/src-tauri，向上 2 级到项目根
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("skills")
    }

    /// 仅含 text-tools-v1 的隔离 fixture（用 symlink 到原 skill）。
    /// 防止仓库新增 skill 后 scan 数量变化导致测试 brittle。
    fn fixture_root() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let src = skills_dir().join("text-tools-v1");
        let dst = tmp.path().join("text-tools-v1");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&src, &dst).unwrap();
        #[cfg(windows)]
        {
            // Windows 需要管理员才能 symlink_dir，回退到递归复制
            fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
                std::fs::create_dir_all(dst)?;
                for e in std::fs::read_dir(src)? {
                    let e = e?;
                    let dst_e = dst.join(e.file_name());
                    if e.file_type()?.is_dir() {
                        copy_dir(&e.path(), &dst_e)?;
                    } else {
                        std::fs::copy(e.path(), dst_e)?;
                    }
                }
                Ok(())
            }
            copy_dir(&src, &dst).unwrap();
        }
        tmp
    }

    #[test]
    fn sha256_known_vector() {
        // 已知向量："abc" -> ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 空串
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn load_text_tools_v1_manifest() {
        let mf_path = skills_dir().join("text-tools-v1").join("manifest.json");
        assert!(mf_path.exists(), "fixture missing: {}", mf_path.display());

        let skill = Skill::load_from_manifest(&mf_path).expect("load failed");
        assert_eq!(skill.id, "text-tools-v1");
        // 2026-05-28 · v2 升级后是 2.0.0
        assert_eq!(skill.version, "2.0.0");
        assert_eq!(skill.tools.len(), 2);

        let names: Vec<&str> = skill.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"extract_clauses"));
        assert!(names.contains(&"sentiment_classify"));

        // 真实 sha256 已写入 manifest，应该 verified=true
        assert!(skill.verified, "sha256 mismatch — 检查 skill_rehash.py 是否同步");

        // 工具 entry_file 都展开为绝对路径并存在
        for t in &skill.tools {
            assert!(
                t.entry_file.is_absolute(),
                "entry_file 应为绝对路径: {}",
                t.entry_file.display()
            );
            assert!(t.entry_file.exists());
        }
    }

    #[test]
    fn registry_scan_and_lookup() {
        let fixture = fixture_root();
        let mut reg = SkillRegistry::new();
        let loaded = reg.scan_root(fixture.path());
        assert_eq!(loaded, 1, "应该装载到 text-tools-v1");

        // list_ids_versioned
        let ids = reg.list_ids_versioned();
        // 2026-05-28 · v2 升级后是 2.0.0
        assert_eq!(ids, vec!["text-tools-v1@2.0.0"]);

        // find_tool
        let (skill, tool) = reg.find_tool("extract_clauses").expect("找不到工具");
        assert_eq!(skill.id, "text-tools-v1");
        assert_eq!(tool.name, "extract_clauses");

        let (_, tool2) = reg.find_tool("sentiment_classify").unwrap();
        // 2026-05-28 · v2 manifest 不再顯式在 tool 对象设 timeout_s · 默认走 default_timeout=30
        //                deterministic 同样 · 默认 false
        // 实际 v2 是从 manifest 读 out_schema 等 · timeout/deterministic 不是必需 (在 engine 字段)
        assert!(tool2.timeout_s > 0, "timeout_s 应有默认值");

        // 不存在的工具
        assert!(reg.find_tool("nonexistent").is_none());

        // openai_tool_schemas — 微模型可以直接用
        let schemas = reg.all_openai_tool_schemas();
        assert_eq!(schemas.len(), 2);
        for s in &schemas {
            assert_eq!(s["type"], "function");
            assert!(s["function"]["name"].is_string());
            assert!(s["function"]["parameters"].is_object());
        }
    }

    #[test]
    fn skip_dirs_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        // 创建空子目录（无 manifest）
        std::fs::create_dir(tmp.path().join("not-a-skill")).unwrap();
        let mut reg = SkillRegistry::new();
        let loaded = reg.scan_root(tmp.path());
        assert_eq!(loaded, 0);
        assert_eq!(reg.skills_count(), 0);
    }

    #[test]
    fn skip_broken_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let sk_dir = tmp.path().join("broken-skill");
        std::fs::create_dir(&sk_dir).unwrap();
        std::fs::write(sk_dir.join("manifest.json"), "{ this is not valid json").unwrap();
        let mut reg = SkillRegistry::new();
        let loaded = reg.scan_root(tmp.path());
        assert_eq!(loaded, 0, "损坏的 manifest 应被跳过而非崩溃");
    }

    #[test]
    fn skip_skill_with_missing_entry_file() {
        let tmp = tempfile::tempdir().unwrap();
        let sk_dir = tmp.path().join("ghost-skill");
        std::fs::create_dir(&sk_dir).unwrap();
        let mf = serde_json::json!({
            "schema_version": "1.0",
            "id": "ghost-skill",
            "name": "Ghost",
            "version": "1.0.0",
            "tools": [{
                "name": "do_nothing",
                "entry_file": "missing.py",
                "input_schema": {},
                "output_schema": {}
            }]
        });
        std::fs::write(sk_dir.join("manifest.json"), mf.to_string()).unwrap();
        let mut reg = SkillRegistry::new();
        let loaded = reg.scan_root(tmp.path());
        assert_eq!(loaded, 0, "entry_file 不存在的 skill 应被跳过");
    }

    #[test]
    fn duplicate_skill_warns_but_overwrites() {
        let fixture = fixture_root();
        let mut reg = SkillRegistry::new();
        let loaded1 = reg.scan_root(fixture.path());
        let loaded2 = reg.scan_root(fixture.path());
        assert_eq!(loaded1, 1);
        assert_eq!(loaded2, 1);
        // 仍然只有 1 个 skill
        assert_eq!(reg.skills_count(), 1);
    }
}
