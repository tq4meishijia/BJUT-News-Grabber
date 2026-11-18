//! 核心 DTO：解析产物 / 抽取产物 / 打分与规则摘要入参。
//! 字段命名与 `core.mjs` 导出的 JSON 形状对齐（camelCase），保证 golden 可逐字段对照。

use chrono::{DateTime, Local};
use serde::{Deserialize, Deserializer, Serialize};

/// `normalizeDue` 输出："YYYY-MM-DDTHH:mm"（本地时区语义）；`None` 等价于 JS 空串。
pub type DueText = Option<String>;

/// 列表解析产物，对应 `core.mjs` 的 `{ title, url, publishedAt, context }`（JSON 源另带 `itemId`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawItem {
    /// JSON 源的行 id；HTML/RSS 源为空串。
    #[serde(default, rename = "itemId")]
    pub item_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    /// YYYY-MM-DD 或空串。
    #[serde(default, rename = "publishedAt")]
    pub published_at: String,
    /// 列表页碎片 / 接口摘要。
    #[serde(default)]
    pub context: String,
}

/// 数据源类型。`wechat` 分支在 Node 侧不可达（`store.mjs:169`），Rust 侧不实现。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    #[default]
    Html,
    Rss,
    Json,
}

/// `urlTemplate` 的占位符语义（`{id}` / `{index}` / `{title}`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateKind {
    #[default]
    Id,
    Index,
    Title,
}

/// JSON 源的字段映射；`None`/空串在使用点回退到 `title`/`url`/`id`/`date`/`summary`。
/// `link` 是 `core.mjs` 的 JS 兼容字段（`map.url || map.link || 'url'`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldMap {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

impl FieldMap {
    /// JS `map.title || fallback` 语义：None 与空串都回退。
    pub fn field<'a>(value: &'a Option<String>, fallback: &'a str) -> &'a str {
        match value {
            Some(s) if !s.is_empty() => s.as_str(),
            _ => fallback,
        }
    }
}

fn de_null_to_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

/// 数据源定义，与 `store.json` 的 source 字段对应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSpec {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: SourceKind,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub link_pattern: Option<String>,
    #[serde(default)]
    pub items_path: Option<String>,
    #[serde(default, deserialize_with = "de_null_to_default")]
    pub map: FieldMap,
    #[serde(default)]
    pub url_template: Option<String>,
    #[serde(default)]
    pub url_template_kind: TemplateKind,
}

impl Default for SourceSpec {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: SourceKind::Html,
            url: String::new(),
            link_pattern: None,
            items_path: None,
            map: FieldMap::default(),
            url_template: None,
            url_template_kind: TemplateKind::Id,
        }
    }
}

/// 截止时间：`iso` 为 "YYYY-MM-DDTHH:mm"，`raw` 为命中的原文片段。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deadline {
    pub iso: String,
    #[serde(default)]
    pub raw: String,
}

/// 抽取结果，对应 golden 输出的 `extract`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub tags: Vec<String>,
    /// `None` 等价于 JS `findDeadline` 的空串。
    #[serde(default)]
    pub deadline: Option<Deadline>,
    /// `None` 等价于 JS `findContact` 的空串。
    #[serde(default)]
    pub contact: Option<String>,
}

/// `score` 入参，对齐 `core.mjs::scoreNotice({ title, content, tags, deadline, keywords })`。
/// `content` 为 `title + " " + body` 的组合文本（与 `store.mjs` 补全阶段一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreInput {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    /// "YYYY-MM-DDTHH:mm" 或 None（JS 侧为空串）。
    pub deadline: Option<String>,
    pub keywords: Vec<String>,
    pub now: DateTime<Local>,
}

/// `rule_summary` 入参，对齐 `core.mjs::ruleSummary({ title, content, tags, deadline, contact })`。
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryInput {
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub deadline: Option<String>,
    pub contact: Option<String>,
    pub now: DateTime<Local>,
}

/// 规则摘要产物。`core.mjs` 只返回 `summary/useful`；`why` 为规格补充字段
/// （内容与 `useful` 相同，作为"对我有什么用"的理由列表），golden 只对照前两项。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSummary {
    pub summary: String,
    pub useful: Vec<String>,
    #[serde(default)]
    pub why: Vec<String>,
}
