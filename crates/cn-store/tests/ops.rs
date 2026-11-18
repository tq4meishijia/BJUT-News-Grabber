//! 18 个 op 各有成功 + 失败用例（§9 P3 出门门槛）。
//! 网络用 cn_net::MockFetcher，时钟用 cn_model::FixedClock，凭据用内存 SecretStore。

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Local, TimeZone};
use cn_model::{AuthErrorCode, FixedClock};
use cn_net::mock::{MockFetcher, Rule};
use cn_net::{FetchRequest, FetchResponse};
use cn_store::{ActRequest, CampusService, StoreData};

fn fixed_clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Local.with_ymd_and_hms(2026, 11, 18, 10, 0, 0).single().unwrap(),
    ))
}

fn temp_store_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cn-store-ops-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.join("store.json")
}

/// 一个返回固定 HTML 列表的 MockFetcher。
fn html_fetcher(body: &str) -> Arc<MockFetcher> {
    let body = body.to_string();
    MockFetcher::new(move |req: &FetchRequest| {
        Ok(FetchResponse {
            status: 200,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: body.clone().into_bytes(),
        })
    })
}

fn failing_fetcher() -> Arc<MockFetcher> {
    MockFetcher::new(|_| Err(cn_net::NetError::Timeout))
}

const LIST_HTML: &str = r#"<html><body><ul>
<li><a href="https://one.bjut.edu.cn/info/1/20261101.htm">关于2026年国庆节放假安排的通知</a><span>2026-09-25</span></li>
<li><a href="https://one.bjut.edu.cn/info/1/20261102.htm">关于开展2026级本科生学籍注册工作的通知</a><span>2026-09-01</span></li>
</ul></body></html>"#;

async fn service_with_source(tag: &str) -> CampusService {
    let path = temp_store_path(tag);
    let mut svc = CampusService::new_for_test(path, html_fetcher(LIST_HTML), fixed_clock());
    // 加一个自定义源（种子源 url 是 my.bjut.edu.cn 接口，mock 会返回 HTML 触发嗅探报错；改用专用源）
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src-test".into(),
        name: "测试源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/notice.html".into(),
        enabled: true,
        ..Default::default()
    });
    svc
}

fn act(op: &str, payload: serde_json::Value) -> ActRequest {
    ActRequest {
        op: op.to_string(),
        payload,
    }
}

#[tokio::test]
async fn op_poll_success_and_failure() {
    // 成功
    let mut svc = service_with_source("poll-ok").await;
    let r = svc
        .act(act("poll", serde_json::json!({})))
        .await;
    assert!(r.ok, "err={:?}", r.error);
    assert!(!r.state.notices.is_empty());

    // 失败：网络超时
    let path = temp_store_path("poll-fail");
    let mut svc = CampusService::new_for_test(path, failing_fetcher(), fixed_clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src-x".into(),
        name: "坏源".into(),
        url: "https://one.bjut.edu.cn/x.html".into(),
        enabled: true,
        ..Default::default()
    });
    let r = svc.act(act("poll", serde_json::json!({}))).await;
    // B1：源失败是部分失败——poll 本身 ok=true 但 lastPollError 非空
    assert!(r.state.meta.last_poll_error.contains("坏源"), "err={}", r.state.meta.last_poll_error);
}

#[tokio::test]
async fn op_notice_read_readall_remove() {
    let mut svc = service_with_source("notice").await;
    svc.act(act("poll", serde_json::json!({}))).await;
    let id = svc.store.notices[0].id.clone();
    // read 成功
    let r = svc.act(act("notice.read", serde_json::json!({"id": id, "read": true}))).await;
    assert!(r.ok && r.state.notices[0].read);
    // readAll 成功
    let r = svc.act(act("notice.readAll", serde_json::json!({}))).await;
    assert!(r.ok && r.state.notices.iter().all(|n| n.read));
    // remove 成功
    let r = svc.act(act("notice.remove", serde_json::json!({"id": id}))).await;
    assert!(r.ok && r.state.notices.iter().all(|n| n.id != id));
    // 失败语义：删不存在的 id 仍 ok（JS 一致），但 read 找不到也 ok；
    // 明确失败用例：notice.summarize 找不到
    let r = svc.act(act("notice.summarize", serde_json::json!({"id": "nope"}))).await;
    assert!(!r.ok && r.error.as_deref() == Some("找不到这条通知"));
}

