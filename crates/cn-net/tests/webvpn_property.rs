//! WebVPN 属性测试：restore(rewrite(u)) == u（1000 例，固定种子手写随机）。
//! 另：restore_urls_in_text 还原 HTML 内链接、网关登录页识别 6 样本。

use cn_net::{
    decode_wengine_host, encode_wengine_host, matches_login_pattern, restore_url,
    restore_urls_in_text, rewrite_url, webvpn_session_expired, WebvpnConfig, WEBVPN_GATEWAY,
};

fn vpn_on() -> WebvpnConfig {
    WebvpnConfig {
        enabled: true,
        ..WebvpnConfig::default()
    }
}

/// 固定种子 xorshift64* —— 保证可复现。
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn rand_host(rng: &mut Rng) -> String {
    const LABELS: &[&str] = &["www", "my", "jwc", "one", "gs", "cc", "lib", "portal", "xk", "vpn2"];
    let a = LABELS[rng.below(LABELS.len() as u64) as usize];
    let b = LABELS[rng.below(LABELS.len() as u64) as usize];
    let n = rng.below(900) + 100;
    format!("{a}-{n}.{b}.bjut.edu.cn")
}

fn rand_path(rng: &mut Rng) -> String {
    const SEGS: &[&str] = &["info", "news", "tzgg", "1001", "2026", "detail", "a-b", "x_y", "12345.html"];
    let mut path = String::new();
    let depth = rng.below(4) + 1;
    for _ in 0..depth {
        path.push('/');
        path.push_str(SEGS[rng.below(SEGS.len() as u64) as usize]);
    }
    path
}

fn rand_query(rng: &mut Rng) -> String {
    match rng.below(3) {
        0 => String::new(),
        1 => format!("?p={}", rng.below(20) + 1),
        _ => format!("?id={}&tab=news", rng.below(9000) + 1000),
    }
}

/// 生成规范化 URL（含路径，保证 rewrite/restore 往返可比）。
fn rand_url(rng: &mut Rng) -> String {
    let scheme = if rng.below(2) == 0 { "https" } else { "http" };
    let host = rand_host(rng);
    let port = match rng.below(4) {
        0 => format!(":{}", rng.below(3000) + 8000), // 非标准端口
        1 if scheme == "https" => ":443".to_string(), // 默认端口（序列化时省略）
        2 if scheme == "http" => ":80".to_string(),
        _ => String::new(),
    };
    format!(
        "{scheme}://{host}{port}{}{}",
        rand_path(rng),
        rand_query(rng)
    )
}

/// 与 JS `new URL(u).href` 一致的规范化（默认端口省略、空路径补 /）。
fn normalize(url: &str) -> String {
    url::Url::parse(url).map(|u| u.to_string()).unwrap_or_else(|_| url.to_string())
}

