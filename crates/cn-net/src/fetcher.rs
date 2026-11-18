//! Fetcher 契约（§3.2）与 reqwest 实现。
//!
//! 重定向手动跟随（上限由 `follow_redirects` 决定，超限即失败——B10）；
//! 响应体超过 `max_bytes` 直接报错而非截断（B10）。

use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::NetError;
use crate::scope::ScopeId;
use crate::ssrf::validate_url;

/// HTTP 方法（只用到 GET/POST）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// 出站请求（§3.2）。
#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub url: String,
    pub method: Method,
    /// 额外请求头（cookie 由本结构单独携带，避免与网关 Cookie 混发——B8）。
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// 决定用哪个 Cookie 罐 + 哪个网关配置。
    pub scope: ScopeId,
    /// 门户 Cookie（数据源登录态）。
    pub cookie: String,
    /// 网关 Cookie（WebVPN 会话，独立于门户 Cookie）。
    pub gateway_cookie: String,
    pub timeout: Duration,
    /// 响应体上限（默认 6 MiB）。
    pub max_bytes: usize,
    /// 重定向上限（列表 4 / 登录 8）。
    pub follow_redirects: u8,
    pub resolve_ip: Option<IpAddr>,
    /// 仅该源显式开启时跳过证书校验（S7）。
    pub allow_invalid_cert: bool,
}

impl FetchRequest {
    /// 构造 GET 请求，其余取默认值（6 MiB / 4 次重定向 / 25 s）。
    pub fn get(url: impl Into<String>, scope: ScopeId) -> Self {
        Self {
            url: url.into(),
            method: Method::Get,
            headers: Vec::new(),
            body: None,
            scope,
            cookie: String::new(),
            gateway_cookie: String::new(),
            timeout: Duration::from_millis(25_000),
            max_bytes: crate::DEFAULT_MAX_BYTES,
            follow_redirects: crate::LIST_REDIRECTS,
            resolve_ip: None,
            allow_invalid_cert: false,
        }
    }

    /// 构造 POST 请求。
    pub fn post(url: impl Into<String>, scope: ScopeId, body: Vec<u8>) -> Self {
        Self {
            method: Method::Post,
            body: Some(body),
            ..Self::get(url, scope)
        }
    }
}

/// 出站响应。`headers` 里 `set-cookie` 可出现多次。
#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub status: u16,
    /// 还原后的最终地址（跟随重定向之后）。
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl FetchResponse {
    /// 取首个匹配请求头（大小写不敏感）。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// 全部 Set-Cookie 值。
    pub fn set_cookies(&self) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

/// 抽象出站（§3.2）。测试注入 MockFetcher，生产用 ReqwestFetcher。
#[async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, req: FetchRequest) -> Result<FetchResponse, NetError>;
}

/// reqwest 实现：手动跟随重定向、SSRF 防护、6 MiB 上限、可选 DNS 覆盖与证书跳过。
pub struct ReqwestFetcher {
    client: reqwest::Client,
    insecure_client: reqwest::Client,
}

impl ReqwestFetcher {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("reqwest client");
        let insecure_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(true)
            .build()
            .expect("reqwest insecure client");
        Self {
            client,
            insecure_client,
        }
    }
}

impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self::new()
    }
}

fn map_reqwest_err(err: reqwest::Error) -> NetError {
    if err.is_timeout() {
        NetError::Timeout
    } else if err.is_connect() {
        let text = err.to_string();
        if text.contains("certificate") || text.contains("TLS") || text.contains("ssl") {
            NetError::Tls(text)
        } else {
            NetError::Network(text)
        }
    } else {
        NetError::Network(err.to_string())
    }
}