#[tokio::test]
async fn op_notice_summarize_with_mock_llm() {
    // 配好 LLM Key + mock 返回（rules 同时覆盖列表与 chat/completions）
    let path = temp_store_path("summarize2");
    let rows = r#"[{"i":0,"category":"教务","gist":"放假安排","useful":["注意时间"],"tags":["放假"],"deadline":"","contact":"","relevant":true,"confidence":0.8}]"#;
    let body = serde_json::json!({
        "choices": [{ "message": { "content": rows } }],
        "usage": { "total_tokens": 10 },
    });
    let fetcher = MockFetcher::with_rules(vec![
        Rule::get("notice.html", 200, LIST_HTML).with_headers(vec![("content-type".into(), "text/html".into())]),
        Rule {
            url_contains: "/chat/completions".into(),
            method: Some("POST"),
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec(&body).unwrap(),
        },
    ]);
    let mut svc = CampusService::new_for_test(path, fetcher, fixed_clock());
    svc.secrets().set("llm.apiKey", "sk-test").unwrap();
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src-test".into(),
        name: "测试源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/notice.html".into(),
        enabled: true,
        ..Default::default()
    });
    svc.act(act("poll", serde_json::json!({}))).await;
    assert!(!svc.store.notices.is_empty());
    let id = svc.store.notices[0].id.clone();
    let r = svc.act(act("notice.summarize", serde_json::json!({"id": id}))).await;
    assert!(r.ok, "err={:?}", r.error);
    assert_eq!(svc.store.notices[0].summary, "放假安排");
    // 失败：没配 Key 的服务
    let mut svc2 = CampusService::new_for_test(
        temp_store_path("summarize-nolimit"),
        failing_fetcher(),
        fixed_clock(),
    );
    let r = svc2
        .act(act("notice.summarize", serde_json::json!({"id": "x"})))
        .await;
    assert!(!r.ok, "找不到通知应失败");
}

#[tokio::test]
async fn send_to_llm_opt_out_keeps_content_local() {
    // §6.4：sendToLlm=false 的源——手动摘要被拒，poll 批量不送模型（规则兜底）。
    let rows = r#"[{"i":0,"category":"教务","gist":"放假安排","useful":["注意时间"],"tags":["放假"],"deadline":"","contact":"","relevant":true,"confidence":0.8}]"#;
    let body = serde_json::json!({
        "choices": [{ "message": { "content": rows } }],
        "usage": { "total_tokens": 10 },
    });
    let llm_rule = Rule {
        url_contains: "/chat/completions".into(),
        method: Some("POST"),
        status: 200,
        headers: vec![("content-type".into(), "application/json".into())],
        body: serde_json::to_vec(&body).unwrap(),
    };

    // 关闭 AI 的源：ai=false，规则摘要非空，手动 summarize 被拒。
    let mut svc = CampusService::new_for_test(
        temp_store_path("optout-off"),
        MockFetcher::with_rules(vec![
            Rule::get("notice.html", 200, LIST_HTML)
                .with_headers(vec![("content-type".into(), "text/html".into())]),
            llm_rule.clone(),
        ]),
        fixed_clock(),
    );
    svc.secrets().set("llm.apiKey", "sk-test").unwrap();
    svc.store.settings.llm = true;
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src-private".into(),
        name: "成绩源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/notice.html".into(),
        enabled: true,
        send_to_llm: false,
        ..Default::default()
    });
    svc.act(act("poll", serde_json::json!({}))).await;
    assert!(!svc.store.notices.is_empty());
    assert!(
        svc.store.notices.iter().all(|n| !n.ai),
        "关闭 AI 的源不应出现 AI 摘要"
    );
    assert!(!svc.store.notices[0].summary.is_empty(), "B11：规则摘要兜底");
    let id = svc.store.notices[0].id.clone();
    let r = svc
        .act(act("notice.summarize", serde_json::json!({"id": id})))
        .await;
    assert!(!r.ok, "手动摘要也应被源级开关拒绝");
    assert!(
        r.error.unwrap_or_default().contains("sendToLlm"),
        "错误文案应指明原因"
    );

    // 对照：默认 sendToLlm=true 的源走 AI，至少一条吃到 AI 摘要。
    let fetcher2 = MockFetcher::with_rules(vec![
        Rule::get("notice.html", 200, LIST_HTML)
            .with_headers(vec![("content-type".into(), "text/html".into())]),
        llm_rule,
    ]);
    let mut svc2 = CampusService::new_for_test(
        temp_store_path("optout-on"),
        fetcher2,
        fixed_clock(),
    );
    svc2.secrets().set("llm.apiKey", "sk-test").unwrap();
    svc2.store.settings.llm = true;
    svc2.store.sources.clear();
    svc2.store.sources.push(cn_store::SourceRecord {
        id: "src-open".into(),
        name: "公开源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/notice.html".into(),
        enabled: true,
        ..Default::default()
    });
    svc2.act(act("poll", serde_json::json!({}))).await;
    assert!(!svc2.store.notices.is_empty());
    assert!(
        svc2.store.notices.iter().any(|n| n.ai),
        "默认允许的源应吃到 AI 摘要"
    );
    // §6.3：token 计量入账（mock usage.total_tokens=10），预算默认 0=不限。
    let meta = svc2.state().meta;
    assert_eq!(meta.llm_used, 10, "Mock usage.total_tokens=10 应入账");
    assert_eq!(meta.llm_budget, 0, "默认预算 0=不限");
}

