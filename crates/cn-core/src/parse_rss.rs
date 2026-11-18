//! RSS/Atom 列表解析（对照 `core.mjs::parseRss`）。

use std::sync::LazyLock;

use chrono::{DateTime, Local};
use cn_model::{Clock, RawItem};
use regex::Regex;
use url::Url;

/// `<link>text</link>`（CDATA 兼容）。
static LINK_ELEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<link[^>]*>([\s\S]*?)</link>").unwrap());
/// Atom 的 `<link href="...">`。
static LINK_HREF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<link[^>]+href=["']([^"']+)["']"#).unwrap());

use crate::date::parse_feed_date;
use crate::text::{clean_text, clean_title, excerpt_of};

fn strip_cdata(text: &str) -> String {
    let re = Regex::new(r"(?is)<!\[CDATA\[([\s\S]*?)\]\]>").unwrap();
    re.replace_all(text, "$1").into_owned()
}

/// 从块里取 `<tag>...</tag>` 的清洗文本。
fn pick(block: &str, tag: &str) -> String {
    let pattern = format!(r"(?is)<{tag}[^>]*>([\s\S]*?)</{tag}>");
    let re = match Regex::new(&pattern) {
        Ok(re) => re,
        Err(_) => return String::new(),
    };
    match re.captures(block) {
        Some(caps) => clean_text(&strip_cdata(caps.get(1).map(|m| m.as_str()).unwrap_or(""))),
        None => String::new(),
    }
}

/// JS `a || b || c` 语义：取第一个非空串。
fn first_non_empty(values: Vec<String>) -> String {
    values.into_iter().find(|s| !s.is_empty()).unwrap_or_default()
}

/// `core.mjs::parseRss(xml, baseUrl)`：RSS `<item>` 优先，没有则用 Atom `<entry>`。
pub fn parse_rss(xml: &str, base: &Url, now: &dyn Clock) -> Vec<RawItem> {
    let now_dt: DateTime<Local> = now.now();
    let item_sep = Regex::new(r"(?i)<item[\s>]").unwrap();
    let entry_sep = Regex::new(r"(?i)<entry[\s>]").unwrap();
    let mut blocks: Vec<&str> = item_sep.split(xml).skip(1).collect();
    if blocks.is_empty() {
        blocks = entry_sep.split(xml).skip(1).collect();
    }
    let mut out: Vec<RawItem> = Vec::new();
    for block in blocks {
        let title = clean_title(&pick(block, "title"));
        if title.is_empty() {
            continue;
        }
        let mut link = String::new();
        if let Some(caps) = LINK_ELEM.captures(block) {
            link = clean_text(&strip_cdata(caps.get(1).map(|m| m.as_str()).unwrap_or("")));
        }
        if link.is_empty() {
            if let Some(caps) = LINK_HREF.captures(block) {
                link = caps
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
            }
        }
        let absolute = if link.is_empty() {
            String::new()
        } else {
            base.join(&link).map(|u| u.to_string()).unwrap_or_default()
        };
        let pub_raw = first_non_empty(vec![
            pick(block, "pubDate"),
            pick(block, "published"),
            pick(block, "updated"),
            pick(block, "dc:date"),
            pick(block, "date"),
        ]);
        let context = excerpt_of(&first_non_empty(vec![
            pick(block, "content:encoded"),
            pick(block, "description"),
            pick(block, "summary"),
        ]));
        out.push(RawItem {
            item_id: String::new(),
            title,
            url: if absolute.is_empty() {
                base.to_string()
            } else {
                absolute
            },
            published_at: parse_feed_date(&pub_raw, now_dt),
            context,
        });
    }
    out
}
