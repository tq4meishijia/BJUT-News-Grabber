//! QA 独立补充的边界单测（不改动既有测试）：日期范围校验/过滤、附件链接提取/规范化的易漏边界。
//!
//! 关注点：日历非法日期、单端范围、clamp 边界、大小写扩展名、相对/协议相对 URL、
//! tracking query 去重与有意义 query 区分、同名不同 URL、文件名清洗退化输入。

use url::Url;

use cn_core::{
    extract_attachment_links, extension_of, in_date_range, is_allowed_extension, is_iso_day,
    normalize_attachment_url, sanitize_file_name, validate_range, ALLOWED_ATTACHMENT_EXTS,
    INVALID_DATE_MESSAGE,
};

const TODAY: &str = "2026-11-18";

#[test]
fn validate_rejects_calendar_impossible_and_malformed_dates() {
    // 形状合法但日历上不存在：2026 不是闰年、4 月没有 31 号。
    for bad in [
        "2026-02-30",
        "2026-04-31",
        "2026-13-01",
        "2026-00-10",
        "2026-02-00",
    ] {
        assert_eq!(
            validate_range(bad, "", TODAY).unwrap_err(),
            INVALID_DATE_MESSAGE,
            "{bad} 在日历上不存在，必须判非法"
        );
    }
    // 形状本身就不对：位数不足/多余、分隔符不对、缺前导零。
    for bad in ["2026-9-5", "2026-09-1", "2026-09-255", "20260925", "2026/09/25", "2026.09.25"] {
        assert_eq!(
            validate_range("", bad, TODAY).unwrap_err(),
            INVALID_DATE_MESSAGE,
            "{bad} 不是 YYYY-MM-DD，必须判非法"
        );
    }
    // is_iso_day 只做形状检查（文档化语义），日历有效性由 validate_range 兜底。
    assert!(is_iso_day("2026-02-30"), "is_iso_day 只管形状");
    assert!(!is_iso_day("2026-2-30"));
    assert!(!is_iso_day("2026-02-30T10:00"));
}

#[test]
fn validate_trims_accepts_equal_bounds_and_clamps_future_end_only() {
    // 两端带空白（含制表符）应被 trim 后接受。
    let trimmed = validate_range("  2026-09-01 ", "\t2026-09-01\t", TODAY).expect("trim 后合法");
    assert_eq!(trimmed.start, "2026-09-01");
    assert_eq!(trimmed.end, "2026-09-01", "start == end 是合法闭区间");

    // 起止都等于今天：合法（闭区间包含今天）。
    let today_pair = validate_range(TODAY, TODAY, TODAY).expect("今天应合法");
    assert_eq!(today_pair.start, TODAY);
    assert_eq!(today_pair.end, TODAY);

    // end > today：合法但 clamp 到今天。
    let far_end = validate_range("", "2099-12-31", TODAY).expect("未来结束日期允许");
    assert_eq!(far_end.start, "");
    assert_eq!(far_end.end, TODAY, "end > today 必须 clamp 到今天");

    // clamp 后过滤的真实边界：今天保留、明天丢弃。
    assert!(in_date_range(TODAY, &far_end), "clamp 上界包含今天");
    assert!(!in_date_range("2026-11-19", &far_end), "clamp 后明天应被过滤");

    // today 本身无法解析时不做 clamp（防御性，不应 panic）。
    let weird = validate_range("", "2099-12-31", "not-a-day").expect("today 非法不阻止");
    assert_eq!(weird.end, "2099-12-31");
}

