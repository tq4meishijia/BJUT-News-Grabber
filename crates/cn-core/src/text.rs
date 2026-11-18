//! 文本工具：去标签 / 实体解码 / 清洗 / 标题规整 / 红头剥除 / 截断。
//! 逐行对照 `core.mjs` 的 stripTags / decodeEntities / cleanText / cleanTitle /
//! stripRedHeader / cut / excerptOf / htmlToText。

use std::sync::LazyLock;

use regex::Regex;

/// JS 字符串长度语义：BMP 内与 `chars().count()` 一致（fixtures 全为 BMP）。
pub fn js_len(text: &str) -> usize {
    text.chars().count()
}

/// `core.mjs::stripTags`：块级闭合标签转空格，其余标签去掉。
pub fn strip_tags(html: &str) -> String {
    static SCRIPT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<script[\s\S]*?</script>").unwrap());
    static STYLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<style[\s\S]*?</style>").unwrap());
    static BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    static BLOCK_CLOSE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)</(p|div|li|tr|h[0-9])>").unwrap());
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<[^>]+>").unwrap());

    let s = SCRIPT.replace_all(html, " ");
    let s = STYLE.replace_all(&s, " ");
    let s = BR.replace_all(&s, " ");
    let s = BLOCK_CLOSE.replace_all(&s, " ");
    TAG.replace_all(&s, " ").into_owned()
}

/// `core.mjs::decodeEntities`：命名实体表 + `&#x..;` / `&#..;` 数字实体。
pub fn decode_entities(text: &str) -> String {
    const NAMED: &[(&str, &str)] = &[
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&ldquo;", "\u{201c}"),
        ("&rdquo;", "\u{201d}"),
        ("&mdash;", "\u{2014}"),
        ("&hellip;", "\u{2026}"),
        ("&middot;", "\u{00b7}"),
    ];
    static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&#x([0-9a-fA-F]+);").unwrap());
    static DEC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&#([0-9]+);").unwrap());

    let mut out = text.to_string();
    for (key, value) in NAMED {
        out = out.replace(key, value);
    }
    out = HEX
        .replace_all(&out, |caps: &regex::Captures| {
            u32::from_str_radix(&caps[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned();
    DEC.replace_all(&out, |caps: &regex::Captures| {
        caps[1]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_else(|| caps[0].to_string())
    })
    .into_owned()
}

/// `core.mjs::cleanText`：去标签 → 实体解码 → 编辑器残渣 → 零宽字符 → 空白折叠。
pub fn clean_text(html: &str) -> String {
    static EDITORS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("窗体顶端|窗体底端|上一条：|下一条：").unwrap());
    static ZWSP: LazyLock<Regex> = LazyLock::new(|| Regex::new("[\u{200b}\u{feff}]").unwrap());
    static SPACES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[\s\u{00a0}\u{3000}]+").unwrap());

    let s = decode_entities(&strip_tags(html));
    let s = EDITORS.replace_all(&s, " ");
    let s = ZWSP.replace_all(&s, "");
    let s = SPACES.replace_all(&s, " ");
    s.trim().to_string()
}

/// `core.mjs::cleanTitle`：去掉栏目前缀（【xx】/ [xx]）与日期前缀。
pub fn clean_title(raw: &str) -> String {
    static BRACKET_CN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^【[^】]{1,14}】\s*").unwrap());
    static BRACKET_EN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\[[^\]]{1,14}\]\s*").unwrap());
    static DATE_PREFIX_SHORT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[0-9]{1,2}\s+[0-9]{4}[.\-/][0-9]{1,2}\s+").unwrap());
    static DATE_PREFIX_FULL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[0-9]{4}\s*[.\-/年]\s*[0-9]{1,2}\s*[.\-/月]\s*[0-9]{1,2}\s*日?\s+").unwrap());

    let text = clean_text(raw);
    let text = BRACKET_CN.replace(&text, "");
    let text = BRACKET_EN.replace(&text, "");
    let text = DATE_PREFIX_SHORT.replace(&text, "");
    let text = DATE_PREFIX_FULL.replace(&text, "");
    text.trim().to_string()
}

/// `core.mjs::cut`：按字符数截断，超出时留 `max-1` 个字符 + 省略号。
pub fn cut(text: &str, max: usize) -> String {
    let count = js_len(text);
    if count <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// `core.mjs::excerptOf`：清洗后取前 400 字符（不加省略号）。
pub fn excerpt_of(html: &str) -> String {
    clean_text(html).chars().take(400).collect()
}

/// `core.mjs::stripRedHeader`：剥掉红头文件开头的「XX大学文件 工大发〔2022〕15号 ————」页眉块。
pub fn strip_red_header(text: &str) -> String {
    static HEADER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\s\S]{0,160}?[—\-_=]{16,}").unwrap());
    let source = text;
    match HEADER.find(source) {
        Some(m) => source[m.end()..].trim().to_string(),
        None => source.to_string(),
    }
}

/// `core.mjs::htmlToText`：块级换行 / 行内标签去掉后走 cleanText（最终空白仍折叠成单空格）。
pub fn html_to_text(html: &str) -> String {
    static SCRIPT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<script[\s\S]*?</script>").unwrap());
    static STYLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<style[\s\S]*?</style>").unwrap());
    static BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());
    static BLOCK_CLOSE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)</(p|div|li|tr|h[0-9]|td|section|article)>").unwrap()
    });
    static INLINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)</?(span|font|b|strong|em|i|a|u|sub|sup|label|small|big|abbr|cite)\b[^>]*>",
        )
        .unwrap()
    });

    let s = SCRIPT.replace_all(html, " ");
    let s = STYLE.replace_all(&s, " ");
    let s = BR.replace_all(&s, "\n");
    let s = BLOCK_CLOSE.replace_all(&s, "\n");
    let s = INLINE.replace_all(&s, "");
    clean_text(&s)
}

/// 按字符窗口截取（JS `String.slice` 语义，BMP 内与 UTF-16 下标一致）。
pub fn char_slice(text: &str, start: usize, end: usize) -> String {
    text.chars().skip(start).take(end.saturating_sub(start)).collect()
}

/// 取 `start_byte..end_byte` 匹配前后各 `before`/`after` 个字符的窗口（对照
/// `parseHtmlList` 的 `html.slice(match.index - 180, match_end + 180)`）。
pub fn char_window(text: &str, start_byte: usize, end_byte: usize, before: usize, after: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start_char = text[..start_byte].chars().count();
    let end_char = start_char + text[start_byte..end_byte].chars().count();
    let lo = start_char.saturating_sub(before);
    let hi = (end_char + after).min(chars.len());
    chars[lo..hi].iter().collect()
}

/// `encodeURIComponent` 语义（除 `A-Z a-z 0-9 - _ . ! ~ * ' ( )` 外全部百分号编码）。
pub fn encode_uri_component(text: &str) -> String {
    const KEEP: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        if KEEP.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
