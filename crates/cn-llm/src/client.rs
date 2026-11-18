//! OpenAI 兼容客户端：批量调用 → 提取 JSON 片段 → Schema 校验 → 一次修复 → 降级规则（B11）。

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Local, TimeZone};
use cn_model::{SummaryInput, ScoreInput};
use cn_net::{FetchRequest, Fetcher, ScopeId};

use crate::cache::{ai_hash, LlmCache};
use crate::prompt::{build_prompt, system_prompt, PromptNotice};
use crate::schema::{extract_json_array, repair_rows, validate_rows, CheckedRow};

/// 模型配置（OpenAI 兼容端点）。
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    /// 月度 token 预算（0 = 不限）。
    pub monthly_token_budget: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
            monthly_token_budget: 0,
        }
    }
}

impl LlmConfig {
    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

/// 一条通知的调用入参。
#[derive(Debug, Clone)]
pub struct LlmNotice {
    pub title: String,
    pub published_at: String,
    pub content: String,
}

/// 单条输出（校验后）。与 store 的 notice 补全字段对齐。
#[derive(Debug, Clone, PartialEq)]
pub struct LlmRow {
    pub category: String,
    pub gist: String,
    pub useful: Vec<String>,
    pub tags: Vec<String>,
    pub deadline: String,
    pub contact: String,
    pub audience: String,
    pub relevant: bool,
    pub confidence: f64,
    /// true = AI 产出；false = 规则降级（B11）。
    pub ai: bool,
}

impl LlmRow {
    fn from_checked(c: CheckedRow, ai: bool) -> Self {
        Self {
            category: c.category,
            gist: c.gist,
            useful: c.useful,
            tags: c.tags,
            deadline: c.deadline,
            contact: c.contact,
            audience: c.audience,
            relevant: c.relevant,
            confidence: c.confidence,
            ai,
        }
    }
}

/// 批量结果。`map` 的 key 为批次内下标。
#[derive(Debug)]
pub struct BatchResult {
    pub ok: bool,
    pub map: HashMap<usize, LlmRow>,
    pub reason: String,
    /// Ai = 模型产出；RuleFallback = 整批降级（B11 保底）。
    pub mode: BatchMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchMode {
    Ai,
    RuleFallback,
}

struct Usage {
    month: String,
    tokens: u64,
}

/// token 用量记账（月度预算）。`budget` 可随配置更新（0 = 不限）。
pub struct TokenMeter {
    inner: Mutex<Usage>,
    budget: std::sync::atomic::AtomicU64,
}

impl TokenMeter {
    pub fn new(budget: u64) -> Self {
        Self {
            inner: Mutex::new(Usage {
                month: String::new(),
                tokens: 0,
            }),
            budget: std::sync::atomic::AtomicU64::new(budget),
        }
    }

    /// 更新月度预算（配置热更新）。
    pub fn set_budget(&self, budget: u64) {
        self.budget.store(budget, std::sync::atomic::Ordering::Relaxed);
    }