#[async_trait]
impl Fetcher for ReqwestFetcher {
    async fn fetch(&self, req: FetchRequest) -> Result<FetchResponse, NetError> {
        // SSRF 防护（S6）：每跳都要过，重定向目标同样检查。
        // follow_redirects 语义：0 = 不跟随（jar_request 自己跟，返回 302 原始响应）；
        // N>0 = 最多跟 N 跳，出现第 N+1 个重定向即失败（B10 超限即失败）。
        let mut current = req.url.clone();
        let mut method = req.method;
        let mut body = req.body.clone();
        let mut hops_used: u8 = 0;

        loop {
            validate_url(&current)?;
            // DNS 覆盖：显式 IP 时把 Host 保持原样、连接走 IP。
            let connect_url = if let Some(ip) = req.resolve_ip {
                let parsed = url::Url::parse(&current)
                    .map_err(|_| NetError::InvalidUrl(current.clone()))?;
                let port = parsed
                    .port_or_known_default()
                    .ok_or_else(|| NetError::InvalidUrl(current.clone()))?;
                format!(
                    "{}://{}:{}{}{}",
                    parsed.scheme(),
                    ip,
                    port,
                    parsed.path(),
                    parsed
                        .query()
                        .map(|q| format!("?{q}"))
                        .unwrap_or_default()
                )
            } else {
                current.clone()
            };

            let builder = if method == Method::Post {
                self.client
                    .post(&connect_url)
                    .body(body.clone().unwrap_or_default())
            } else {
                self.client.get(&connect_url)
            };
            let builder = if req.allow_invalid_cert {
                // 重建到 insecure client（仅该源显式开启——S7）。
                if method == Method::Post {
                    self.insecure_client
                        .post(&connect_url)
                        .body(body.clone().unwrap_or_default())
                } else {
                    self.insecure_client.get(&connect_url)
                }
            } else {
                builder
            };

            // B8：门户 Cookie 与网关 Cookie 分开附加、顺序固定。
            let mut cookie_parts: Vec<&str> = Vec::new();
            if !req.cookie.trim().is_empty() {
                cookie_parts.push(req.cookie.trim());
            }
            if !req.gateway_cookie.trim().is_empty() {
                cookie_parts.push(req.gateway_cookie.trim());
            }

            let mut builder = builder.timeout(req.timeout);
            if !cookie_parts.is_empty() {
                builder = builder.header("cookie", cookie_parts.join("; "));
            }
            for (k, v) in &req.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            // 默认头：调用方已带则不重复。
            let has = |name: &str| req.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name));
            if !has("user-agent") {
                builder = builder.header("user-agent", crate::cas::USER_AGENT);
            }
            if !has("accept") {
                builder = builder.header(
                    "accept",
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                );
            }
            if !has("accept-language") {
                builder = builder.header("accept-language", "zh-CN,zh;q=0.9,en;q=0.8");
            }
            if req.resolve_ip.is_some() {
                if let Ok(parsed) = url::Url::parse(&current) {
                    if let Some(host) = parsed.host_str() {
                        builder = builder.header("host", host);
                    }
                }
            }

            let response = builder.send().await.map_err(map_reqwest_err)?;
            let status = response.status().as_u16();

            // 手动跟随重定向（还原 Location 里的网关地址由调用方负责）。
            if (300..400).contains(&status) {
                if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                    // 不跟随模式：把 302 原样交给调用方（登录流程自己逐跳处理）。
                    if req.follow_redirects == 0 {
                        let headers = collect_headers(&response);
                        return Ok(FetchResponse {
                            status,
                            url: current.clone(),
                            headers,
                            body: Vec::new(),
                        });
                    }
                    if hops_used >= req.follow_redirects {
                        return Err(NetError::TooManyRedirects);
                    }
                    hops_used += 1;
                    let location = location
                        .to_str()
                        .map_err(|_| NetError::Other("非法 Location 头".into()))?;
                    let next = url::Url::parse(&current)
                        .ok()
                        .and_then(|base| base.join(location).ok())
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| location.to_string());
                    current = next;
                    method = Method::Get;
                    body = None;
                    continue;
                }
            }

            // 响应体上限：超限即失败而非截断（B10）。
            if let Some(len) = response.content_length() {
                if len as usize > req.max_bytes {
                    return Err(NetError::TooLarge(req.max_bytes));
                }
            }
            // headers 先取（bytes() 消费 response）。
            let headers = collect_headers(&response);
            let bytes = response.bytes().await.map_err(map_reqwest_err)?;
            if bytes.len() > req.max_bytes {
                return Err(NetError::TooLarge(req.max_bytes));
            }
            return Ok(FetchResponse {
                status,
                url: current.clone(),
                headers,
                body: bytes.to_vec(),
            });
        }
    }
}

fn collect_headers(response: &reqwest::Response) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for (key, value) in response.headers().iter() {
        let value = value.to_str().unwrap_or_default().to_string();
        headers.push((key.as_str().to_string(), value));
    }
    headers
}