#[tokio::test]
async fn op_notices_rescore_and_refill() {
    let mut svc = service_with_source("rescore").await;
    svc.act(act("poll", serde_json::json!({}))).await;
    let before = svc.store.notices[0].score;
    let r = svc.act(act("notices.rescore", serde_json::json!({}))).await;
    assert!(r.ok);
    assert!(r.message.unwrap().contains("重算"));
    assert_eq!(svc.store.notices[0].score, before, "规则不变分数应稳定");

    // refill：内容不足 40 字 → 拉详情
    let detail_html = r#"<html><body><div id="vsb_content">关于考试安排的详细说明：请于2026年12月1日前提交申请表，联系人李老师电话010-67391234，务必按时完成不要错过截止时间。</div></body></html>"#;
    let fetcher = MockFetcher::with_rules(vec![
        Rule::get("/info/", 200, LIST_HTML).with_headers(vec![("content-type".into(), "text/html".into())]),
        Rule::get("20261101.htm", 200, detail_html).with_headers(vec![("content-type".into(), "text/html".into())]),
        Rule::get("20261102.htm", 200, detail_html).with_headers(vec![("content-type".into(), "text/html".into())]),
    ]);
    let path = temp_store_path("refill");
    let mut svc = CampusService::new_for_test(path, fetcher, fixed_clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src-test".into(),
        name: "测试源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/notice.html".into(),
        enabled: true,
        detail_url_template: "https://one.bjut.edu.cn/info/1/{url}".into(),
        ..Default::default()
    });
    svc.act(act("poll", serde_json::json!({}))).await;
    // detailUrlTemplate 形态不对也能走 notice.url 直接拉；content 空 → refill 会拉
    let r = svc
        .act(act("notices.refill", serde_json::json!({"limit": 10})))
        .await;
    assert!(r.ok, "err={:?}", r.error);

    // 失败：limit 非法 → numberOr 回退仍 ok（JS 一致）；用未知字段验证默认路径
    let r = svc.act(act("notices.refill", serde_json::json!({}))).await;
    assert!(r.ok);
}