#[test]
fn in_date_range_single_sided_and_disabled_boundaries() {
    let start_only = validate_range("2026-09-01", "", TODAY).unwrap();
    assert!(in_date_range("2026-09-01", &start_only), "闭区间下界包含");
    assert!(!in_date_range("2026-08-31", &start_only));
    assert!(in_date_range("2099-12-31", &start_only), "只填 start 时另一端不限");

    let end_only = validate_range("", "2026-09-10", TODAY).unwrap();
    assert!(in_date_range("2026-09-10", &end_only), "闭区间上界包含");
    assert!(!in_date_range("2026-09-11", &end_only));
    assert!(in_date_range("1999-01-01", &end_only), "只填 end 时另一端不限");

    // 两端都空 = 不过滤；任何值（含空、非法、带时间的 ISO）都保留。
    let disabled = validate_range("", "", TODAY).unwrap();
    for value in ["", "not-a-date", "2026-13-40", "2026-09-25T10:00", "随便写的日期"] {
        assert!(in_date_range(value, &disabled), "不过滤时 {value:?} 应保留");
    }
    // 空/不可解析的 published_at 在任何范围下都保留（A5 宁可保留不误丢）。
    for value in ["", "not-a-date", "2026-13-40", "2026/09/25"] {
        assert!(in_date_range(value, &start_only), "未知日期 {value:?} 必须保留");
    }
}

#[test]
fn extract_handles_uppercase_relative_protocol_relative_and_skips_odd_schemes() {
    let base = Url::parse("https://one.bjut.edu.cn/notice/detail.htm").unwrap();
    let html = r#"
      <a href="/files/REPORT.PDF">大写扩展名</a>
      <a href="files/x.Jpg">无前导斜杠的相对地址</a>
      <img src="//cdn.example.com/img/pic.GIF">
      <a href="  /files/spaced.pdf  ">首尾空白</a>
      <a href="ftp://files.example.com/a.pdf">ftp</a>
      <a href="data:application/pdf;base64,AAAA">data</a>
      <a href="tel:123456">tel</a>
      <a href="javascript:void(0)">js</a>
      <a href="/files/readme">无扩展名</a>
    "#;
    let links = extract_attachment_links(html, &base);
    assert_eq!(links.len(), 4, "应恰好提取 4 条白名单链接: {links:?}");

    // 大小写扩展名识别，扩展名统一小写，文件名扩展名也归一为小写。
    assert_eq!(links[0].url, "https://one.bjut.edu.cn/files/REPORT.PDF", "路径大小写保持原样");
    assert_eq!(links[0].ext, "pdf");
    assert_eq!(links[0].file_name, "REPORT.pdf");

    // 无前导斜杠：相对详情页所在目录补全，而不是站点根。
    assert_eq!(links[1].url, "https://one.bjut.edu.cn/notice/files/x.Jpg");
    assert_eq!(links[1].ext, "jpg");
    assert_eq!(links[1].file_name, "x.jpg");

    // 协议相对地址按页面 scheme 补全。
    assert_eq!(links[2].url, "https://cdn.example.com/img/pic.GIF");
    assert_eq!(links[2].ext, "gif");

    // 首尾空白被 trim。
    assert_eq!(links[3].url, "https://one.bjut.edu.cn/files/spaced.pdf");

    assert!(links.iter().all(|l| !l.url.starts_with("ftp")));
    assert!(links.iter().all(|l| l.ext != "exe"));
}