    /// 当前月用量与预算：`(used_this_month, budget)`，预算 0 = 不限。
    pub fn usage(&self, now: DateTime<Local>) -> (u64, u64) {
        let month = now.format("%Y-%m").to_string();
        let usage = self.inner.lock().expect("meter lock");
        let used = if usage.month == month { usage.tokens } else { 0 };
        (
            used,
            self.budget.load(std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// 是否还能发一次请求（预算 0 = 不限）。
    pub fn allow(&self, now: DateTime<Local>) -> bool {
        let budget = self.budget.load(std::sync::atomic::Ordering::Relaxed);
        if budget == 0 {
            return true;
        }
        let month = now.format("%Y-%m").to_string();
        let usage = self.inner.lock().expect("meter lock");
        usage.month == month && usage.tokens < budget || usage.month != month
    }

    /// 记账（usage 字段来自响应，缺省按 0）。
    pub fn record(&self, now: DateTime<Local>, tokens: u64) {
        let month = now.format("%Y-%m").to_string();
        let mut usage = self.inner.lock().expect("meter lock");
        if usage.month != month {
            usage.month = month;
            usage.tokens = 0;
        }
        usage.tokens += tokens;
    }
}

/// 调用一个 OpenAI 兼容端点并解析出文本内容（返回内容与 usage token 数）。
async fn chat_completions(
    fetcher: &dyn Fetcher,
    cfg: &LlmConfig,
    user_prompt: &str,
) -> Result<(String, u64), String> {
    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [
            { "role": "system", "content": system_prompt() },
            { "role": "user", "content": user_prompt },
        ],
        "temperature": 0.2,
        "max_tokens": 2400,
        "stream": false,
    });
    let mut req = FetchRequest::post(
        cfg.endpoint(),
        ScopeId::Portal("llm".to_string()),
        serde_json::to_vec(&body).map_err(|e| e.to_string())?,
    );
    req.timeout = std::time::Duration::from_millis(90_000);
    req.headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("authorization".to_string(), format!("Bearer {}", cfg.api_key)),
    ];
    let resp = fetcher.fetch(req).await.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&resp.body).into_owned();
    if !(200..300).contains(&resp.status) {
        let hint = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(format!("接口返回 HTTP {}：{}", resp.status, hint));
    }
    let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let content = parsed
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    let tokens = parsed
        .get("usage")
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    Ok((content, tokens))
}

/// 解析模型文本 → 行数组 Value。
fn parse_rows(text: &str) -> Result<Vec<serde_json::Value>, String> {
    let slice = extract_json_array(text).ok_or("模型没有返回 JSON 数组")?;
    let rows: serde_json::Value = serde_json::from_str(slice).map_err(|e| format!("JSON 解析失败：{e}"))?;
    let arr = rows.as_array().ok_or("模型返回的不是数组")?;
    Ok(arr.clone())
}

/// §6.3 主入口：批量摘要（带缓存 / 一次修复重试 / 预算）。
/// 失败时 `ok=false` 并给出 reason；调用方若要 B11 保底请用 [`summarize_with_fallback`]。
pub async fn summarize_batch(
    fetcher: &dyn Fetcher,
    cfg: &LlmConfig,
    notices: &[LlmNotice],
    cache: &mut LlmCache,
    meter: &TokenMeter,
    now: DateTime<Local>,
) -> BatchResult {
    if notices.is_empty() {
        return BatchResult {
            ok: true,
            map: HashMap::new(),
            reason: String::new(),
            mode: BatchMode::Ai,
        };
    }
    if cfg.api_key.trim().is_empty() {
        return BatchResult {
            ok: false,
            map: HashMap::new(),
            reason: "没有配置 API Key（设置页填 DeepSeek Key，或设环境变量 DEEPSEEK_API_KEY）".to_string(),
            mode: BatchMode::RuleFallback,
        };
    }
    if !meter.allow(now) {
        return BatchResult {
            ok: false,
            map: HashMap::new(),
            reason: "本月 token 预算已用完，已自动改用规则摘要".to_string(),
            mode: BatchMode::RuleFallback,
        };
    }

    // 缓存命中先行。
    let mut map: HashMap<usize, LlmRow> = HashMap::new();
    let mut missing: Vec<usize> = Vec::new();
    for (index, notice) in notices.iter().enumerate() {
        let key = ai_hash(&notice.title, &notice.content, &cfg.model);
        match cache.get(&key) {
            Some(row) => {
                map.insert(index, row.clone());
            }
            None => missing.push(index),
        }
    }
    if missing.is_empty() {
        return BatchResult {
            ok: true,
            map,
            reason: String::new(),
            mode: BatchMode::Ai,
        };
    }

    let batch: Vec<LlmNotice> = missing.iter().map(|&i| notices[i].clone()).collect();
    let prompt_notices: Vec<PromptNotice<'_>> = batch
        .iter()
        .map(|n| PromptNotice {
            title: &n.title,
            published_at: &n.published_at,
            content: &n.content,
        })
        .collect();
    let user_prompt = build_prompt(&prompt_notices);
    let published: Vec<String> = batch.iter().map(|n| n.published_at.clone()).collect();

    let attempt = |text: String| -> Result<(Vec<CheckedRow>, Vec<String>), String> {
        let rows = parse_rows(&text)?;
        let (checked, errors) = validate_rows_with_dates(&rows, batch.len(), &published);
        Ok((checked, errors))
    };

    let first = chat_completions(fetcher, cfg, &user_prompt).await;
    let text = match first {
        Ok((t, tokens)) => {
            meter.record(now, tokens);
            t
        }
        Err(reason) => {
            return BatchResult {
                ok: false,
                map,
                reason: format!("调用失败：{reason}"),
                mode: BatchMode::RuleFallback,
            }
        }
    };

    let mut rows = match attempt(text) {
        Ok((checked, errors)) if errors.is_empty() => checked,
        Ok((_checked, errors)) => {
            // 带校验错误重试 1 次。
            let retry_prompt = format!("{user_prompt}\n\n{}", repair_rows(&errors));
            match chat_completions(fetcher, cfg, &retry_prompt).await {
                Ok((retry_text, tokens)) => {
                    meter.record(now, tokens);
                    match attempt(retry_text) {
                        Ok((checked, _)) => checked,
                        Err(reason) => {
                            return BatchResult {
                                ok: false,
                                map,
                                reason,
                                mode: BatchMode::RuleFallback,
                            }
                        }
                    }
                }
                Err(reason) => {
                    return BatchResult {
                        ok: false,
                        map,
                        reason: format!("修复重试仍失败：{reason}"),
                        mode: BatchMode::RuleFallback,
                    }
                }
            }
        }
        Err(reason) => {
            // 解析失败也重试 1 次。
            let retry_prompt = format!("{user_prompt}\n\n{}", repair_rows(std::slice::from_ref(&reason)));
            match chat_completions(fetcher, cfg, &retry_prompt).await {
                Ok((retry_text, tokens)) => {
                    meter.record(now, tokens);
                    match attempt(retry_text) {
                        Ok((checked, _)) => checked,
                        Err(_) => {
                            return BatchResult {
                                ok: false,
                                map,
                                reason,
                                mode: BatchMode::RuleFallback,
                            }
                        }
                    }
                }
                Err(reason2) => {
                    return BatchResult {
                        ok: false,
                        map,
                        reason: format!("修复重试仍失败：{reason2}"),
                        mode: BatchMode::RuleFallback,
                    }
                }
            }
        }
    };
    rows.sort_by_key(|c| c.i);

    for checked in rows {
        let batch_index = checked.i;
        if let Some(&orig) = missing.get(batch_index) {
            let key = ai_hash(
                &notices[orig].title,
                &notices[orig].content,
                &cfg.model,
            );
            let row = LlmRow::from_checked(checked, true);
            cache.put(key, row.clone());
            map.insert(orig, row);
        }
    }
    BatchResult {
        ok: true,
        map,
        reason: String::new(),
        mode: BatchMode::Ai,
    }
}

fn validate_rows_with_dates(
    rows: &[serde_json::Value],
    batch_len: usize,
    published: &[String],
) -> (Vec<CheckedRow>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for row in rows {
        let (mut checked, errs) = validate_rows(std::slice::from_ref(row), batch_len);
        errors.extend(errs);
        if let Some(c) = checked.pop() {
            // 发布日下限：不早于发布日 -1 天（§6.2）。
            let pub_date = published.get(c.i).map(String::as_str).unwrap_or("");
            if !c.deadline.is_empty() && !deadline_after_publish(&c.deadline, pub_date) {
                errors.push(format!("时间早于发布日：{}", c.deadline));
                out.push(CheckedRow {
                    deadline: String::new(),
                    ..c
                });
            } else {
                out.push(c);
            }
        }
    }
    (out, errors)
}

fn deadline_after_publish(deadline: &str, published: &str) -> bool {
    if published.is_empty() || published.len() < 10 {
        return true;
    }
    let Some(pub_day) = chrono::NaiveDate::parse_from_str(&published[..10], "%Y-%m-%d").ok() else {
        return true;
    };
    let Some(due) = cn_core::parse_due_local(&deadline.chars().take(16).collect::<String>()) else {
        return false;
    };
    let floor = pub_day.and_hms_opt(0, 0, 0).map(|d| {
        Local
            .from_local_datetime(&d)
            .earliest()
            .map(|t| t.timestamp())
            .unwrap_or(0)
            - 86_400
    });
    match floor {
        Some(ts) => due.timestamp() >= ts,
        None => true,
    }
}

/// B11 保底：AI 失败整批降级为规则摘要 / pick_tags / find_deadline。
/// 返回的行 `ai=false`，调用方按需采用；任何情况下「摘要 + 对我有什么用」非空。
pub fn summarize_with_fallback(
    result: BatchResult,
    notices: &[LlmNotice],
    keywords: &[String],
    now: DateTime<Local>,
) -> BatchResult {
    if result.ok {
        return result;
    }
    let mut map = result.map;
    for (index, notice) in notices.iter().enumerate() {
        if map.contains_key(&index) {
            continue;
        }
        let text = format!("{} {}", notice.title, notice.content);
        let tags = cn_core::pick_tags(&text, keywords);
        let deadline = cn_core::find_deadline(&text, now)
            .map(|d| d.iso)
            .unwrap_or_default();
        let contact = cn_core::find_contact(&notice.content);
        let rule = cn_core::rule_summary(&SummaryInput {
            title: notice.title.clone(),
            content: notice.content.clone(),
            tags: tags.clone(),
            deadline: if deadline.is_empty() {
                None
            } else {
                Some(deadline.clone())
            },
            contact: if contact.is_empty() {
                None
            } else {
                Some(contact.clone())
            },
            now,
        });
        let score = cn_core::score(&ScoreInput {
            title: notice.title.clone(),
            content: text,
            tags: tags.clone(),
            deadline: if deadline.is_empty() {
                None
            } else {
                Some(deadline.clone())
            },
            keywords: keywords.to_vec(),
            now,
        });
        let _ = score;
        map.insert(
            index,
            LlmRow {
                category: "其他".to_string(),
                gist: rule.summary,
                useful: rule.useful,
                tags,
                deadline,
                contact,
                audience: "不限".to_string(),
                relevant: true,
                confidence: 0.5,
                ai: false,
            },
        );
    }
    BatchResult {
        ok: false,
        map,
        reason: result.reason,
        mode: BatchMode::RuleFallback,
    }
}

// 供 llm.test 便捷构造样例通知。
impl From<&str> for LlmNotice {
    fn from(title: &str) -> Self {
        Self {
            title: title.to_string(),
            published_at: String::new(),
            content: String::new(),
        }
    }
}
