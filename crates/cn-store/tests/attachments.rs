//! 后端全链路集成测试：时间过滤前置、白名单附件、去重共享、失败隔离、级联删除、旧 store 兼容。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Local, TimeZone};
use cn_model::{AttachmentStatus, FixedClock};
use cn_net::mock::{MockFetcher, Rule};
use cn_net::{FetchRequest, NetError, ATTACHMENT_MAX_BYTES, DEFAULT_MAX_BYTES};
use cn_store::{ActRequest, CampusService, StoreData};
use serde_json::json;

const LIST_URL: &str = "https://one.bjut.edu.cn/notice.html";

fn fixed_clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Local.with_ymd_and_hms(2026, 11, 18, 10, 0, 0).single().unwrap(),
    ))
}

fn temp_store_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cn-store-att-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.join("store.json")
}

fn list_html() -> String {
    let filler = "填充文本用于隔离日期上下文".repeat(20);
    format!(
        r#"<html><body>
<p>{filler}</p>
<ul><li><a href="https://one.bjut.edu.cn/info/4.htm">关于无日期测试的通知</a></li></ul>
<p>{filler}</p>
<ul>
<li><a href="https://one.bjut.edu.cn/info/1.htm">2026-09-25 关于开展奖学金申请的通知</a></li>
<li><a href="https://one.bjut.edu.cn/info/2.htm">2026-09-26 关于奖学金公示结果的通知</a></li>
<li><a href="https://one.bjut.edu.cn/info/3.htm">2026-09-27 关于图书馆开放时间的通知</a></li>
</ul></body></html>"#
    )
}

fn detail_html(extra: &str) -> String {
    format!(
        r#"<html><body><div>这里是通知正文内容，足够长以便正文抽取逻辑正常工作并保留下来。{}</div>
<a href="/files/notice.pdf">申请表</a>
<a href="{}">申请表副本</a>
<a href="/files/malware.exe">非白名单</a>
</body></html>"#,
        "正文补充文字。".repeat(6),
        extra
    )
}

fn base_rules() -> Vec<Rule> {
    vec![
        // 用路径片段匹配：开 WebVPN 后 URL 会被重写成网关形式，完整 URL 匹配不到。
        Rule::get("/notice.html", 200, &list_html()),
        Rule::get("/info/1.htm", 200, &detail_html("/files/notice.pdf?utm_source=x")),
        Rule::get("/info/2.htm", 200, &detail_html("/files/notice.pdf?utm_source=y")),
        Rule::get("/info/3.htm", 200, &detail_html("/files/notice.pdf?utm_source=z")),
        Rule::get("/info/4.htm", 200, &detail_html("/files/notice.pdf?utm_source=z")),
        Rule::get("/files/notice.pdf", 200, "%PDF-1.4 fake attachment bytes")
            .with_headers(vec![("content-type".into(), "application/pdf".into())]),
        Rule::get("/files/malware.exe", 200, "MZ"),
    ]
}

fn service_with(tag: &str, fetcher: &Arc<MockFetcher>, attachments: bool) -> CampusService {
    let mut svc = CampusService::new_for_test(temp_store_path(tag), fetcher.clone(), fixed_clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src-test".into(),
        name: "测试源".into(),
        r#type: "html".into(),
        url: LIST_URL.into(),
        enabled: true,
        cookie: "JSESSIONID=test-cookie".into(),
        ..Default::default()
    });
    svc.store.settings.llm = false;
    svc.store.settings.fetch_detail = true;
    svc.store.settings.fetch_attachments = attachments;
    svc
}

fn act(op: &str, payload: serde_json::Value) -> ActRequest {
    ActRequest {
        op: op.to_string(),
        payload,
    }
}

fn attachment_requests(fetcher: &MockFetcher) -> Vec<FetchRequest> {
    fetcher
        .calls
        .lock()
        .expect("calls")
        .iter()
        .filter(|req| req.url.contains("/files/"))
        .cloned()
        .collect()
}

fn detail_requests(fetcher: &MockFetcher) -> Vec<FetchRequest> {
    fetcher
        .calls
        .lock()
        .expect("calls")
        .iter()
        .filter(|req| req.url.contains("/info/"))
        .cloned()
        .collect()
}

