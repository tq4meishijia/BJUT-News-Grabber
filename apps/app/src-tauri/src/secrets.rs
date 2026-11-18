//! SecretStore 的 OS 密钥库实现（Windows Credential Manager，S1）。
//! keyring 不可用时回退 cn_store::FileSecretStore（AES-CFB 密文落盘，仍无明文）。

use cn_store::{FileSecretStore, SecretStore, StoreError};

const SERVICE: &str = "edu.bjut.campus-notice";

/// Windows Credential Manager 实现。
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    fn entry(key: &str) -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(SERVICE, key).map_err(|e| StoreError::msg(format!("钥匙串不可用：{e}")))
    }

    /// 探测钥匙串是否可用（一次空读）。
    pub fn available() -> bool {
        match Self::entry("__probe__") {
            Ok(entry) => {
                // 读不到（NoEntry）也算可用；只有平台错误才回退。
                !matches!(
                    entry.get_password(),
                    Err(keyring::Error::PlatformFailure(_)) | Err(keyring::Error::NoStorageAccess(_))
                )
            }
            Err(_) => false,
        }
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        Self::entry(key)
            .and_then(|e| e.get_password().map_err(|err| StoreError::msg(err.to_string())))
            .ok()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| StoreError::msg(format!("写入钥匙串失败：{e}")))
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(StoreError::msg(format!("删除钥匙串条目失败：{e}"))),
        }
    }
}

/// 组合实现：优先 OS 密钥库，不可用时用加密文件（无明文落盘）。
pub enum SecretBackend {
    Keyring(KeyringSecretStore),
    File(FileSecretStore),
}

impl SecretBackend {
    /// 打开最佳可用后端。dir 为回退文件存储目录（%APPDATA%\campus-notice）。
    pub fn open(dir: &std::path::Path) -> Self {
        if KeyringSecretStore::available() {
            Self::Keyring(KeyringSecretStore)
        } else {
            match FileSecretStore::open(dir) {
                Ok(store) => Self::File(store),
                Err(_) => Self::Keyring(KeyringSecretStore),
            }
        }
    }

    /// 当前后端名（诊断用）。
    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Keyring(_) => "windows-credential-manager",
            Self::File(_) => "file-encrypted-fallback",
        }
    }
}

impl SecretStore for SecretBackend {
    fn get(&self, key: &str) -> Option<String> {
        match self {
            Self::Keyring(s) => s.get(key),
            Self::File(s) => s.get(key),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StoreError> {
        match self {
            Self::Keyring(s) => s.set(key, value),
            Self::File(s) => s.set(key, value),
        }
    }

    fn delete(&self, key: &str) -> Result<(), StoreError> {
        match self {
            Self::Keyring(s) => s.delete(key),
            Self::File(s) => s.delete(key),
        }
    }
}
