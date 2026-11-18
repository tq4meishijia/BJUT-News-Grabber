//! 流水线行为保留：B1 单源失败不影响其他源 / B4 截断 / B5 去重 / B7 过期不建日程 / B11 永不空白。

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Local, TimeZone};
use cn_model::FixedClock;
use cn_net::mock::MockFetcher;
use cn_store::{ActRequest, CampusService};

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Local.with_ymd_and_hms(2026, 11, 18, 10, 0, 0).single().unwrap(),
    ))
}

fn temp_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cn-pipe-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.join("store.json")
}

fn list_html(n: usize) -> String {
    let mut items = String::new();
    for i in 0..n {
        let day = i % 9 + 1;
        items.push_str(&format!(
            r#"<li><a href="https://one.bjut.edu.cn/info/1/{i}.html">关于第{i}项考试安排的通知</a><span>2026-11-0{day}</span></li>"#
        ));
    }
    format!("<html><body><ul>{items}</ul></body></html>")
}

fn act(op: &str, payload: serde_json::Value) -> ActRequest {
    ActRequest {
        op: op.to_string(),
        payload,
    }
}

#[tokio::test]
async fn b1_single_source_failure_does_not_block_others() {
    let fetcher = MockFetcher::new(|req: &cn_net::FetchRequest| {
        if req.url.contains("bad.") {
            return Err(cn_net::NetError::Timeout);
        }
        Ok(cn_net::FetchResponse {
            status: 200,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "text/html".into())],
            body: list_html(3).into_bytes(),
        })
    });
    let mut svc = CampusService::new_for_test(temp_path("b1"), fetcher, clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "good".into(),
        name: "好源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/good.html".into(),
        enabled: true,
        ..Default::default()
    });
    svc.store.sources.push(cn_store::SourceRecord {
        id: "bad".into(),
        name: "坏源".into(),
        r#type: "html".into(),
        url: "https://bad.example.com/x.html".into(),
        enabled: true,
        ..Default::default()
    });
    let r = svc.act(act("poll", serde_json::json!({}))).await;
    // B1：好源的条目照常入库
    assert!(r.state.notices.len() >= 3, "好源应抓到条目");
    assert!(r.state.notices.iter().all(|n| n.source_id == "good"));
    // 失败源记在 lastPollError
    assert!(r.state.meta.last_poll_error.contains("坏源"));
    assert_eq!(r.state.sources[1].status, "error");
}

#[tokio::test]
async fn b4_max_items_per_source_truncates() {
    let fetcher = MockFetcher::new(|req: &cn_net::FetchRequest| {
        Ok(cn_net::FetchResponse {
            status: 200,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "text/html".into())],
            body: list_html(20).into_bytes(),
        })
    });
    let mut svc = CampusService::new_for_test(temp_path("b4"), fetcher, clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src".into(),
        name: "源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/list.html".into(),
        enabled: true,
        ..Default::default()
    });
    svc.store.settings.max_items_per_source = 5;
    svc.act(act("poll", serde_json::json!({}))).await;
    assert_eq!(svc.store.notices.len(), 5, "B4：maxItemsPerSource 截断");
}

#[tokio::test]
async fn b5_dedupe_across_polls() {
    let fetcher = MockFetcher::new(|req: &cn_net::FetchRequest| {
        Ok(cn_net::FetchResponse {
            status: 200,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "text/html".into())],
            body: list_html(4).into_bytes(),
        })
    });
    let mut svc = CampusService::new_for_test(temp_path("b5"), fetcher, clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src".into(),
        name: "源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/list.html".into(),
        enabled: true,
        ..Default::default()
    });
    svc.act(act("poll", serde_json::json!({}))).await;
    let first = svc.store.notices.len();
    assert!(first >= 4);
    // 第二次抓同内容 → 新增 0 条
    svc.act(act("poll", serde_json::json!({}))).await;
    assert_eq!(svc.store.notices.len(), first, "B5：第二次起新增 0 条");
}