#[test]
fn extract_dedupes_tracking_query_but_keeps_meaningful_query_and_same_names() {
    let base = Url::parse("https://one.bjut.edu.cn/notice/detail.htm").unwrap();
    let html = r#"
      <a href="/files/q.pdf?utm_source=a">1</a>
      <a href="/files/q.pdf?utm_source=b">2</a>
      <a href="/files/q.pdf#page=2">3</a>
      <a href="/files/q.pdf?b=1">4</a>
      <a href="/files/a/report.pdf">5</a>
      <a href="/files/b/report.pdf">6</a>
      <a href="/download?filename=report.PDF">7</a>
    "#;
    let links = extract_attachment_links(html, &base);
    assert_eq!(links.len(), 5, "去重后应为 5 条: {links:?}");

    // 仅 tracking query / fragment 不同的三条合并成一条；有意义的 ?b=1 单独一条。
    let q_norms: Vec<String> = links
        .iter()
        .filter(|l| l.url.contains("/files/q.pdf"))
        .map(|l| normalize_attachment_url(&l.url))
        .collect();
    assert_eq!(q_norms.len(), 2, "utm/fragment 变体应合并: {q_norms:?}");
    assert!(q_norms.iter().any(|n| n == "https://one.bjut.edu.cn/files/q.pdf"));
    assert!(q_norms.iter().any(|n| n == "https://one.bjut.edu.cn/files/q.pdf?b=1"));

    // 同名不同目录的两个文件都必须保留（同名不是去重依据）。
    let same_name = links.iter().filter(|l| l.file_name == "report.pdf").count();
    assert_eq!(same_name, 3, "a/report.pdf、b/report.pdf、download?filename 共 3 条同名不同 URL");

    // query 里的真实文件名也能识别（下载接口常见写法）。
    let from_query = links
        .iter()
        .find(|l| l.url.contains("/download"))
        .expect("query 文件名链接");
    assert_eq!(from_query.ext, "pdf");
    assert_eq!(from_query.file_name, "report.pdf");
}

#[test]
fn normalize_keeps_path_case_sorts_query_and_drops_tracking() {
    assert_eq!(
        normalize_attachment_url("HTTPS://EXAMPLE.COM:443/A/B.PDF#x"),
        "https://example.com/A/B.PDF",
        "scheme/host 小写、默认端口与 fragment 去掉；路径大小写保持"
    );
    assert_eq!(
        normalize_attachment_url("https://x.test/f?id=7&b=1&a=2"),
        "https://x.test/f?a=2&b=1&id=7",
        "query 需排序"
    );
    assert_eq!(
        normalize_attachment_url("https://x.test/f?from=qq&timestamp=9&_t=1&spm=3"),
        "https://x.test/f",
        "只剩 tracking query 时整体剔除"
    );
    // 设计只统一 host 大小写：路径大小写不同视为不同附件（避免误合并不同资源）。
    assert_ne!(
        normalize_attachment_url("https://x.test/A.PDF"),
        normalize_attachment_url("https://x.test/a.PDF")
    );
}

#[test]
fn sanitize_degenerate_names_and_whitelist_flags() {
    assert_eq!(sanitize_file_name("REPORT.PDF", "PDF"), "REPORT.pdf");
    assert_eq!(sanitize_file_name("a\nb.pdf", "pdf"), "a_b.pdf", "控制字符替换");
    assert_eq!(sanitize_file_name(".pdf", "pdf"), "attachment.pdf", "只有扩展名时补默认主名");
    assert_eq!(sanitize_file_name("...", "pdf"), "attachment.pdf");
    assert_eq!(sanitize_file_name("..\\..\\x.pdf", "pdf"), "x.pdf", "反斜杠路径前缀被剥离");
    assert_eq!(sanitize_file_name("x", ""), "x", "无扩展名不强行补点");

    assert!(is_allowed_extension(".PDF"));
    assert!(is_allowed_extension(" PdF "));
    assert!(!is_allowed_extension("tar.gz"));
    assert_eq!(extension_of("报告.PDF"), "pdf");
    assert_eq!(extension_of("noext"), "");
    assert_eq!(extension_of("a.toolongext"), "", "超过 5 字符的后缀不当作扩展名");

    // 白名单必须与 PRD §B3 的 16 项逐一一致（多一项会扩大攻击面，少一项会漏抓）。
    let expected = [
        "7z", "doc", "docx", "gif", "jpeg", "jpg", "pdf", "png", "ppt", "pptx", "rar", "txt",
        "webp", "xls", "xlsx", "zip",
    ];
    let mut actual: Vec<&str> = ALLOWED_ATTACHMENT_EXTS.to_vec();
    actual.sort_unstable();
    assert_eq!(actual, expected.to_vec(), "白名单与 PRD 不一致");
}
