//! 日期工具：findDate / findDeadline / isoDate / parseFeedDate / daysUntil。
//! 逐分支对照 `core.mjs`；`(?![\d.])` 等断言走 [`crate::regex_compat`]（fancy-regex）。

use std::sync::LazyLock;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};
use cn_model::Deadline;
use regex::Regex;

use crate::regex_compat::LookRegex;

/// `core.mjs::pad2`。
pub fn pad2(n: u32) -> String {
    format!("{n:02}")
}

/// `core.mjs::isoDate`：范围校验（2000–2100 / 1–12 / 1–31），不校验月份实际天数。
pub fn iso_date(y: i32, m: u32, d: u32) -> String {
    if !(2000..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return String::new();
    }
    format!("{y:04}-{m:02}-{d:02}")
}

/// 分支 1：完整 `YYYY<sep>M<sep>D`（无断言，plain regex）。
static DATE_FULL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([0-9]{4})\s*[-/年.]\s*([0-9]{1,2})\s*[-/月.]\s*([0-9]{1,2})\s*日?").unwrap()
});
/// 分支 2：「22 2026.10 标题」日 + 年月（`(?![\d.])`）。
static DATE_DAY_FIRST: LazyLock<LookRegex> = LazyLock::new(|| {
    LookRegex::new(r"(?:^|\s)([0-9]{1,2})\s+([0-9]{4})[.]([0-9]{1,2})(?![0-9.])")
});
/// 分支 3：`YYYY<sep>M` 补 1 日（`(?![\d-/.])`）。
static DATE_YEAR_MONTH: LazyLock<LookRegex> = LazyLock::new(|| {
    LookRegex::new(r"([0-9]{4})\s*[-/年.]\s*([0-9]{1,2})\s*月?(?![0-9\-/.])")
});
/// 分支 4：`M<sep>D` 补当前年（`(?![\d-/.])`）。
static DATE_MONTH_DAY: LazyLock<LookRegex> = LazyLock::new(|| {
    LookRegex::new(r"([0-9]{1,2})\s*[-/月]\s*([0-9]{1,2})\s*日?(?![0-9\-/.])")
});

fn num(group: Option<&str>) -> i64 {
    group.and_then(|s| s.parse::<i64>().ok()).unwrap_or(-1)
}

/// `core.mjs::findDate(text)` 语义，年份取 `now` 的本地年（JS 用 `new Date().getFullYear()`）。
/// 四个分支按顺序尝试；某分支命中但 `isoDate` 越界时继续下一分支。
pub fn find_date(text: &str, now: DateTime<Local>) -> String {
    if let Some(caps) = DATE_FULL.captures(text) {
        let out = iso_date(
            num(caps.get(1).map(|m| m.as_str())) as i32,
            num(caps.get(2).map(|m| m.as_str())) as u32,
            num(caps.get(3).map(|m| m.as_str())) as u32,
        );
        if !out.is_empty() {
            return out;
        }
    }
    if let Some(caps) = DATE_DAY_FIRST.captures(text) {
        // JS：isoDate(m[2], m[3], m[1]) —— 年、月、日
        let out = iso_date(num(caps.get(2)) as i32, num(caps.get(3)) as u32, num(caps.get(1)) as u32);
        if !out.is_empty() {
            return out;
        }
    }
    if let Some(caps) = DATE_YEAR_MONTH.captures(text) {
        let out = iso_date(num(caps.get(1)) as i32, num(caps.get(2)) as u32, 1);
        if !out.is_empty() {
            return out;
        }
    }
    if let Some(caps) = DATE_MONTH_DAY.captures(text) {
        let out = iso_date(now.year(), num(caps.get(1)) as u32, num(caps.get(2)) as u32);
        if !out.is_empty() {
            return out;
        }
    }
    String::new()
}

