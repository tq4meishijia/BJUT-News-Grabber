//! 结果缓存：`ai_hash = sha1(title + content[0..2000] + model + PROMPT_VERSION)`。

use std::collections::HashMap;

use sha1::{Digest, Sha1};

use crate::prompt::PROMPT_VERSION;

/// 计算 ai_hash（十六进制 sha1）。
pub fn ai_hash(title: &str, content: &str, model: &str) -> String {
    let head: String = content.chars().take(2000).collect();
    let mut hasher = Sha1::new();
    hasher.update(title.as_bytes());
    hasher.update(head.as_bytes());
    hasher.update(model.as_bytes());
    hasher.update(PROMPT_VERSION.to_string().as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 进程内缓存（key = ai_hash，value = 校验后的行）。
#[derive(Debug, Default)]
pub struct LlmCache {
    map: HashMap<String, crate::client::LlmRow>,
}

impl LlmCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&crate::client::LlmRow> {
        self.map.get(key)
    }

    pub fn put(&mut self, key: String, row: crate::client::LlmRow) {
        self.map.insert(key, row);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
