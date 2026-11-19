//! cn-net — HTTP / 编码嗅探 / Cookie 罐 / DNS 覆盖 / WebVPN / CAS 登录。
//!
//! 硬约束（§2.2 / §7.2）：
//! - 不依赖 tauri；平台能力通过 cn-model 的 trait 注入。
//! - 响应体上限 6 MiB、重定向上限 4（列表）/ 8（登录），超限即失败（B10）。
//! - 网关 Cookie 与门户 Cookie 分离发送（B8）。
//! - SSRF 防护（S6）：禁 file/ftp/data scheme 与 link-local 169.254.0.0/16、fe80::/10，允许内网。

pub mod cas;
pub mod cookie;
pub mod encode;
pub mod error;
pub mod fetcher;
pub mod mock;
pub mod scope;
pub mod ssrf;
pub mod webvpn;

pub use cas::{
    cas_callback_candidates, cas_login, extract_execution, extract_error, looks_logged_in,
    needs_captcha, page_title, CasLoginSuccess, NO_PORTAL_SESSION,
};
pub use cookie::{parse_set_cookie, CookieJar};
pub use encode::{decode_body, sniff_charset};
pub use error::NetError;
pub use fetcher::{FetchRequest, FetchResponse, Fetcher, Method, ReqwestFetcher};
pub use mock::MockFetcher;
pub use scope::ScopeId;
pub use ssrf::validate_url;
pub use webvpn::{
    decode_wengine_host, encode_wengine_host, has_portal_data, is_webvpn_enabled,
    json_session_expired, matches_login_pattern, restore_url, restore_urls_in_text, rewrite_url,
    webvpn_session_expired, WebvpnConfig, DEFAULT_LOGIN_PAGE_PATTERN, WEBVPN_GATEWAY,
    WENGINE_KEY,
};

/// 默认响应体上限 6 MiB（B10）。
pub const DEFAULT_MAX_BYTES: usize = 6 * 1024 * 1024;
/// 附件响应体上限 25 MiB（B5）：仅附件请求覆写，普通请求仍用 6 MiB。
pub const ATTACHMENT_MAX_BYTES: usize = 25 * 1024 * 1024;
/// 列表抓取重定向上限（B10）。
pub const LIST_REDIRECTS: u8 = 4;
/// 登录流程重定向上限（B10）。
pub const LOGIN_REDIRECTS: u8 = 8;
