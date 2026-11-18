//! 打分（对照 `core.mjs::scoreNotice`）：22 起算，5..=98 夹紧。

use std::sync::LazyLock;

use chrono::{DateTime, Local};
use cn_model::ScoreInput;
use regex::Regex;

use crate::date::days_until;

/// `core.mjs::scoreNotice`：逐行移植，返回 u8。
pub fn score(input: &ScoreInput) -> u8 {
    static URGENT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("紧急|务必|立即|马上|今天|今日|明天").unwrap());
    static LECTURE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("讲座|报告会|学术报告|宣讲").unwrap());
    static GRADE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("成绩|绩点|学分|毕业|学位|学籍").unwrap());
    static BID_TITLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("招标|中标|采购|物业|食堂改造").unwrap());
    static DOCS_TITLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("下载专区|规章制度|资料汇编").unwrap());

    let mut score: i64 = 22;
    let body = format!("{} {}", input.title, input.content);
    for tag in &input.tags {
        if body.contains(tag.as_str()) {
            score += 6;
        }
    }
    if let Some(days) = input.deadline.as_deref().and_then(|d| days_until(d, input.now)) {
        if days < 0.0 {
            score += 2;
        } else if days <= 3.0 {
            score += 30;
        } else if days <= 7.0 {
            score += 24;
        } else if days <= 30.0 {
            score += 12;
        }
    }
    if URGENT.is_match(&body) {
        score += 8;
    }
    if LECTURE.is_match(&body) {
        score += 4;
    }
    if GRADE.is_match(&body) {
        score += 8;
    }
    if BID_TITLE.is_match(&input.title) {
        score -= 18;
    }
    if DOCS_TITLE.is_match(&input.title) {
        score -= 10;
    }
    // JS：Math.max(5, Math.min(98, Math.round(score)))；加减均为整数，round 为恒等。
    score.clamp(5, 98) as u8
}

/// 便捷入口：不需要完整 [`ScoreInput`] 时使用。
pub fn score_notice(
    title: &str,
    content: &str,
    tags: &[String],
    deadline: Option<&str>,
    now: DateTime<Local>,
) -> u8 {
    score(&ScoreInput {
        title: title.to_string(),
        content: content.to_string(),
        tags: tags.to_vec(),
        deadline: deadline.map(str::to_string),
        keywords: Vec::new(),
        now,
    })
}
