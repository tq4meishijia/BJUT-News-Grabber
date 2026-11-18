//! 认证错误码（§5.3，共 11 个）与统一错误对象。

use serde::{Deserialize, Serialize};

/// 11 个稳定错误码；序列化值即前端/日志里使用的字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AuthErrorCode {
    #[serde(rename = "AUTH_E_NETWORK")]
    Network,
    #[serde(rename = "AUTH_E_TLS")]
    Tls,
    #[serde(rename = "AUTH_E_CREDENTIAL")]
    Credential,
    #[serde(rename = "AUTH_E_CAPTCHA")]
    Captcha,
    #[serde(rename = "AUTH_E_MFA")]
    Mfa,
    #[serde(rename = "AUTH_E_FORM")]
    Form,
    #[serde(rename = "AUTH_E_NO_PORTAL_SESSION")]
    NoPortalSession,
    #[serde(rename = "AUTH_E_GATEWAY")]
    Gateway,
    #[serde(rename = "AUTH_E_PORTAL_BIZ")]
    PortalBiz,
    #[serde(rename = "AUTH_E_CANCELLED")]
    Cancelled,
    #[serde(rename = "AUTH_E_INTERNAL")]
    Internal,
}

impl AuthErrorCode {
    /// 稳定错误码字符串（`AUTH_E_*`）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Network => "AUTH_E_NETWORK",
            Self::Tls => "AUTH_E_TLS",
            Self::Credential => "AUTH_E_CREDENTIAL",
            Self::Captcha => "AUTH_E_CAPTCHA",
            Self::Mfa => "AUTH_E_MFA",
            Self::Form => "AUTH_E_FORM",
            Self::NoPortalSession => "AUTH_E_NO_PORTAL_SESSION",
            Self::Gateway => "AUTH_E_GATEWAY",
            Self::PortalBiz => "AUTH_E_PORTAL_BIZ",
            Self::Cancelled => "AUTH_E_CANCELLED",
            Self::Internal => "AUTH_E_INTERNAL",
        }
    }

    /// 全部 11 个错误码。
    pub fn all() -> &'static [AuthErrorCode] {
        &[
            Self::Network,
            Self::Tls,
            Self::Credential,
            Self::Captcha,
            Self::Mfa,
            Self::Form,
            Self::NoPortalSession,
            Self::Gateway,
            Self::PortalBiz,
            Self::Cancelled,
            Self::Internal,
        ]
    }

    /// 用户文案要点（§5.3 表）。
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::Network => "无法连接学校门户",
            Self::Tls => "证书不受信任",
            Self::Credential => "账号或密码不对",
            Self::Captcha => "需要验证码",
            Self::Mfa => "需要二次验证",
            Self::Form => "登录页结构变了",
            Self::NoPortalSession => "登录成功但门户没给会话",
            Self::Gateway => "WebVPN 会话过期",
            Self::PortalBiz => "门户登录态失效",
            Self::Cancelled => "已取消",
            Self::Internal => "未知错误",
        }
    }

    /// 下一步动作（§5.3 表）。
    pub fn next_action(&self) -> &'static str {
        match self {
            Self::Network => "检查是否在校内网；一键开启 WebVPN",
            Self::Tls => "展示指纹，需显式信任（仅该源）",
            Self::Credential => "重填；连续 3 次转 WebView",
            Self::Captcha => "转 WebView 登录",
            Self::Mfa => "转 WebView 登录",
            Self::Form => "转 WebView；附诊断信息",
            Self::NoPortalSession => "自动尝试 2 个候选回调地址，再引导改 service",
            Self::Gateway => "重新登录网关，更新网关 Cookie",
            Self::PortalBiz => "触发续期；失败转 WebView",
            Self::Cancelled => "可重试",
            Self::Internal => "导出诊断包",
        }
    }

    /// 是否可重试。
    pub fn retryable(&self) -> bool {
        !matches!(self, Self::Cancelled)
    }
}

/// 统一错误对象：`{code, message, detail, retryable, next_action}`。
/// 禁止把后端英文异常直接抛给 UI。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthError {
    pub code: AuthErrorCode,
    pub message: String,
    #[serde(default)]
    pub detail: String,
    pub retryable: bool,
    pub next_action: String,
}

impl AuthError {
    /// 按错误码构造带默认文案与下一步动作的错误。
    pub fn new(code: AuthErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            message: code.user_message().to_string(),
            detail: detail.into(),
            retryable: code.retryable(),
            next_action: code.next_action().to_string(),
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} ({})", self.code.as_str(), self.message, self.detail)
    }
}

impl std::error::Error for AuthError {}
