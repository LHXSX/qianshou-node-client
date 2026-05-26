//! node_id 持久化到 app_data 目录下的纯文本文件。
//!
//! 为什么不用 Keyring：node_id 不是敏感数据，存 Keyring 每次启动
//! 都要求用户授权弹窗，体验差。用普通文件即可。
//!
//! 路径示例（macOS）：
//!   ~/Library/Application Support/com.edgecompute.client/node_id.txt

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static NODE_ID_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init_node_id_path(app_data_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(app_data_dir)?;
    let p = app_data_dir.join("node_id.txt");
    let _ = NODE_ID_PATH.set(p);
    Ok(())
}

pub fn load_node_id() -> Option<String> {
    let p = NODE_ID_PATH.get()?;
    let s = fs::read_to_string(p).ok()?;
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn save_node_id(node_id: &str) -> std::io::Result<()> {
    let p = match NODE_ID_PATH.get() {
        Some(p) => p,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "node_id path not initialized",
            ))
        }
    };
    // 原子写：先写 .tmp 再 rename
    let tmp = p.with_extension("txt.tmp");
    fs::write(&tmp, node_id)?;
    fs::rename(&tmp, p)?;
    Ok(())
}
