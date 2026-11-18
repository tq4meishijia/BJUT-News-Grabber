//! 抓取流水线的步骤实现（对照 `store.mjs:128-261` fetchPage/fetchSource/fetchDetailText + §4.1）。
//! 编排（重试/排序/裁剪）在 service.rs 的 `poll` 里。

use std::sync::LazyLock;

use chrono::{DateTime, Local};
use cn_model::{Clock, RawItem, ScoreInput, SummaryInput};
use cn_net::{
    decode_body, restore_urls_in_text, webvpn_session_expired, FetchRequest, Fetcher, ScopeId,
    WebvpnConfig,
};
use regex::Regex;

use crate::error::StoreError;
use crate::state::{NoticeRecord, SourceRecord};

/// 校外模式会话过期的统一提示。
pub const WEBVPN_EXPIRED_MESSAGE: &str = "WebVPN 网关会话已过期：返回的是登录页而不是通知内容。请在浏览器重新登录 WebVPN，把新的网关 Cookie 粘到「设置 → 校外模式」，再抓取一次";

/// `store.mjs::isJsonText`。
pub fn is_json_text(text: &str) -> bool {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

static RSS_SNIFF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*<\?xml|^\s*<rss[\s>]|^\s*<feed[\s>]|^\s*<rdf:RDF").unwrap()
});
static LOGIN_PAGE_HTML: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(id=["']?(?:unPassword|pwPassword|cptPassword))|cas/login|login-normal\.html"#)
        .unwrap()
});
static BIZ_LOGIN_WORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("请登录|未登录|登录超时|重新登录").unwrap());

/// 构造出站请求（B8：门户 Cookie 与网关 Cookie 分离）。
fn build_request(url: &str, source: &SourceRecord, vpn: &WebvpnConfig, timeout_ms: u64) -> FetchRequest {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default();
    let mut req = FetchRequest::get(url, ScopeId::Portal(host));
    req.cookie = source.cookie.clone();
    req.gateway_cookie = vpn.cookie.clone();
    req.timeout = std::time::Duration::from_millis(timeout_ms);
    if !source.resolve_ip.trim().is_empty() {
        req.resolve_ip = source.resolve_ip.trim().parse().ok();
    }
    req
}

/// `store.mjs::fetchPage`：取页 + 登录态/会话过期判定 + 网关链接还原。
pub async fn fetch_page(
    fetcher: &dyn Fetcher,
    source: &SourceRecord,
    url: &str,
    vpn: &WebvpnConfig,
) -> Result<String, StoreError> {
    let req = build_request(url, source, vpn, 25_000);
    let response = fetcher.fetch(req).await?;
    let content_type = response.header("content-type").unwrap_or("").to_string();
    let text = decode_body(&response.body, &content_type);
    if response.status == 401 || response.status == 403 {
        return Err(StoreError::msg(format!(
            "需要登录（HTTP {}），请在「数据源」里填写登录态 Cookie",
            response.status
        )));
    }
    if response.status >= 400 {
        return Err(StoreError::msg(format!("HTTP {}", response.status)));
    }
    if webvpn_session_expired(&text, vpn) {
        return Err(StoreError::msg(WEBVPN_EXPIRED_MESSAGE));
    }
    if content_type.to_lowercase().contains("json") {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(json) => {
                let message = json
                    .get("m")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let e = json.get("e").and_then(|v| v.as_i64());
                if e == Some(10042) || e == Some(10041) || BIZ_LOGIN_WORDS.is_match(&message) {
                    return Err(StoreError::msg(format!(
                        "登录态失效（接口返回「{}」），请更新 Cookie",
                        if message.is_empty() {
                            e.map(|v| v.to_string()).unwrap_or_default()
                        } else {
                            message
                        }
                    )));
                }
            }
            Err(_) => {
                if vpn.enabled {
                    return Err(StoreError::msg(WEBVPN_EXPIRED_MESSAGE));
                }
            }
        }
    } else if LOGIN_PAGE_HTML.is_match(&text) && text.chars().count() < 30_000 {
        return Err(StoreError::msg(
            "被重定向到登录页：登录态失效，请更新 Cookie",
        ));
    }
    Ok(restore_urls_in_text(&text, vpn))
}

