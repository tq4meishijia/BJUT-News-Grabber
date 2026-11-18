//! ScopeId：网关 Cookie 罐与门户 Cookie 罐永不合并（B8）。

/// 会话作用域。`Gateway(host)` 与 `Portal(host)` 是两个独立罐。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScopeId {
    /// WebVPN 网关自己的会话（settings.webvpn.cookie）。
    Gateway(String),
    /// 门户/数据源会话（source.cookie）。
    Portal(String),
}

impl ScopeId {
    /// 关联主机（不含端口）。
    pub fn host(&self) -> &str {
        match self {
            Self::Gateway(h) | Self::Portal(h) => h,
        }
    }

    /// 是否网关作用域。
    pub fn is_gateway(&self) -> bool {
        matches!(self, Self::Gateway(_))
    }
}
