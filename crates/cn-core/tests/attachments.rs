//! cn-core 纯逻辑单测：时间范围校验 / 过滤、附件白名单、相对 URL、规范化去重、文件名清洗。

use url::Url;

use cn_core::{
    extract_attachment_links, in_date_range, is_iso_day, is_allowed_extension,
    normalize_attachment_url, sanitize_file_name, validate_range, AttachmentLink,
    INVALID_DATE_MESSAGE, START_AFTER_END_MESSAGE, START_AFTER_TODAY_MESSAGE,
};

#[test]
fn date_validation_messages_clamp_and_one_sided_ranges() {
    let today = "2026-11-18";
    let ok = validate_range("2026-09-01", "2026-09-10", today).expect("valid range");
    assert_eq!(ok.start, "2026-09-01");
    assert_eq!(ok.end, "2026-09-10");

    assert_eq!(
        validate_range("2024-13-40", "", today).unwrap_err(),
        INVALID_DATE_MESSAGE
    );
    assert_eq!(
        validate_range("", "2026/09/10", today).unwrap_err(),
        INVALID_DATE_MESSAGE
    );
    assert_eq!(
        validate_range("2026-09-10", "2026-09-01", today).unwrap_err(),
        START_AFTER_END_MESSAGE
    );
    assert_eq!(
        validate_range("2026-12-01", "", today).unwrap_err(),
        START_AFTER_TODAY_MESSAGE
    );

    let clamped = validate_range("", "2026-12-31", today).expect("future end allowed");
    assert_eq!(clamped.start, "");
    assert_eq!(clamped.end, today, "end > today 被 clamp");

    let open = validate_range("2026-09-01", "", today).expect("one sided");
    assert_eq!(open.end, "");

    let empty = validate_range("  ", "", today).expect("empty means no filter");
    assert!(empty.start.is_empty() && empty.end.is_empty());

    assert!(is_iso_day("2026-02-28"));
    assert!(!is_iso_day("2026-2-28"));
    assert!(!is_iso_day("2026/02/28"));
}

#[test]
fn in_date_range_keeps_empty_and_unparsable_dates() {
    let range = validate_range("2026-09-01", "2026-09-10", "2026-11-18").unwrap();
    for unknown in ["", "not-a-date", "2024-13-40", "2026/09/05"] {
        assert!(in_date_range(unknown, &range), "unknown date must be kept: {unknown}");
    }
    assert!(!in_date_range("2026-08-31", &range));
    assert!(in_date_range("2026-09-01", &range), "闭区间下界");
    assert!(in_date_range("2026-09-05", &range));
    assert!(in_date_range("2026-09-10", &range), "闭区间上界");
    assert!(!in_date_range("2026-09-11", &range));

    let open_start = validate_range("", "2026-09-10", "2026-11-18").unwrap();
    assert!(in_date_range("2000-01-01", &open_start));
    assert!(!in_date_range("2026-09-11", &open_start));
    let open_end = validate_range("2026-09-01", "", "2026-11-18").unwrap();
    assert!(in_date_range("2099-01-01", &open_end));
}

#[test]
fn extract_links_whitelist_relative_urls_and_dedupe() {
    let base = Url::parse("https://one.bjut.edu.cn/notice/detail.htm").unwrap();
    let html = r#"
      <a href="/files/%E9%80%9A%E7%9F%A5.pdf">通知</a>
      <a href='https://EXAM.example.com:443/f/download.docx?utm_source=x&b=2&a=1#frag'>docx</a>
      <a href="https://exam.example.com/f/download.docx?a=1&b=2">duplicate</a>
      <img src="../files/pic.PNG">
      <a href="/files/malware.exe">exe</a>
      <a href="javascript:alert(1)">js</a>
      <a href="mailto:a@b.cn">mail</a>
      <img src="data:image/png;base64,AAAA">
      <a href="/files/archive.zip">zip</a>
    "#;
    let links: Vec<AttachmentLink> = extract_attachment_links(html, &base);
    assert_eq!(links.len(), 4, "白名单内且去重后应有 4 条: {links:?}");
    assert_eq!(links[0].url, "https://one.bjut.edu.cn/files/%E9%80%9A%E7%9F%A5.pdf");
    assert_eq!(links[0].file_name, "通知.pdf");
    assert_eq!(links[0].ext, "pdf");
    assert_eq!(
        normalize_attachment_url(&links[1].url),
        "https://exam.example.com/f/download.docx?a=1&b=2"
    );
    assert_eq!(links[2].url, "https://one.bjut.edu.cn/files/pic.PNG");
    assert_eq!(links[2].ext, "png");
    assert_eq!(links[3].ext, "zip");
    assert!(links.iter().all(|l| l.ext != "exe"));
}

#[test]
fn normalize_url_strips_tracking_fragment_and_sorts_query() {
    let normalized = normalize_attachment_url(
        "HTTPS://Example.COM:443/a/b.PDF?utm_medium=mail&z=1&spm=x&a=2&timestamp=1#page=2",
    );
    assert_eq!(normalized, "https://example.com/a/b.PDF?a=2&z=1");

    let filename_query = normalize_attachment_url("https://x.test/down?id=7&_t=99&from=qq");
    assert_eq!(filename_query, "https://x.test/down?id=7");

    assert_eq!(normalize_attachment_url("  not a url  "), "not a url");
}

#[test]
fn sanitize_file_name_and_extension_whitelist() {
    assert_eq!(sanitize_file_name("../../2026 通知.pdf", "pdf"), "2026 通知.pdf");
    assert_eq!(sanitize_file_name("a:b*c?.docx", "DOCX"), "a_b_c_.docx");
    assert_eq!(sanitize_file_name("", "zip"), "attachment.zip");
    assert_eq!(
        sanitize_file_name("这是一个非常长的文件名".repeat(20).as_str(), "txt").chars().count(),
        104
    );

    for ext in [
        "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "png", "jpg", "jpeg", "gif",
        "webp", "zip", "rar", "7z",
    ] {
        assert!(is_allowed_extension(ext), "{ext} 应在白名单");
    }
    for ext in ["exe", "html", "js", "php", "md", "mp4"] {
        assert!(!is_allowed_extension(ext), "{ext} 不应在白名单");
    }
}
