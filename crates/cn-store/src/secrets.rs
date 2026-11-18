//! SecretStore trait + 本地文件实现（AES-CFB 加密 sessions.json.enc 风格）。
//! OS 密钥库实现留给 P4（trait 已预留）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes::cipher::{AsyncStreamCipher, KeyIvInit};

use crate::error::StoreError;

/// 凭据存取抽象（P4 可换 OS 密钥库实现）。
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> Result<(), StoreError>;
    fn delete(&self, key: &str) -> Result<(), StoreError>;
}

/// 内存实现（测试用）。
#[derive(Debug, Default)]
pub struct MemSecretStore {
    map: Mutex<HashMap<String, String>>,
}

impl MemSecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        self.map.lock().expect("secrets lock").get(key).cloned()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.map
            .lock()
            .expect("secrets lock")
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.map.lock().expect("secrets lock").remove(key);
        Ok(())
    }
}

type Aes128CfbEnc = cfb_mode::Encryptor<aes::Aes128>;
type Aes128CfbDec = cfb_mode::Decryptor<aes::Aes128>;

/// 文件实现：整份 map 序列化为 JSON 后 AES-CFB 加密落盘；密钥文件与数据文件同目录。
/// 注：P4 将把密钥移入 OS 密钥库，此实现等价于「sessions.json.enc + 本地密钥」。
pub struct FileSecretStore {
    path: PathBuf,
    key_path: PathBuf,
    cache: Mutex<HashMap<String, String>>,
}

impl FileSecretStore {
    /// 在 `dir` 下使用 `sessions.json.enc` + `sessions.key`。
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("sessions.json.enc");
        let key_path = dir.join("sessions.key");
        let key = load_or_create_key(&key_path)?;
        let store = Self {
            path,
            key_path,
            cache: Mutex::new(HashMap::new()),
        };
        if store.path.exists() {
            let bytes = std::fs::read(&store.path)?;
            if !bytes.is_empty() {
                let plain = decrypt(&key, &bytes);
                if let Ok(map) = serde_json::from_slice::<HashMap<String, String>>(&plain) {
                    *store.cache.lock().expect("secrets lock") = map;
                }
            }
        }
        Ok(store)
    }

    fn persist(&self) -> Result<(), StoreError> {
        let key = load_or_create_key(&self.key_path)?;
        let map = self.cache.lock().expect("secrets lock").clone();
        let plain = serde_json::to_vec(&map)?;
        let cipher = encrypt(&key, &plain);
        let tmp = self.path.with_extension("enc.tmp");
        std::fs::write(&tmp, &cipher)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        self.cache.lock().expect("secrets lock").get(key).cloned()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.cache
            .lock()
            .expect("secrets lock")
            .insert(key.to_string(), value.to_string());
        self.persist()
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.cache.lock().expect("secrets lock").remove(key);
        self.persist()
    }
}

fn load_or_create_key(path: &Path) -> Result<Vec<u8>, StoreError> {
    if path.exists() {
        let bytes = std::fs::read(path)?;
        if bytes.len() == 16 {
            return Ok(bytes);
        }
    }
    // 16 字节随机密钥（非密码学强随机也可接受的场景由 P4 密钥库接管）。
    let mut key = [0u8; 16];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1);
    let mut state = nanos | 1;
    for slot in key.iter_mut() {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *slot = (state >> 16) as u8;
    }
    std::fs::write(path, key)?;
    Ok(key.to_vec())
}

fn encrypt(key: &[u8], plain: &[u8]) -> Vec<u8> {
    let mut buf = plain.to_vec();
    Aes128CfbEnc::new(key.into(), key.into()).encrypt(&mut buf);
    buf
}

fn decrypt(key: &[u8], cipher: &[u8]) -> Vec<u8> {
    let mut buf = cipher.to_vec();
    Aes128CfbDec::new(key.into(), key.into()).decrypt(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_secret_round_trip() {
        let dir = std::env::temp_dir().join(format!("cn-secret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::open(&dir).expect("open");
        assert_eq!(store.get("llm.apiKey"), None);
        store.set("llm.apiKey", "sk-abc").expect("set");
        store.set("account.password", "pw").expect("set");
        drop(store);
        // 重新打开能读回
        let store2 = FileSecretStore::open(&dir).expect("reopen");
        assert_eq!(store2.get("llm.apiKey").as_deref(), Some("sk-abc"));
        assert_eq!(store2.get("account.password").as_deref(), Some("pw"));
        store2.delete("account.password").expect("delete");
        assert_eq!(store2.get("account.password"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mem_secret_round_trip() {
        let store = MemSecretStore::new();
        store.set("a", "1").unwrap();
        assert_eq!(store.get("a").as_deref(), Some("1"));
        store.delete("a").unwrap();
        assert_eq!(store.get("a"), None);
    }
}
