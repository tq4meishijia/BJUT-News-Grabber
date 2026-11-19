//! 附件链接的纯逻辑：白名单、相对 URL 补全、URL 规范化去重、文件名清洗。
//!
//! 硬约束：零 IO、零网络、零 async、零时钟；所有输入均由调用方传入。

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use crate::text::decode_entities;

/// 允许自动下载的附件扩展名（小写，不含点）。
pub const ALLOWED_ATTACHMENT_EXTS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "png", "jpg", "jpeg", "gif",
    "webp", "zip", "rar", "7z",
];

/// 从详情 HTML 中识别出的一个候选附件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentLink {
    /// 绝对 URL（已还原网关地址，可直接交给重写链路）。
    pub url: String,
    /// 清洗后的展示文件名（含扩展名）。
    pub file_name: String,
    /// 小写扩展名（不含点）。
    pub ext: String,
}

/// href / src 属性（单引号、双引号、无引号三种写法）。
static LINK_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:href|src)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap()
});

/// 从 HTML 的 `href` / `src` 中提取白名单附件链接，按规范化 URL 去重并保持出现顺序。
pub fn extract_attachment_links(html: &str, base: &Url) -> Vec<AttachmentLink> {
    let mut links: Vec<AttachmentLink> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    if html.is_empty() {
        return links;
    }
    for caps in LINK_ATTR.captures_iter(html) {
        let raw = caps
            .get(1)
            .or_else(|| caps.get(2))
            .or_else(|| caps.get(3))
            .map(|m| m.as_str())
            .unwrap_or("");
        let raw = decode_entities(raw).trim().to_string();
        if raw.is_empty()
            || raw.starts_with('#')
            || raw.to_ascii_lowercase().starts_with("javascript:")
            || raw.to_ascii_lowercase().starts_with("mailto:")
            || raw.to_ascii_lowercase().starts_with("data:")
            || raw.to_ascii_lowercase().starts_with("tel:")
        {
            continue;
        }
        let Ok(absolute) = base.join(&raw) else {
            continue;
        };
        if !matches!(absolute.scheme(), "http" | "https") {
            continue;
        }
        let (ext, file_name) = link_file_info(&absolute);
        if ext.is_empty() || !is_allowed_extension(&ext) {
            continue;
        }
        let url = absolute.to_string();
        let norm = normalize_attachment_url(&url);
        if seen.insert(norm) {
            links.push(AttachmentLink {
                url,
                file_name,
                ext,
            });
        }
    }
    links
}

/// URL 是否命中附件白名单。
pub fn is_allowed_extension(ext: &str) -> bool {
    ALLOWED_ATTACHMENT_EXTS.contains(&ext.trim().trim_start_matches('.').to_ascii_lowercase().as_str())
}

/// 从文件名（或含点的 URL 片段）里取小写扩展名，取不到返回空串。
pub fn extension_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, tail))
            if !tail.is_empty()
                && tail.len() <= 5
                && tail.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            tail.to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

/// 规范化附件 URL：scheme/host 小写、去默认端口与 fragment、剔除 tracking query、剩余 query 排序。
/// 解析失败时退化为去首尾空白的原串（仍可用于去重）。
pub fn normalize_attachment_url(raw: &str) -> String {
    let trimmed = raw.trim();
    let Ok(mut parsed) = Url::parse(trimmed) else {
        return trimmed.to_string();
    };
    parsed.set_fragment(None);
    let mut pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| !is_tracking_query(key.as_ref()))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    pairs.sort();
    parsed.set_query(None);
    if !pairs.is_empty() {
        parsed
            .query_pairs_mut()
            .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    parsed.to_string()
}

fn is_tracking_query(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.starts_with("utm_") || matches!(lower.as_str(), "spm" | "from" | "timestamp" | "_t")
}

/// 清洗展示文件名：去掉路径前缀与非法字符，强制小写扩展名，最长 100 字符。
pub fn sanitize_file_name(name: &str, ext: &str) -> String {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let mut stem = name.trim().to_string();
    if let Some(pos) = stem.rfind(['/', '\\']) {
        stem = stem[pos + 1..].to_string();
    }
    if !ext.is_empty() {
        let suffix = format!(".{ext}");
        if stem.to_ascii_lowercase().ends_with(&suffix) {
            stem.truncate(stem.len() - suffix.len());
        }
    }
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    let stem: String = cleaned.chars().take(100).collect();
    let stem = if stem.is_empty() {
        "attachment".to_string()
    } else {
        stem
    };
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

fn link_file_info(url: &Url) -> (String, String) {
    let segment = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or("");
    if segment.is_empty() {
        return (String::new(), String::new());
    }
    let mut candidate = percent_decode(segment);
    let mut ext = extension_of(&candidate);
    if ext.is_empty() {
        // 常见下载接口把真实文件名放在 query（filename / file / name）里。
        for (key, value) in url.query_pairs() {
            let key = key.to_ascii_lowercase();
            if !matches!(key.as_str(), "filename" | "file" | "name") {
                continue;
            }
            let value = value.into_owned();
            let found = extension_of(&value);
            if !found.is_empty() {
                ext = found;
                candidate = value;
                break;
            }
        }
    }
    if ext.is_empty() {
        return (String::new(), String::new());
    }
    (ext.clone(), sanitize_file_name(&candidate, &ext))
}

/// 纯字符串 percent 解码（不依赖任何额外 crate）。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[index + 1]), hex_value(bytes[index + 2])) {
                out.push(hi * 16 + lo);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
