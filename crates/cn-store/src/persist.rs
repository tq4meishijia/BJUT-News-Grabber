//! 原子落盘：临时文件 + rename；崩溃安全测试入口 `save_store_crashable`。

use std::path::Path;

use crate::error::StoreError;
use crate::state::StoreData;

/// 读取 store.json；文件缺失/损坏回退默认（`store.mjs::load` 语义）。
pub fn load_store(path: &Path) -> StoreData {
    let base = StoreData::default();
    let Ok(text) = std::fs::read_to_string(path) else {
        return base;
    };
    match serde_json::from_str::<StoreData>(&text) {
        Ok(raw) => base.merged_with(raw),
        Err(_) => base,
    }
}

/// 原子写（临时文件 + rename）。写入过程任一步失败，原文件保持完好。
pub fn save_store(path: &Path, data: &StoreData) -> Result<(), StoreError> {
    let snapshot = serde_json::to_string(data)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &snapshot)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 崩溃模拟写入：在 `crash_after` 字节处截断（模拟写到一半进程被杀），
/// 不执行 rename —— 与真实崩溃一致，目标文件保持旧内容。
pub fn save_store_crashable(
    path: &Path,
    data: &StoreData,
    crash_after: Option<usize>,
) -> Result<(), StoreError> {
    let snapshot = serde_json::to_string(data)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    match crash_after {
        Some(n) => {
            let end = n.min(snapshot.len());
            std::fs::write(&tmp, &snapshot.as_bytes()[..end])?;
            // 崩溃：不 rename
            Ok(())
        }
        None => {
            std::fs::write(&tmp, &snapshot)?;
            std::fs::rename(&tmp, path)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_gives_default() {
        let path = std::env::temp_dir().join("cn-store-missing/store.json");
        let data = load_store(&path);
        assert_eq!(data.version, 1);
        assert!(!data.sources.is_empty(), "默认带 4 个种子源");
    }

    #[test]
    fn save_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("cn-store-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("store.json");
        let mut data = StoreData::default();
        data.settings.poll_minutes = 42;
        save_store(&path, &data).expect("save");
        let loaded = load_store(&path);
        assert_eq!(loaded.settings.poll_minutes, 42);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
