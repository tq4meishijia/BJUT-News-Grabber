//! store.json / config.json 的存储形状（对照 `core.mjs::emptyStore` + `store.mjs::load`）。

use std::path::PathBuf;

use cn_model::state::{number_or, AttachmentStatus, MAX_ITEMS_PER_SOURCE_DEFAULT};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::secrets::SecretStore;

/// `core.mjs::DEFAULT_WEBVPN` + `emptyStore` 的 webvpn 块。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebvpnSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_gateway")]
    pub gateway: String,
    #[serde(default)]
    pub cookie: String,
    #[serde(default = "default_login_pattern")]
    pub login_page_pattern: String,
}

fn default_gateway() -> String {
    cn_net::WEBVPN_GATEWAY.to_string()
}
fn default_login_pattern() -> String {
    cn_net::DEFAULT_LOGIN_PAGE_PATTERN.to_string()
}

impl Default for WebvpnSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            gateway: default_gateway(),
            cookie: String::new(),
            login_page_pattern: default_login_pattern(),
        }
    }
}

impl WebvpnSettings {
    /// 转成 cn-net 的运行时配置（网关地址与登录页特征一律用内置值——已知缺陷口径）。
    pub fn to_net_config(&self) -> cn_net::WebvpnConfig {
        cn_net::WebvpnConfig {
            enabled: self.enabled,
            gateway: default_gateway(),
            cookie: self.cookie.clone(),
            login_page_pattern: default_login_pattern(),
        }
    }
}

/// settings 块。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSettings {
    #[serde(default = "default_poll_minutes")]
    pub poll_minutes: u32,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default = "default_true")]
    pub llm: bool,
    #[serde(default = "default_llm_max")]
    pub llm_max_per_poll: u32,
    #[serde(default = "default_true")]
    pub auto_schedule: bool,
    #[serde(default = "default_remind_before")]
    pub auto_remind_before: Vec<u32>,
    /// 默认 100（已知缺陷统一口径），范围 1–200。
    #[serde(default = "default_max_items")]
    pub max_items_per_source: u32,
    #[serde(default = "default_true")]
    pub fetch_detail: bool,
    #[serde(default = "default_detail_limit")]
    pub detail_limit: u32,
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default)]
    pub webvpn: WebvpnSettings,
    /// 默认抓取起始日期（YYYY-MM-DD，空 = 不限最早）。
    #[serde(default)]
    pub date_range_start: String,
    /// 默认抓取结束日期（YYYY-MM-DD，空 = 不限最晚）。
    #[serde(default)]
    pub date_range_end: String,
    /// 自动抓取附件（默认关闭）。
    #[serde(default)]
    pub fetch_attachments: bool,
}

fn default_true() -> bool {
    true
}
fn default_poll_minutes() -> u32 {
    30
}
fn default_llm_max() -> u32 {
    20
}
fn default_max_items() -> u32 {
    MAX_ITEMS_PER_SOURCE_DEFAULT
}
fn default_detail_limit() -> u32 {
    25
}
fn default_remind_before() -> Vec<u32> {
    vec![1440, 120]
}

impl Default for StoreSettings {
    fn default() -> Self {
        Self {
            poll_minutes: default_poll_minutes(),
            keywords: cn_core::DEFAULT_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            llm: true,
            llm_max_per_poll: default_llm_max(),
            auto_schedule: true,
            auto_remind_before: default_remind_before(),
            max_items_per_source: MAX_ITEMS_PER_SOURCE_DEFAULT,
            fetch_detail: true,
            detail_limit: default_detail_limit(),
            notify: true,
            webvpn: WebvpnSettings::default(),
            date_range_start: String::new(),
            date_range_end: String::new(),
            fetch_attachments: false,
        }
    }
}

/// 附件记录（store.json 的 attachments[]；按 normUrl 全局唯一，noticeIds 支持共享）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub notice_ids: Vec<String>,
    /// 取 Cookie 用（对应 SourceRecord.cookie）。
    #[serde(default)]
    pub source_id: String,
    /// 绝对原始 URL（网关还原后），供「打开原网页」。
    #[serde(default)]
    pub source_url: String,
    /// 规范化去重键（全局唯一）。
    #[serde(default)]
    pub norm_url: String,
    /// 清洗后的展示文件名。
    #[serde(default)]
    pub file_name: String,
    /// 相对 attachments 目录：`<stem>_<hash8>.<ext>`。
    #[serde(default)]
    pub local_path: String,
    #[serde(default)]
    pub status: AttachmentStatus,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub fetched_at: String,
    /// 失败原因（截断 300 字）。
    #[serde(default)]
    pub error: String,
}

