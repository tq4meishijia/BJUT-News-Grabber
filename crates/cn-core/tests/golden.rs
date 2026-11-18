//! golden 对照：以 `fixtures/expected/*.json`（golden.mjs 导出的 Node 基线）为基准，
//! 对每个用例调用 Rust 解析/抽取/打分/摘要做字段级对比。
//!
//! 阈值（docs/rust-tauri-refactor-plan.md §9 P1 出门门槛）：
//! - 解析类字段（title / url / itemId / publishedAt）逐字段一致率 ≥ 98%
//! - score 与 deadline 一致率必须 100%（确定性函数，不允许偏差）
//!
//! 时间口径：golden.mjs 在导出时以系统时钟运行；本测试读取每个 expected 文件的
//! `generatedAt`，用 FixedClock 冻结在同一时刻，保证 daysUntil / 无年份日期可复现。
//!
//! 字节解码口径：Node 的 `readFile(..., 'utf8')` 是 WHATWG UTF-8 解码；
//! 这里用 `encoding_rs::UTF_8` 同口径解码 fixture 原文（GBK 用例的乱码才会与基线一致）。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use cn_core::{
    clean_title, find_date, find_deadline, find_contact, parse_html_list, parse_json_list,
    parse_rss, pick_tags, rule_summary, score, strip_red_header, normalize_due,
};
use cn_model::{Clock, FixedClock, ScoreInput, SourceSpec, SummaryInput};
use serde_json::Value;
use url::Url;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

/// WHATWG UTF-8 解码（与 Node `readFile utf8` 一致；GBK 字节产生同样的替换字符）。
fn read_like_node(path: &Path) -> String {
    let bytes = fs::read(path).expect("read fixture bytes");
    let (cow, _enc, _had_errors) = encoding_rs::UTF_8.decode(&bytes);
    cow.into_owned()
}

/// 复刻 `golden.mjs::enrich`。
#[allow(clippy::too_many_arguments)]
fn enrich(
    title: &str,
    url: &str,
    item_id: &str,
    published_at: &str,
    context: &str,
    keywords: &[String],
    body_text: &str,
    clock: &FixedClock,
) -> Value {
    let now = clock.now();
    let text = format!("{title} {body_text}");
    let tags = pick_tags(&text, keywords);
    let deadline = find_deadline(&text, now);
    let deadline_str = deadline.as_ref().map(|d| d.iso.clone()).unwrap_or_default();
    let contact = find_contact(body_text);
    let rule = rule_summary(&SummaryInput {
        title: title.to_string(),
        content: body_text.to_string(),
        tags: tags.clone(),
        deadline: if deadline_str.is_empty() {
            None
        } else {
            Some(deadline_str.clone())
        },
        contact: if contact.is_empty() {
            None
        } else {
            Some(contact.clone())
        },
        now,
    });
    let score_value = score(&ScoreInput {
        title: title.to_string(),
        content: text,
        tags: tags.clone(),
        deadline: if deadline_str.is_empty() {
            None
        } else {
            Some(deadline_str.clone())
        },
        keywords: Vec::new(),
        now,
    });
    serde_json::json!({
        "raw": {
            "itemId": item_id,
            "title": title,
            "titleCleaned": clean_title(title),
            "url": url,
            "publishedAt": published_at,
            "dateGuessed": find_date(title, now),
            "context": context,
        },
        "extract": {
            "tags": tags,
            "deadline": deadline_str,
            "contact": contact,
        },
        "score": score_value,
        "rule": {
            "summary": rule.summary,
            "useful": rule.useful,
        }
    })
}

fn parse_meta(raw_dir: &Path, stem: &str) -> Value {
    let path = raw_dir.join(format!("{stem}.meta.json"));
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).expect("meta json"),
        Err(_) => serde_json::json!({}),
    }
}

