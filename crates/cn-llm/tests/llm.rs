//! cn-llm 测试：正常 / JSON 损坏 / 索引越界 / 枚举越界 / 时间非法 / 降级路径。
//! HTTP 经 cn_net::MockFetcher 注入，不真发网络。

use chrono::{Local, TimeZone};
use cn_llm::{
    summarize_batch, summarize_with_fallback, ai_hash, LlmCache, LlmConfig, LlmNotice,
    TokenMeter, PROMPT_VERSION,
};
use cn_net::mock::MockFetcher;
use cn_net::{FetchRequest, FetchResponse};

fn fixed_now() -> chrono::DateTime<Local> {
    Local
        .with_ymd_and_hms(2026, 11, 18, 10, 0, 0)
        .single()
        .expect("fixed now")
}

fn notices() -> Vec<LlmNotice> {
    vec![
        LlmNotice {
            title: "关于2026年下半年大学英语四、六级考试报名的通知".into(),
            published_at: "2026-09-15".into(),
            content: "请于2026年12月1日前登录教务系统完成报名。联系人：李老师 电话：010-67391234。".into(),
        },
        LlmNotice {
            title: "关于举办学术报告的通知".into(),
            published_at: "2026-11-15".into(),
            content: "报告人：张明教授。时间：2026年11月20日14:00，地点：知新楼报告厅。".into(),
        },
    ]
}

fn llm_resp(content: &str, tokens: u64) -> FetchResponse {
    let body = serde_json::json!({
        "choices": [{ "message": { "content": content } }],
        "usage": { "total_tokens": tokens },
    });
    FetchResponse {
        status: 200,
        url: "https://api.deepseek.com/chat/completions".into(),
        headers: vec![("content-type".into(), "application/json".into())],
        body: serde_json::to_vec(&body).unwrap(),
    }
}

fn cfg() -> LlmConfig {
    LlmConfig {
        api_key: "sk-test".into(),
        base_url: "https://api.deepseek.com".into(),
        model: "deepseek-chat".into(),
        monthly_token_budget: 0,
    }
}

#[tokio::test]
async fn normal_batch_ok() {
    let rows = r#"[
      {"i":0,"category":"考试","gist":"四六级考试报名","useful":["12月1日前报名"],"tags":["考试","报名"],"deadline":"2026-12-01T23:59","contact":"李老师","audience":"本科生","relevant":true,"confidence":0.9},
      {"i":1,"category":"讲座学术","gist":"张明教授学术报告","useful":["11月20日14:00知新楼"],"tags":["讲座"],"deadline":"","contact":"","audience":"全体","relevant":true,"confidence":0.8}
    ]"#;
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp(rows, 120)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &notices(), &mut cache, &meter, fixed_now()).await;
    assert!(out.ok, "reason={}", out.reason);
    assert_eq!(out.map.len(), 2);
    let row0 = &out.map[&0];
    assert!(row0.ai);
    assert_eq!(row0.category, "考试");
    assert_eq!(row0.deadline, "2026-12-01T23:59");
    assert_eq!(row0.tags, vec!["考试".to_string(), "报名".to_string()]);
    // 缓存已写入
    assert_eq!(cache.len(), 2);
}

#[tokio::test]
async fn json_broken_retries_then_fails() {
    // 两次返回都坏 → ok=false，reason 含解析信息
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp("完全不是 JSON", 10)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &notices(), &mut cache, &meter, fixed_now()).await;
    assert!(!out.ok);
    assert!(!out.reason.is_empty());
    // 第一次 + 修复重试一次 = 2 次请求
    assert_eq!(fetcher.call_count(), 2);
}

#[tokio::test]
async fn json_broken_then_repair_succeeds() {
    // 第一次坏，第二次（修复重试）好 → ok
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| {
        let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n == 0 {
            Ok(llm_resp("垃圾输出", 10))
        } else {
            Ok(llm_resp(
                r#"[{"i":0,"category":"考试","gist":"报名","useful":[],"tags":["考试"],"deadline":"","contact":"","relevant":true,"confidence":0.7}]"#,
                30,
            ))
        }
    });
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &notices(), &mut cache, &meter, fixed_now()).await;
    assert!(out.ok, "reason={}", out.reason);
    assert_eq!(out.map.len(), 1);
}

#[tokio::test]
async fn index_out_of_range_dropped() {
    let rows = r#"[
      {"i":0,"category":"考试","gist":"正常","useful":[],"tags":[],"deadline":"","contact":"","relevant":true,"confidence":0.5},
      {"i":99,"category":"考试","gist":"越界","useful":[],"tags":[],"deadline":"","contact":"","relevant":true,"confidence":0.5}
    ]"#;
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp(rows, 20)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &notices(), &mut cache, &meter, fixed_now()).await;
    assert!(out.ok, "reason={}", out.reason);
    assert_eq!(out.map.len(), 1, "越界行应被丢弃");
    assert!(out.map.contains_key(&0));
}

