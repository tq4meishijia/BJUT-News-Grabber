//! SSRF 防护（S6）：禁 file/ftp/data scheme 与 link-local 169.254.0.0/16、fe80::/10；
//! 允许内网（业务需要）。每次出站与每跳重定向都要过。

use std::net::IpAddr;

use crate::error::NetError;

/// 校验出站 URL；违规返回 [`NetError::SsrfBlocked`]。
pub fn validate_url(raw: &str) -> Result<(), NetError> {
    let parsed = url::Url::parse(raw).map_err(|_| NetError::InvalidUrl(raw.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(NetError::SsrfBlocked(format!("{raw}（禁止的 scheme {scheme}）")));
        }
    }
    if let Some(host) = parsed.host() {
        match host {
            url::Host::Ipv4(ip) => {
                if is_ipv4_link_local(ip) {
                    return Err(NetError::SsrfBlocked(format!("{raw}（link-local {ip}）")));
                }
            }
            url::Host::Ipv6(ip) => {
                if is_ipv6_link_local(ip) {
                    return Err(NetError::SsrfBlocked(format!("{raw}（link-local {ip}）")));
                }
            }
            url::Host::Domain(_) => {}
        }
    }
    Ok(())
}

fn is_ipv4_link_local(ip: std::net::Ipv4Addr) -> bool {
    // 169.254.0.0/16
    ip.octets()[0] == 169 && ip.octets()[1] == 254
}

fn is_ipv6_link_local(ip: std::net::Ipv6Addr) -> bool {
    // fe80::/10
    let seg = ip.segments();
    (seg[0] & 0xffc0) == 0xfe80
}

/// 判定字符串主机是否 link-local（供测试与诊断复用）。
pub fn is_link_local(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_ipv4_link_local(ip),
        Ok(IpAddr::V6(ip)) => is_ipv6_link_local(ip),
        Err(_) => false,
    }
}