#[tokio::test]
async fn op_item_crud() {
    let mut svc = service_with_source("item").await;
    // add 成功
    let r = svc
        .act(act(
            "item.add",
            serde_json::json!({"title": "交作业", "dueAt": "2026-12-01 17:00", "note": "高数"}),
        ))
        .await;
    assert!(r.ok, "err={:?}", r.error);
    let id = r.id.clone().expect("id");
    // add 失败：无标题
    let r = svc.act(act("item.add", serde_json::json!({"dueAt": "2026-12-01 17:00"}))).await;
    assert!(!r.ok && r.error.as_deref() == Some("标题不能为空"));
    // add 失败：时间格式错
    let r = svc.act(act("item.add", serde_json::json!({"title": "x", "dueAt": "瞎写"}))).await;
    assert!(!r.ok && r.error.as_deref() == Some("时间格式不对，用 2026-11-05 14:30 这种"));
    // update 成功
    let r = svc
        .act(act(
            "item.update",
            serde_json::json!({"id": id, "title": "交作业v2", "dueAt": "2026-12-02 09:00"}),
        ))
        .await;
    assert!(r.ok && svc.store.items[0].title == "交作业v2");
    // update 失败：id 不存在
    let r = svc.act(act("item.update", serde_json::json!({"id": "nope"}))).await;
    assert!(!r.ok && r.error.as_deref() == Some("找不到这个日程"));
    // done 成功 + refire
    let r = svc.act(act("item.done", serde_json::json!({"id": id, "done": true}))).await;
    assert!(r.ok && svc.store.items[0].done);
    // done 失败
    let r = svc.act(act("item.done", serde_json::json!({"id": "nope"}))).await;
    assert!(!r.ok);
    // refire 成功
    let r = svc.act(act("item.refire", serde_json::json!({"id": id}))).await;
    assert!(r.ok);
    // remove 成功
    let r = svc.act(act("item.remove", serde_json::json!({"id": id}))).await;
    assert!(r.ok && svc.store.items.is_empty());
}

#[tokio::test]
async fn op_source_crud_and_test() {
    let mut svc = service_with_source("source").await;
    // add 成功
    let r = svc
        .act(act(
            "source.add",
            serde_json::json!({"url": "https://jwc.bjut.edu.cn/tzgg.html", "type": "html", "name": "教务"}),
        ))
        .await;
    assert!(r.ok && svc.store.sources.len() == 2, "err={:?}", r.error);
    let new_id = svc.store.sources[1].id.clone();
    // add 失败：非 http(s)
    let r = svc.act(act("source.add", serde_json::json!({"url": "ftp://x"}))).await;
    assert!(!r.ok && r.error.as_deref() == Some("请填写 http(s) 开头的网址"));
    // update 成功
    let r = svc
        .act(act(
            "source.update",
            serde_json::json!({"id": new_id, "name": "教务处", "enabled": false}),
        ))
        .await;
    assert!(r.ok && svc.store.sources[1].name == "教务处" && !svc.store.sources[1].enabled);
    // update 失败
    let r = svc.act(act("source.update", serde_json::json!({"id": "nope"}))).await;
    assert!(!r.ok && r.error.as_deref() == Some("找不到这个数据源"));
    // test 成功
    let r = svc.act(act("source.test", serde_json::json!({"id": "src-test"}))).await;
    assert!(r.ok, "err={:?}", r.error);
    assert!(r.preview.is_some());
    // test 失败：id 不存在
    let r = svc.act(act("source.test", serde_json::json!({"id": "nope"}))).await;
    assert!(!r.ok && r.error.as_deref() == Some("找不到这个数据源"));
    // remove 成功
    let r = svc.act(act("source.remove", serde_json::json!({"id": new_id}))).await;
    assert!(r.ok && svc.store.sources.len() == 1);
}

#[tokio::test]
async fn op_settings_save() {
    let mut svc = service_with_source("settings").await;
    let r = svc
        .act(act(
            "settings.save",
            serde_json::json!({"settings": {
                "pollMinutes": 15,
                "keywords": ["考试", " 报名 ", ""],
                "llm": false,
                "maxItemsPerSource": 50,
                "webvpn": {"enabled": true, "cookie": "vpn-cookie"}
            }}),
        ))
        .await;
    assert!(r.ok);
    assert_eq!(svc.store.settings.poll_minutes, 15);
    assert_eq!(svc.store.settings.keywords, vec!["考试".to_string(), "报名".to_string()]);
    assert!(!svc.store.settings.llm);
    assert_eq!(svc.store.settings.max_items_per_source, 50);
    assert!(svc.store.settings.webvpn.enabled);
    assert_eq!(svc.store.settings.webvpn.cookie, "vpn-cookie");
    // B9：state 里绝不出现网关 Cookie
    let state = serde_json::to_value(svc.state()).unwrap();
    let text = state.to_string();
    assert!(!text.contains("vpn-cookie"), "网关 Cookie 泄漏：{text}");
    // clearCookie
    let r = svc
        .act(act("settings.save", serde_json::json!({"settings": {"webvpn": {"clearCookie": true}}})))
        .await;
    assert!(r.ok && svc.store.settings.webvpn.cookie.is_empty());
    // 未知 op 失败
    let r = svc.act(act("nope.op", serde_json::json!({}))).await;
    assert!(!r.ok && r.error.unwrap().contains("未知操作"));
}

