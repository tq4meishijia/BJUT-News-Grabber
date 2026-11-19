//! QA 独立补充的集成边界测试（不改动既有测试、不改动产品代码）：
//! 时间范围（单端/非法/clamp/过滤前置）与附件（开关/大小写/query 去重/同名/级联/兼容）。
//!
//! 命名约定：`*_should_*` 的用例对应 PRD 明确要求，若失败即为源码 Bug；
//! 其余用例锁定已实现行为，防止回归。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{Local, TimeZone};
use cn_model::{AttachmentStatus, FixedClock};
use cn_net::mock::{MockFetcher, Rule};
use cn_net::{FetchRequest, NetError, ATTACHMENT_MAX_BYTES, DEFAULT_MAX_BYTES};
use cn_store::{ActRequest, CampusService, SourceRecord, StoreData};
use serde_json::json;

const LIST_URL: &str = "https://one.bjut.edu.cn/notice.html";

/* ------------------------------- helpers ------------------------------- */

fn clock_at(year: i32, month: u32, day: u32) -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Local
            .with_ymd_and_hms(year, month, day, 10, 0, 0)
            .single()
            .expect("合法日期"),
    ))
}

fn temp_store_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cn-store-qa-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir.join("store.json")
}

fn test_source() -> SourceRecord {
    SourceRecord {
        id: "src-test".into(),
        name: "测试源".into(),
        r#type: "html".into(),
        url: LIST_URL.into(),
        enabled: true,
        cookie: "JSESSIONID=qa".into(),
        ..Default::default()
    }
}

fn service(
    tag: &str,
    fetcher: &Arc<MockFetcher>,
    clock: Arc<FixedClock>,
    attachments: bool,
) -> CampusService {
    let mut svc = CampusService::new_for_test(temp_store_path(tag), fetcher.clone(), clock);
    svc.store.sources.clear();
    svc.store.sources.push(test_source());
    svc.store.settings.llm = false;
    svc.store.settings.fetch_detail = true;
    svc.store.settings.fetch_attachments = attachments;
    svc.store.settings.auto_schedule = false;
    svc
}

fn act(op: &str, payload: serde_json::Value) -> ActRequest {
    ActRequest {
        op: op.to_string(),
        payload,
    }
}

/// 列表页：每条链接用不含数字的长段落隔开，避免「无日期」条目从相邻条目上下文误取日期。
fn list_page(entries: &[(&str, &str)]) -> String {
    let filler = "填充文本用于隔离日期上下文".repeat(20);
    let mut items = String::new();
    for (href, text) in entries {
        items.push_str(&format!(
            "<p>{filler}</p><ul><li><a href=\"{href}\">{text}</a></li></ul>"
        ));
    }
    format!("<html><body>{items}</body></html>")
}

fn detail_page(links: &[&str]) -> String {
    let body = "这里是通知正文内容，足够长以便正文抽取逻辑正常工作并保留下来。".repeat(3);
    let anchors: String = links
        .iter()
        .map(|href| format!("<a href=\"{href}\">附件下载</a>"))
        .collect();
    format!("<html><body><div>{body}</div>{anchors}</body></html>")
}

fn calls(fetcher: &MockFetcher) -> Vec<FetchRequest> {
    fetcher.calls.lock().expect("calls").clone()
}

fn calls_containing(fetcher: &MockFetcher, needle: &str) -> Vec<FetchRequest> {
    calls(fetcher)
        .into_iter()
        .filter(|req| req.url.contains(needle))
        .collect()
}

/// 3 条带日期（09-25/26/27）+ 1 条无日期，与既有测试数据同构但独立构造。
fn four_entries() -> [(&'static str, &'static str); 4] {
    [
        ("https://one.bjut.edu.cn/info/1.htm", "2026-09-25 关于甲类事项的通知"),
        ("https://one.bjut.edu.cn/info/2.htm", "2026-09-26 关于乙类事项的通知"),
        ("https://one.bjut.edu.cn/info/3.htm", "2026-09-27 关于丙类事项的通知"),
        ("https://one.bjut.edu.cn/info/4.htm", "关于无日期事项的通知"),
    ]
}

fn rules_for_four(detail_links: &[&str]) -> Vec<Rule> {
    let entries = four_entries();
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    for path in ["/info/1.htm", "/info/2.htm", "/info/3.htm", "/info/4.htm"] {
        rules.push(Rule::get(path, 200, &detail_page(detail_links)));
    }
    rules
}

/* ---------------------------- 时间范围边界 ---------------------------- */

#[tokio::test]
async fn one_sided_temporary_ranges_filter_correctly() {
    // 只填 start：另一端不限（含空日期保留）。
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("range-start-only", &fetcher, clock_at(2026, 11, 18), false);
    let result = svc
        .act(act("poll", json!({"dateRange": {"start": "2026-09-26", "end": ""}})))
        .await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(result.state.notices.len(), 3, "只填 start：09-26、09-27、空日期");
    assert!(result
        .state
        .notices
        .iter()
        .all(|n| n.published_at.is_empty() || n.published_at.as_str() >= "2026-09-26"));
    assert!(
        result.message.as_deref().unwrap_or("").contains("时间范围跳过 1 条"),
        "{:?}",
        result.message
    );

    // 只填 end：另一端不限。
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("range-end-only", &fetcher, clock_at(2026, 11, 18), false);
    let result = svc
        .act(act("poll", json!({"dateRange": {"start": "", "end": "2026-09-26"}})))
        .await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(result.state.notices.len(), 3, "只填 end：09-25、09-26、空日期");
    assert!(result
        .state
        .notices
        .iter()
        .all(|n| n.published_at.is_empty() || n.published_at.as_str() <= "2026-09-26"));
    assert!(result.message.as_deref().unwrap_or("").contains("时间范围跳过 1 条"));
}