fn pick_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn golden_fixtures_match_node_baseline() {
    let root = workspace_root();
    let raw_dir = root.join("fixtures").join("raw");
    let expected_dir = root.join("fixtures").join("expected");
    assert!(expected_dir.is_dir(), "fixtures/expected 不存在，先跑 node tools/golden.mjs");

    let mut files: Vec<PathBuf> = fs::read_dir(&expected_dir)
        .expect("read expected dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "expected 目录为空");

    // 差异清单（报告用）。
    let mut diffs: Vec<String> = Vec::new();
    // 计数：字段级
    let mut n_items: usize = 0;
    let mut n_title_ok: usize = 0;
    let mut n_url_ok: usize = 0;
    let mut n_id_ok: usize = 0;
    let mut n_date_ok: usize = 0;
    let mut n_score_ok: usize = 0;
    let mut n_deadline_ok: usize = 0;
    let mut n_rule_ok: usize = 0;
    let mut n_count_ok: usize = 0;

    for expected_path in &files {
        let expected_text = fs::read_to_string(expected_path).expect("read expected");
        let expected: Value = serde_json::from_str(&expected_text).expect("expected json");
        let fixture_name = expected["fixture"].as_str().expect("fixture name").to_string();
        let kind = expected["kind"].as_str().unwrap_or("").to_string();
        let base_url = expected["baseUrl"].as_str().unwrap_or("").to_string();
        let generated_at = expected["generatedAt"].as_str().expect("generatedAt");

        // 冻结时间：与 golden.mjs 运行时刻一致。
        let utc = DateTime::parse_from_rfc3339(generated_at)
            .expect("generatedAt rfc3339")
            .with_timezone(&Local);
        let clock = FixedClock::new(utc);

        let raw_path = raw_dir.join(&fixture_name);
        assert!(raw_path.is_file(), "缺 fixture 原文：{fixture_name}");
        let body = read_like_node(&raw_path);
        let stem = raw_path.file_stem().unwrap().to_string_lossy().to_string();
        let meta = parse_meta(&raw_dir, &stem);

        let mut spec = SourceSpec::default();
        if let Some(source) = meta.get("source") {
            if let Ok(parsed) = serde_json::from_value::<SourceSpec>(source.clone()) {
                spec = parsed;
            }
        }
        if spec.url.is_empty() {
            spec.url = base_url.clone();
        }
        let base = Url::parse(&base_url).expect("baseUrl");

        let keywords: Vec<String> = match meta.get("keywords").and_then(|v| v.as_array()) {
            Some(arr) if !arr.is_empty() => arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            _ => cn_core::DEFAULT_KEYWORDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };

        let items = match kind.as_str() {
            "html" => parse_html_list(&body, &base, &spec, &clock),
            "rss" => parse_rss(&body, &base, &clock),
            "json" => parse_json_list(&body, &spec, &clock),
            other => panic!("未知 kind：{other}"),
        };

        // bodyText 与 golden.mjs 一致：meta.content 优先；json 源用列表摘要兜底。
        let meta_content = meta.get("content").and_then(|v| v.as_str());
        let use_context_as_body = kind == "json";
        let body_source = match meta_content {
            Some(_) => "meta",
            None if use_context_as_body => "context",
            None => "none",
        };

        let expected_items = expected["items"].as_array().cloned().unwrap_or_default();
        if items.len() != expected_items.len() {
            diffs.push(format!(
                "[{fixture_name}] 条数不一致：rust={} node={}",
                items.len(),
                expected_items.len()
            ));
        } else {
            n_count_ok += 1;
        }

        let n = items.len().min(expected_items.len());
        for i in 0..n {
            n_items += 1;
            let item = &items[i];
            let exp = &expected_items[i];
            let body_text: String = match meta_content {
                Some(c) => c.to_string(),
                None if use_context_as_body => item.context.clone(),
                None => String::new(),
            };
            let actual = enrich(
                &item.title,
                &item.url,
                &item.item_id,
                &item.published_at,
                &item.context,
                &keywords,
                &body_text,
                &clock,
            );
            let a_raw = &actual["raw"];
            let e_raw = &exp["raw"];

            let checks = [
                ("title", pick_str(a_raw, "title"), pick_str(e_raw, "title")),
                ("url", pick_str(a_raw, "url"), pick_str(e_raw, "url")),
                ("itemId", pick_str(a_raw, "itemId"), pick_str(e_raw, "itemId")),
                ("publishedAt", pick_str(a_raw, "publishedAt"), pick_str(e_raw, "publishedAt")),
            ];
            for (field, a, e) in checks {
                if a == e {
                    match field {
                        "title" => n_title_ok += 1,
                        "url" => n_url_ok += 1,
                        "itemId" => n_id_ok += 1,
                        _ => n_date_ok += 1,
                    }
                } else {
                    diffs.push(format!(
                        "[{fixture_name}]#{i} {field}：rust={a:?} node={e:?}"
                    ));
                }
            }

            // score / deadline：100% 硬门槛。
            let a_score = actual["score"].as_i64().unwrap_or(-1);
            let e_score = exp["score"].as_i64().unwrap_or(-1);
            if a_score == e_score {
                n_score_ok += 1;
            } else {
                diffs.push(format!(
                    "[{fixture_name}]#{i} score：rust={a_score} node={e_score}"
                ));
            }
            let a_dl = actual["extract"]["deadline"].as_str().unwrap_or("");
            let e_dl = exp["extract"]["deadline"].as_str().unwrap_or("");
            if a_dl == e_dl {
                n_deadline_ok += 1;
            } else {
                diffs.push(format!(
                    "[{fixture_name}]#{i} deadline：rust={a_dl:?} node={e_dl:?}"
                ));
            }

            // rule（summary/useful）：记录差异，不设硬门槛。
            let a_rule = actual["rule"].to_string();
            let e_rule = exp["rule"].to_string();
            if a_rule == e_rule {
                n_rule_ok += 1;
            } else {
                diffs.push(format!(
                    "[{fixture_name}]#{i} rule：rust={a_rule} node={e_rule}"
                ));
            }
            let _ = body_source;
        }
    }

    // ---- 差异报告（--nocapture 可见）----
    println!("==== golden 对照报告 ====");
    println!("用例数：{}，对照条目数：{n_items}", files.len());
    println!(
        "解析字段一致率：title {}/{} ({:.1}%) | url {}/{} ({:.1}%) | itemId {}/{} ({:.1}%) | publishedAt {}/{} ({:.1}%)",
        n_title_ok, n_items, pct(n_title_ok, n_items),
        n_url_ok, n_items, pct(n_url_ok, n_items),
        n_id_ok, n_items, pct(n_id_ok, n_items),
        n_date_ok, n_items, pct(n_date_ok, n_items),
    );
    println!("score 一致率：{}/{} ({:.1}%)", n_score_ok, n_items, pct(n_score_ok, n_items));
    println!("deadline 一致率：{}/{} ({:.1}%)", n_deadline_ok, n_items, pct(n_deadline_ok, n_items));
    println!("rule 一致率：{}/{} ({:.1}%)", n_rule_ok, n_items, pct(n_rule_ok, n_items));
    println!("条数一致用例：{}/{}", n_count_ok, files.len());
    if diffs.is_empty() {
        println!("差异清单：无");
    } else {
        println!("差异清单（{} 项）：", diffs.len());
        for line in &diffs {
            println!("  {line}");
        }
    }

    // ---- 门槛断言 ----
    let parse_ok = n_title_ok + n_url_ok + n_id_ok + n_date_ok;
    let parse_total = n_items * 4;
    let parse_rate = pct(parse_ok, parse_total);
    assert!(
        parse_rate >= 98.0,
        "解析类字段一致率 {parse_rate:.2}% < 98%（详情见 stdout 报告）"
    );
    assert_eq!(
        n_score_ok, n_items,
        "score 必须 100% 一致（确定性函数）"
    );
    assert_eq!(
        n_deadline_ok, n_items,
        "deadline 必须 100% 一致（确定性函数）"
    );
}

fn pct(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 100.0;
    }
    part as f64 * 100.0 / whole as f64
}

