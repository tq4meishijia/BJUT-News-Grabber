//! 编码嗅探与解码（对照 `core.mjs::sniffCharset / decodeBody`，顺序：charset 头 → BOM → meta → UTF-8）。

use std::sync::LazyLock;

use regex::Regex;

/// `core.mjs::sniffCharset` + BOM 探测。
/// 返回小写 charset 标签；未识别时 "utf-8"。
pub fn sniff_charset(content_type: &str, body: &[u8]) -> String {
    static CT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)charset=["']?([\w-]+)"#).unwrap());
    static META: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)<meta[^>]+charset=["']?([\w-]+)"#).unwrap());

    if let Some(caps) = CT.captures(content_type) {
        if let Some(m) = caps.get(1) {
            return m.as_str().to_ascii_lowercase();
        }
    }
    // BOM 优先于 meta（UTF-8/UTF-16）。
    if body.starts_with(&[0xef, 0xbb, 0xbf]) {
        return "utf-8".to_string();
    }
    if body.starts_with(&[0xff, 0xfe]) {
        return "utf-16le".to_string();
    }
    if body.starts_with(&[0xfe, 0xff]) {
        return "utf-16be".to_string();
    }
    // JS：head = buffer.subarray(0, 3000).toString('latin1')——逐字节映射。
    let head: Vec<u8> = body.iter().take(3000).copied().collect();
    let head_latin1: String = head.iter().map(|b| *b as char).collect();
    if let Some(caps) = META.captures(&head_latin1) {
        if let Some(m) = caps.get(1) {
            return m.as_str().to_ascii_lowercase();
        }
    }
    "utf-8".to_string()
}

fn encoding_for_label(label: &str) -> &'static encoding_rs::Encoding {
    let normalized = if label == "gb2312" { "gbk" } else { label };
    encoding_rs::Encoding::for_label(normalized.as_bytes()).unwrap_or(encoding_rs::UTF_8)
}

/// `core.mjs::decodeBody`：按嗅探出的 charset 解码；失败回退 UTF-8（lossy）。
/// UTF-8 解码与 Node `readFile utf8` / TextDecoder 同为 WHATWG 口径。
pub fn decode_body(body: &[u8], content_type: &str) -> String {
    let charset = sniff_charset(content_type, body);
    let encoding = encoding_for_label(&charset);
    let (cow, _enc, _had_errors) = encoding.decode(body);
    cow.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_from_header_and_meta() {
        assert_eq!(
            sniff_charset("text/html; charset=GBK", b""),
            "gbk"
        );
        assert_eq!(
            sniff_charset("", b"<html><meta charset=\"gb2312\"><body>"),
            "gb2312"
        );
        assert_eq!(sniff_charset("", b"plain"), "utf-8");
        assert_eq!(sniff_charset("", &[0xef, 0xbb, 0xbf, b'a']), "utf-8");
    }

    #[test]
    fn decode_gbk_round_trip() {
        let original = "关于考试安排的通知";
        // encoding_rs 的 GBK 解码器配 gbk/gb18030 编码输出即可。
        let (cow, _, _) = encoding_rs::GBK.encode(original);
        let encoded = cow.into_owned();
        let decoded = decode_body(&encoded, "text/html; charset=gbk");
        assert_eq!(decoded, original);
    }
}
