//! 日程时间归一与倒计时文案（对照 `core.mjs::normalizeDue / describeMinutes / countdownText`）。

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use cn_model::DueText;

use crate::date::pad2;

/// `core.mjs::normalizeDue`：各种写法 → "YYYY-MM-DDTHH:mm"（本地时区语义）。
/// `None` 等价于 JS 空串。第三分支覆盖常见 JS `Date` 可解析格式（非穷举，见 impl-notes）。
pub fn normalize_due(value: &str) -> DueText {
    static DATE_ONLY: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$").unwrap()
    });
    static DATE_TIME_PREFIX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}[T ][0-9]{2}:[0-9]{2}").unwrap()
    });

    let text = value.trim();
    if text.is_empty() {
        return None;
    }
    if DATE_ONLY.is_match(text) {
        return Some(format!("{text}T23:59"));
    }
    if DATE_TIME_PREFIX.is_match(text) {
        let replaced = text.replacen(' ', "T", 1);
        return Some(replaced.chars().take(16).collect());
    }
    // JS `new Date(text)` 的常见格式子集。
    let local = parse_js_date_like(text)?;
    Some(format!(
        "{}-{}-{}T{}:{}",
        local.year(),
        pad2(local.month()),
        pad2(local.day()),
        pad2(local.hour()),
        pad2(local.minute())
    ))
}

fn parse_js_date_like(text: &str) -> Option<DateTime<Local>> {
    if let Ok(dt) = DateTime::parse_from_rfc2822(text) {
        return Some(dt.with_timezone(&Local));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
        return Some(dt.with_timezone(&Local));
    }
    for fmt in [
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d",
        "%Y.%m.%d %H:%M:%S",
        "%Y.%m.%d",
    ] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(text, fmt) {
            return Local.from_local_datetime(&ndt).earliest();
        }
    }
    if let Ok(nd) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let midnight = nd.and_hms_opt(0, 0, 0)?;
        return Local.from_local_datetime(&midnight).earliest();
    }
    None
}

/// `core.mjs::describeMinutes`。
pub fn describe_minutes(minutes: i64) -> String {
    if minutes == 0 {
        return "准时".to_string();
    }
    if minutes % 1440 == 0 {
        return format!("{} 天前", minutes / 1440);
    }
    if minutes % 60 == 0 {
        return format!("{} 小时前", minutes / 60);
    }
    format!("{minutes} 分钟前")
}

/// `core.mjs::countdownText`：「还有 2 天 3 小时」这类人话倒计时。
pub fn countdown_text(due_at: &str, now: DateTime<Local>) -> String {
    let normalized = match normalize_due(due_at) {
        Some(v) => v,
        None => return String::new(),
    };
    let date = match crate::date::parse_due_local(&normalized) {
        Some(v) => v,
        None => return String::new(),
    };
    let diff = date - now;
    let abs = diff.num_milliseconds().unsigned_abs() as i64;
    let days = abs / 86_400_000;
    let hours = (abs % 86_400_000) / 3_600_000;
    let minutes = (abs % 3_600_000) / 60_000;
    let body = if days > 0 {
        format!("{days} 天 {hours} 小时")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分")
    } else {
        format!("{minutes} 分钟")
    };
    if diff >= Duration::zero() {
        format!("还有 {body}")
    } else {
        format!("已过 {body}")
    }
}