/// 数据源记录（store.json 的 sources[]，含登录态 Cookie——仅本地存储，state() 只回 hasCookie）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// html | rss | json（wechat 分支不可达，不实现——已知缺陷）。
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cookie: String,
    #[serde(default)]
    pub resolve_ip: String,
    #[serde(default)]
    pub link_pattern: String,
    #[serde(default)]
    pub items_path: String,
    #[serde(default = "default_one")]
    pub pages: u32,
    #[serde(default)]
    pub page_url: String,
    #[serde(default)]
    pub url_template: String,
    #[serde(default)]
    pub detail_url_template: String,
    #[serde(default)]
    pub content_path: String,
    #[serde(default)]
    pub map: Option<Value>,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub status_text: String,
    #[serde(default)]
    pub last_fetch_at: String,
    #[serde(default)]
    pub last_count: u32,
    #[serde(default)]
    pub login_expired: bool,
    /// §6.4：是否允许该源正文送 LLM（敏感源可单独关闭，默认开）。
    #[serde(default = "default_true")]
    pub send_to_llm: bool,
}

fn default_type() -> String {
    "html".to_string()
}
fn default_one() -> u32 {
    1
}
fn default_status() -> String {
    "idle".to_string()
}

impl Default for SourceRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            r#type: default_type(),
            url: String::new(),
            enabled: false,
            cookie: String::new(),
            resolve_ip: String::new(),
            link_pattern: String::new(),
            items_path: String::new(),
            pages: 1,
            page_url: String::new(),
            url_template: String::new(),
            detail_url_template: String::new(),
            content_path: String::new(),
            map: None,
            note: String::new(),
            status: default_status(),
            status_text: String::new(),
            last_fetch_at: String::new(),
            last_count: 0,
            login_expired: false,
            send_to_llm: true,
        }
    }
}

/// 通知记录（notices[]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub dedupe_key: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub source_name: String,
    #[serde(default)]
    pub source_item_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub fetched_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub useful: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub deadline: String,
    #[serde(default)]
    pub contact: String,
    #[serde(default)]
    pub score: u8,
    #[serde(default)]
    pub read: bool,
    #[serde(default = "default_true")]
    pub relevant: bool,
    #[serde(default)]
    pub ai: bool,
    /// 最近一次为该通知扫描附件的时间（RFC3339，空 = 从未扫描）。
    /// 负缓存：已扫描过但无附件的老通知不再每轮占用附件补抓名额（T06b）。
    #[serde(default)]
    pub attachments_scanned_at: String,
}

impl Default for NoticeRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            dedupe_key: String::new(),
            source_id: String::new(),
            source_name: String::new(),
            source_item_id: String::new(),
            title: String::new(),
            url: String::new(),
            published_at: String::new(),
            context: String::new(),
            content: String::new(),
            fetched_at: String::new(),
            updated_at: String::new(),
            summary: String::new(),
            useful: Vec::new(),
            tags: Vec::new(),
            deadline: String::new(),
            contact: String::new(),
            score: 0,
            read: false,
            relevant: true,
            ai: false,
            attachments_scanned_at: String::new(),
        }
    }
}

/// 日程记录（items[]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub due_at: String,
    #[serde(default)]
    pub remind_before: Vec<u32>,
    #[serde(default)]
    pub fired: Vec<u32>,
    #[serde(default)]
    pub done: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub done_at: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub notice_id: String,
    #[serde(default)]
    pub url: String,
}

impl Default for ItemRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            note: String::new(),
            due_at: String::new(),
            remind_before: Vec::new(),
            fired: Vec::new(),
            done: false,
            done_at: String::new(),
            created_at: String::new(),
            source: "manual".to_string(),
            notice_id: String::new(),
            url: String::new(),
        }
    }
}

/// 提醒记录（meta.reminders[]，存储上限 40）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub due_at: String,
    #[serde(default)]
    pub at: String,
}

/// meta 块。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreMeta {
    #[serde(default)]
    pub last_poll_at: String,
    #[serde(default)]
    pub last_poll_error: String,
    #[serde(default)]
    pub poll_count: u32,
    #[serde(default)]
    pub reminders: Vec<ReminderRecord>,
}

/// store.json 顶层。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreData {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: StoreSettings,
    #[serde(default)]
    pub sources: Vec<SourceRecord>,
    #[serde(default)]
    pub notices: Vec<NoticeRecord>,
    #[serde(default)]
    pub items: Vec<ItemRecord>,
    #[serde(default)]
    pub attachments: Vec<AttachmentRecord>,
    #[serde(default)]
    pub meta: StoreMeta,
}

fn default_version() -> u32 {
    1
}

