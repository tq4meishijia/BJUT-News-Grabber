//! WebVPN（Wengine 网关）编解码（对照 `core.mjs:447-614`）。
//!
//! hex 形态：AES-128-CFB（key = IV = `wrdvpnisthebest!`）对主机名加密，
//! IV 明文（即 key 自身的 hex）拼在密文 hex 前。已用 core.mjs 同款算法生成的
//! 种子（fixtures/raw/html__webvpn-* 与单测向量）双向核对。

use std::sync::LazyLock;

use aes::cipher::{AsyncStreamCipher, KeyIvInit};
use regex::Regex;

/// BJUT 专用网关内置地址。
pub const WEBVPN_GATEWAY: &str = "https://webvpn.bjut.edu.cn";

/// Wengine 固定密钥/IV。
pub const WENGINE_KEY: &str = "wrdvpnisthebest!";

/// 默认「网关会话过期」登录页特征（不写死某校文案）。
pub const DEFAULT_LOGIN_PAGE_PATTERN: &str = concat!(
    r#"<input[^>]+type=["']?password|"#,
    r#"<title[^>]*>[^<]*(?:登录|登陆|login|身份认证)[^<]*</title>|"#,
    r#"id=["']?(?:unPassword|pwPassword|cptPassword|loginForm)"#
);

/// 校外模式配置（gateway/loginPagePattern 用内置值，只存开关与 Cookie）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebvpnConfig {
    pub enabled: bool,
    pub gateway: String,
    pub cookie: String,
    pub login_page_pattern: String,
}

impl Default for WebvpnConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gateway: WEBVPN_GATEWAY.to_string(),
            cookie: String::new(),
            login_page_pattern: DEFAULT_LOGIN_PAGE_PATTERN.to_string(),
        }
    }
}

