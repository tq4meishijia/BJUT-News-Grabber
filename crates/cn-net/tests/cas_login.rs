//! CAS 登录骨架：MockFetcher 注入，覆盖成功 / 账号密码错 / 验证码 / 无 execution /
//! 无门户会话 / MFA / 回调候选 等路径（不真发网络）。

use std::sync::Arc;

use cn_model::AuthErrorCode;
use cn_net::mock::MockFetcher;
use cn_net::{
    cas_callback_candidates, cas_login, extract_error, extract_execution, looks_logged_in,
    needs_captcha, page_title, FetchRequest, FetchResponse, NetError, NO_PORTAL_SESSION,
};

const CAS: &str = "https://cas.bjut.edu.cn";
const SERVICE: &str = "https://my.bjut.edu.cn/index.html";

fn login_page() -> String {
    r#"<html><head><title>统一身份认证</title></head><body>
    <form action="/login" method="post">
    <input type="hidden" name="execution" value="e1s1">
    <input type="text" name="username"><input type="password" name="password">
    </form></body></html>"#
        .to_string()
}

fn portal_page() -> String {
    "<html><head><title>一站式服务大厅</title></head><body>欢迎回来</body></html>".to_string()
}

fn resp(url: &str, status: u16, headers: Vec<(String, String)>, body: &str) -> FetchResponse {
    FetchResponse {
        status,
        url: url.to_string(),
        headers,
        body: body.as_bytes().to_vec(),
    }
}

/// 成功流：GET 登录页（execution + CAS Cookie）→ POST 302 → GET 门户（门户 Cookie）。
fn success_fetcher() -> Arc<MockFetcher> {
    MockFetcher::new(|req: &FetchRequest| {
        let u = req.url.clone();
        if u.contains("/login?service=") && req.method.as_str() == "GET" {
            return Ok(resp(
                &u,
                200,
                vec![
                    (
                        "set-cookie".to_string(),
                        "TGC=ticket-1; Path=/; Domain=cas.bjut.edu.cn".to_string(),
                    ),
                    ("content-type".to_string(), "text/html".to_string()),
                ],
                &login_page(),
            ));
        }
        if u.contains("/login?service=") && req.method.as_str() == "POST" {
            // 302 到门户，中间带门户 Cookie
            return Ok(resp(
                &u,
                302,
                vec![
                    (
                        "set-cookie".to_string(),
                        "MYSESSION=portal-1; Path=/; Domain=my.bjut.edu.cn".to_string(),
                    ),
                    (
                        "location".to_string(),
                        "https://my.bjut.edu.cn/index.html".to_string(),
                    ),
                ],
                "",
            ));
        }
        if u.contains("my.bjut.edu.cn/index.html") {
            return Ok(resp(
                &u,
                200,
                vec![("content-type".to_string(), "text/html".to_string())],
                &portal_page(),
            ));
        }
        Ok(resp(&u, 404, Vec::new(), ""))
    })
}

#[tokio::test]
async fn cas_login_success() {
    let fetcher = success_fetcher();
    let out = cas_login(fetcher.as_ref(), CAS, SERVICE, "alice", "pw", None)
        .await
        .expect("login ok");
    assert!(out.cookie.contains("MYSESSION=portal-1"));
    assert!(out.cookie.contains("TGC=ticket-1"));
    assert!(out.cookies.iter().any(|(k, _)| k == "MYSESSION"));
}

#[tokio::test]
async fn cas_login_bad_password_credential_error() {
    let fetcher = MockFetcher::new(|req: &FetchRequest| {
        let u = req.url.clone();
        if req.method.as_str() == "GET" {
            return Ok(resp(
                &u,
                200,
                vec![("set-cookie".to_string(), "TGC=x; Domain=cas.bjut.edu.cn".into())],
                &login_page(),
            ));
        }
        // POST 停留在登录页 + errorDiv
        Ok(resp(
            &u,
            200,
            vec![],
            r#"<div id="errorDiv">用户名或密码错误</div><input type="hidden" name="execution" value="e1s2">"#,
        ))
    });
    let err = cas_login(fetcher.as_ref(), CAS, SERVICE, "alice", "wrong", None)
        .await
        .expect_err("should fail");
    assert_eq!(err.code, AuthErrorCode::Credential);
    assert!(err.detail.contains("用户名或密码错误"), "detail={}", err.detail);
    assert!(!err.next_action.is_empty());
}

#[tokio::test]
async fn cas_login_captcha_blocked() {
    let fetcher = MockFetcher::new(|req: &FetchRequest| {
        let u = req.url.clone();
        Ok(resp(
            &u,
            200,
            vec![],
            r#"<input name="captcha"><input type="hidden" name="execution" value="e1s1">请输入验证码"#,
        ))
    });
    let err = cas_login(fetcher.as_ref(), CAS, SERVICE, "alice", "pw", None)
        .await
        .expect_err("should fail");
    assert_eq!(err.code, AuthErrorCode::Captcha);
}

