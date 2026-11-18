//! CAS 统一身份认证登录骨架（对照 `login.mjs`）。
//!
//! 流程：GET 登录页（拿 execution 与会话 Cookie）→ POST 凭据 → 跟回调拿门户 Cookie。
//! 错误分类产出 cn_model::AuthError（11 个 AUTH_E_*，§5.3）。测试用 MockFetcher，不真发网络。
//!
//! 与 jarRequest 的对应：[`jar_request`] 自带 Cookie 罐并手动跟随重定向（登录 8 次上限——B10），
//! 网关 Cookie 与门户 Cookie 分离发送（B8）。

use std::collections::HashSet;
use std::sync::Arc;

use regex::Regex;

use cn_model::{AuthError, AuthErrorCode};

use crate::cookie::{parse_set_cookie, CookieJar, SetCookie};
use crate::encode::decode_body;
use crate::error::NetError;
use crate::fetcher::{FetchRequest, FetchResponse, Fetcher, Method};
use crate::scope::ScopeId;
use crate::webvpn::{restore_url, WebvpnConfig};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36";

/// 登录走完但门户没下发会话 Cookie 时给用户的提示。
pub const NO_PORTAL_SESSION: &str = "登录走通了，但门户没有下发自己的会话 Cookie（只拿到认证服务器的）：把「登录后返回的地址」改成门户的认证回调地址，通常形如 https://门户/common/actionCasLogin?redirect_url=https://门户/首页";

/// 登录成功产物：Cookie 头 + 明细。
#[derive(Debug, Clone)]
pub struct CasLoginSuccess {
    /// `name=value; name2=value2` 形式。
    pub cookie: String,
    pub cookies: Vec<(String, String)>,
}

fn host_of(value: &str) -> String {
    url::Url::parse(value)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default()
}

