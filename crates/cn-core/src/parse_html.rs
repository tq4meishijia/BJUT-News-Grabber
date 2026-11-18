//! HTML 列表解析（对照 `core.mjs::parseHtmlList`）。
//!
//! 与 §3.1 草图的差异：额外接收 `now`。原因：`findDate` 的「M月D日」分支需要当前年，
//! 而 `cn-core` 禁止读系统时钟（`SystemTime::now()`），时间必须显式注入。

use std::collections::HashSet;
use std::sync::LazyLock;

use chrono::{DateTime, Local};
use cn_model::{Clock, RawItem, SourceSpec};
use regex::Regex;
use url::Url;

use crate::constants::{is_noise_title, NOTICE_WORDS};
use crate::date::find_date;
use crate::text::{char_window, clean_text, clean_title, js_len};

static A_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<a\b([^>]*)>([\s\S]*?)</a>").unwrap());
static HREF_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)href\s*=\s*("([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap()
});
static DIGITS_ONLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9\s.]+$").unwrap());
static BAD_SCHEME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(javascript|mailto|tel):").unwrap());
static INFO_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/info/").unwrap());
static NUM_HTML: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/[0-9]{3,}\.(?:s?html?)$").unwrap());

/// `core.mjs::parseHtmlList(html, baseUrl, source)`。
pub fn parse_html_list(
    html: &str,
    base: &Url,
    spec: &SourceSpec,
    now: &dyn Clock,
) -> Vec<RawItem> {
    let now_dt: DateTime<Local> = now.now();
    let mut out: Vec<RawItem> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let link_filter: Option<Regex> = spec
        .link_pattern
        .as_deref()
        .filter(|p| !p.is_empty())
        .and_then(|p| Regex::new(&format!("(?i){p}")).ok());

    for caps in A_TAG.captures_iter(html) {
        let whole = caps.get(0).expect("group 0");
        let attrs = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let href_cap = HREF_ATTR.captures(attrs);
        let href: String = match href_cap {
            Some(h) => {
                // JS：hrefMatch[2] ?? hrefMatch[3] ?? hrefMatch[4]
                h.get(2)
                    .map(|m| m.as_str().to_string())
                    .or_else(|| h.get(3).map(|m| m.as_str().to_string()))
                    .or_else(|| h.get(4).map(|m| m.as_str().to_string()))
                    .unwrap_or_default()
            }
            None => String::new(),
        };
        if href.is_empty() {
            continue;
        }
        let raw_title = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let text = clean_text(raw_title);
        if text.is_empty() || js_len(&text) < 4 || js_len(&text) > 300 {
            continue;
        }
        let title = clean_title(&text);
        if title.is_empty() || js_len(&title) < 4 {
            continue;
        }
        if is_noise_title(&title) {
            continue;
        }
        if DIGITS_ONLY.is_match(&title) {
            continue;
        }
        if BAD_SCHEME.is_match(&href) || href.starts_with('#') {
            continue;
        }
        let absolute = match base.join(&href) {
            Ok(u) => u.to_string(),
            Err(_) => continue,
        };
        if !(absolute.starts_with("http://") || absolute.starts_with("https://")) {
            continue;
        }
        if let Some(filter) = &link_filter {
            if !filter.is_match(&absolute) && !filter.is_match(&title) {
                continue;
            }
        }
        // JS：context = cleanText(html.slice(match.index - 180, match_end + 180))
        let context = clean_text(&char_window(html, whole.start(), whole.end(), 180, 180));
        let looks_like_notice = NOTICE_WORDS.is_match(&title);
        let date = {
            let direct = find_date(&text, now_dt);
            if !direct.is_empty() {
                direct
            } else if looks_like_notice {
                find_date(&context, now_dt)
            } else {
                String::new()
            }
        };
        let detail_href = INFO_PATH.is_match(&absolute) || NUM_HTML.is_match(&absolute);
        let accept = !date.is_empty()
            || (detail_href && js_len(&title) >= 12)
            || (looks_like_notice && js_len(&title) >= 6);
        if !accept {
            continue;
        }
        let key = format!("{title}|{absolute}");
        if !seen.insert(key) {
            continue;
        }
        out.push(RawItem {
            item_id: String::new(),
            title,
            url: absolute,
            published_at: date,
            context,
        });
    }
    out
}