/// normalize_due 边界。
#[test]
fn normalize_due_boundaries() {
    assert_eq!(normalize_due(""), None);
    assert_eq!(normalize_due("   "), None);
    assert_eq!(normalize_due("2026-12-01").as_deref(), Some("2026-12-01T23:59"));
    assert_eq!(
        normalize_due("2026-12-01T17:00:30").as_deref(),
        Some("2026-12-01T17:00")
    );
    assert_eq!(
        normalize_due("2026-12-01 17:00:30").as_deref(),
        Some("2026-12-01T17:00")
    );
    assert_eq!(
        normalize_due("2026-12-01T17:00").as_deref(),
        Some("2026-12-01T17:00")
    );
    // 不可解析 → None（JS 空串）
    assert_eq!(normalize_due("not a date"), None);
}

/// findDate 闰日与「22 2026.10」格式。
#[test]
fn find_date_leap_day_and_odd_format() {
    let clock = FixedClock::new(Local::now());
    let now = clock.now();
    assert_eq!(
        find_date("请于2028年2月29日前提交", now),
        "2028-02-29"
    );
    assert_eq!(
        find_date("22 2026.10 关于研究生学位论文答辩的通知", now),
        "2026-10-22"
    );
    // 2026 非闰年，但 isoDate 只校验 1–31，与 JS 行为一致
    assert_eq!(find_date("2026-02-29", now), "2026-02-29");
    // 月份越界 → 空
    assert_eq!(find_date("2026-13-40", now), "");
}