impl WebvpnConfig {
    /// `core.mjs::isWebvpnEnabled`。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// `core.mjs::isWebvpnEnabled`。
pub fn is_webvpn_enabled(cfg: &WebvpnConfig) -> bool {
    cfg.enabled
}

/// 网关地址（去掉尾斜杠）。
fn gateway_of(cfg: &WebvpnConfig) -> &str {
    cfg.gateway.trim_end_matches('/')
}

type Aes128CfbEnc = cfb_mode::Encryptor<aes::Aes128>;
type Aes128CfbDec = cfb_mode::Decryptor<aes::Aes128>;

/// `core.mjs::encodeWengineHost`：key hex + 密文 hex（截到主机名字节数）。
pub fn encode_wengine_host(host: &str) -> String {
    let key = WENGINE_KEY.as_bytes();
    let mut secret = host.as_bytes().to_vec();
    Aes128CfbEnc::new(key.into(), key.into()).encrypt(&mut secret);
    let mut out = hex_encode(key);
    out.push_str(&hex_encode(&secret));
    out
}

/// `core.mjs::decodeWengineHost`：不合法或解码失败返回空串。
pub fn decode_wengine_host(hex: &str) -> String {
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) || hex.len() < 34 || !hex.len().is_multiple_of(2) {
        return String::new();
    }
    let key = match hex_decode(&hex[..32]) {
        Some(k) if k.len() == 16 => k,
        _ => return String::new(),
    };
    let secret = match hex_decode(&hex[32..]) {
        Some(s) => s,
        None => return String::new(),
    };
    let mut buf = secret;
    Aes128CfbDec::new(key.as_slice().into(), key.as_slice().into()).decrypt(&mut buf);
    String::from_utf8(buf).unwrap_or_default()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

/// 网关 URL 形态：`[https?://gateway]/(https?(-port)?)/<hex>(/path)?`
fn wengine_pattern(anchored: bool) -> Regex {
    let host = regex::escape("webvpn.bjut.edu.cn");
    let source = format!(
        "{}(?:https?://{host})?/(https?(?:-[0-9]+)?)/([0-9a-fA-F]{{34,}})(/[^\"'<>\\s]*)?{}",
        if anchored { "^" } else { "" },
        if anchored { "$" } else { "" },
    );
    Regex::new(&format!("(?i){source}")).expect("wengine pattern")
}

static WENGINE_ABS: LazyLock<Regex> = LazyLock::new(|| wengine_pattern(true));
static WENGINE_ALL: LazyLock<Regex> = LazyLock::new(|| {
    let host = regex::escape("webvpn.bjut.edu.cn");
    let source = format!(
        "(?:https?://{host})?/(https?(?:-[0-9]+)?)/([0-9a-fA-F]{{34,}})(/[^\"'<>\\s]*)?"
    );
    Regex::new(&format!("(?i){source}")).expect("wengine pattern all")
});

/// 单个匹配还原（restoreWengineMatch）；解码失败返回原样。
fn restore_wengine_match(all: &str, scheme: &str, hex: &str, path: Option<&str>) -> String {
    let host = decode_wengine_host(hex);
    if host.is_empty() {
        return all.to_string();
    }
    // JS：/-((\d+)$/ 于 scheme（形如 https-8443）；保留原始数字串避免前导零差异。
    let port = scheme
        .rsplit_once('-')
        .map(|(_, p)| p)
        .filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let protocol = if scheme.starts_with("https") {
        "https"
    } else {
        "http"
    };
    let port_part = match port {
        Some(p) => format!(":{p}"),
        None => String::new(),
    };
    format!(
        "{protocol}://{host}{port_part}{}",
        path.filter(|p| !p.is_empty()).unwrap_or("/")
    )
}

fn has_http_scheme(value: &str) -> bool {
    (value.len() >= 7 && value[..7].eq_ignore_ascii_case("http://"))
        || (value.len() >= 8 && value[..8].eq_ignore_ascii_case("https://"))
}

/// `core.mjs::rewriteUrl`：门户原始网址 → Wengine 网关地址（关闭时原样返回）。
pub fn rewrite_url(url: &str, cfg: &WebvpnConfig) -> String {
    if !is_webvpn_enabled(cfg) {
        return url.to_string();
    }
    let value = url;
    let gateway = gateway_of(cfg);
    if !has_http_scheme(value) {
        return value.to_string();
    }
    if value.len() >= gateway.len() && value[..gateway.len()].eq_ignore_ascii_case(gateway) {
        return value.to_string();
    }
    let target = match url::Url::parse(value) {
        Ok(u) => u,
        Err(_) => return value.to_string(),
    };
    let protocol = target.scheme();
    // url crate 会把默认端口从序列化里去掉：port() = Some 仅当非标准端口（JS target.port 同义）。
    let non_standard_port = target.port().is_some();
    let scheme = if non_standard_port {
        format!(
            "{protocol}-{}",
            target.port().map(|p| p.to_string()).unwrap_or_default()
        )
    } else {
        protocol.to_string()
    };
    let host = target.host_str().unwrap_or("");
    let path = target.path();
    let search = target.query().map(|q| format!("?{q}")).unwrap_or_default();
    format!(
        "{gateway}/{scheme}/{}{path}{search}",
        encode_wengine_host(host)
    )
}

/// `core.mjs::restoreUrl`：网关地址 → 门户原始网址（与 rewrite_url 互为逆运算）。
pub fn restore_url(url: &str, cfg: &WebvpnConfig) -> String {
    if !is_webvpn_enabled(cfg) {
        return url.to_string();
    }
    match WENGINE_ABS.captures(url) {
        Some(caps) => restore_wengine_match(
            caps.get(0).map(|m| m.as_str()).unwrap_or(url),
            caps.get(1).map(|m| m.as_str()).unwrap_or(""),
            caps.get(2).map(|m| m.as_str()).unwrap_or(""),
            caps.get(3).map(|m| m.as_str()),
        ),
        None => url.to_string(),
    }
}

/// `core.mjs::restoreUrlsInText`：还原响应文本里被网关改写过的链接。
pub fn restore_urls_in_text(text: &str, cfg: &WebvpnConfig) -> String {
    if !is_webvpn_enabled(cfg) {
        return text.to_string();
    }
    if text.is_empty() {
        return text.to_string();
    }
    WENGINE_ALL
        .replace_all(text, |caps: &regex::Captures| {
            restore_wengine_match(
                caps.get(0).map(|m| m.as_str()).unwrap_or(""),
                caps.get(1).map(|m| m.as_str()).unwrap_or(""),
                caps.get(2).map(|m| m.as_str()).unwrap_or(""),
                caps.get(3).map(|m| m.as_str()),
            )
        })
        .into_owned()
}

/* ---------------------- 网关会话过期的响应识别 ---------------------- */

/// 门户正常业务响应里常见的数据容器。
const PORTAL_DATA_KEYS: [&str; 8] = [
    "d", "data", "items", "list", "rows", "records", "result", "content",
];

/// 网关 JSON 错误体里的登录/会话语义。
static SESSION_WORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("(?i)请登录|未登录|登录超时|重新登录|session expired|login required|unauthorized|请先登录")
        .unwrap()
});