#[tokio::test]
async fn b7_auto_schedule_skips_stale_deadline() {
    // meta.content 场景改走 json 源摘要：构造一条 deadline 已过 1 小时以上的通知
    let json_body = serde_json::json!({
        "list": [
            {"title": "关于过期截止的通知", "date": "2026-11-01",
             "summary": "已于2026年11月18日08:00截止的事项，早已结束。"},
            {"title": "关于未来截止的通知", "date": "2026-11-01",
             "summary": "请于2026年12月1日前提交材料，联系人李老师。"}
        ]
    });
    let fetcher = MockFetcher::new(move |req: &cn_net::FetchRequest| {
        Ok(cn_net::FetchResponse {
            status: 200,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec(&json_body).unwrap(),
        })
    });
    let mut svc = CampusService::new_for_test(temp_path("b7"), fetcher, clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src".into(),
        name: "源".into(),
        r#type: "json".into(),
        url: "https://my.bjut.edu.cn/api/list".into(),
        enabled: true,
        items_path: "list".into(),
        ..Default::default()
    });
    let r = svc.act(act("poll", serde_json::json!({}))).await;
    assert_eq!(r.state.notices.len(), 2);
    // now = 2026-11-18 10:00；过期 deadline = 2026-11-18T08:00（< now-1h）→ 不建
    // 未来 deadline = 2026-12-01T23:59 → 建
    let due_items: Vec<_> = svc.store.items.iter().collect();
    assert_eq!(due_items.len(), 1, "B7：只给未来截止建日程，items={:?}", due_items);
    assert!(due_items[0].due_at.starts_with("2026-12-01"));
}

#[tokio::test]
async fn b11_every_notice_has_summary_and_useful() {
    let fetcher = MockFetcher::new(|req: &cn_net::FetchRequest| {
        Ok(cn_net::FetchResponse {
            status: 200,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "text/html".into())],
            body: list_html(6).into_bytes(),
        })
    });
    let mut svc = CampusService::new_for_test(temp_path("b11"), fetcher, clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src".into(),
        name: "源".into(),
        r#type: "html".into(),
        url: "https://one.bjut.edu.cn/list.html".into(),
        enabled: true,
        ..Default::default()
    });
    svc.act(act("poll", serde_json::json!({}))).await;
    for n in &svc.store.notices {
        assert!(!n.summary.is_empty(), "B11：summary 永不空白（{}）", n.title);
        assert!(!n.useful.is_empty(), "B11：useful 永不空白（{}）", n.title);
        assert!(n.score >= 5 && n.score <= 98);
        assert!(!n.dedupe_key.is_empty());
    }
}

#[tokio::test]
async fn json_source_context_fallback_bodytext() {
    // json 源无详情正文时用列表摘要兜底（bodySource=context 口径）
    let json_body = serde_json::json!({
        "list": [
            {"title": "关于奖学金评审的通知", "date": "2026-11-02",
             "summary": "请于2026年11月25日前提交申请表，涉及评选资助事项。联系人：赵老师 电话：010-67391111。"}
        ]
    });
    let fetcher = MockFetcher::new(move |req: &cn_net::FetchRequest| {
        Ok(cn_net::FetchResponse {
            status: 200,
            url: req.url.clone(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec(&json_body).unwrap(),
        })
    });
    let mut svc = CampusService::new_for_test(temp_path("ctx"), fetcher, clock());
    svc.store.sources.clear();
    svc.store.sources.push(cn_store::SourceRecord {
        id: "src".into(),
        name: "源".into(),
        r#type: "json".into(),
        url: "https://my.bjut.edu.cn/api/list".into(),
        enabled: true,
        items_path: "list".into(),
        ..Default::default()
    });
    svc.act(act("poll", serde_json::json!({}))).await;
    let n = &svc.store.notices[0];
    assert!(!n.deadline.is_empty(), "摘要兜底仍要抽出截止：{}", n.summary);
    assert!(n.deadline.starts_with("2026-11-25"));
    assert!(n.contact.contains("赵老师"), "contact={}", n.contact);
}