/// 把 SourceRecord 转 cn-core 的 SourceSpec。
pub fn to_spec(source: &SourceRecord) -> cn_model::SourceSpec {
    use cn_model::{FieldMap, SourceKind, SourceSpec, TemplateKind};
    let kind = match source.r#type.as_str() {
        "rss" => SourceKind::Rss,
        "json" => SourceKind::Json,
        _ => SourceKind::Html,
    };
    let map: FieldMap = source
        .map
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    SourceSpec {
        id: source.id.clone(),
        kind,
        url: source.url.clone(),
        link_pattern: if source.link_pattern.trim().is_empty() {
            None
        } else {
            Some(source.link_pattern.trim().to_string())
        },
        items_path: if source.items_path.trim().is_empty() {
            None
        } else {
            Some(source.items_path.trim().to_string())
        },
        map,
        url_template: if source.url_template.trim().is_empty() {
            None
        } else {
            Some(source.url_template.trim().to_string())
        },
        url_template_kind: TemplateKind::Id,
    }
}

/// `store.mjs::fetchSource`：翻页 + 解析（含 JSON/RSS 嗅探）+ 标题去重。
pub async fn fetch_source(
    fetcher: &dyn Fetcher,
    source: &SourceRecord,
    vpn: &WebvpnConfig,
    now: DateTime<Local>,
) -> Result<Vec<RawItem>, StoreError> {
    let clock = FixedAt(now);
    let mut pages: Vec<(String, String)> = Vec::new();
    let first = fetch_page(fetcher, source, &source.url, vpn).await?;
    pages.push((first, source.url.clone()));
    let total = cn_model::state::number_or(Some(source.pages as f64), 1, 1, 5);
    if total > 1 && !source.page_url.is_empty() {
        for page in 2..=total {
            let url = source.page_url.replace("{n}", &page.to_string());
            let absolute = url::Url::parse(&source.url)
                .ok()
                .and_then(|base| base.join(&url).ok())
                .map(|u| u.to_string())
                .unwrap_or(url.clone());
            match fetch_page(fetcher, source, &absolute, vpn).await {
                Ok(html) => pages.push((html, absolute)),
                Err(_) => break,
            }
        }
    }
    let mut all: Vec<RawItem> = Vec::new();
    for (body, page_url) in pages {
        let sniffed_json = source.r#type == "html" && is_json_text(&body);
        let sniffed_rss = source.r#type == "html" && RSS_SNIFF.is_match(&body);
        let spec = to_spec(source);
        let base = url::Url::parse(&page_url)
            .or_else(|_| url::Url::parse(&source.url))
            .map_err(|_| StoreError::msg("数据源地址不是合法网址"))?;
        let parsed = if source.r#type == "rss" || sniffed_rss {
            cn_core::parse_rss(&body, &base, &clock)
        } else if source.r#type == "json" || sniffed_json {
            cn_core::parse_json_list(&body, &spec, &clock)
        } else {
            cn_core::parse_html_list(&body, &base, &spec, &clock)
        };
        if sniffed_json && parsed.is_empty() {
            return Err(StoreError::msg(
                "这个地址返回的是 JSON，但没解析出条目：把解析方式改成「JSON 接口」，必要时填 itemsPath（如 d.data）",
            ));
        }
        if sniffed_rss && parsed.is_empty() {
            return Err(StoreError::msg(
                "这个地址返回的是 RSS/Atom，但没解析出条目：检查 feed 是否为空，或把解析方式改成「RSS / Atom」",
            ));
        }
        for item in parsed {
            if !all.iter().any(|existing| existing.title == item.title) {
                all.push(item);
            }
        }
    }
    Ok(all)
}

/// 便捷 Clock：流水线内部需要一个 &dyn Clock 传给 cn-core。
struct FixedAt(DateTime<Local>);
impl Clock for FixedAt {
    fn now(&self) -> DateTime<Local> {
        self.0
    }
}

