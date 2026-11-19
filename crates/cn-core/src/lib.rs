//! cn-core — 纯逻辑核心：HTML/RSS/JSON 列表解析、正文定位、截止与联系人抽取、打分、规则摘要。
//!
//! 硬约束（对照 `docs/rust-tauri-refactor-plan.md` §2.2 / §3.1）：
//! - 零 IO、零网络、零 async；`Cargo.toml` 不得出现 reqwest/tokio/tauri。
//! - 不读系统时钟：所有时间经 `&Clock`（[`cn_model::Clock`]）显式传入。
//! - 带前后向断言的正则只出现在 [`regex_compat`]（fancy-regex）。
//!
//! 与 §3.1 接口草图的偏差：`parse_html_list` / `parse_rss` / `parse_json_list`
//! 额外接收 `now: &dyn Clock`——`findDate` 的「M月D日」分支需要当前年，
//! 而本 crate 禁止读系统时钟，时间必须显式注入。

pub mod constants;
pub mod attachments;
pub mod contact;
pub mod content;
pub mod date;
pub mod normalize;
pub mod parse_html;
pub mod parse_json;
pub mod parse_rss;
pub mod redact;
pub mod regex_compat;
pub mod score;
pub mod summary;
pub mod tags;
pub mod text;

pub use constants::{is_noise_title, DefaultSource, DEFAULT_KEYWORDS, DEFAULT_SOURCES, NOTICE_WORDS};
pub use attachments::{
    extract_attachment_links, is_allowed_extension, normalize_attachment_url, sanitize_file_name,
    extension_of, AttachmentLink, ALLOWED_ATTACHMENT_EXTS,
};
pub use contact::find_contact;
pub use content::extract_main_content;
pub use date::{
    days_until, find_date, find_deadline, in_date_range, is_iso_day, iso_date, pad2,
    parse_due_local, parse_feed_date, validate_range, DateRange, INVALID_DATE_MESSAGE,
    START_AFTER_END_MESSAGE, START_AFTER_TODAY_MESSAGE,
};
pub use normalize::{countdown_text, describe_minutes, normalize_due};
pub use parse_html::parse_html_list;
pub use parse_json::{guess_list, parse_json_list, pick_path};
pub use parse_rss::parse_rss;
pub use redact::{redact_line, LogLine};
pub use score::score;
pub use summary::rule_summary;
pub use tags::pick_tags;
pub use text::{
    char_slice, char_window, clean_text, clean_title, cut, decode_entities, encode_uri_component,
    excerpt_of, html_to_text, js_len, strip_red_header, strip_tags,
};

use chrono::DateTime;
use cn_model::{Clock, Extract, ScoreInput, SummaryInput};

/// 抽取：标签 + 截止时间 + 联系人（对照 `golden.mjs::enrich` 的组装口径：
/// tags/deadline 取自 `title + " " + body`，contact 只取自 body）。
pub fn extract(title: &str, body: &str, keywords: &[String], now: &dyn Clock) -> Extract {
    let now_dt: DateTime<chrono::Local> = now.now();
    let text = format!("{title} {body}");
    let tags = pick_tags(&text, keywords);
    let deadline = find_deadline(&text, now_dt);
    let contact_raw = find_contact(body);
    let contact = if contact_raw.is_empty() {
        None
    } else {
        Some(contact_raw)
    };
    Extract {
        tags,
        deadline,
        contact,
    }
}

/// 打分（契约入口）：22 起算，5..=98 夹紧。
pub fn score_input(input: &ScoreInput) -> u8 {
    score(input)
}

/// 规则摘要（契约入口）：无 AI 时仍产出摘要与「对我有什么用」（B11）。
pub fn rule_summary_input(input: &SummaryInput) -> cn_model::RuleSummary {
    rule_summary(input)
}