impl Default for StoreData {
    fn default() -> Self {
        Self {
            version: 1,
            settings: StoreSettings::default(),
            sources: default_sources(),
            notices: Vec::new(),
            items: Vec::new(),
            attachments: Vec::new(),
            meta: StoreMeta::default(),
        }
    }
}

/// `core.mjs::DEFAULT_SOURCES` 的 4 个一站式接口源（种子数据）。
pub fn default_sources() -> Vec<SourceRecord> {
    cn_core::DEFAULT_SOURCES
        .iter()
        .map(|s| SourceRecord {
            id: s.id.to_string(),
            name: s.name.to_string(),
            r#type: s.kind.to_string(),
            url: s.url.to_string(),
            enabled: true,
            pages: s.pages,
            page_url: s.page_url.to_string(),
            items_path: s.items_path.to_string(),
            map: Some(serde_json::json!({
                "title": s.map_title,
                "id": s.map_id,
                "date": s.map_date,
                "summary": s.map_summary,
            })),
            detail_url_template: s.detail_url_template.to_string(),
            content_path: s.content_path.to_string(),
            note: s.note.to_string(),
            ..SourceRecord::default()
        })
        .collect()
}

impl StoreData {
    /// `store.mjs::trim`（B12 存储上限）：notices 400 / items 600 / reminders 40。
    pub fn trim_storage(&mut self) {
        self.notices.truncate(400);
        self.items.truncate(600);
        self.meta.reminders.truncate(40);
    }

    /// `core.mjs::emptyStore` 语义的合并加载（base 覆盖缺失字段）。
    pub fn merged_with(self, raw: StoreData) -> Self {
        let mut base = self;
        base.version = raw.version;
        base.settings = raw.settings;
        if !raw.sources.is_empty() {
            base.sources = raw.sources;
        }
        base.notices = raw.notices;
        base.items = raw.items;
        base.attachments = raw.attachments;
        base.meta = StoreMeta {
            last_poll_at: raw.meta.last_poll_at,
            last_poll_error: raw.meta.last_poll_error,
            poll_count: raw.meta.poll_count,
            reminders: raw.meta.reminders,
        };
        base
    }
}

/// config.json（配置 + 凭据索引；密码/Key 本体在 SecretStore）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub account: Option<AccountConfig>,
    #[serde(default)]
    pub llm: Option<LlmKeyConfig>,
    #[serde(default)]
    pub port: u32,
}

/// 账号配置（passwordEnc 不落这里——密码在 SecretStore）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountConfig {
    #[serde(default)]
    pub cas_base: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub username: String,
}

/// LLM 配置（apiKey 在 SecretStore）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmKeyConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// §6.3 月度 token 预算（0 = 不限），超限自动只走规则摘要。
    #[serde(default)]
    pub monthly_token_budget: u64,
}

/// SecretStore 键名约定。
pub const SECRET_LLM_KEY: &str = "llm.apiKey";
pub const SECRET_ACCOUNT_PASSWORD: &str = "account.password";

/// 从 SecretStore 拼出 LlmConfig。
pub fn resolve_llm_config(config: &AppConfig, secrets: &dyn SecretStore) -> cn_llm::LlmConfig {
    let api_key = secrets.get(SECRET_LLM_KEY).unwrap_or_default();
    let llm = config.llm.clone().unwrap_or_default();
    cn_llm::LlmConfig {
        api_key,
        base_url: if llm.base_url.is_empty() {
            "https://api.deepseek.com".to_string()
        } else {
            llm.base_url
        },
        model: if llm.model.is_empty() {
            "deepseek-chat".to_string()
        } else {
            llm.model
        },
        monthly_token_budget: llm.monthly_token_budget,
    }
}

/// numberOr 便捷包装（core.mjs 语义）。
pub fn clamp_u32(value: Option<f64>, fallback: u32, min: u32, max: u32) -> u32 {
    number_or(value, fallback, min, max)
}

/// 唯一 id 生成（`n-<base36时间>-<sha1前8>` / `s-<base36时间>-<rand>` 形态）。
pub fn new_id(prefix: &str, seed: &str, now_ms: i64, counter: u64) -> String {
    let mut hasher = sha1::Sha1::new();
    use sha1::Digest;
    hasher.update(seed.as_bytes());
    hasher.update(counter.to_string().as_bytes());
    let digest = hasher.finalize();
    let tail: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{}-{tail}", base36(now_ms as u64))
}

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// 默认数据目录（由调用方指定更常见；这里给测试一个临时目录回退）。
pub fn default_data_dir() -> PathBuf {
    std::env::temp_dir().join("campus-notice-data")
}