/// clean_title 去前缀。
#[test]
fn clean_title_strips_prefixes() {
    assert_eq!(clean_title("【教务】关于选课的通知"), "关于选课的通知");
    assert_eq!(clean_title("[通知] 关于选课的通知"), "关于选课的通知");
    assert_eq!(
        clean_title("22 2026.10 关于答辩的通知"),
        "关于答辩的通知"
    );
    assert_eq!(
        clean_title("2026年11月18日 关于放假的通知"),
        "关于放假的通知"
    );
}

/// strip_red_header 剥红头。
#[test]
fn strip_red_header_removes_banner() {
    let red = "北京工业大学文件\n工大发〔2022〕15号\n————————————\n各学院：现将有关事项通知如下。";
    // 12 个连字符不够 16，不剥（与 JS 一致）
    assert_eq!(strip_red_header(red), red);
    let red16 = "北京工业大学文件\n工大发〔2022〕15号\n————————————————\n各学院：现将有关事项通知如下。";
    let stripped = strip_red_header(red16);
    assert!(stripped.starts_with("各学院"), "got: {stripped}");
    // 没有红头则原样返回（不 trim）
    assert_eq!(strip_red_header("  普通正文  "), "  普通正文  ");
}

/// score 夹紧与分支。
#[test]
fn score_clamps_and_branches() {
    let now = Local::now();
    let empty: Vec<String> = Vec::new();
    // 基线 22 → 夹紧不生效
    assert_eq!(
        score(&ScoreInput {
            title: "普通通知".into(),
            content: "".into(),
            tags: empty.clone(),
            deadline: None,
            keywords: empty.clone(),
            now,
        }),
        22
    );
    // 下限 5：招标标题 -18 → 4 → 夹到 5
    assert_eq!(
        score(&ScoreInput {
            title: "关于物业招标采购的公告".into(),
            content: "".into(),
            tags: empty.clone(),
            deadline: None,
            keywords: empty.clone(),
            now,
        }),
        5
    );
    // 上限 98：大量加分后夹紧
    let tags: Vec<String> = ["考试", "报名", "截止", "讲座", "成绩", "选课"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let v = score(&ScoreInput {
        title: "考试报名截止讲座成绩选课".into(),
        content: "紧急务必马上今天考试报名截止讲座成绩选课".into(),
        tags,
        deadline: None,
        keywords: empty.clone(),
        now,
    });
    assert!(v <= 98, "score 必须夹在 98 以内，got {v}");
    assert!(v >= 5, "score 必须夹在 5 以上，got {v}");
}