/// `core.mjs`/`login.mjs` 的 `cleanText` 子集（登录页判定只用到去标签+压缩空白）。
fn quick_clean(html: &str) -> String {
    let re = Regex::new(r"(?is)<[^>]+>").unwrap();
    let text = re.replace_all(html, " ");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `login.mjs::extractExecution`：不依赖属性顺序/引号有无。
pub fn extract_execution(html: &str) -> String {
    let tag_re = Regex::new(r"(?i)<input\b[^>]*>").unwrap();
    let name_re =
        Regex::new(r#"(?i)\bname\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap();
    let id_re = Regex::new(r#"(?i)\bid\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap();
    let value_re =
        Regex::new(r#"(?i)\bvalue\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap();
    for tag in tag_re.find_iter(html) {
        let tag = tag.as_str();
        let key = [name_re.captures(tag), id_re.captures(tag)]
            .into_iter()
            .flatten()
            .flat_map(|caps| {
                (1..=3)
                    .filter_map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect::<Vec<_>>()
            })
            .find(|s| !s.is_empty())
            .unwrap_or_default()
            .to_lowercase();
        if key != "execution" {
            continue;
        }
        if let Some(caps) = value_re.captures(tag) {
            let raw = (1..=3)
                .filter_map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            if !raw.is_empty() {
                return raw;
            }
        }
    }
    String::new()
}

/// `login.mjs::looksLoggedIn`。
pub fn looks_logged_in(html: &str) -> bool {
    let text = quick_clean(html);
    let text: String = text.chars().take(4000).collect();
    Regex::new("已登录|已登陆|欢迎回来|继续访问|正在跳转|跳转中")
        .unwrap()
        .is_match(&text)
}

/// `login.mjs::pageTitle`。
pub fn page_title(html: &str) -> String {
    let re = Regex::new(r"(?is)<title[^>]*>([\s\S]*?)</title>").unwrap();
    re.captures(html)
        .and_then(|c| c.get(1).map(|m| quick_clean(m.as_str())))
        .map(|t| t.chars().take(40).collect::<String>())
        .unwrap_or_default()
}

/// `login.mjs::needsCaptcha`：输入框存在且可见（或有验证码文案）→ 需要人工（Blocked{captcha}）。
pub fn needs_captcha(html: &str) -> bool {
    let input_re = Regex::new(r#"(?i)<input[^>]+name=["']?captcha["']"#).unwrap();
    if input_re.is_match(html) {
        let style_re =
            Regex::new(r#"(?i)id=["']?captchaParent["']?[^>]*style=["']([^"']*)["']"#).unwrap();
        match style_re.captures(html) {
            Some(caps) => {
                let style = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                if !Regex::new(r"(?i)display\s*:\s*none").unwrap().is_match(style) {
                    return true;
                }
            }
            None => return true,
        }
    }
    Regex::new("请输入验证码|验证码错误").unwrap().is_match(html)
}

/// `login.mjs::extractError`：errorDiv 优先，其次 class 含 error 的 <p>。
pub fn extract_error(html: &str) -> String {
    let div_re = Regex::new(
        r#"(?is)<div[^>]*id=["']?errorDiv["']?[^>]*>([\s\S]{0,300}?)</div>"#,
    )
    .unwrap();
    if let Some(caps) = div_re.captures(html) {
        if let Some(m) = caps.get(1) {
            let text = quick_clean(m.as_str());
            if !text.is_empty() {
                return text;
            }
        }
    }
    let p_re = Regex::new(
        r#"(?is)<p[^>]*class=["'][^"']*error[^"']*["'][^>]*>([\s\S]{0,200}?)</p>"#,
    )
    .unwrap();
    p_re.captures(html)
        .and_then(|c| c.get(1).map(|m| quick_clean(m.as_str())))
        .unwrap_or_default()
}

/// 门户 Cookie 归属探测：记录门户域名下发过的 Cookie 名（NO_PORTAL_SESSION 判定）。
struct PortalProbe {
    portal_host: String,
    names: HashSet<String>,
}

impl PortalProbe {
    fn new(portal_host: String) -> Self {
        Self {
            portal_host,
            names: HashSet::new(),
        }
    }

    /// 与 `login.mjs::onSetCookie` 同逻辑：host 或 Domain 归属门户即计入。
    fn on_set_cookie(&mut self, host: &str, pairs: &[SetCookie]) {
        for pair in pairs {
            let domain = pair.domain.as_str();
            let from_portal = host == self.portal_host
                || domain == self.portal_host
                || (!domain.is_empty() && self.portal_host.ends_with(&format!(".{domain}")))
                || (!host.is_empty() && host.ends_with(&format!(".{}", self.portal_host)));
            if from_portal {
                self.names.insert(pair.name.clone());
            }
        }
    }
}

/// 登录收尾：Cookie 罐非空且门户下发过会话才算成功。
fn success_with_jar(
    jar: &CookieJar,
    probe: &PortalProbe,
) -> Result<CasLoginSuccess, AuthError> {
    let cookie = jar.to_header();
    if cookie.is_empty() {
        return Err(AuthError::new(
            AuthErrorCode::Internal,
            "登录成功但没拿到任何 Cookie",
        ));
    }
    if probe.names.is_empty() {
        return Err(AuthError::new(AuthErrorCode::NoPortalSession, NO_PORTAL_SESSION));
    }
    Ok(CasLoginSuccess {
        cookie,
        cookies: jar.entries().to_vec(),
    })
}

/// 自带 Cookie 罐、手动跟随重定向的请求（对应 `login.mjs::jarRequest`）。
/// 网关 Cookie 单独附着、不进门户罐；返回最后一跳的响应（url 为还原后的地址）。
#[allow(clippy::too_many_arguments)]
pub async fn jar_request(
    fetcher: &dyn Fetcher,
    url: &str,
    jar: &mut CookieJar,
    method: Method,
    body: Option<Vec<u8>>,
    extra_headers: Vec<(String, String)>,
    webvpn: Option<&WebvpnConfig>,
    on_set_cookie: &mut (dyn FnMut(&str, &[SetCookie]) + Send),
) -> Result<FetchResponse, NetError> {
    let max_redirects = crate::LOGIN_REDIRECTS;
    let mut current = url.to_string();
    let mut current_method = method;
    let mut current_body = body;
    let gateway_host = webvpn
        .filter(|c| c.enabled)
        .and_then(|_| url::Url::parse(crate::webvpn::WEBVPN_GATEWAY).ok())
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default();

    for _hop in 0..=max_redirects {
        let cookie = jar.to_header();
        // UA/accept 等默认头由 Fetcher 实现统一补充，这里只带 Cookie 与调用方的额外头。
        let mut headers: Vec<(String, String)> = Vec::new();
        if !cookie.is_empty() {
            headers.push(("cookie".to_string(), cookie));
        }
        headers.extend(extra_headers.iter().cloned());

        // 出站地址统一先过重写（WebVPN 开启时）。
        let outgoing = match webvpn {
            Some(cfg) if cfg.enabled => crate::webvpn::rewrite_url(&current, cfg),
            _ => current.clone(),
        };
        let gateway_cookie = webvpn
            .map(|c| c.cookie.clone())
            .filter(|c| !c.trim().is_empty())
            .unwrap_or_default();

        let req = FetchRequest {
            url: outgoing,
            method: current_method,
            headers,
            body: current_body.clone(),
            scope: ScopeId::Portal(host_of(&current)),
            cookie: String::new(), // 已并入 headers
            gateway_cookie,
            timeout: std::time::Duration::from_millis(20_000),
            max_bytes: crate::DEFAULT_MAX_BYTES,
            follow_redirects: 0, // 自己跟随
            resolve_ip: None,
            allow_invalid_cert: false,
        };
        let response = fetcher.fetch(req).await?;

        // Set-Cookie 过滤：WebVPN 下 host-only 与网关 Domain 的 Cookie 不进门户罐。
        let mut accepted: Vec<SetCookie> = Vec::new();
        for raw in response.set_cookies() {
            if let Some(pair) = parse_set_cookie(raw) {
                if !gateway_host.is_empty() {
                    let domain = pair.domain.clone();
                    if domain.is_empty() {
                        continue;
                    }
                    if domain == gateway_host || domain.ends_with(&format!(".{gateway_host}")) {
                        continue;
                    }
                }
                jar.set(pair.name.clone(), pair.value.clone());
                accepted.push(pair);
            }
        }
        if !accepted.is_empty() {
            on_set_cookie(&host_of(&current), &accepted);
        }

        if (300..400).contains(&response.status) {
            if let Some(location) = response.header("location") {
                let next = url::Url::parse(&current)
                    .ok()
                    .and_then(|base| base.join(location).ok())
                    .map(|u| u.to_string())
                    .unwrap_or_else(|| location.to_string());
                // 网关回的 Location 先还原成门户/CAS 原始地址。
                current = match webvpn {
                    Some(cfg) if cfg.enabled => restore_url(&next, cfg),
                    _ => next,
                };
                current_method = Method::Get;
                current_body = None;
                continue;
            }
        }
        return Ok(FetchResponse {
            url: current.clone(),
            ..response
        });
    }
    Err(NetError::TooManyRedirects)
}

/// `login.mjs::casLogin` 的 Rust 移植。错误分类见 §5.3。
pub async fn cas_login(
    fetcher: &dyn Fetcher,
    cas_base: &str,
    service: &str,
    username: &str,
    password: &str,
    webvpn: Option<&WebvpnConfig>,
) -> Result<CasLoginSuccess, AuthError> {
    if username.is_empty() || password.is_empty() {
        return Err(AuthError::new(
            AuthErrorCode::Credential,
            "账号或密码为空",
        ));
    }
    if cas_base.is_empty() || service.is_empty() {
        return Err(AuthError::new(
            AuthErrorCode::Form,
            "还没配置「认证入口地址」和「登录后返回的地址」（设置页可以填）",
        ));
    }
    let vpn = webvpn.filter(|c| c.enabled);
    // 走 WebVPN 时把地址统一还原成门户/CAS 原始地址。
    let effective_cas = match vpn {
        Some(cfg) => restore_url(cas_base, cfg),
        None => cas_base.to_string(),
    };
    let effective_service = match vpn {
        Some(cfg) => restore_url(service, cfg),
        None => service.to_string(),
    };
    let cas_host = host_of(&effective_cas);
    let portal_host = host_of(&effective_service);
    if cas_host.is_empty() || portal_host.is_empty() {
        return Err(AuthError::new(
            AuthErrorCode::Form,
            "认证入口或返回地址不是合法网址",
        ));
    }

    let mut jar = CookieJar::new();
    let login_url = format!(
        "{}/login?service={}",
        effective_cas.trim_end_matches('/'),
        urlencoding_encode(&effective_service)
    );

    // 记录门户域名下发过哪些 Cookie。
    let mut probe = PortalProbe::new(portal_host.clone());

    let decode = |resp: &FetchResponse| decode_body(&resp.body, resp.header("content-type").unwrap_or(""));

    let page = jar_request(
        fetcher,
        &login_url,
        &mut jar,
        Method::Get,
        None,
        Vec::new(),
        vpn,
        &mut |h, p| probe.on_set_cookie(h, p),
    )
    .await
    .map_err(|e| e.to_auth_error())?;
    let mut html = decode(&page);
    let mut execution = extract_execution(&html);

    if execution.is_empty() && host_of(&page.url) != cas_host {
        return success_with_jar(&jar, &probe);
    }

    // 有些 CAS 先给中间页，再取一次拿真正的表单。
    if execution.is_empty() {
        let again = jar_request(
            fetcher,
            &login_url,
            &mut jar,
            Method::Get,
            None,
            Vec::new(),
            vpn,
            &mut |h, p| probe.on_set_cookie(h, p),
        )
        .await
        .map_err(|e| e.to_auth_error())?;
        let again_html = decode(&again);
        let again_execution = extract_execution(&again_html);
        if again_execution.is_empty()
            && (looks_logged_in(&html)
                || looks_logged_in(&again_html)
                || host_of(&again.url) != cas_host)
        {
            return success_with_jar(&jar, &probe);
        }
        if !again_execution.is_empty() {
            html = again_html;
            execution = again_execution;
        }
    }

    if execution.is_empty() {
        if needs_captcha(&html) {
            return Err(AuthError::new(
                AuthErrorCode::Captcha,
                "统一身份认证要求输入验证码，自动登录不可用，请手动更新登录态",
            ));
        }
        let title = page_title(&html);
        return Err(AuthError::new(
            AuthErrorCode::Form,
            format!(
                "登录页里没找到 execution 字段{}：确认「认证入口地址」填的是登录页本身（一般形如 https://cas.xxx.edu.cn），或过一会儿再试",
                if title.is_empty() {
                    String::new()
                } else {
                    format!("（页面标题：{title}）")
                }
            ),
        ));
    }
    if needs_captcha(&html) {
        return Err(AuthError::new(
            AuthErrorCode::Captcha,
            "统一身份认证要求输入验证码，自动登录不可用，请手动更新登录态",
        ));
    }
    // MFA（短信/动态码字段）检测（§5.3 AUTH_E_MFA）。
    if Regex::new(r#"(?i)name=["']?(smsCode|dynamicCode|otp)["']|短信验证码|动态码"#)
        .unwrap()
        .is_match(&html)
    {
        return Err(AuthError::new(
            AuthErrorCode::Mfa,
            "需要二次验证，自动登录不可用，请手动更新登录态",
        ));
    }

    let form = form_encode(&[
        ("username", username),
        ("password", password),
        ("type", "username_password"),
        ("execution", &execution),
        ("_eventId", "submit"),
        ("submit", "登录"),
    ]);

    let posted = jar_request(
        fetcher,
        &login_url,
        &mut jar,
        Method::Post,
        Some(form.into_bytes()),
        vec![
            (
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
            ("referer".to_string(), login_url.clone()),
            (
                "origin".to_string(),
                url::Url::parse(&effective_cas)
                    .map(|u| u.origin().ascii_serialization())
                    .unwrap_or_default(),
            ),
        ],
        vpn,
        &mut |h, p| probe.on_set_cookie(h, p),
    )
    .await
    .map_err(|e| e.to_auth_error())?;
    let posted_html = decode(&posted);

    if host_of(&posted.url) == cas_host && posted.url.contains("/login") {
        if needs_captcha(&posted_html) {
            return Err(AuthError::new(
                AuthErrorCode::Captcha,
                "登录失败：账号密码错误次数偏多，需要验证码，请手动更新登录态",
            ));
        }
        let error = extract_error(&posted_html);
        return Err(AuthError::new(
            AuthErrorCode::Credential,
            if error.is_empty() {
                "登录失败：仍停留在登录页（账号或密码不对）".to_string()
            } else {
                format!("登录失败：{error}")
            },
        ));
    }

    let cookie = jar.to_header();
    if probe.names.is_empty() {
        return Err(AuthError::new(AuthErrorCode::NoPortalSession, NO_PORTAL_SESSION));
    }
    if host_of(&posted.url) != portal_host {
        return Err(AuthError::new(
            AuthErrorCode::NoPortalSession,
            format!(
                "登录流程走完了，但没有回到「{portal_host}」，检查「登录后返回的地址」"
            ),
        ));
    }
    if cookie.is_empty() {
        return Err(AuthError::new(AuthErrorCode::Internal, "登录成功但没拿到任何 Cookie"));
    }
    Ok(CasLoginSuccess {
        cookie,
        cookies: jar.entries().to_vec(),
    })
}

/// `login.mjs::casCallbackCandidates`：由门户首页推出常见的认证回调候选地址。
pub fn cas_callback_candidates(service: &str) -> Vec<String> {
    match url::Url::parse(service) {
        Ok(u) => {
            let origin = u.origin().ascii_serialization();
            let encoded = urlencoding_encode(service);
            vec![
                format!("{origin}/common/actionCasLogin?redirect_url={encoded}"),
                format!("{origin}/casLogin?redirect_url={encoded}"),
            ]
        }
        Err(_) => Vec::new(),
    }
}

/// application/x-www-form-urlencoded 编码（JS URLSearchParams 语义）。
fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding_encode(k), urlencoding_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// encodeURIComponent 语义。
fn urlencoding_encode(text: &str) -> String {
    cn_core::encode_uri_component(text)
}

/// 便捷包装：Arc 注入。
pub async fn cas_login_arc(
    fetcher: &Arc<dyn Fetcher>,
    cas_base: &str,
    service: &str,
    username: &str,
    password: &str,
    webvpn: Option<&WebvpnConfig>,
) -> Result<CasLoginSuccess, AuthError> {
    cas_login(fetcher.as_ref(), cas_base, service, username, password, webvpn).await
}
