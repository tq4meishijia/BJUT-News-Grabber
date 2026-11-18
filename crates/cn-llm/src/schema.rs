//! Schema 校验与修复（§6.2 表）：索引合法 / 长度截断 / 枚举归一 / 时间校验 / 置信度缺省。

use regex::Regex;

/// 8 个封闭类别；越界一律归「其他」（计入 category_fallback_rate）。
pub const CATEGORIES: [&str; 8] = [
    "教务",
    "考试",
    "评奖资助",
    "讲座学术",
    "就业实习",
    "后勤生活",
    "招标采购",
    "其他",
];

/// 校验后的单条结构化结果。
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedRow {
    pub i: usize,
    pub category: String,
    pub gist: String,
    pub useful: Vec<String>,
    pub tags: Vec<String>,
    /// "YYYY-MM-DDTHH:mm" 或空（校验失败置空，由调用方回落 find_deadline）。
    pub deadline: String,
    pub contact: String,
    pub audience: String,
    pub relevant: bool,
    pub confidence: f64,
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// 时间合法性：正则 + 年份 2000–2100（发布日下限由 client 侧统一校验，§6.2）。
fn valid_deadline(iso: &str) -> bool {
    static RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}").unwrap());
    if iso.is_empty() {
        return true;
    }
    if !RE.is_match(iso) {
        return false;
    }
    let year: i32 = iso.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0);
    (2000..=2100).contains(&year)
}

/// 从模型文本中提取 JSON 数组片段（首个 `[` 到末个 `]`）。
pub fn extract_json_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
}

/// 校验并修复一批行（§6.2）：
/// - 索引越界 → 丢弃该条
/// - 字符串超长 → 截断（gist ≤120 / useful ≤4 条每条 ≤60 / tags ≤5）
/// - category 越界 → 归「其他」
/// - deadline 非法 → 置空（调用方回落 find_deadline）
/// - confidence 缺失或越界 → 0.5 / 夹紧
///
/// 返回值附带校验错误说明（用于带错重试一次）。
pub fn validate_rows(rows: &[serde_json::Value], batch_len: usize) -> (Vec<CheckedRow>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for row in rows {
        let Some(i) = row.get("i").and_then(|v| v.as_i64()) else {
            errors.push("缺少合法索引 i".to_string());
            continue;
        };
        if i < 0 || i as usize >= batch_len {
            errors.push(format!("索引越界：{i}"));
            continue;
        }
        let i = i as usize;
        let gist = truncate_chars(row.get("gist").and_then(|v| v.as_str()).unwrap_or(""), 120);
        let mut useful: Vec<String> = row
            .get("useful")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| truncate_chars(s, 60))
                    .collect()
            })
            .unwrap_or_default();
        useful.truncate(4);
        let mut tags: Vec<String> = row
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        tags.truncate(5);

        let raw_category = row.get("category").and_then(|v| v.as_str()).unwrap_or("");
        let category = if CATEGORIES.contains(&raw_category) {
            raw_category.to_string()
        } else {
            if !raw_category.is_empty() {
                errors.push(format!("类别越界：{raw_category}"));
            }
            "其他".to_string()
        };

        let raw_deadline = row.get("deadline").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let deadline = if valid_deadline(&raw_deadline) {
            raw_deadline.chars().take(16).collect::<String>()
        } else {
            errors.push(format!("时间非法：{raw_deadline}"));
            String::new()
        };

        let contact = truncate_chars(row.get("contact").and_then(|v| v.as_str()).unwrap_or(""), 60);
        let audience = row
            .get("audience")
            .and_then(|v| v.as_str())
            .filter(|s| matches!(*s, "本科生" | "研究生" | "全体" | "教师" | "不限"))
            .unwrap_or("不限")
            .to_string();
        let relevant = row.get("relevant").and_then(|v| v.as_bool()).unwrap_or(true);
        let confidence = match row.get("confidence").and_then(|v| v.as_f64()) {
            Some(c) if (0.0..=1.0).contains(&c) => c,
            Some(c) => {
                errors.push(format!("置信度越界：{c}"));
                c.clamp(0.0, 1.0)
            }
            None => 0.5,
        };
        out.push(CheckedRow {
            i,
            category,
            gist,
            useful,
            tags,
            deadline,
            contact,
            audience,
            relevant,
            confidence,
        });
    }
    (out, errors)
}

/// 带「校验错误」的修复提示（一次重试用）。
pub fn repair_rows(errors: &[String]) -> String {
    format!(
        "你上次的输出有以下问题，请只输出修正后的 JSON 数组，不要解释：\n{}",
        errors.join("\n")
    )
}