/// `store.mjs::fetchDetailText`：详情正文（接口型用 contentPath，页面型用 extract_main_content）。
pub async fn fetch_detail_text(
    fetcher: &dyn Fetcher,
    notice: &NoticeRecord,
    source: &SourceRecord,
    vpn: &WebvpnConfig,
) -> String {
    if !source.detail_url_template.is_empty() {
        let url = source
            .detail_url_template
            .replace("{id}", &cn_core::encode_uri_component(&notice.source_item_id))
            .replace("{url}", &cn_core::encode_uri_component(&notice.url));
        let req = build_request(&url, source, vpn, 20_000);
        let Ok(response) = fetcher.fetch(req).await else {
            return String::new();
        };
        if response.status >= 400 {
            return String::new();
        }
        let content_type = response.header("content-type").unwrap_or("").to_string();
        let text = decode_body(&response.body, &content_type);
        if webvpn_session_expired(&text, vpn) {
            return String::new();
        }
        if !source.content_path.is_empty() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                let html = cn_core::pick_path(&json, &source.content_path)
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                let body = cn_core::extract_main_content(&html);
                return if body.chars().count() >= 20 {
                    body
                } else {
                    cn_core::extract_main_content(&text)
                };
            }
            return String::new();
        }
        return cn_core::extract_main_content(&text);
    }
    if notice.url.is_empty() {
        return String::new();
    }
    let req = build_request(&notice.url, source, vpn, 20_000);
    let Ok(response) = fetcher.fetch(req).await else {
        return String::new();
    };
    if response.status >= 400 {
        return String::new();
    }
    let content_type = response.header("content-type").unwrap_or("").to_string();
    let html = decode_body(&response.body, &content_type);
    if webvpn_session_expired(&html, vpn) {
        return String::new();
    }
    let text = cn_core::extract_main_content(&html);
    if text.chars().count() >= 20 {
        return text;
    }
    if Regex::new(r"(?i)<img[\s>]").unwrap().is_match(&html) {
        return "〔正文以图片或附件形式发布，建议打开原文查看〕".to_string();
    }
    text
}

/// B5：去重键 = 源 id + 标题 sha1 前 16 位。
pub fn build_dedupe_key(source_id: &str, title: &str) -> String {
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(cn_core::clean_title(title).as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{source_id}|{}", &hex[..16])
}

/// 单条通知的 AI 行（cn-llm 校验后）。
pub struct AiRow {
    pub gist: String,
    pub useful: Vec<String>,
    pub tags: Vec<String>,
    pub deadline: String,
    pub contact: String,
    pub relevant: bool,
}

/// `store.mjs:542-561` 逐条补全：有 AI 用 AI，无则规则（B11 永不空白）。
pub fn enrich_notice(notice: &mut NoticeRecord, source: Option<&SourceRecord>, keywords: &[String], ai: Option<&AiRow>, now: DateTime<Local>) {
    // HTML 源的 context 是页面碎片，不能当正文；接口源的列表摘要可以兜底。
    let body_text = if !notice.content.is_empty() {
        notice.content.clone()
    } else if source.map(|s| s.r#type.as_str()) == Some("json") {
        notice.context.clone()
    } else {
        String::new()
    };
    let text = format!("{} {}", notice.title, body_text);
    let tags = match ai {
        Some(row) if !row.tags.is_empty() => {
            let mut t: Vec<String> = row.tags.iter().filter(|s| !s.is_empty()).cloned().collect();
            t.truncate(5);
            t
        }
        _ => cn_core::pick_tags(&text, keywords),
    };
    let deadline = match ai {
        Some(row) if !row.deadline.trim().is_empty() => row.deadline.trim().to_string(),
        _ => cn_core::find_deadline(&text, now)
            .map(|d| d.iso)
            .unwrap_or_default(),
    };
    let contact = cn_core::find_contact(&body_text);
    let rule = cn_core::rule_summary(&SummaryInput {
        title: notice.title.clone(),
        content: body_text.clone(),
        tags: tags.clone(),
        deadline: if deadline.is_empty() {
            None
        } else {
            Some(deadline.clone())
        },
        contact: if contact.is_empty() {
            None
        } else {
            Some(contact.clone())
        },
        now,
    });
    notice.tags = tags.clone();
    notice.deadline = deadline.clone();
    notice.contact = contact;
    notice.summary = match ai {
        Some(row) if !row.gist.is_empty() => cn_core::cut(&row.gist, 160),
        _ => rule.summary.clone(),
    };
    notice.useful = match ai {
        Some(row) if !row.useful.is_empty() => {
            let mut u: Vec<String> = row.useful.iter().filter(|s| !s.is_empty()).cloned().collect();
            u.truncate(4);
            u
        }
        _ => rule.useful.clone(),
    };
    notice.relevant = match ai {
        Some(row) => row.relevant,
        None => true,
    };
    notice.ai = ai.is_some();
    notice.score = cn_core::score(&ScoreInput {
        title: notice.title.clone(),
        content: text,
        tags,
        deadline: if deadline.is_empty() {
            None
        } else {
            Some(deadline)
        },
        keywords: keywords.to_vec(),
        now,
    });
}