#[tokio::test]
async fn cas_login_no_execution_form_error() {
    let fetcher = MockFetcher::new(|req: &FetchRequest| {
        let u = req.url.clone();
        Ok(resp(&u, 200, vec![], "<html><head><title>维护中</title></head><body>稍后再试</body></html>"))
    });
    let err = cas_login(fetcher.as_ref(), CAS, SERVICE, "alice", "pw", None)
        .await
        .expect_err("should fail");
    assert_eq!(err.code, AuthErrorCode::Form);
    assert!(err.detail.contains("execution"), "detail={}", err.detail);
}

#[tokio::test]
async fn cas_login_no_portal_session() {
    // CAS 下发 Cookie，但门户侧没有 set-cookie → NO_PORTAL_SESSION
    let fetcher = MockFetcher::new(|req: &FetchRequest| {
        let u = req.url.clone();
        // 门户页不下发任何 Cookie（会话没建立）。注意 service 参数里也会带门户域名，
        // 用 "://my.bjut.edu.cn" 匹配主机部分而不是子串。
        if u.contains("://my.bjut.edu.cn") && !u.contains("/login?") {
            return Ok(resp(
                &u,
                200,
                vec![("content-type".to_string(), "text/html".into())],
                &portal_page(),
            ));
        }
        if req.method.as_str() == "GET" {
            return Ok(resp(
                &u,
                200,
                vec![("set-cookie".to_string(), "TGC=x; Domain=cas.bjut.edu.cn".into())],
                &login_page(),
            ));
        }
        Ok(resp(
            &u,
            302,
            vec![("location".to_string(), "https://my.bjut.edu.cn/index.html".into())],
            "",
        ))
    });
    let err = cas_login(fetcher.as_ref(), CAS, SERVICE, "alice", "pw", None)
        .await
        .expect_err("should fail");
    assert_eq!(err.code, AuthErrorCode::NoPortalSession);
    assert_eq!(err.detail, NO_PORTAL_SESSION);
}

#[tokio::test]
async fn cas_login_mfa_detected() {
    let fetcher = MockFetcher::new(|req: &FetchRequest| {
        let u = req.url.clone();
        Ok(resp(
            &u,
            200,
            vec![],
            r#"<input type="hidden" name="execution" value="e1s1"><input name="smsCode">短信验证码"#,
        ))
    });
    let err = cas_login(fetcher.as_ref(), CAS, SERVICE, "alice", "pw", None)
        .await
        .expect_err("should fail");
    assert_eq!(err.code, AuthErrorCode::Mfa);
}

#[tokio::test]
async fn cas_login_empty_account_is_credential() {
    let fetcher = MockFetcher::new(|_| Err(NetError::Timeout));
    let err = cas_login(fetcher.as_ref(), CAS, SERVICE, "", "", None)
        .await
        .expect_err("should fail");
    assert_eq!(err.code, AuthErrorCode::Credential);
}

#[tokio::test]
async fn cas_login_network_error_maps_to_auth_network() {
    let fetcher = MockFetcher::new(|_| Err(NetError::Timeout));
    let err = cas_login(fetcher.as_ref(), CAS, SERVICE, "alice", "pw", None)
        .await
        .expect_err("should fail");
    assert_eq!(err.code, AuthErrorCode::Network);
}

#[test]
fn callback_candidates_shape() {
    let list = cas_callback_candidates("https://my.bjut.edu.cn/index.html");
    assert_eq!(list.len(), 2);
    assert!(list[0].starts_with("https://my.bjut.edu.cn/common/actionCasLogin?redirect_url="));
    assert!(list[1].starts_with("https://my.bjut.edu.cn/casLogin?redirect_url="));
    assert!(cas_callback_candidates("bad url").is_empty());
}

#[test]
fn page_helpers() {
    let html = r#"<title>统一身份认证登录</title><div id="errorDiv">账号或密码不对</div>"#;
    assert_eq!(page_title(html), "统一身份认证登录");
    assert_eq!(extract_error(html), "账号或密码不对");
    assert!(extract_execution(r#"<input name="execution" value="e1s1">"#) == "e1s1");
    assert!(extract_execution(r#"<input id="execution" value='e2s2'>"#) == "e2s2");
    assert!(extract_execution("<input name=\"other\">").is_empty());
    assert!(looks_logged_in("正在跳转，请稍候"));
    assert!(!looks_logged_in("请输入用户名"));
    // 验证码隐藏时不算需要验证码
    assert!(!needs_captcha(
        r#"<input name="captcha"><div id="captchaParent" style="display:none"></div>"#
    ));
    assert!(needs_captcha(r#"<input name="captcha"><div id="captchaParent"></div>"#));
}