#[tokio::test]
async fn date_filter_runs_before_dedupe_and_detail() {
    let fetcher = MockFetcher::with_rules(base_rules());
    let mut svc = service_with("date-filter", &fetcher, false);

    let result = svc
        .act(act("poll", json!({"dateRange": {"start": "2026-09-25", "end": "2026-09-25"}})))
        .await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(
        result.state.notices.len(),
        2,
        "区间内 1 条 + 空日期 1 条: {:?}",
        result
            .state
            .notices
            .iter()
            .map(|n| (n.title.as_str(), n.published_at.as_str()))
            .collect::<Vec<_>>()
    );
    assert!(result.state.notices.iter().any(|n| n.published_at.is_empty()));
    // 空日期通知按 A5 保留，也会进入详情跟进；范围外（09-26/09-27）两条才不应请求详情。
    assert_eq!(detail_requests(&fetcher).len(), 2, "仅范围内1条+空日期1条应请求详情，范围外2条不应请求");
    assert!(
        result.message.as_deref().unwrap_or("").contains("时间范围跳过 2 条"),
        "{:?}",
        result.message
    );

    // 范围外通知此前连 dedupe key 都没建：换区间后能重新入库。
    let second = svc
        .act(act("poll", json!({"dateRange": {"start": "2026-09-26", "end": "2026-09-27"}})))
        .await;
    assert!(second.ok, "err={:?}", second.error);
    assert_eq!(second.state.notices.len(), 4);
    // 累计详情请求：第一次 2 次（09-25 + 空日期），第二次 2 次（09-26 + 09-27）。
    assert_eq!(detail_requests(&fetcher).len(), 4);
    assert!(svc.store.settings.date_range_start.is_empty(), "临时范围不写回设置");
}

#[tokio::test]
async fn invalid_range_returns_state_error_without_mutating_settings() {
    let fetcher = MockFetcher::with_rules(base_rules());
    let mut svc = service_with("invalid-range", &fetcher, false);

    let before = svc.store.settings.llm;
    let bad = svc
        .act(act(
            "settings.save",
            json!({"settings": {"llm": false, "dateRangeStart": "2026-13-40"}}),
        ))
        .await;
    assert!(!bad.ok);
    assert_eq!(bad.error.as_deref(), Some(cn_core::INVALID_DATE_MESSAGE));
    assert_eq!(bad.state.settings.llm, before, "校验失败时其它设置字段也不得被改动");
    assert_eq!(svc.store.settings.llm, before, "校验失败不得修改任何设置字段");

    let inverted = svc
        .act(act(
            "settings.save",
            json!({"settings": {"dateRangeStart": "2026-09-10", "dateRangeEnd": "2026-09-01"}}),
        ))
        .await;
    assert_eq!(inverted.error.as_deref(), Some(cn_core::START_AFTER_END_MESSAGE));

    let future = svc
        .act(act("poll", json!({"dateRange": {"start": "2026-12-01", "end": ""}})))
        .await;
    assert_eq!(future.error.as_deref(), Some(cn_core::START_AFTER_TODAY_MESSAGE));
    assert_eq!(detail_requests(&fetcher).len(), 0, "校验失败不抓取");
}