#[test]
fn restore_rewrite_round_trip_1000() {
    let cfg = vpn_on();
    let mut rng = Rng::new(0x9E3779B97F4A7C15);
    let mut failures: Vec<String> = Vec::new();
    for i in 0..1000u32 {
        let original = rand_url(&mut rng);
        let normalized = normalize(&original);
        let rewritten = rewrite_url(&normalized, &cfg);
        // 必须是网关地址形态
        if !rewritten.starts_with(WEBVPN_GATEWAY) {
            failures.push(format!("#{i} rewrite 未走网关：{original} -> {rewritten}"));
            continue;
        }
        let restored = restore_url(&rewritten, &cfg);
        if restored != normalized {
            failures.push(format!(
                "#{i} 往返不一致：{normalized} -> {rewritten} -> {restored}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "1000 例属性测试失败 {} 例，前 5 条：\n{}",
        failures.len(),
        failures.iter().take(5).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn rewrite_disabled_is_identity() {
    let cfg = WebvpnConfig::default(); // enabled=false
    let url = "https://www.bjut.edu.cn/info/1/2.html";
    assert_eq!(rewrite_url(url, &cfg), url);
    assert_eq!(restore_url(url, &cfg), url);
}

#[test]
fn rewrite_skips_already_gateway() {
    let cfg = vpn_on();
    let gateway_url = format!("{WEBVPN_GATEWAY}/https/77726476706e69737468656265737421e7e056d2253a7d44300d8db9d6562d/info/1.htm");
    assert_eq!(rewrite_url(&gateway_url, &cfg), gateway_url);
}

#[test]
fn restore_urls_in_text_restores_html_links() {
    let cfg = vpn_on();
    let hex_www = encode_wengine_host("www.bjut.edu.cn");
    let hex_jwc = encode_wengine_host("jwc.bjut.edu.cn");
    let text = format!(
        r#"<a href="https://webvpn.bjut.edu.cn/https/{hex_www}/info/1.htm">通知一</a>"#,
    );
    let text2 = format!(
        r#"{text}<a href="/https/{hex_jwc}/tzgg/2.html">通知二</a><span>网关自身 https://webvpn.bjut.edu.cn/keep</span>"#
    );
    let out = restore_urls_in_text(&text2, &cfg);
    assert!(out.contains("https://www.bjut.edu.cn/info/1.htm"), "got: {out}");
    assert!(out.contains("https://jwc.bjut.edu.cn/tzgg/2.html"), "got: {out}");
    // 未编码的网关地址不应被改写（无 hex 段）
    assert!(out.contains("https://webvpn.bjut.edu.cn/keep"), "got: {out}");
}

#[test]
fn wengine_host_hex_fixture_seeds() {
    // fixtures/raw/html__webvpn-* 里嵌入的种子。
    let seeds = [
        (
            "one.bjut.edu.cn",
            "77726476706e69737468656265737421fff944d2253a7d44300d8db9d6562d",
        ),
        (
            "gs.bjut.edu.cn",
            "77726476706e69737468656265737421f7e40f9e2d257c1e7b0c9ce29b5b",
        ),
        (
            "cc.bjut.edu.cn",
            "77726476706e69737468656265737421f3f40f9e2d257c1e7b0c9ce29b5b",
        ),
    ];
    for (host, hex) in seeds {
        assert_eq!(&encode_wengine_host(host), hex);
        assert_eq!(&decode_wengine_host(hex), host);
    }
}

/* ---------------------- 网关登录页识别（6 样本） ---------------------- */

fn cfg_with_pattern() -> WebvpnConfig {
    vpn_on()
}

#[test]
fn login_page_detection_6_samples() {
    let cfg = cfg_with_pattern();
    // 1. 密码输入框登录页
    let s1 = r#"<html><body><form><input type="password" name="pwd"></form></body></html>"#;
    // 2. 标题含「登录」
    let s2 = "<html><head><title>统一身份认证登录</title></head><body>请登录</body></html>";
    // 3. unPassword 输入框
    let s3 = r#"<input id="unPassword" type="text">"#;
    // 4. 网关登录表单 id=loginForm
    let s4 = r#"<div id="loginForm"><p>会话已过期</p></div>"#;
    // 5. JSON 错误体（msg 含登录语义）
    let s5 = r#"{"code": 401, "msg": "请登录"}"#;
    // 6. 正常业务 JSON（d 有数据）——绝不能误判
    let s6 = r#"{"e": 0, "d": {"data": [{"title": "通知"}]}}"#;

    assert!(matches_login_pattern(s1, &cfg), "s1");
    assert!(matches_login_pattern(s2, &cfg), "s2");
    assert!(matches_login_pattern(s3, &cfg), "s3");
    assert!(matches_login_pattern(s4, &cfg), "s4");
    assert!(!matches_login_pattern(s6, &cfg), "s6");

    assert!(webvpn_session_expired(s1, &cfg), "s1 expired");
    assert!(webvpn_session_expired(s2, &cfg), "s2 expired");
    assert!(webvpn_session_expired(s5, &cfg), "s5 expired");
    assert!(!webvpn_session_expired(s6, &cfg), "s6 not expired");

    // e=10041/10042 交给门户业务分支，不算网关过期
    assert!(!webvpn_session_expired(r#"{"e": 10041, "msg": "请登录"}"#, &cfg));
    assert!(!webvpn_session_expired(r#"{"e": 10042, "message": "登录超时"}"#, &cfg));

    // 未启用 WebVPN 时一律 false
    let off = WebvpnConfig::default();
    assert!(!webvpn_session_expired(s1, &off));
}

#[test]
fn port_and_path_edge_cases() {
    let cfg = vpn_on();
    // 非标准端口往返
    let u = "https://www.bjut.edu.cn:8443/portal/x?y=1";
    let n = normalize(u);
    assert_eq!(restore_url(&rewrite_url(&n, &cfg), &cfg), n);
    // 查询串保留
    let u2 = "http://my.bjut.edu.cn/site/get_news_list?category_id=1&p=1&size=32";
    let n2 = normalize(u2);
    assert_eq!(restore_url(&rewrite_url(&n2, &cfg), &cfg), n2);
}
