//! 联系人抽取（对照 `core.mjs::findContact`）。

use std::sync::LazyLock;

use regex::Regex;

/// 从正文里找「找谁办」：联系人与电话，以 ` · ` 连接；没有则空串。
pub fn find_contact(text: &str) -> String {
    static NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:联系人|联系老师|负责老师)[：:\s]*([\u{4e00}-\u{9fa5}]{2,4}(?:老师|同学)?)")
            .unwrap()
    });
    static PHONE_MAIN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?:电话|联系电话|联系方式)[：:\s]*(0[0-9]{2,3}-?[0-9]{7,8}|1[3-9][0-9]{9}|[0-9]{8})",
        )
        .unwrap()
    });
    static PHONE_FALLBACK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(0[0-9]{2,3}-?[0-9]{7,8})").unwrap());

    let mut parts: Vec<String> = Vec::new();
    if let Some(caps) = NAME.captures(text) {
        if let Some(name) = caps.get(1) {
            parts.push(name.as_str().to_string());
        }
    }
    let phone = PHONE_MAIN
        .captures(text)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .or_else(|| {
            PHONE_FALLBACK
                .captures(text)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        });
    if let Some(p) = phone {
        parts.push(p);
    }
    parts.join(" · ")
}