#[tokio::test]
async fn settings_default_range_is_used_when_poll_omits_range() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("settings-default-range", &fetcher, clock_at(2026, 11, 18), false);

    let saved = svc
        .act(act(
            "settings.save",
            json!({"settings": {"dateRangeStart": "2026-09-27"}}),
        ))
        .await;
    assert!(saved.ok, "err={:?}", saved.error);
    assert_eq!(svc.store.settings.date_range_start, "2026-09-27");

    // 不带 dateRange 的 poll 必须使用设置页默认范围。
    let first = svc.act(act("poll", json!({}))).await;
    assert!(first.ok, "err={:?}", first.error);
    assert_eq!(first.state.notices.len(), 2, "默认 start=09-27：09-27 + 空日期");
    assert!(
        first.message.as_deref().unwrap_or("").contains("时间范围跳过 2 条"),
        "{:?}",
        first.message
    );

    // A6：清空范围后恢复改动前行为，此前跳过的重新入库。
    let cleared = svc
        .act(act(
            "settings.save",
            json!({"settings": {"dateRangeStart": "", "dateRangeEnd": ""}}),
        ))
        .await;
    assert!(cleared.ok, "err={:?}", cleared.error);
    let second = svc.act(act("poll", json!({}))).await;
    assert_eq!(second.state.notices.len(), 4, "清空范围后全量入库");
    assert_eq!(calls_containing(&fetcher, "/info/").len(), 4, "每条通知只抓一次详情");
}

#[tokio::test]
async fn invalid_ranges_block_poll_before_any_network_request() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("invalid-temp-range", &fetcher, clock_at(2026, 11, 18), false);

    let cases = [
        ("2026-02-30", "", cn_core::INVALID_DATE_MESSAGE),
        ("", "2026/09/25", cn_core::INVALID_DATE_MESSAGE),
        ("2026-09-1", "", cn_core::INVALID_DATE_MESSAGE),
        (
            "2026-09-10",
            "2026-09-01",
            cn_core::START_AFTER_END_MESSAGE,
        ),
        ("2027-01-01", "", cn_core::START_AFTER_TODAY_MESSAGE),
    ];
    for (start, end, expected) in cases {
        let result = svc
            .act(act(
                "poll",
                json!({"dateRange": {"start": start, "end": end}}),
            ))
            .await;
        assert!(!result.ok, "{start:?}..{end:?} 必须被拒绝");
        assert_eq!(
            result.error.as_deref(),
            Some(expected),
            "{start:?}..{end:?} 的错误文案"
        );
        assert_eq!(fetcher.call_count(), 0, "校验失败不得发出任何网络请求");
    }

    // end 晚于今天合法（clamp），不报错。
    let ok = svc
        .act(act(
            "poll",
            json!({"dateRange": {"start": "", "end": "2099-12-31"}}),
        ))
        .await;
    assert!(ok.ok, "未来结束日期允许: {:?}", ok.error);
    assert_eq!(ok.state.notices.len(), 4);
}

#[tokio::test]
async fn end_after_today_is_clamped_to_today_without_error() {
    let entries = [
        ("https://one.bjut.edu.cn/info/1.htm", "2026-12-01 关于未来事项的通知"),
        ("https://one.bjut.edu.cn/info/2.htm", "2026-09-25 关于过去事项的通知"),
        ("https://one.bjut.edu.cn/info/3.htm", "关于无日期事项的通知"),
    ];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    for path in ["/info/1.htm", "/info/2.htm", "/info/3.htm"] {
        rules.push(Rule::get(path, 200, &detail_page(&[])));
    }
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("clamp-future-end", &fetcher, clock_at(2026, 11, 18), false);

    let result = svc
        .act(act(
            "poll",
            json!({"dateRange": {"start": "", "end": "2099-12-31"}}),
        ))
        .await;
    assert!(result.ok, "结束晚于今天不应报错: {:?}", result.error);
    assert_eq!(result.state.notices.len(), 2, "clamp 到今天后：09-25 + 空日期");
    assert!(result
        .state
        .notices
        .iter()
        .all(|n| n.published_at != "2026-12-01"));
    assert!(
        result.message.as_deref().unwrap_or("").contains("时间范围跳过 1 条"),
        "{:?}",
        result.message
    );
    assert!(
        calls_containing(&fetcher, "/info/1.htm").is_empty(),
        "范围外通知不得请求详情"
    );
}

#[tokio::test]
async fn out_of_range_notices_get_no_dedupe_key_and_return_later() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("dedupe-after-filter", &fetcher, clock_at(2026, 11, 18), false);

    let first = svc
        .act(act(
            "poll",
            json!({"dateRange": {"start": "2026-09-25", "end": "2026-09-25"}}),
        ))
        .await;
    assert!(first.ok, "err={:?}", first.error);
    assert_eq!(first.state.notices.len(), 2, "09-25 + 空日期");
    assert!(first
        .state
        .notices
        .iter()
        .all(|n| n.published_at != "2026-09-26" && n.published_at != "2026-09-27"));

    // 范围外通知未建立去重键 → 换回不限范围后应重新入库。
    let second = svc.act(act("poll", json!({}))).await;
    assert!(second.ok, "err={:?}", second.error);
    assert_eq!(second.state.notices.len(), 4);
    let keys: HashSet<&str> = second
        .state
        .notices
        .iter()
        .map(|n| n.dedupe_key.as_str())
        .collect();
    assert_eq!(keys.len(), 4, "4 条各一个去重键，无重复入库");

    // 已在库的两条不得再次请求详情（去重生效）。
    for path in ["/info/1.htm", "/info/2.htm", "/info/3.htm", "/info/4.htm"] {
        assert_eq!(
            calls_containing(&fetcher, path).len(),
            1,
            "{path} 只应请求一次"
        );
    }
}

