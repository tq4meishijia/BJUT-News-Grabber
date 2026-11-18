//! 日志脱敏（S3）：cookie / authorization / api_key / password / token 的值替换为 ***。
//! 用单测自证（出门门槛 3）。

use std::sync::LazyLock;

use regex::Regex;

/// `sk-...` 形态的 API Key 一律打码。
static SK_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9_\-]{6,}").unwrap());

/// `key=value` / `key: value` / `key: value` 形态的敏感键。
static KV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(cookie|set-cookie|authorization|api[-_]?key|password|passwd|pwd|token|secret)["']?\s*[=:]\s*["']?([^"';,\s][^"';,]*)"#)
        .unwrap()
});

/// 脱敏一行日志文本。
pub fn redact_line(line: &str) -> String {
    let out = KV.replace_all(line, |caps: &regex::Captures| {
        format!("{}=***", &caps[1])
    });
    SK_KEY.replace_all(&out, "sk-***").into_owned()
}

/// 结构化日志（level + text），text 过脱敏。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    pub level: String,
    pub text: String,
}

impl LogLine {
    pub fn new(level: &str, text: impl AsRef<str>) -> Self {
        Self {
            level: level.to_string(),
            text: redact_line(text.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_cookie_values() {
        let line = "发送 cookie: JSESSIONID=abc123def; Path=/";
        let out = redact_line(line);
        assert!(out.contains("cookie=***"), "out={out}");
        assert!(!out.contains("abc123def"), "out={out}");
    }

    #[test]
    fn redacts_authorization_bearer() {
        let line = r#"headers: {"authorization": "Bearer sk-abcdef123456"}"#;
        let out = redact_line(line);
        assert!(!out.contains("sk-abcdef123456"), "out={out}");
        assert!(out.contains("***"), "out={out}");
    }

    #[test]
    fn redacts_api_key_and_password() {
        let out = redact_line("config api_key=sk-live-99887766 password=hunter2");
        assert!(!out.contains("hunter2"), "out={out}");
        assert!(!out.contains("sk-live"), "out={out}");
    }

    #[test]
    fn redacts_set_cookie_header() {
        let out = redact_line("set-cookie: TGC=ticket-xyz; Domain=cas.bjut.edu.cn");
        assert!(!out.contains("ticket-xyz"), "out={out}");
    }

    #[test]
    fn keeps_normal_text() {
        let line = "抓到 5 条，新增 3 条";
        assert_eq!(redact_line(line), line);
    }

    #[test]
    fn log_line_wrapper_redacts() {
        let log = LogLine::new("info", "cookie: a=b");
        assert!(log.text.contains("***"));
    }
}
