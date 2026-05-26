//! 机器名持久化（M3.5.3）：用户可改的 device_name 落在 app_data/device_name.txt。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DEVICE_NAME_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init_path(app_data_dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(app_data_dir)?;
    let _ = DEVICE_NAME_PATH.set(app_data_dir.join("device_name.txt"));
    Ok(())
}

pub fn load() -> Option<String> {
    let p = DEVICE_NAME_PATH.get()?;
    if !p.exists() {
        return None;
    }
    let s = fs::read_to_string(p).ok()?.trim().to_string();
    if s.is_empty() {
        return None;
    }
    Some(s)
}

pub fn save(name: &str) -> std::io::Result<()> {
    let p = DEVICE_NAME_PATH.get().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "device path not initialized")
    })?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "device name cannot be empty",
        ));
    }
    fs::write(p, trimmed)?;
    Ok(())
}