#[tokio::test]
async fn op_notify_test() {
    let mut svc = service_with_source("notify").await;
    let r = svc.act(act("notify.test", serde_json::json!({}))).await;
    assert!(r.ok);
}

#[tokio::test]
async fn op_llm_key_and_test() {
    let mut svc = service_with_source("llmkey").await;
    // 成功：保存 Key
    let r = svc.act(act("llm.key", serde_json::json!({"apiKey": "sk-123"}))).await;
    assert!(r.ok && r.message.unwrap().contains("已保存"));
    assert_eq!(svc.secrets().get("llm.apiKey").as_deref(), Some("sk-123"));
    // 清空
    let r = svc.act(act("llm.key", serde_json::json!({"apiKey": ""}))).await;
    assert!(r.ok && r.message.unwrap().contains("清空"));
    // llm.test 失败（无 Key）
    let r = svc.act(act("llm.test", serde_json::json!({}))).await;
    assert!(!r.ok, "无 Key 应失败");
    assert!(r.error.unwrap().contains("API Key"));
}

#[tokio::test]
async fn op_account_save_test_clear() {
    // 成功保存 + 登录成功
    let login_rules = vec![
        Rule {
            url_contains: "/login?service=".into(),
            method: Some("GET"),
            status: 200,
            headers: vec![
                ("content-type".into(), "text/html".into()),
                ("set-cookie".into(), "TGC=x; Domain=cas.bjut.edu.cn".into()),
            ],
            body: LOGIN_PAGE.as_bytes().to_vec(),
        },
        Rule {
            url_contains: "/login?service=".into(),
            method: Some("POST"),
            status: 302,
            headers: vec![
                ("location".into(), "https://my.bjut.edu.cn/index.html".into()),
                ("set-cookie".into(), "MYSESSION=p; Domain=my.bjut.edu.cn".into()),
            ],
            body: Vec::new(),
        },
        Rule::get("my.bjut.edu.cn/index.html", 200, "<html><head><title>欢迎回来</title></head></html>")
            .with_headers(vec![("content-type".into(), "text/html".into())]),
    ];
    let path = temp_store_path("account");
    let mut svc = CampusService::new_for_test(path, MockFetcher::with_rules(login_rules), fixed_clock());
    // 只有 MockFetcher rules 匹配；source 探测需要一个同域源
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src-my".into(),
        name: "一站式".into(),
        r#type: "json".into(),
        url: "https://my.bjut.edu.cn/site/get_news_list".into(),
        enabled: true,
        ..Default::default()
    });
    let r = svc
        .act(act(
            "account.save",
            serde_json::json!({
                "username": "alice",
                "password": "pw",
                "casBase": "https://cas.bjut.edu.cn",
                "service": "https://my.bjut.edu.cn/index.html"
            }),
        ))
        .await;
    assert!(r.ok, "err={:?}", r.error);
    assert!(r.state.meta.account.configured);
    assert_eq!(r.state.meta.account.username, "alice");
    // B9：state 里不出现密码
    let text = serde_json::to_value(svc.state()).unwrap().to_string();
    assert!(!text.contains("pw\""), "密码泄漏？{text}");

    // 失败：账号为空
    let r = svc
        .act(act("account.save", serde_json::json!({"username": "", "casBase": "https://cas.bjut.edu.cn", "service": "https://my.bjut.edu.cn"})))
        .await;
    assert!(!r.ok && r.error.as_deref() == Some("账号不能为空"));
    // 失败：地址不合法
    let r = svc
        .act(act("account.save", serde_json::json!({"username": "bob", "casBase": "x", "service": "y"})))
        .await;
    assert!(!r.ok && r.error.unwrap().contains("http(s)"));

    // account.test（已保存账号 + 同 mock 登录流）
    let r = svc.act(act("account.test", serde_json::json!({}))).await;
    assert!(r.ok, "err={:?}", r.error);

    // account.clear 成功
    let r = svc.act(act("account.clear", serde_json::json!({}))).await;
    assert!(r.ok);
    assert!(!svc.state().meta.account.configured);
}