/// `core.mjs::findDeadline` 的四条高置信度固定句式。
static DEADLINE_STRICT_0: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:截止|截至)[^。；;\n]{0,14}?([0-9]{4})\s*[年./-]\s*([0-9]{1,2})\s*[月./-]\s*([0-9]{1,2})\s*日?").unwrap()
});
static DEADLINE_STRICT_1: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:于|在)\s*([0-9]{4})\s*[年./-]\s*([0-9]{1,2})\s*[月./-]\s*([0-9]{1,2})\s*日?\s*(?:前|之前|以前|止|截止)").unwrap()
});
static DEADLINE_STRICT_2: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([0-9]{4})\s*[年./-]\s*([0-9]{1,2})\s*[月./-]\s*([0-9]{1,2})\s*日?\s*(?:前|之前|以前|截止)").unwrap()
});
static DEADLINE_STRICT_3: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([0-9]{1,2})\s*月\s*([0-9]{1,2})\s*日\s*(?:前|之前|以前|截止)").unwrap()
});
/// 锚点词（带 /g 全局扫描）。
static DEADLINE_ANCHORS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"截止|截至|不晚于|务必于|请于|须于|需要在|前完成|前提交|前报名").unwrap()
});
/// 「11月5日前」但没有「截止」字样。
static DEADLINE_MONTH_DAY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([0-9]{1,2})\s*月\s*([0-9]{1,2})\s*日\s*(?:之?前|截止)").unwrap()
});
/// `HH:MM` 时钟。
static CLOCK_HM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([0-9]{1,2})\s*[:：]\s*([0-9]{2})").unwrap()
});
/// 锚点窗口里的 `HH:MM`（前置非数字断言用字符类等价改写：`(?:^|[^0-9])`）。
static CLOCK_HM_ANCHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^0-9])([0-9]{1,2})\s*[:：]\s*([0-9]{2})").unwrap()
});
/// `N点` / `N时`（其后不能跟数字，断言走 fancy-regex）。
static CLOCK_HOUR_ONLY: LazyLock<LookRegex> =
    LazyLock::new(|| LookRegex::new(r"([0-9]{1,2})\s*[点时](?![0-9])"));

fn hm_from_after(after: &str) -> String {
    if let Some(caps) = CLOCK_HM.captures(after) {
        let h = num(caps.get(1).map(|m| m.as_str())) as u32;
        let m = num(caps.get(2).map(|m| m.as_str())) as u32;
        return format!("{}:{}", pad2(h), pad2(m));
    }
    if let Some(caps) = CLOCK_HOUR_ONLY.captures(after) {
        let h = num(caps.get(1)) as u32;
        return format!("{}:00", pad2(h));
    }
    "23:59".to_string()
}

fn strict_result(text: &str, caps: &regex::Captures, group_count: usize, now: DateTime<Local>) -> Option<Deadline> {
    // JS：hit.length === 4（3 组）用组 1/2/3；否则（2 组）用当前年 + 组 1/2。
    let (year, month, day) = if group_count == 4 {
        (
            num(caps.get(1).map(|m| m.as_str())) as i32,
            num(caps.get(2).map(|m| m.as_str())) as u32,
            num(caps.get(3).map(|m| m.as_str())) as u32,
        )
    } else {
        (
            now.year(),
            num(caps.get(1).map(|m| m.as_str())) as u32,
            num(caps.get(2).map(|m| m.as_str())) as u32,
        )
    };
    let date = iso_date(year, month, day);
    if date.is_empty() {
        return None;
    }
    let m0 = caps.get(0)?;
    // JS：after = s.slice(hit.index, hit.index + hit[0].length + 30)
    let after = crate::text::char_window(text, m0.start(), m0.end(), 0, 30);
    let hm = hm_from_after(&after);
    Some(Deadline {
        iso: format!("{date}T{hm}"),
        raw: m0.as_str().to_string(),
    })
}