#[tokio::test]
async fn attachments_whitelist_dedupe_cascade_and_size_limit() {
    let fetcher = MockFetcher::with_rules(base_rules());
    let mut svc = service_with("attachments", &fetcher, true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(result.state.notices.len(), 4);

    // 四条通知共享一条 PDF 记录（utm 被规范化剔除）；exe 不在白名单。
    assert_eq!(svc.store.attachments.len(), 1, "{:?}", svc.store.attachments);
    let shared = svc
        .store
        .attachments
        .iter()
        .find(|a| a.file_name == "notice.pdf")
        .cloned()
        .expect("shared attachment");
    assert_eq!(shared.notice_ids.len(), 4);
    assert_eq!(shared.status, AttachmentStatus::Downloaded);
    assert!(!shared.local_path.ends_with(".exe"));

    let dir = svc.attachments_dir();
    let file = dir.join(&shared.local_path);
    assert!(file.is_file(), "附件应落盘：{}", file.to_string_lossy());
    assert_eq!(std::fs::read(&file).unwrap().len(), shared.size_bytes as usize);

    let dto = result
        .state
        .attachments
        .iter()
        .find(|a| a.id == shared.id)
        .expect("dto");
    assert_eq!(PathBuf::from(&dto.local_path), file, "DTO 给前端绝对路径");
    assert!(pathbuf_of(&dto.local_path).is_absolute());

    // 普通请求仍 6 MiB；附件请求 25 MiB。
    {
        let calls = fetcher.calls.lock().expect("calls");
        assert!(calls.iter().any(|r| r.max_bytes == DEFAULT_MAX_BYTES && !r.url.contains("/files/")));
        assert!(calls.iter().filter(|r| r.url.contains("/files/")).all(|r| r.max_bytes == ATTACHMENT_MAX_BYTES));
        assert!(calls.iter().filter(|r| r.url.contains("/files/")).all(|r| r.cookie == "JSESSIONID=test-cookie"));
    }

    // 删除其中一个通知：共享文件仍被其它通知引用，不能删。
    let ids: Vec<String> = svc.store.notices.iter().map(|n| n.id.clone()).collect();
    svc.act(act("notice.remove", json!({"id": ids[0]}))).await;
    assert!(file.is_file(), "共享附件不能因一方删除而被删");
    assert_eq!(
        svc.store.attachments.iter().find(|a| a.id == shared.id).unwrap().notice_ids.len(),
        3
    );

    // 删除最后一个引用：文件与记录一并删除，目录无孤儿。
    for id in ids.iter().skip(1) {
        svc.act(act("notice.remove", json!({"id": id}))).await;
    }
    assert!(!file.exists(), "最后一个引用删除后应删除本地文件");
    assert!(svc.store.attachments.iter().all(|a| a.id != shared.id));
    assert_eq!(std::fs::read_dir(&dir).map(|it| it.count()).unwrap_or(0), 0);
}

#[tokio::test]
async fn attachment_request_uses_webvpn_rewrite_and_gateway_cookie() {
    // 校外模式下所有 URL 都会被重写成网关地址，规则按路径片段匹配。
    let fetcher = MockFetcher::new(|req: &FetchRequest| {
        let body = if req.url.contains("/info/") {
            detail_html("/files/notice.pdf?utm_source=x")
        } else if req.url.contains("/files/") {
            "%PDF-1.4 fake attachment bytes".to_string()
        } else if req.url.contains("notice.html") {
            list_html()
        } else {
            String::new()
        };
        let status = if body.is_empty() { 404 } else { 200 };
        Ok(cn_net::FetchResponse {
            status,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: body.into_bytes(),
        })
    });
    let mut svc = service_with("webvpn", &fetcher, true);
    svc.store.settings.webvpn = cn_store::WebvpnSettings {
        enabled: true,
        cookie: "wengine_token=abc".into(),
        ..Default::default()
    };

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    let requests = attachment_requests(&fetcher);
    assert!(!requests.is_empty(), "应发出附件请求");
    let req = &requests[0];
    assert!(
        req.url.starts_with(cn_net::WEBVPN_GATEWAY),
        "附件 URL 应经网关重写: {}",
        req.url
    );
    assert_eq!(req.gateway_cookie, "wengine_token=abc");
    assert_eq!(req.cookie, "JSESSIONID=test-cookie");
    assert_eq!(req.max_bytes, ATTACHMENT_MAX_BYTES);
}

#[tokio::test]
async fn too_large_is_not_retried_and_failure_never_blocks_notice() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen_max = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let max_seen = seen_max.clone();
    let fetcher = MockFetcher::new(move |req: &FetchRequest| {
        if req.url.contains("/files/") {
            counter.fetch_add(1, Ordering::SeqCst);
            max_seen.store(req.max_bytes, Ordering::SeqCst);
            return Err(NetError::TooLarge(DEFAULT_MAX_BYTES));
        }
        for rule in base_rules() {
            if req.url.contains(rule.url_contains.as_str()) {
                return Ok(cn_net::FetchResponse {
                    status: rule.status,
                    url: req.url.clone(),
                    headers: rule.headers.clone(),
                    body: rule.body.clone(),
                });
            }
        }
        Ok(cn_net::FetchResponse {
            status: 404,
            url: req.url.clone(),
            headers: Vec::new(),
            body: Vec::new(),
        })
    });

    let mut svc = service_with("too-large", &fetcher, true);
    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok);
    assert_eq!(result.state.notices.len(), 4, "附件异常不得阻断通知入库");
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "过大附件本轮只请求一次且不重试");
    assert_eq!(seen_max.load(Ordering::SeqCst), ATTACHMENT_MAX_BYTES);
    assert!(svc
        .store
        .attachments
        .iter()
        .all(|a| a.status == AttachmentStatus::TooLarge));
    assert!(svc.store.attachments.iter().all(|a| !a.error.is_empty()));
    assert_eq!(std::fs::read_dir(svc.attachments_dir()).map(|it| it.count()).unwrap_or(0), 0);
}

