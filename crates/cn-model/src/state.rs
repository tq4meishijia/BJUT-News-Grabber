//! `act(op, payload) -> ActionResult` 契约与 `StateDto` 视图。
//! 字段与 `store.mjs:1025-1078` 的 `state()` 逐项对齐（本轮只定义结构，store 逻辑在后续任务实现）。
//!
//! 截断约定（B12）：`state` 视图 notices 200 / items 300 / reminders 20；
//! 存储上限 notices 400 / items 600 / reminders 40。截断由 store 层执行。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 已知缺陷统一：`maxItemsPerSource` 默认 100（`emptyStore` 为 100、`settings.save` 回退 25，
/// 重构统一为 100），范围 1–200。
pub const MAX_ITEMS_PER_SOURCE_DEFAULT: u32 = 100;

/// `core.mjs::numberOr` 语义：非有限数回退，整数夹紧到 [min, max]。
pub fn number_or(value: Option<f64>, fallback: u32, min: u32, max: u32) -> u32 {
    match value {
        Some(n) if n.is_finite() => {
            let rounded = n.round() as i64;
            rounded.clamp(min as i64, max as i64) as u32
        }
        _ => fallback,
    }
}

/// 预览条目（`preview` 字段），对应 `store.mjs` 的 `items.slice(0, 5).map(...)`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewItem {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub date: String,
}

/// `act` 统一响应：成功与失败都必须带 `state`（B12 口径）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<PreviewItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub state: StateDto,
}

/// 校外模式（WebVPN）公开视图：网关 Cookie 永不回传（B9）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebvpnDto {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub gateway: String,
    #[serde(default)]
    pub has_cookie: bool,
}

impl Default for WebvpnDto {
    fn default() -> Self {
        Self {
            enabled: false,
            gateway: "https://webvpn.bjut.edu.cn".to_string(),
            has_cookie: false,
        }
    }
}

/// 设置视图（`state.settings`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    #[serde(default = "default_poll_minutes")]
    pub poll_minutes: u32,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default = "default_true")]
    pub llm: bool,
    #[serde(default = "default_llm_max_per_poll")]
    pub llm_max_per_poll: u32,
    #[serde(default = "default_true")]
    pub auto_schedule: bool,
    #[serde(default = "default_auto_remind_before")]
    pub auto_remind_before: Vec<u32>,
    /// 默认统一为 100（已知缺陷处理），范围 1–200。
    #[serde(default = "default_max_items_per_source")]
    pub max_items_per_source: u32,
    #[serde(default = "default_true")]
    pub fetch_detail: bool,
    #[serde(default = "default_detail_limit")]
    pub detail_limit: u32,
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default)]
    pub webvpn: WebvpnDto,
}

fn default_true() -> bool {
    true
}
fn default_poll_minutes() -> u32 {
    30
}
fn default_llm_max_per_poll() -> u32 {
    20
}
fn default_max_items_per_source() -> u32 {
    MAX_ITEMS_PER_SOURCE_DEFAULT
}
fn default_detail_limit() -> u32 {
    25
}
fn default_auto_remind_before() -> Vec<u32> {
    vec![1440, 120]
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            poll_minutes: default_poll_minutes(),
            keywords: Vec::new(),
            llm: true,
            llm_max_per_poll: default_llm_max_per_poll(),
            auto_schedule: true,
            auto_remind_before: default_auto_remind_before(),
            max_items_per_source: MAX_ITEMS_PER_SOURCE_DEFAULT,
            fetch_detail: true,
            detail_limit: default_detail_limit(),
            notify: true,
            webvpn: WebvpnDto::default(),
        }
    }
}

/// 数据源视图（`state.sources[]`）：`cookie` 换成 `hasCookie`，含 `loginExpired`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDto {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub has_cookie: bool,
    #[serde(default)]
    pub login_expired: bool,
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
    /// §6.4：是否允许该源正文送 LLM（敏感源可单独关闭，默认开）。
    #[serde(default = "default_true")]
    pub send_to_llm: bool,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub status_text: String,
    #[serde(default)]
    pub last_fetch_at: String,
    #[serde(default)]
    pub last_count: u32,
}

fn default_one() -> u32 {
    1
}
fn default_status() -> String {
    "idle".to_string()
}

/// 通知条目视图（`state.notices[]`，已截断到 200）。
/// 与 `store.mjs` 流水线产出的 notice 逐字段对齐（state() 原样透传）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticeDto {
    #[serde(default)]
    pub id: String,
    /// B5：`{sourceId}|{sha1(cleanTitle)[0..16]}`。
    #[serde(default)]
    pub dedupe_key: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub source_name: String,
    /// JSON 源的行 id（详情接口用）。
    #[serde(default)]
    pub source_item_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub fetched_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub deadline: String,
    #[serde(default)]
    pub contact: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub useful: Vec<String>,
    #[serde(default)]
    pub relevant: bool,
    #[serde(default)]
    pub ai: bool,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub score: u8,
}

/// 日程条目视图（`state.items[]`，已截断到 300）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateItemDto {
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

/// 提醒记录（`meta.reminders`，视图截断 20，存储上限 40）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderDto {
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
    /// 触发时刻（JS 字段名为 `at`）。
    #[serde(default)]
    pub at: String,
}

/// 运行状态小结（`meta.llm` / `account.lastLogin` 的 `{at, ok, message}` 形状）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusDto {
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub ok: Option<bool>,
    #[serde(default)]
    pub message: String,
}

/// 账号视图（凭据只暴露 `configured`，不回传密码）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub cas_base: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub last_login: StatusDto,
}

/// `state.meta`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaDto {
    #[serde(default)]
    pub last_poll_at: String,
    #[serde(default)]
    pub last_poll_error: String,
    #[serde(default)]
    pub poll_count: u32,
    /// 前 20 条（B12）。
    #[serde(default)]
    pub reminders: Vec<ReminderDto>,
    #[serde(default)]
    pub llm: StatusDto,
    #[serde(default)]
    pub account: AccountDto,
    #[serde(default)]
    pub polling: bool,
    #[serde(default)]
    pub data_path: String,
    #[serde(default)]
    pub port: u32,
    #[serde(default)]
    pub llm_key_set: bool,
    /// §6.3：本月已用 token / 月度预算（0 = 不限）。
    #[serde(default)]
    pub llm_used: u64,
    #[serde(default)]
    pub llm_budget: u64,
}

/// `get_state` / 每次 `act` 返回的完整视图。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StateDto {
    #[serde(default)]
    pub settings: SettingsDto,
    #[serde(default)]
    pub sources: Vec<SourceDto>,
    /// 前 200 条（B12）。
    #[serde(default)]
    pub notices: Vec<NoticeDto>,
    /// 前 300 条（B12）。
    #[serde(default)]
    pub items: Vec<StateItemDto>,
    #[serde(default)]
    pub meta: MetaDto,
}

impl StateDto {
    /// 按 B12 截断：notices 200 / items 300 / reminders 20。
    pub fn trim_to_view(&mut self) {
        self.notices.truncate(200);
        self.items.truncate(300);
        self.meta.reminders.truncate(20);
    }
}