/// `core.mjs::findDeadline(text)`：返回 `None` 等价于 JS 空串。
pub fn find_deadline(text: &str, now: DateTime<Local>) -> Option<Deadline> {
    // 四条高置信度句式（组数 3 → js len 4；组数 2 → js len 3）。
    let strict: [(&Regex, usize); 4] = [
        (&DEADLINE_STRICT_0, 4),
        (&DEADLINE_STRICT_1, 4),
        (&DEADLINE_STRICT_2, 4),
        (&DEADLINE_STRICT_3, 3),
    ];
    for (pattern, js_len) in strict {
        if let Some(caps) = pattern.captures(text) {
            if let Some(deadline) = strict_result(text, &caps, js_len, now) {
                return Some(deadline);
            }
        }
    }
    // 锚点窗口扫描。
    for m in DEADLINE_ANCHORS.find_iter(text) {
        let window = crate::text::char_window(text, m.start(), m.end(), 30, 60);
        let date = find_date(&window, now);
        if date.is_empty() {
            continue;
        }
        let mut hm = "23:59".to_string();
        if let Some(caps) = CLOCK_HM_ANCHOR.captures(&window) {
            let h = num(caps.get(1).map(|m| m.as_str())) as u32;
            let mm = num(caps.get(2).map(|m| m.as_str())) as u32;
            hm = format!("{}:{}", pad2(h), pad2(mm));
        } else if let Some(caps) = CLOCK_HOUR_ONLY.captures(&window) {
            let h = num(caps.get(1)) as u32;
            hm = format!("{}:00", pad2(h));
        }
        return Some(Deadline {
            iso: format!("{date}T{hm}"),
            raw: window,
        });
    }
    // 「11月5日前」兜底（补当前年）。
    if let Some(caps) = DEADLINE_MONTH_DAY.captures(text) {
        let date = iso_date(
            now.year(),
            num(caps.get(1).map(|m| m.as_str())) as u32,
            num(caps.get(2).map(|m| m.as_str())) as u32,
        );
        if !date.is_empty() {
            return Some(Deadline {
                iso: format!("{date}T23:59"),
                raw: caps.get(0).map(|m| m.as_str().to_string()).unwrap_or_default(),
            });
        }
    }
    None
}

/// `core.mjs::daysUntil`：`(due - now) / 86400000`，解析失败或空串返回 `None`。
pub fn days_until(deadline_iso: &str, now: DateTime<Local>) -> Option<f64> {
    if deadline_iso.is_empty() {
        return None;
    }
    let due = parse_due_local(deadline_iso)?;
    Some((due - now).num_milliseconds() as f64 / 86_400_000.0)
}

/// 把 "YYYY-MM-DDTHH:mm"（本地时区语义，与 JS `new Date(str)` 一致）解析为本地时间。
pub fn parse_due_local(iso: &str) -> Option<DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M").ok()?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&naive).single())
}

/// `core.mjs::parseFeedDate`：数值日期优先，其次 JS `new Date` 的常见格式（RFC2822/ISO）。
pub fn parse_feed_date(raw: &str, now: DateTime<Local>) -> String {
    let text = crate::text::clean_text(raw);
    if text.is_empty() {
        return String::new();
    }
    let numeric = find_date(&text, now);
    if !numeric.is_empty() {
        return numeric;
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(&text) {
        let local = dt.with_timezone(&Local);
        return iso_date(local.year(), local.month(), local.day());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(&text) {
        let local = dt.with_timezone(&Local);
        return iso_date(local.year(), local.month(), local.day());
    }
    for fmt in ["%Y/%m/%d %H:%M:%S", "%Y/%m/%d %H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y/%m/%d", "%Y.%m.%d"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(&text, fmt) {
            if let Some(local) = Local.from_local_datetime(&ndt).earliest() {
                return iso_date(local.year(), local.month(), local.day());
            }
        }
    }
    if let Ok(nd) = NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
        return iso_date(nd.year(), nd.month(), nd.day());
    }
    String::new()
}
