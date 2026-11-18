//! 网络层错误。CAS 登录把它们分类成 cn-model 的 AuthErrorCode（§5.3）。

use cn_model::{AuthError, AuthErrorCode};

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("请求超时")]
    Timeout,
    #[error("响应过大（超过 {0} 字节上限）")]
    TooLarge(usize),
    #[error("重定向次数过多")]
    TooManyRedirects,
    #[error("网址不合法：{0}")]
    InvalidUrl(String),
    #[error("SSRF 防护：已阻止 {0}")]
    SsrfBlocked(String),
    #[error("网络错误：{0}")]
    Network(String),
    #[error("证书不受信任：{0}")]
    Tls(String),
    #[error("HTTP {0}")]
    Http(u16),
    #[error("{0}")]
    Other(String),
}

impl NetError {
    /// §5.3 错误分类：网络/超时 → AUTH_E_NETWORK，证书链 → AUTH_E_TLS，其余 → INTERNAL。
    pub fn to_auth_error(&self) -> AuthError {
        match self {
            Self::Timeout | Self::Network(_) | Self::SsrfBlocked(_) | Self::InvalidUrl(_) => {
                AuthError::new(AuthErrorCode::Network, self.to_string())
            }
            Self::Tls(_) => AuthError::new(AuthErrorCode::Tls, self.to_string()),
            Self::TooLarge(_) | Self::TooManyRedirects | Self::Http(_) | Self::Other(_) => {
                AuthError::new(AuthErrorCode::Internal, self.to_string())
            }
        }
    }
}
