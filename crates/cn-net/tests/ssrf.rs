//! SSRF 防护（S6）：12 个 URL 用例——禁 file/ftp/data 与 link-local，允许内网。

use cn_net::validate_url;

#[test]
fn ssrf_12_cases() {
    // (url, 期望放行)
    let cases = [
        ("https://www.bjut.edu.cn/info/1.htm", true),
        ("http://my.bjut.edu.cn/api/list", true),
        ("file:///etc/passwd", false),
        ("ftp://files.bjut.edu.cn/x", false),
        ("data:text/html,<script>alert(1)</script>", false),
        ("https://169.254.1.1/latest/meta-data", false),
        ("http://169.255.1.1/", true), // 169.255 不是 link-local
        ("http://10.20.30.40/portal", true), // 内网允许
        ("https://192.168.1.1/router", true), // 内网允许
        ("http://127.0.0.1:8080/local", true), // 回环允许（本地服务）
        ("https://[::1]/", true),
        ("https://[fe80::1]/", false), // IPv6 link-local
    ];
    for (url, allowed) in cases {
        let result = validate_url(url);
        match (result.is_ok(), allowed) {
            (true, true) => {}
            (false, false) => {}
            (true, false) => panic!("{url} 应被阻止但通过了"),
            (false, true) => panic!("{url} 应放行但被阻止：{:?}", result.err()),
        }
    }
}

#[test]
fn invalid_url_is_rejected() {
    assert!(validate_url("not a url").is_err());
    assert!(validate_url("").is_err());
}