/// `core.mjs::matchesLoginPattern`。
pub fn matches_login_pattern(text: &str, cfg: &WebvpnConfig) -> bool {
    let pattern = if cfg.login_page_pattern.trim().is_empty() {
        DEFAULT_LOGIN_PAGE_PATTERN
    } else {
        cfg.login_page_pattern.trim()
    };
    match Regex::new(&format!("(?i){pattern}")) {
        Ok(re) => re.is_match(text),
        Err(_) => false,
    }
}

/// `core.mjs::hasPortalData`：含非空业务数据容器的 JSON 一律当正常数据。
pub fn has_portal_data(value: &serde_json::Value) -> bool {
    if !value.is_object() {
        return value.as_array().map(|a| !a.is_empty()).unwrap_or(false);
    }
    for key in PORTAL_DATA_KEYS {
        if let Some(nested) = value.get(key) {
            if let Some(arr) = nested.as_array() {
                if !arr.is_empty() {
                    return true;
                }
            }
            if nested.is_object() && !nested.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                return true;
            }
        }
    }
    false
}

/// `core.mjs::jsonSessionExpired`：已排除正常业务数据的 JSON 按登录语义判定。
pub fn json_session_expired(value: &serde_json::Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    let mut texts: Vec<String> = Vec::new();
    let mut flagged_code = false;
    let mut has_message = false;
    for (key, val) in obj {
        let lower = key.to_lowercase();
        if lower == "msg" || lower == "message" || lower == "m" {
            has_message = true;
        }
        if matches!(lower.as_str(), "code" | "e" | "status") {
            let num = val.as_i64().or_else(|| val.as_str().and_then(|s| s.parse().ok()));
            if matches!(num, Some(401) | Some(403) | Some(302)) {
                flagged_code = true;
            }
        }
        match val {
            serde_json::Value::String(s) => texts.push(s.clone()),
            serde_json::Value::Number(n) => texts.push(n.to_string()),
            serde_json::Value::Object(inner) => {
                for v in inner.values() {
                    match v {
                        serde_json::Value::String(s) => texts.push(s.clone()),
                        serde_json::Value::Number(n) => texts.push(n.to_string()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    SESSION_WORDS.is_match(&texts.join(" ")) || (flagged_code && has_message)
}

/// `core.mjs::webvpnSessionExpired`：200 + 登录页 / JSON 错误体识别。
pub fn webvpn_session_expired(text: &str, cfg: &WebvpnConfig) -> bool {
    if !is_webvpn_enabled(cfg) {
        return false;
    }
    if text.is_empty() {
        return false;
    }
    let parsed: Option<serde_json::Value> = serde_json::from_str(text).ok();
    let Some(parsed) = parsed else {
        return matches_login_pattern(text, cfg);
    };
    if has_portal_data(&parsed) {
        return false;
    }
    if parsed.is_object() {
        if let Some(e) = parsed.get("e").and_then(|v| v.as_i64()) {
            if e == 10041 || e == 10042 {
                return false;
            }
        }
    }
    matches_login_pattern(text, cfg) || json_session_expired(&parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// core.mjs 同款算法离线生成的种子（Node 侧 encodeWengineHost 输出）。
    const VECTORS: &[(&str, &str)] = &[
        (
            "www.bjut.edu.cn",
            "77726476706e69737468656265737421e7e056d2253a7d44300d8db9d6562d",
        ),
        (
            "my.bjut.edu.cn",
            "77726476706e69737468656265737421fdee0f9e2d257c1e7b0c9ce29b5b",
        ),
        (
            "jwc.bjut.edu.cn",
            "77726476706e69737468656265737421fae042d2253a7d44300d8db9d6562d",
        ),
    ];

    #[test]
    fn wengine_hex_matches_node_vectors() {
        for (host, hex) in VECTORS {
            assert_eq!(&encode_wengine_host(host), hex, "host={host}");
            assert_eq!(&decode_wengine_host(hex), host, "hex={hex}");
        }
    }

    #[test]
    fn decode_rejects_bad_hex() {
        assert_eq!(decode_wengine_host("zz"), "");
        assert_eq!(decode_wengine_host("abcd"), ""); // 太短
        assert_eq!(decode_wengine_host(&"a".repeat(35)), ""); // 奇数长
    }
}
