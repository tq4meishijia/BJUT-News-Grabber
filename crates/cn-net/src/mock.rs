//! MockFetcher：测试用脚本化出站（不真发网络）。
//!
//! 处理器闭包对每个请求返回编排好的响应；全部请求记录在 `calls` 里供断言。

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::NetError;
use crate::fetcher::{FetchRequest, FetchResponse, Fetcher};

type Handler = Box<dyn Fn(&FetchRequest) -> Result<FetchResponse, NetError> + Send + Sync>;

/// 脚本化 Fetcher。生产代码只依赖 [`Fetcher`] trait。
pub struct MockFetcher {
    handler: Handler,
    /// 已发出的请求（按序）。
    pub calls: Mutex<Vec<FetchRequest>>,
}

impl MockFetcher {
    /// 用闭包构造。
    pub fn new<F>(handler: F) -> std::sync::Arc<Self>
    where
        F: Fn(&FetchRequest) -> Result<FetchResponse, NetError> + Send + Sync + 'static,
    {
        std::sync::Arc::new(Self {
            handler: Box::new(handler),
            calls: Mutex::new(Vec::new()),
        })
    }

    /// 按「url 子串 + 可选方法」匹配的静态规则构造；无命中返回 404。
    pub fn with_rules(
        rules: Vec<Rule>,
    ) -> std::sync::Arc<Self> {
        Self::new(move |req| {
            for rule in &rules {
                let method_ok = match rule.method {
                    Some(m) => req.method.as_str() == m,
                    None => true,
                };
                if method_ok && req.url.contains(rule.url_contains.as_str()) {
                    return Ok(FetchResponse {
                        status: rule.status,
                        url: req.url.clone(),
                        headers: rule.headers.clone(),
                        body: rule.body.clone(),
                    });
                }
            }
            Ok(FetchResponse {
                status: 404,
                url: req.url.clone(),
                headers: Vec::new(),
                body: Vec::new(),
            })
        })
    }

    /// 已发出的请求数。
    pub fn call_count(&self) -> usize {
        self.calls.lock().expect("calls lock").len()
    }
}

/// 静态规则。
#[derive(Debug, Clone)]
pub struct Rule {
    pub url_contains: String,
    pub method: Option<&'static str>,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Rule {
    /// GET 规则快捷构造。
    pub fn get(url_contains: &str, status: u16, body: &str) -> Self {
        Self {
            url_contains: url_contains.to_string(),
            method: None,
            status,
            headers: vec![(
                "content-type".to_string(),
                "text/html; charset=utf-8".to_string(),
            )],
            body: body.as_bytes().to_vec(),
        }
    }

    /// 带自定义头的规则。
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    /// 302 跳转规则。
    pub fn redirect(url_contains: &str, location: &str) -> Self {
        Self {
            url_contains: url_contains.to_string(),
            method: None,
            status: 302,
            headers: vec![("location".to_string(), location.to_string())],
            body: Vec::new(),
        }
    }
}

#[async_trait]
impl Fetcher for MockFetcher {
    async fn fetch(&self, req: FetchRequest) -> Result<FetchResponse, NetError> {
        self.calls.lock().expect("calls lock").push(req.clone());
        (self.handler)(&req)
    }
}