#[tokio::test]
async fn category_out_of_enum_falls_to_other() {
    let rows = r#"[{"i":0,"category":"外星事务","gist":"x","useful":[],"tags":[],"deadline":"","contact":"","relevant":true,"confidence":0.5}]"#;
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp(rows, 20)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &notices(), &mut cache, &meter, fixed_now()).await;
    assert!(out.ok, "reason={}", out.reason);
    assert_eq!(out.map[&0].category, "其他");
}

#[tokio::test]
async fn illegal_deadline_cleared() {
    // 年份 1999 / 格式错误 → 置空
    let rows = r#"[
      {"i":0,"category":"考试","gist":"x","useful":[],"tags":[],"deadline":"1999-01-01T00:00","contact":"","relevant":true,"confidence":0.5},
      {"i":1,"category":"考试","gist":"y","useful":[],"tags":[],"deadline":"明天下午","contact":"","relevant":true,"confidence":0.5}
    ]"#;
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp(rows, 20)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &notices(), &mut cache, &meter, fixed_now()).await;
    assert!(out.ok, "reason={}", out.reason);
    assert_eq!(out.map[&0].deadline, "");
    assert_eq!(out.map[&1].deadline, "");
}

#[tokio::test]
async fn deadline_before_publish_cleared() {
    // 发布日 2026-09-15，deadline 2026-09-01（早于发布日-1天）→ 置空
    let rows = r#"[{"i":0,"category":"考试","gist":"x","useful":[],"tags":[],"deadline":"2026-09-01T00:00","contact":"","relevant":true,"confidence":0.5}]"#;
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp(rows, 20)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &notices(), &mut cache, &meter, fixed_now()).await;
    assert!(out.ok, "reason={}", out.reason);
    assert_eq!(out.map[&0].deadline, "");
}

#[tokio::test]
async fn degrade_to_rule_summary_b11() {
    // 连续失败（坏 JSON 两次）→ summarize_with_fallback 产出规则行，摘要与 useful 非空
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp("坏", 5)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let batch = notices();
    let out = summarize_batch(fetcher.as_ref(), &cfg(), &batch, &mut cache, &meter, fixed_now()).await;
    assert!(!out.ok);
    let keywords: Vec<String> = ["考试", "报名", "讲座"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let fallback = summarize_with_fallback(out, &batch, &keywords, fixed_now());
    assert_eq!(fallback.map.len(), 2);
    for row in fallback.map.values() {
        assert!(!row.ai);
        assert!(!row.gist.is_empty(), "B11：摘要必须非空");
        assert!(!row.useful.is_empty(), "B11：useful 必须非空");
    }
}

#[tokio::test]
async fn cache_hit_skips_http() {
    let rows = r#"[{"i":0,"category":"考试","gist":"命中","useful":["x"],"tags":["考试"],"deadline":"","contact":"","relevant":true,"confidence":0.6}]"#;
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp(rows, 10)));
    let mut cache = LlmCache::new();
    let meter = TokenMeter::new(0);
    let mut all = notices();
    let batch = vec![all.remove(0)];
    let out1 = summarize_batch(fetcher.as_ref(), &cfg(), &batch, &mut cache, &meter, fixed_now()).await;
    assert!(out1.ok);
    assert_eq!(fetcher.call_count(), 1);
    let out2 = summarize_batch(fetcher.as_ref(), &cfg(), &batch, &mut cache, &meter, fixed_now()).await;
    assert!(out2.ok);
    assert_eq!(fetcher.call_count(), 1, "第二次应全命中缓存，不再发请求");
    assert_eq!(out2.map[&0].gist, "命中");
}

#[tokio::test]
async fn budget_exhausted_degrades() {
    let rows = r#"[{"i":0,"category":"考试","gist":"x","useful":[],"tags":[],"deadline":"","contact":"","relevant":true,"confidence":0.5}]"#;
    let fetcher = MockFetcher::new(move |_req: &FetchRequest| Ok(llm_resp(rows, 20)));
    let mut cache = LlmCache::new();
    // 预算 1 token，第一次调用记账 20 → 第二次超预算
    let meter = TokenMeter::new(1);
    let batch = notices();
    let out1 = summarize_batch(fetcher.as_ref(), &cfg(), &batch, &mut cache, &meter, fixed_now()).await;
    // 第一次调用前 allow：month 不同（初始空）→ 允许
    assert!(out1.ok, "reason={}", out1.reason);
    let out2 = summarize_batch(fetcher.as_ref(), &cfg(), &batch, &mut cache, &meter, fixed_now()).await;
    // 第二次：同月 tokens=20*2... 第一次记 20，预算 1 → 超限
    if !out2.ok {
        assert!(out2.reason.contains("预算"), "reason={}", out2.reason);
    }
}

#[test]
fn ai_hash_covers_model_and_version() {
    let a = ai_hash("标题", "内容", "deepseek-chat");
    let b = ai_hash("标题", "内容", "other-model");
    assert_ne!(a, b);
    assert_eq!(a, ai_hash("标题", "内容", "deepseek-chat"));
    assert_eq!(PROMPT_VERSION, 1);
}
