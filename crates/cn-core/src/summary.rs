//! 规则摘要（对照 `core.mjs::ruleSummary`）：无 AI 时保证「摘要 + 对我有什么用」永远有内容（B11）。

use std::sync::LazyLock;

use cn_model::{RuleSummary, SummaryInput};
use regex::Regex;

use crate::date::days_until;
use crate::text::{cut, js_len, strip_red_header};

/// `core.mjs::ruleSummary`：逐行移植。
pub fn rule_summary(input: &SummaryInput) -> RuleSummary {
    static ACTION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("报名|申请|提交|上传|填报|填写").unwrap());
    static EXAM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("考试|补考|重修|缓考|选课").unwrap());
    static LECTURE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("讲座|报告会|学术报告").unwrap());
    static AWARD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("评选|奖学金|助学金|申请").unwrap());
    static WS_RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

    let source = strip_red_header(&input.content);
    let flat_title: String = WS_RUN.replace_all(&input.title, "").into_owned();
    let flat_body: String = WS_RUN
        .replace_all(&WS_RUN.replace_all(&source, " "), " ")
        .into_owned();
    let mut body = source.clone();
    let flat_body_nowhitespace: String = WS_RUN.replace_all(&flat_body, "").into_owned();
    if js_len(&flat_title) >= 8 && flat_body_nowhitespace.starts_with(&flat_title) {
        body = flat_body_nowhitespace[flat_title.len()..].to_string();
    }
    let body_for_match = format!("{} {}", input.title, body);

    let mut useful: Vec<String> = Vec::new();
    if !input.tags.is_empty() {
        useful.push(format!("关联：{}", input.tags.join("、")));
    }
    if let Some(deadline) = input.deadline.as_deref().filter(|d| !d.is_empty()) {
        let when = deadline.replacen('T', " ", 1);
        match days_until(deadline, input.now) {
            Some(days) if days >= 0.0 => {
                useful.push(format!(
                    "截止：{}（还有 {} 天）",
                    when,
                    days.ceil().max(0.0) as i64
                ));
            }
            Some(_) => {
                useful.push(format!("截止时间已过：{when}"));
            }
            None => {
                useful.push(format!("截止：{when}"));
            }
        }
    }
    if let Some(contact) = input.contact.as_deref().filter(|c| !c.is_empty()) {
        useful.push(format!("找谁办：{contact}"));
    }
    if ACTION.is_match(&body_for_match) {
        useful.push("需要你主动报名/提交材料".to_string());
    }
    if EXAM.is_match(&body_for_match) {
        useful.push("和考试/选课安排相关，建议进系统确认".to_string());
    }
    if LECTURE.is_match(&body_for_match) {
        useful.push("讲座类，注意时间地点是否需要报名".to_string());
    }
    if AWARD.is_match(&body_for_match) {
        useful.push("涉及评选或资助，留意资格条件".to_string());
    }
    if useful.is_empty() {
        useful.push("校园事务类，快速浏览即可".to_string());
    }
    useful.truncate(4);

    let first_sentence = body
        .split(['。', '！', '？', '\n'])
        .map(str::trim)
        .find(|s| js_len(s) >= 12)
        .map(str::to_string);
    let summary = cut(
        &first_sentence.unwrap_or_else(|| input.title.clone()),
        110,
    );
    RuleSummary {
        why: useful.clone(),
        summary,
        useful,
    }
}