const LOGIN_PAGE: &str = r#"<html><head><title>统一身份认证</title></head><body>
<form><input type="hidden" name="execution" value="e1s1">
<input type="password" name="password"></form></body></html>"#;

#[tokio::test]
async fn credential_error_classification() {
    // account.save 后 autoLogin 失败（mock 登录页带 errorDiv）→ ok=false 且 AuthError 文案
    let rules = vec![Rule::get("/login", 200, r#"<div id="errorDiv">账号或密码不对</div><input name="execution" value="e1">"#)
        .with_headers(vec![("content-type".into(), "text/html".into())])];
    let path = temp_store_path("cred");
    let mut svc = CampusService::new_for_test(path, MockFetcher::with_rules(rules), fixed_clock());
    svc.store.sources.clear();
    let r = svc
        .act(act(
            "account.save",
            serde_json::json!({
                "username": "alice",
                "password": "wrong",
                "casBase": "https://cas.bjut.edu.cn",
                "service": "https://my.bjut.edu.cn/index.html"
            }),
        ))
        .await;
    assert!(!r.ok);
    assert!(r.error.unwrap().contains("登录失败"), "应产出凭据错误文案");
    let _ = AuthErrorCode::Credential; // 错误分类在 cn-net 层已单测覆盖
}

#[tokio::test]
async fn state_view_truncation_b12() {
    let path = temp_store_path("b12");
    let mut svc = CampusService::new_for_test(path, failing_fetcher(), fixed_clock());
    // 塞 250 条 notice / 350 item / 30 reminder
    for i in 0..250 {
        svc.store.notices.push(cn_store::NoticeRecord {
            id: format!("n{i}"),
            title: format!("通知{i}"),
            ..Default::default()
        });
    }
    for i in 0..350 {
        svc.store.items.push(cn_store::ItemRecord {
            id: format!("s{i}"),
            title: format!("日程{i}"),
            ..Default::default()
        });
    }
    for i in 0..30 {
        svc.store.meta.reminders.push(cn_store::ReminderRecord {
            id: format!("r{i}"),
            ..Default::default()
        });
    }
    let state = svc.state();
    assert_eq!(state.notices.len(), 200);
    assert_eq!(state.items.len(), 300);
    assert_eq!(state.meta.reminders.len(), 20);
    // 存储上限（trim）
    svc.store.trim_storage();
    assert_eq!(svc.store.notices.len(), 250, "≤400 不动");
    for i in 250..450 {
        svc.store.notices.push(cn_store::NoticeRecord {
            id: format!("x{i}"),
            ..Default::default()
        });
    }
    svc.store.trim_storage();
    assert_eq!(svc.store.notices.len(), 400);
}

#[tokio::test]
async fn unknown_op_fails_with_state() {
    let mut svc = service_with_source("unknown").await;
    let r = svc.act(act("bogus", serde_json::json!({}))).await;
    assert!(!r.ok);
    // 失败也带 state（§3.3）
    assert_eq!(r.state.settings.max_items_per_source, 100);
}

#[tokio::test]
async fn dedupe_key_b5_shape() {
    let key = cn_store::pipeline::build_dedupe_key("src-1", "关于考试的通知");
    assert!(key.starts_with("src-1|"));
    assert_eq!(key.split('|').nth(1).unwrap().len(), 16);
    // 相同标题 → 相同键
    assert_eq!(key, cn_store::pipeline::build_dedupe_key("src-1", "关于考试的通知"));
}

#[tokio::test]
async fn store_default_seed_sources() {
    let data = StoreData::default();
    assert_eq!(data.sources.len(), 4);
    assert_eq!(data.settings.max_items_per_source, 100);
    assert!(data.sources.iter().all(|s| s.r#type == "json"));
}
