//! Cookie 罐：按 scope 隔离（B8），登录流程自带跟随重定向的 jarRequest 对应实现放 cas.rs。

/// 简单 name=value 罐（顺序保留，jarToHeader 语义）。
#[derive(Debug, Default, Clone)]
pub struct CookieJar {
    entries: Vec<(String, String)>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    /// 覆盖写入（同名替换，保持原位置——与 JS Map.set 一致）。
    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        for entry in self.entries.iter_mut() {
            if entry.0 == name {
                entry.1 = value;
                return;
            }
        }
        self.entries.push((name, value));
    }

    /// 读取。
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `name=value; name2=value2` 头（jarToHeader）。
    pub fn to_header(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// 全部条目。
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }
}

/// Set-Cookie 解析产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetCookie {
    pub name: String,
    pub value: String,
    /// Domain 属性（已小写、去前导点）；host-only Cookie 为空。
    pub domain: String,
}

/// `core.mjs::parseSetCookie`：只取 name=value 与 Domain 属性。
pub fn parse_set_cookie(raw: &str) -> Option<SetCookie> {
    let mut parts = raw.split(';');
    let pair = parts.next()?;
    let index = pair.find('=')?;
    if index == 0 {
        return None;
    }
    let name = pair[..index].to_string();
    let value = pair[index + 1..].to_string();
    let mut domain = String::new();
    for part in parts {
        let part = part.trim();
        if let Some(rest) = part
            .strip_prefix("domain=")
            .or_else(|| part.strip_prefix("Domain="))
        {
            domain = rest.trim_start_matches('.').to_lowercase();
        }
    }
    Some(SetCookie {
        name,
        value,
        domain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jar_round_trip() {
        let mut jar = CookieJar::new();
        jar.set("a", "1");
        jar.set("b", "2");
        jar.set("a", "3");
        assert_eq!(jar.get("a"), Some("3"));
        assert_eq!(jar.to_header(), "a=3; b=2");
    }

    #[test]
    fn parse_set_cookie_domain() {
        let c = parse_set_cookie("JSESSIONID=abc; Path=/; Domain=.bjut.edu.cn; HttpOnly")
            .expect("cookie");
        assert_eq!(c.name, "JSESSIONID");
        assert_eq!(c.value, "abc");
        assert_eq!(c.domain, "bjut.edu.cn");
        let host_only = parse_set_cookie("sid=x; Path=/").expect("cookie");
        assert_eq!(host_only.domain, "");
    }
}