/* ------------------------------ 附件边界 ------------------------------ */

#[tokio::test]
async fn attachments_off_sends_zero_attachment_requests() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&["/files/qa.pdf"]));
    let mut svc = service("attachments-off", &fetcher, clock_at(2026, 11, 18), false);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(result.state.notices.len(), 4);

    let urls: Vec<String> = calls(&fetcher).into_iter().map(|r| r.url).collect();
    assert!(
        calls_containing(&fetcher, "/files/").is_empty(),
        "开关关闭时不得发出任何附件请求: {urls:?}"
    );
    assert!(svc.store.attachments.is_empty());
    assert!(result.state.attachments.is_empty());
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        4,
        "正文抓取不受开关影响"
    );
}

#[tokio::test]
async fn turning_attachments_off_keeps_records_and_downloads_nothing_new() {
    let entries = [
        ("https://one.bjut.edu.cn/info/1.htm", "2026-09-25 关于附件开关的通知"),
        ("https://one.bjut.edu.cn/info/2.htm", "2026-09-26 关于附件开关的通知二"),
    ];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    for path in ["/info/1.htm", "/info/2.htm"] {
        rules.push(Rule::get(path, 200, &detail_page(&["/files/shared.pdf"])));
    }
    rules.push(Rule::get("/files/shared.pdf", 200, "PDF-BYTES"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("attachments-toggle", &fetcher, clock_at(2026, 11, 18), true);

    let first = svc
        .act(act(
            "poll",
            json!({"dateRange": {"start": "2026-09-25", "end": "2026-09-25"}}),
        ))
        .await;
    assert!(first.ok, "err={:?}", first.error);
    assert_eq!(first.state.notices.len(), 1);
    assert_eq!(svc.store.attachments.len(), 1);
    assert_eq!(calls_containing(&fetcher, "/files/").len(), 1);

    let off = svc
        .act(act(
            "settings.save",
            json!({"settings": {"fetchAttachments": false}}),
        ))
        .await;
    assert!(off.ok, "err={:?}", off.error);
    assert!(!svc.store.settings.fetch_attachments);

    // 关闭后新通知入库：不得解析/下载附件，也不得删除已有附件（B6）。
    let second = svc.act(act("poll", json!({}))).await;
    assert!(second.ok, "err={:?}", second.error);
    assert_eq!(second.state.notices.len(), 2, "关闭开关不影响通知抓取");
    assert_eq!(
        calls_containing(&fetcher, "/files/").len(),
        1,
        "关闭后不得新增附件请求"
    );
    assert_eq!(svc.store.attachments.len(), 1, "关闭开关不删除已有记录");
    let record = &svc.store.attachments[0];
    assert_eq!(record.status, AttachmentStatus::Downloaded);
    assert!(
        svc.attachments_dir().join(&record.local_path).is_file(),
        "已有本地文件不得被删除"
    );
}

#[tokio::test]
async fn uppercase_extensions_are_downloaded_and_normalized() {
    let entries = [("https://one.bjut.edu.cn/info/1.htm", "关于附件大小写测试的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get(
        "/info/1.htm",
        200,
        &detail_page(&[
            "/files/REPORT.PDF",
            "/files/Pic.Jpg",
            "/files/tool.EXE",
            "/files/noext",
        ]),
    ));
    rules.push(Rule::get("/files/REPORT.PDF", 200, "PDF-DATA"));
    rules.push(Rule::get("/files/Pic.Jpg", 200, "JPG-DATA"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("uppercase-ext", &fetcher, clock_at(2026, 11, 18), true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(
        svc.store.attachments.len(),
        2,
        "大写扩展名应识别，exe/无扩展名不抓: {:?}",
        svc.store.attachments
    );
    let mut names: Vec<&str> = svc
        .store
        .attachments
        .iter()
        .map(|a| a.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["Pic.jpg", "REPORT.pdf"]);

    for record in &svc.store.attachments {
        assert_eq!(record.status, AttachmentStatus::Downloaded);
        assert!(record.error.is_empty());
        assert!(
            record.local_path.ends_with(".pdf") || record.local_path.ends_with(".jpg"),
            "落盘扩展名应小写: {}",
            record.local_path
        );
        let file = svc.attachments_dir().join(&record.local_path);
        assert!(file.is_file(), "{}", file.to_string_lossy());
        assert_eq!(std::fs::read(&file).unwrap().len() as u64, record.size_bytes);
    }
    assert!(
        calls_containing(&fetcher, "tool.EXE").is_empty(),
        "exe 不在白名单，不得请求"
    );
    assert!(
        calls_containing(&fetcher, "noext").is_empty(),
        "无扩展名不得请求"
    );
}

#[tokio::test]
async fn query_variants_share_one_record_per_normalized_url() {
    let entries = [("https://one.bjut.edu.cn/info/1.htm", "关于附件 query 去重的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get(
        "/info/1.htm",
        200,
        &detail_page(&[
            "/files/q.pdf?utm_source=a",
            "/files/q.pdf?utm_source=b",
            "/files/q.pdf?b=1",
            "/files/q.pdf?b=1#frag",
        ]),
    ));
    rules.push(Rule::get("/files/q.pdf", 200, "Q-BYTES"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("query-dedupe", &fetcher, clock_at(2026, 11, 18), true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(
        svc.store.attachments.len(),
        2,
        "tracking/fragment 变体合并，有意义的 ?b=1 独立: {:?}",
        svc.store.attachments
    );
    let norms: HashSet<&str> = svc
        .store
        .attachments
        .iter()
        .map(|a| a.norm_url.as_str())
        .collect();
    assert!(norms.contains("https://one.bjut.edu.cn/files/q.pdf"));
    assert!(norms.contains("https://one.bjut.edu.cn/files/q.pdf?b=1"));
    assert_eq!(
        calls_containing(&fetcher, "/files/q.pdf").len(),
        2,
        "同一规范化 URL 只下载一次"
    );
    for record in &svc.store.attachments {
        assert_eq!(record.status, AttachmentStatus::Downloaded);
        assert_eq!(record.notice_ids.len(), 1);
    }
}

#[tokio::test]
async fn same_file_name_from_different_urls_keeps_both_files() {
    let entries = [("https://one.bjut.edu.cn/info/1.htm", "关于同名附件测试的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get(
        "/info/1.htm",
        200,
        &detail_page(&["/files/a/report.pdf", "/files/b/report.pdf"]),
    ));
    rules.push(Rule::get("/files/a/report.pdf", 200, "A-BYTES"));
    rules.push(Rule::get("/files/b/report.pdf", 200, "B-BYTES"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("same-name", &fetcher, clock_at(2026, 11, 18), true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(
        svc.store.attachments.len(),
        2,
        "同名不同 URL 必须各存一条: {:?}",
        svc.store.attachments
    );
    assert!(svc
        .store
        .attachments
        .iter()
        .all(|a| a.file_name == "report.pdf"));
    let paths: HashSet<&str> = svc
        .store
        .attachments
        .iter()
        .map(|a| a.local_path.as_str())
        .collect();
    assert_eq!(paths.len(), 2, "落盘文件名必须带 hash 区分，不能互相覆盖");
    for record in &svc.store.attachments {
        assert_eq!(record.status, AttachmentStatus::Downloaded);
        let file = svc.attachments_dir().join(&record.local_path);
        assert!(file.is_file(), "{}", file.to_string_lossy());
    }
}

#[tokio::test]
async fn duplicate_url_in_one_notice_downloads_once() {
    let entries = [("https://one.bjut.edu.cn/info/1.htm", "关于重复附件链接的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get(
        "/info/1.htm",
        200,
        &detail_page(&[
            "/files/dup.pdf",
            "/files/dup.pdf",
            "/files/dup.pdf",
        ]),
    ));
    rules.push(Rule::get("/files/dup.pdf", 200, "DUP-BYTES"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("dup-url", &fetcher, clock_at(2026, 11, 18), true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(svc.store.attachments.len(), 1, "同一 URL 只建一条记录");
    assert_eq!(
        calls_containing(&fetcher, "/files/dup.pdf").len(),
        1,
        "同一通知内重复 URL 只下载一次"
    );
}

#[tokio::test]
async fn cascade_removes_shared_attachment_only_after_last_reference() {
    let entries = [
        ("https://one.bjut.edu.cn/info/1.htm", "关于共享附件甲的通知"),
        ("https://one.bjut.edu.cn/info/2.htm", "关于共享附件乙的通知"),
    ];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get(
        "/info/1.htm",
        200,
        &detail_page(&["/files/shared.pdf", "/files/only1.pdf"]),
    ));
    rules.push(Rule::get("/info/2.htm", 200, &detail_page(&["/files/shared.pdf"])));
    rules.push(Rule::get("/files/shared.pdf", 200, "SHARED"));
    rules.push(Rule::get("/files/only1.pdf", 200, "ONLY1"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("cascade-shared", &fetcher, clock_at(2026, 11, 18), true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(svc.store.attachments.len(), 2);

    let dir = svc.attachments_dir();
    let shared = svc
        .store
        .attachments
        .iter()
        .find(|a| a.file_name == "shared.pdf")
        .cloned()
        .expect("shared 记录");
    let only1 = svc
        .store
        .attachments
        .iter()
        .find(|a| a.file_name == "only1.pdf")
        .cloned()
        .expect("only1 记录");
    assert_eq!(shared.notice_ids.len(), 2, "两通知共享同一条记录");
    let shared_file = dir.join(&shared.local_path);
    let only1_file = dir.join(&only1.local_path);
    assert!(shared_file.is_file() && only1_file.is_file());

    let first_id = svc
        .store
        .notices
        .iter()
        .find(|n| n.url.ends_with("/info/1.htm"))
        .map(|n| n.id.clone())
        .expect("notice 1");
    let second_id = svc
        .store
        .notices
        .iter()
        .find(|n| n.url.ends_with("/info/2.htm"))
        .map(|n| n.id.clone())
        .expect("notice 2");

    // 删第一个引用：独享附件删除，共享附件保留。
    svc.act(act("notice.remove", json!({"id": first_id}))).await;
    assert!(!only1_file.exists(), "最后一个（也是唯一）引用删除后文件应删除");
    assert!(
        svc.store.attachments.iter().all(|a| a.id != only1.id),
        "独享附件记录应删除"
    );
    let shared_after = svc
        .store
        .attachments
        .iter()
        .find(|a| a.id == shared.id)
        .expect("共享附件仍在");
    assert_eq!(shared_after.notice_ids.len(), 1);
    assert!(shared_file.is_file(), "仍被别的通知引用，文件不能删");

    // 删最后一个引用：共享附件文件与记录一并删除，目录无孤儿。
    svc.act(act("notice.remove", json!({"id": second_id}))).await;
    assert!(!shared_file.exists(), "最后一个引用删除后应删文件");
    assert!(svc.store.attachments.is_empty());
    assert_eq!(
        std::fs::read_dir(&dir).map(|it| it.count()).unwrap_or(0),
        0,
        "attachments 目录不得留下孤儿文件"
    );
}

#[tokio::test]
async fn poll_message_reports_attachment_counts() {
    let mut rules = rules_for_four(&["/files/notice.pdf"]);
    rules.push(Rule::get("/files/notice.pdf", 200, "PDF"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("attachment-counts", &fetcher, clock_at(2026, 11, 18), true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(svc.store.attachments.len(), 1);
    assert_eq!(
        svc.store.attachments[0].notice_ids.len(),
        4,
        "4 条通知共享同一附件"
    );
    let message = result.message.unwrap_or_default();
    assert!(
        message.contains("附件成功 1、失败 0、跳过 3"),
        "B14 结果反馈缺附件统计: {message}"
    );
}

/* ------------------------------ 兼容性 ------------------------------ */

#[tokio::test]
async fn legacy_store_json_without_new_fields_polls_normally() {
    let path = temp_store_path("legacy-store");
    let legacy = json!({
        "version": 1,
        "settings": {"pollMinutes": 30, "llm": false},
        "sources": [{
            "id": "src-test",
            "name": "测试源",
            "type": "html",
            "url": LIST_URL,
            "enabled": true,
            "cookie": "JSESSIONID=qa"
        }],
        "notices": [],
        "items": [],
        "meta": {"lastPollAt": "", "lastPollError": "", "pollCount": 0, "reminders": []}
    });
    std::fs::write(&path, serde_json::to_string_pretty(&legacy).unwrap()).expect("write legacy");

    let mut rules = rules_for_four(&["/files/notice.pdf"]);
    rules.push(Rule::get("/files/notice.pdf", 200, "PDF"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = CampusService::new_for_test(path, fetcher.clone(), clock_at(2026, 11, 18));

    // 新字段全部取默认值，不 panic。
    assert_eq!(svc.store.settings.date_range_start, "");
    assert_eq!(svc.store.settings.date_range_end, "");
    assert!(!svc.store.settings.fetch_attachments);
    assert!(svc.store.attachments.is_empty());
    assert!(svc.state().attachments.is_empty());

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "旧 store 加载后应能正常抓取: {:?}", result.error);
    assert_eq!(result.state.notices.len(), 4);
    assert!(svc.store.attachments.is_empty(), "默认开关关闭，不抓附件");
    assert!(calls_containing(&fetcher, "/files/").is_empty());
}

#[test]
fn partial_store_json_defaults_new_fields() {
    let data: StoreData =
        serde_json::from_str(r#"{"version":1,"settings":{"dateRangeStart":"2026-09-01"}}"#)
            .expect("parse partial");
    assert_eq!(data.settings.date_range_start, "2026-09-01");
    assert_eq!(data.settings.date_range_end, "");
    assert!(!data.settings.fetch_attachments);
    assert!(data.attachments.is_empty());

    let empty: StoreData = serde_json::from_str("{}").expect("parse empty");
    assert_eq!(empty.settings.date_range_start, "");
    assert_eq!(empty.settings.date_range_end, "");
    assert!(!empty.settings.fetch_attachments);
    assert!(empty.attachments.is_empty());
}

#[test]
fn attachment_and_normal_limits_are_unchanged() {
    assert_eq!(DEFAULT_MAX_BYTES, 6 * 1024 * 1024, "普通请求仍为 6 MiB");
    assert_eq!(ATTACHMENT_MAX_BYTES, 25 * 1024 * 1024, "附件请求放宽到 25 MiB");
}

/* --------------------- PRD 判定用例（失败即源码 Bug） --------------------- */

/// PRD A1/A3 + 用户故事「每次抓取只看该日期之后的新通知」：
/// 设置页默认范围必须约束**所有**抓取入口，托盘/定时器走 `CampusService::poll(reason)`，
/// 不能因为不带临时范围就绕过默认范围。
#[tokio::test]
async fn scheduled_timer_poll_should_respect_default_date_range() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("timer-default-range", &fetcher, clock_at(2026, 11, 18), false);

    let saved = svc
        .act(act(
            "settings.save",
            json!({"settings": {"dateRangeStart": "2026-09-27"}}),
        ))
        .await;
    assert!(saved.ok, "err={:?}", saved.error);

    let outcome = svc.poll("timer").await;
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(
        outcome.added, 2,
        "定时抓取应只入库 09-27 + 空日期（2 条）；实际 added={} date_skipped={}，\
         说明 poll(reason) 传 None 绕过了设置页默认范围",
        outcome.added, outcome.date_skipped
    );
    assert_eq!(svc.store.notices.len(), 2);
}

/// PRD §4.1「结束日期 > 今天 → 允许（实际上界按今天处理）」+ 验收「设置页可保存默认起止日期」：
/// clamp 只应作用于「本次抓取的上界」，不能把用户输入改写成保存当天再持久化。
#[tokio::test]
async fn saving_future_default_end_should_keep_user_value() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("future-end-save", &fetcher, clock_at(2026, 11, 18), false);

    let saved = svc
        .act(act(
            "settings.save",
            json!({"settings": {"dateRangeStart": "", "dateRangeEnd": "2099-12-31"}}),
        ))
        .await;
    assert!(saved.ok, "结束日期晚于今天应允许保存: {:?}", saved.error);

    assert_eq!(
        svc.store.settings.date_range_end, "2099-12-31",
        "用户输入的结束日期必须原样保存（clamp 应在每次抓取时做，而不是持久化时改写；\
         否则次日轮询的 end 会冻结在保存当天，之后发布的通知全部被过滤）"
    );
    assert_eq!(saved.state.settings.date_range_end, "2099-12-31");
}

/// 上一条 Bug 的用户可见后果：保存未来结束日期后，次日轮询漏掉当天新通知。
#[tokio::test]
async fn next_day_poll_should_not_miss_notices_after_saving_future_end() {
    let path = temp_store_path("next-day-miss");
    let day1_fetcher =
        MockFetcher::with_rules(vec![Rule::get("/notice.html", 200, &list_page(&[]))]);
    let mut day1 = CampusService::new_for_test(path.clone(), day1_fetcher, clock_at(2026, 11, 18));
    day1.store.sources.clear();
    day1.store.sources.push(test_source());
    day1.store.settings.llm = false;
    day1.store.settings.auto_schedule = false;
    let saved = day1
        .act(act(
            "settings.save",
            json!({"settings": {"dateRangeEnd": "2099-12-31"}}),
        ))
        .await;
    assert!(saved.ok, "err={:?}", saved.error);

    let entries = [("https://one.bjut.edu.cn/info/9.htm", "2026-11-19 关于次日新发布的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get("/info/9.htm", 200, &detail_page(&[])));
    let day2_fetcher = MockFetcher::with_rules(rules);
    let mut day2 = CampusService::new_for_test(path, day2_fetcher, clock_at(2026, 11, 19));

    let result = day2.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(
        result.state.notices.len(),
        1,
        "保存未来结束日期后，次日轮询不应漏掉当天新通知；\
         实际 0 条说明 end 已被持久化成保存当天 2026-11-18"
    );
}

/// 对照用例：只要 end 原样存的是 2099-12-31，抓取时 clamp 到今天就能正确保留当天通知。
#[tokio::test]
async fn runtime_clamp_keeps_same_day_notice_when_stored_end_is_future() {
    let entries = [("https://one.bjut.edu.cn/info/9.htm", "2026-11-19 关于当天新发布的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get("/info/9.htm", 200, &detail_page(&[])));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("runtime-clamp", &fetcher, clock_at(2026, 11, 19), false);
    // 模拟「原样持久化未来结束日期」的正确存储形态。
    svc.store.settings.date_range_end = "2099-12-31".to_string();

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(result.state.notices.len(), 1, "抓取时 clamp 到今天，当天通知必须保留");
    let message = result.message.unwrap_or_default();
    assert!(!message.contains("时间范围跳过"), "当天通知不应被跳过: {message}");
}

/// 口径回归：同一 norm_url 在一次 poll 内跨通知只实际尝试一次（共 3 次：首次 + 重试 2 次），
/// 不会因为多条通知引用同一 URL 而每条各重试 3 次；记录仍被多条通知共享。
#[tokio::test]
async fn shared_attachment_failure_retries_three_times_total_across_notices() {
    let entries = [
        ("https://one.bjut.edu.cn/info/1.htm", "关于共享失败附件甲的通知"),
        ("https://one.bjut.edu.cn/info/2.htm", "关于共享失败附件乙的通知"),
    ];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get("/info/1.htm", 200, &detail_page(&["/files/slow.pdf"])));
    rules.push(Rule::get("/info/2.htm", 200, &detail_page(&["/files/slow.pdf"])));

    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = attempts.clone();
    let fetcher = MockFetcher::new(move |req: &FetchRequest| {
        if req.url.contains("/files/") {
            counter.fetch_add(1, Ordering::SeqCst);
            return Err(NetError::Timeout);
        }
        for rule in &rules {
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
    let mut svc = service("shared-failure-attempts", &fetcher, clock_at(2026, 11, 18), true);

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "附件失败不得阻断通知入库: {:?}", result.error);
    assert_eq!(result.state.notices.len(), 2);
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        3,
        "共享附件本轮只重试 3 次（首次 + 2 次重试），不能每条通知各来一遍"
    );
    assert_eq!(svc.store.attachments.len(), 1);
    let record = &svc.store.attachments[0];
    assert_eq!(record.status, AttachmentStatus::Failed);
    assert_eq!(record.notice_ids.len(), 2, "失败记录仍被两条通知共享");
    assert!(!record.error.is_empty());
    assert_eq!(
        std::fs::read_dir(svc.attachments_dir()).map(|it| it.count()).unwrap_or(0),
        0,
        "失败不得落盘"
    );
}

/// 防御性口径：store.json 被手改出非法默认范围（start > end）时，
/// 定时/托盘抓取应降级为不过滤并正常入库，绝不 panic。
#[tokio::test]
async fn invalid_stored_default_range_degrades_to_no_filter_without_panic() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("invalid-stored-range", &fetcher, clock_at(2026, 11, 18), false);
    svc.store.settings.date_range_start = "2026-09-10".to_string();
    svc.store.settings.date_range_end = "2026-09-01".to_string();

    let outcome = svc.poll("timer").await;
    assert!(outcome.ok, "非法默认范围应降级而不是失败: {}", outcome.message);
    assert_eq!(svc.store.notices.len(), 4, "降级为不过滤，全量入库");
}

/// `act('poll')` 显式传 `dateRange:{start:"",end:""}` 表示「本次不限」，
/// 不能被设置页默认范围回填覆盖（临时范围优先级最高）。
#[tokio::test]
async fn explicit_empty_temporary_range_overrides_settings_default() {
    let fetcher = MockFetcher::with_rules(rules_for_four(&[]));
    let mut svc = service("empty-override", &fetcher, clock_at(2026, 11, 18), false);
    let saved = svc
        .act(act(
            "settings.save",
            json!({"settings": {"dateRangeStart": "2026-09-27"}}),
        ))
        .await;
    assert!(saved.ok, "err={:?}", saved.error);

    let result = svc
        .act(act("poll", json!({"dateRange": {"start": "", "end": ""}})))
        .await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(
        result.state.notices.len(),
        4,
        "显式空范围 = 本次不限，应全量入库（不得回落默认 start=09-27）"
    );
}

/* ------------------------ T06 附件追溯补抓 ------------------------ */

/// 首轮关闭附件开关入库 → 第二轮开启后再 poll（fresh 为空）：
/// 必须对历史通知补抓（重新请求详情 HTML 提取链接）并产出附件记录。
#[tokio::test]
async fn enabling_attachments_later_backfills_history_notices() {
    let entries = [
        ("https://one.bjut.edu.cn/info/1.htm", "2026-09-25 关于历史附件甲的通知"),
        ("https://one.bjut.edu.cn/info/2.htm", "2026-09-26 关于历史附件乙的通知"),
    ];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    for path in ["/info/1.htm", "/info/2.htm"] {
        rules.push(Rule::get(path, 200, &detail_page(&["/files/history.pdf"])));
    }
    rules.push(Rule::get("/files/history.pdf", 200, "HISTORY-PDF"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("backfill-history", &fetcher, clock_at(2026, 11, 18), false);

    // 首轮：附件开关关闭，只入库（fetchDetail 开，2 次详情请求）。
    let first = svc.act(act("poll", json!({}))).await;
    assert!(first.ok, "err={:?}", first.error);
    assert_eq!(first.state.notices.len(), 2);
    assert!(svc.store.attachments.is_empty(), "首轮不应有附件记录");
    assert_eq!(calls_containing(&fetcher, "/info/").len(), 2);
    assert!(calls_containing(&fetcher, "/files/").is_empty());

    // 第二轮：开启附件开关。列表条目未变 → fresh 为空，走历史补抓。
    let on = svc
        .act(act(
            "settings.save",
            json!({"settings": {"fetchAttachments": true}}),
        ))
        .await;
    assert!(on.ok, "err={:?}", on.error);
    let second = svc.act(act("poll", json!({}))).await;
    assert!(second.ok, "err={:?}", second.error);
    assert_eq!(second.state.notices.len(), 2, "不得重复入库");
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        4,
        "历史补抓必须重新请求详情 HTML（首轮 2 + 补抓 2）"
    );
    assert_eq!(
        svc.store.attachments.len(),
        1,
        "历史补抓应产出附件记录: {:?}",
        svc.store.attachments
    );
    let record = &svc.store.attachments[0];
    assert_eq!(record.status, AttachmentStatus::Downloaded);
    assert_eq!(record.notice_ids.len(), 2, "两条历史通知共享同一附件记录");
    assert_eq!(
        calls_containing(&fetcher, "/files/history.pdf").len(),
        1,
        "同一规范化 URL 本轮只下载一次"
    );
}

/// 附件开关关闭时，第二轮 poll 不应对历史通知发出任何详情 / 附件请求。
#[tokio::test]
async fn attachments_off_never_backfills_history() {
    let entries = [("https://one.bjut.edu.cn/info/1.htm", "2026-09-25 关于历史通知的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get("/info/1.htm", 200, &detail_page(&["/files/a.pdf"])));
    rules.push(Rule::get("/files/a.pdf", 200, "A"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("no-backfill", &fetcher, clock_at(2026, 11, 18), false);
    svc.store.settings.fetch_detail = false;

    let first = svc.act(act("poll", json!({}))).await;
    assert!(first.ok, "err={:?}", first.error);
    assert_eq!(first.state.notices.len(), 1);
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        0,
        "详情与附件都关闭时不得请求详情"
    );

    // 第二轮仍关闭：fresh 为空，候选集必须为空（不得为历史通知发请求）。
    let second = svc.act(act("poll", json!({}))).await;
    assert!(second.ok, "err={:?}", second.error);
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        0,
        "附件开关关闭时不得补抓历史通知"
    );
    assert!(
        calls_containing(&fetcher, "/files/").is_empty(),
        "附件开关关闭时不得发出附件请求"
    );
    assert!(svc.store.attachments.is_empty());
}

/// 历史补抓数量受 limit（detail_limit clamp 10–40）约束，避免 271 条一次拖死。
#[tokio::test]
async fn history_backfill_respects_detail_limit() {
    let fetcher =
        MockFetcher::with_rules(vec![Rule::get("/notice.html", 200, &list_page(&[]))]);
    let mut svc = service("backfill-limit", &fetcher, clock_at(2026, 11, 18), true);
    svc.store.settings.fetch_detail = false;
    svc.store.settings.detail_limit = 10;
    // 手工塞 30 条历史通知（均无附件记录，url 指向不存在的详情 → 404 静默跳过）。
    for i in 0..30 {
        svc.store.notices.push(cn_store::NoticeRecord {
            id: format!("n-hist-{i}"),
            title: format!("历史通知 {i}"),
            url: format!("https://one.bjut.edu.cn/info/{i}.htm"),
            source_id: "src-test".into(),
            source_name: "测试源".into(),
            published_at: format!("2026-09-{:02}", (i % 28) + 1),
            ..Default::default()
        });
    }

    let result = svc.act(act("poll", json!({}))).await;
    assert!(result.ok, "err={:?}", result.error);
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        10,
        "历史补抓条数必须等于 limit（10），实际请求数不符"
    );
}

/// detail 关闭 + 附件开启：历史补抓仍拉 HTML（附件链接只能从 HTML 提取），
/// 但不得改写已入库通知的 content（尊重用户关闭详情跟进的设置）。
#[tokio::test]
async fn backfill_fetches_html_but_keeps_content_when_detail_disabled() {
    let entries = [("https://one.bjut.edu.cn/info/1.htm", "2026-09-25 关于正文设置边界的通知")];
    let mut rules = vec![Rule::get("/notice.html", 200, &list_page(&entries))];
    rules.push(Rule::get("/info/1.htm", 200, &detail_page(&["/files/doc.pdf"])));
    rules.push(Rule::get("/files/doc.pdf", 200, "DOC"));
    let fetcher = MockFetcher::with_rules(rules);
    let mut svc = service("backfill-no-content", &fetcher, clock_at(2026, 11, 18), false);
    svc.store.settings.fetch_detail = false;

    // 首轮：detail 关 + 附件关 → 只入库，不请求详情。
    let first = svc.act(act("poll", json!({}))).await;
    assert!(first.ok, "err={:?}", first.error);
    assert_eq!(calls_containing(&fetcher, "/info/").len(), 0);
    let content_before: std::collections::HashMap<String, String> = svc
        .store
        .notices
        .iter()
        .map(|n| (n.id.clone(), n.content.clone()))
        .collect();

    // 第二轮：只开附件 → 补抓必须拉 HTML，但 content 保持原样。
    let on = svc
        .act(act(
            "settings.save",
            json!({"settings": {"fetchAttachments": true}}),
        ))
        .await;
    assert!(on.ok, "err={:?}", on.error);
    let second = svc.act(act("poll", json!({}))).await;
    assert!(second.ok, "err={:?}", second.error);
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        1,
        "附件补抓必须请求详情 HTML"
    );
    assert_eq!(svc.store.attachments.len(), 1);
    assert_eq!(svc.store.attachments[0].status, AttachmentStatus::Downloaded);
    for notice in &svc.store.notices {
        assert_eq!(
            content_before.get(&notice.id).cloned().unwrap_or_default(),
            notice.content,
            "详情跟进关闭时不得改写 content（id={}）",
            notice.id
        );
    }
}

/* ---------------------- T06b 附件补抓负缓存（扫描标记） ---------------------- */

/// 无附件的通知被扫描一次后写入 `attachmentsScannedAt`，下一轮不得重复选中。
#[tokio::test]
async fn scanned_notices_are_not_reselected_next_poll() {
    // 详情页没有任何白名单附件链接（这是生产环境「正文无附件」的真实形态）。
    let fetcher =
        MockFetcher::with_rules(vec![Rule::get("/notice.html", 200, &list_page(&[]))]);
    let mut svc = service("scanned-negative-cache", &fetcher, clock_at(2026, 11, 18), true);
    svc.store.settings.fetch_detail = false;
    for i in 0..3 {
        svc.store.notices.push(cn_store::NoticeRecord {
            id: format!("n-scan-{i}"),
            title: format!("无附件历史通知 {i}"),
            url: format!("https://one.bjut.edu.cn/info/{i}.htm"),
            source_id: "src-test".into(),
            source_name: "测试源".into(),
            published_at: format!("2026-09-{:02}", i + 1),
            ..Default::default()
        });
    }

    // 第一轮：3 条都从未扫描 → 全部被选中（详情 404 → bundle 为空，仍必须记扫描）。
    let first = svc.act(act("poll", json!({}))).await;
    assert!(first.ok, "err={:?}", first.error);
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        3,
        "第一轮应扫描全部 3 条历史通知"
    );
    assert!(
        svc.store
            .notices
            .iter()
            .all(|n| !n.attachments_scanned_at.is_empty()),
        "扫描后必须写入 attachmentsScannedAt（即使无附件/网络失败）"
    );

    // 第二轮：全部已有扫描标记 → 不得再选中（这正是生产环境「同 25 条无限重复」的修复点）。
    let second = svc.act(act("poll", json!({}))).await;
    assert!(second.ok, "err={:?}", second.error);
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        3,
        "已扫描过的通知不得在后续轮次重复请求详情"
    );
    assert!(svc.store.attachments.is_empty());
}

/// 覆盖顺序按 published_at 倒序、每轮不超过 limit，且下一轮换一批（轮转补抓）。
#[tokio::test]
async fn history_backfill_rotates_by_published_at_desc_within_limit() {
    let fetcher =
        MockFetcher::with_rules(vec![Rule::get("/notice.html", 200, &list_page(&[]))]);
    let mut svc = service("backfill-rotation", &fetcher, clock_at(2026, 11, 18), true);
    svc.store.settings.fetch_detail = false;
    svc.store.settings.detail_limit = 10;
    // 12 条历史通知：published_at 从 09-01 到 09-12（i=0 最旧，i=11 最新）。
    for i in 0..12 {
        svc.store.notices.push(cn_store::NoticeRecord {
            id: format!("n-rot-{i}"),
            title: format!("轮转通知 {i}"),
            url: format!("https://one.bjut.edu.cn/info/{i}.htm"),
            source_id: "src-test".into(),
            source_name: "测试源".into(),
            published_at: format!("2026-09-{:02}", i + 1),
            ..Default::default()
        });
    }

    // 第一轮：limit=10 → 只扫描最新的 10 条（09-03..09-12，即 i=2..11）。
    let first = svc.act(act("poll", json!({}))).await;
    assert!(first.ok, "err={:?}", first.error);
    let first_urls: Vec<String> = calls(&fetcher)
        .into_iter()
        .map(|req| req.url)
        .filter(|url| url.contains("/info/"))
        .collect();
    assert_eq!(first_urls.len(), 10, "每轮请求数必须等于 limit");
    for old in [0usize, 1] {
        assert!(
            first_urls
                .iter()
                .all(|url| !url.ends_with(&format!("/info/{old}.htm"))),
            "published_at 倒序：最旧的 i={old} 不应在第一轮被选中: {first_urls:?}"
        );
    }

    // 第二轮：剩余 2 条（i=0,1）补位；已扫描的 10 条不得重复请求。
    let second = svc.act(act("poll", json!({}))).await;
    assert!(second.ok, "err={:?}", second.error);
    let all_urls: Vec<String> = calls(&fetcher)
        .into_iter()
        .map(|req| req.url)
        .filter(|url| url.contains("/info/"))
        .collect();
    assert_eq!(all_urls.len(), 12, "两轮应恰好覆盖全部 12 条");
    for i in 0..12usize {
        assert_eq!(
            all_urls
                .iter()
                .filter(|url| url.ends_with(&format!("/info/{i}.htm")))
                .count(),
            1,
            "i={i} 应恰好被扫描一次"
        );
    }

    // 第三轮：全部已扫描 → 0 次新请求。
    let third = svc.act(act("poll", json!({}))).await;
    assert!(third.ok, "err={:?}", third.error);
    assert_eq!(
        calls_containing(&fetcher, "/info/").len(),
        12,
        "全部扫描完成后不得再发详情请求"
    );
    assert!(svc
        .store
        .notices
        .iter()
        .all(|n| !n.attachments_scanned_at.is_empty()));
}