#[tokio::test]
async fn failed_attachment_retry_recovers_and_marks_downloaded() {
    let flip = Arc::new(AtomicUsize::new(0));
    let switch = flip.clone();
    let fetcher = MockFetcher::new(move |req: &FetchRequest| {
        if req.url.contains("/files/") {
            if switch.load(Ordering::SeqCst) == 0 {
                return Err(NetError::Timeout);
            }
            return Ok(cn_net::FetchResponse {
                status: 200,
                url: req.url.clone(),
                headers: Vec::new(),
                body: b"ok".to_vec(),
            });
        }
        for rule in base_rules() {
            if req.url.contains(rule.url_contains.as_str()) {
                return Ok(cn_net::FetchResponse {
                    status: rule.status,
                    url: req.url.clone(),
                    headers: rule.headers.clone(),
                    body: rule.body.clone(),
                });
            }
        }
        Ok(cn_net::FetchResponse {
            status: 404,
            url: req.url.clone(),
            headers: Vec::new(),
            body: Vec::new(),
        })
    });

    let mut svc = service_with("failed-retry", &fetcher, true);
    let first = svc.act(act("poll", json!({}))).await;
    assert!(first.ok);
    let failed = svc
        .store
        .attachments
        .iter()
        .find(|a| a.file_name == "notice.pdf")
        .expect("failed record")
        .clone();
    assert_eq!(failed.status, AttachmentStatus::Failed);
    assert!(!failed.error.is_empty());
    assert_eq!(attachment_requests(&fetcher).len(), 3, "共享附件本轮只重试一次（共 3 次请求）");

    flip.store(1, Ordering::SeqCst);
    let retried = svc.act(act("attachment.retry", json!({"id": failed.id}))).await;
    assert!(retried.ok, "err={:?}", retried.error);
    let record = svc.store.attachments.iter().find(|a| a.id == failed.id).unwrap();
    assert_eq!(record.status, AttachmentStatus::Downloaded);
    assert!(svc.attachments_dir().join(&record.local_path).is_file());
}

#[tokio::test]
async fn old_store_json_loads_new_fields_with_defaults() {
    let path = temp_store_path("old-store");
    std::fs::write(
        &path,
        r#"{"version":1,"settings":{"pollMinutes":30,"llm":true},"notices":[],"items":[]}"#,
    )
    .expect("write old store");
    let svc = CampusService::new_for_test(path, MockFetcher::with_rules(base_rules()), fixed_clock());
    assert_eq!(svc.store.settings.date_range_start, "");
    assert_eq!(svc.store.settings.date_range_end, "");
    assert!(!svc.store.settings.fetch_attachments);
    assert!(svc.store.attachments.is_empty());
    let state = svc.state();
    assert!(state.attachments.is_empty());

    let data: StoreData = serde_json::from_str(r#"{"version":1,"settings":{}}"#).expect("parse");
    assert!(!data.settings.fetch_attachments);
}

fn pathbuf_of(text: &str) -> PathBuf {
    PathBuf::from(text)
}
