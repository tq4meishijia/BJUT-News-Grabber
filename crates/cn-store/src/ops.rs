//! `act(op, payload) -> ActionResult`：18 个 store op + 5 个凭据类 op（23 个）。
//! 对照 `store.mjs:683-1023` 与 `server.mjs:249-315`。

use serde::Deserialize;
use serde_json::Value;

use cn_model::PreviewItem;

use crate::pipeline;
use crate::service::{CampusService, ItemInput};
use crate::state::{resolve_llm_config, AccountConfig, SECRET_ACCOUNT_PASSWORD, SECRET_LLM_KEY};

/// act 请求（§3.3）。
#[derive(Debug, Clone, Deserialize)]
pub struct ActRequest {
    pub op: String,
    #[serde(flatten)]
    pub payload: Value,
}

impl CampusService {
    /// 统一入口。
    pub async fn act(&mut self, req: ActRequest) -> cn_model::ActionResult {
        let op = req.op.as_str();
        let p = &req.payload;
        match op {
            "poll" => {
                // 临时范围仅本次生效；省略 dateRange 时使用 settings 的默认范围。
                // 校验失败时提前返回 result_err（自带 state），不修改任何设置。
                let today = self.clock.now().format("%Y-%m-%d").to_string();
                let (start, end) = match p.get("dateRange").filter(|v| v.is_object()) {
                    Some(range) => (str_field(range, "start"), str_field(range, "end")),
                    None => (
                        self.store.settings.date_range_start.clone(),
                        self.store.settings.date_range_end.clone(),
                    ),
                };
                let range = match cn_core::validate_range(&start, &end, &today) {
                    Ok(r) => r,
                    Err(message) => return self.result_err(message),
                };
                let active = if range.start.is_empty() && range.end.is_empty() {
                    None
                } else {
                    Some(range)
                };
                let outcome = self.poll_with_range("manual", active).await;
                self.result_with_message(outcome.ok, Some(outcome.message))
            }
            "notice.read" => {
                let id = str_field(p, "id");
                if let Some(notice) = self.store.notices.iter_mut().find(|n| n.id == id) {
                    notice.read = p.get("read").and_then(|v| v.as_bool()).unwrap_or(true);
                }
                self.save_ok()
            }
            "notice.readAll" => {
                for notice in self.store.notices.iter_mut() {
                    notice.read = true;
                }
                self.save_ok()
            }
            "notice.remove" => {
                let id = str_field(p, "id");
                self.remove_notice_and_attachments(&id);
                self.result_ok(None, None)
            }
            "attachment.retry" => {
                let id = str_field(p, "id");
                if !self.store.attachments.iter().any(|a| a.id == id) {
                    return self.result_err("找不到这个附件");
                }
                let vpn = self.webvpn_config();
                let dir = self.attachments_dir();
                let mut logs: Vec<String> = Vec::new();
                let found = crate::attachments::retry_attachment(
                    self.fetcher.as_ref(),
                    &vpn,
                    &dir,
                    &mut self.store,
                    &id,
                    self.clock.as_ref(),
                    &mut logs,
                )
                .await;
                for text in logs {
                    self.emit_pub(crate::service::AppEvent::Log { text });
                }
                let _ = self.save();
                if found {
                    let status = self
                        .store
                        .attachments
                        .iter()
                        .find(|a| a.id == id)
                        .map(|a| a.status)
                        .unwrap_or_default();
                    if status == cn_model::AttachmentStatus::Downloaded {
                        self.result_with_message(true, Some("附件已重新下载".to_string()))
                    } else {
                        self.result_err("附件仍然下载失败，请稍后再试")
                    }
                } else {
                    self.result_err("找不到这个附件")
                }
            }
            "notice.summarize" => self.notice_summarize(&str_field(p, "id")).await,
            "notices.rescore" => {
                let keywords = self.store.settings.keywords.clone();
                let now = self.clock.now();
                let mut count = 0;
                for notice in self.store.notices.iter_mut() {
                    if !notice.content.is_empty() {
                        notice.content = cn_core::strip_red_header(&notice.content);
                    }
                    if notice.ai {
                        continue;
                    }
                    let source_type = self
                        .store
                        .sources
                        .iter()
                        .find(|s| s.id == notice.source_id)
                        .map(|s| s.r#type.clone());
                    let body_text = if !notice.content.is_empty() {
                        notice.content.clone()
                    } else if source_type.as_deref() == Some("json") {
                        notice.context.clone()
                    } else {
                        String::new()
                    };
                    let text = format!("{} {}", notice.title, body_text);
                    let tags = cn_core::pick_tags(&text, &keywords);
                    let deadline = cn_core::find_deadline(&text, now)
                        .map(|d| d.iso)
                        .unwrap_or_default();
                    let contact = cn_core::find_contact(&body_text);
                    let rule = cn_core::rule_summary(&cn_model::SummaryInput {
                        title: notice.title.clone(),
                        content: body_text.clone(),
                        tags: tags.clone(),
                        deadline: non_empty(&deadline),
                        contact: non_empty(&contact),
                        now,
                    });
                    notice.tags = tags.clone();
                    notice.deadline = deadline.clone();
                    notice.contact = contact;
                    notice.summary = rule.summary;
                    notice.useful = rule.useful;
                    notice.score = cn_core::score(&cn_model::ScoreInput {
                        title: notice.title.clone(),
                        content: text,
                        tags,
                        deadline: non_empty(&deadline),
                        keywords: keywords.clone(),
                        now,
                    });
                    count += 1;
                }
                let _ = self.save();
                self.result_with_message(true, Some(format!("已按当前关键词重算 {count} 条摘要")))
            }
            "notices.refill" => self.notices_refill(p).await,
            "item.add" => {
                let item = self.make_item(ItemInput {
                    title: str_field(p, "title"),
                    note: str_field(p, "note"),
                    due_at: str_field(p, "dueAt"),
                    remind_before: u32_list(p, "remindBefore"),
                    source: if str_field(p, "source").is_empty() {
                        "manual".to_string()
                    } else {
                        str_field(p, "source")
                    },
                    notice_id: p.get("noticeId").and_then(|v| v.as_str()).map(str::to_string),
                    url: p.get("url").and_then(|v| v.as_str()).map(str::to_string),
                });
                if item.title.is_empty() {
                    return self.result_err("标题不能为空");
                }
                if item.due_at.is_empty() {
                    return self.result_err("时间格式不对，用 2026-11-05 14:30 这种");
                }
                let id = item.id.clone();
                self.store.items.insert(0, item);
                self.store.trim_storage();
                let _ = self.save();
                let mut result = self.result_with_message(true, Some("已加入日程".to_string()));
                result.id = Some(id);
                result
            }
            "item.update" => {
                let id = str_field(p, "id");
                let Some(item) = self.store.items.iter_mut().find(|i| i.id == id) else {
                    return self.result_err("找不到这个日程");
                };
                if let Some(title) = p.get("title").and_then(|v| v.as_str()) {
                    item.title = title.chars().take(200).collect();
                }
                if let Some(note) = p.get("note").and_then(|v| v.as_str()) {
                    item.note = note.chars().take(600).collect();
                }
                if let Some(due) = p.get("dueAt").and_then(|v| v.as_str()) {
                    if let Some(normalized) = cn_core::normalize_due(due) {
                        item.due_at = normalized;
                        item.fired.clear();
                    }
                }
                if let Some(list) = p.get("remindBefore").and_then(|v| v.as_array()) {
                    item.remind_before = list
                        .iter()
                        .map(|v| {
                            cn_model::state::number_or(v.as_f64(), 30, 0, 100_000)
                        })
                        .collect();
                    item.fired.clear();
                }
                self.save_ok()
            }
            "item.done" => {
                let id = str_field(p, "id");
                let now_iso = self.clock.now().to_rfc3339();
                let Some(item) = self.store.items.iter_mut().find(|i| i.id == id) else {
                    return self.result_err("找不到这个日程");
                };
                item.done = p.get("done").and_then(|v| v.as_bool()).unwrap_or(true);
                item.done_at = if item.done { now_iso } else { String::new() };
                self.save_ok()
            }
            "item.remove" => {
                let id = str_field(p, "id");
                self.store.items.retain(|i| i.id != id);
                self.save_ok()
            }
            "item.refire" => {
                let id = str_field(p, "id");
                if let Some(item) = self.store.items.iter_mut().find(|i| i.id == id) {
                    item.fired.clear();
                }
                self.save_ok()
            }
            "source.add" => {
                let url = str_field(p, "url").trim().to_string();
                let r#type = match str_field(p, "type").as_str() {
                    t @ ("html" | "rss" | "json") => t.to_string(),
                    _ => "html".to_string(),
                };
                if !url.to_lowercase().starts_with("http://") && !url.to_lowercase().starts_with("https://")
                {
                    return self.result_err("请填写 http(s) 开头的网址");
                }
                let fallback_name = url::Url::parse(&url)
                    .ok()
                    .and_then(|u| u.host_str().map(|h| h.to_string()))
                    .unwrap_or_else(|| url.clone());
                let name = {
                    let n = str_field(p, "name");
                    if n.is_empty() {
                        fallback_name
                    } else {
                        n
                    }
                };
                self.store.sources.push(crate::state::SourceRecord {
                    id: format!(
                        "src-{}",
                        crate::state::new_id("", &url, self.clock.now().timestamp_millis(), self.next_counter_pub())
                    ),
                    name: name.chars().take(60).collect(),
                    r#type,
                    url,
                    enabled: p.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                    cookie: str_field(p, "cookie"),
                    resolve_ip: str_field(p, "resolveIp").trim().to_string(),
                    link_pattern: str_field(p, "linkPattern").trim().to_string(),
                    items_path: str_field(p, "itemsPath").trim().to_string(),
                    pages: cn_model::state::number_or(
                        p.get("pages").and_then(|v| v.as_f64()),
                        1,
                        1,
                        5,
                    ),
                    page_url: str_field(p, "pageUrl").trim().to_string(),
                    url_template: str_field(p, "urlTemplate").trim().to_string(),
                    detail_url_template: str_field(p, "detailUrlTemplate").trim().to_string(),
                    content_path: str_field(p, "contentPath").trim().to_string(),
                    map: p.get("map").and_then(|v| v.as_object()).map(|_| p["map"].clone()),
                    note: str_field(p, "note").chars().take(300).collect(),
                    send_to_llm: p.get("sendToLlm").and_then(|v| v.as_bool()).unwrap_or(true),
                    ..crate::state::SourceRecord::default()
                });
                let _ = self.save();
                self.result_with_message(true, Some("已添加数据源".to_string()))
            }
            "source.update" => {
                let id = str_field(p, "id");
                let Some(source) = self.store.sources.iter_mut().find(|s| s.id == id) else {
                    return self.result_err("找不到这个数据源");
                };
                for key in [
                    "name",
                    "type",
                    "url",
                    "resolveIp",
                    "linkPattern",
                    "itemsPath",
                    "note",
                    "cookie",
                    "pageUrl",
                    "urlTemplate",
                    "detailUrlTemplate",
                    "contentPath",
                ] {
                    if let Some(v) = p.get(key).and_then(|v| v.as_str()) {
                        match key {
                            "name" => source.name = v.to_string(),
                            "type" => source.r#type = v.to_string(),
                            "url" => source.url = v.to_string(),
                            "resolveIp" => source.resolve_ip = v.to_string(),
                            "linkPattern" => source.link_pattern = v.to_string(),
                            "itemsPath" => source.items_path = v.to_string(),
                            "note" => source.note = v.to_string(),
                            "cookie" => source.cookie = v.to_string(),
                            "pageUrl" => source.page_url = v.to_string(),
                            "urlTemplate" => source.url_template = v.to_string(),
                            "detailUrlTemplate" => source.detail_url_template = v.to_string(),
                            "contentPath" => source.content_path = v.to_string(),
                            _ => {}
                        }
                    }
                }
                if let Some(map) = p.get("map").and_then(|v| v.as_object()) {
                    let _ = map;
                    source.map = Some(p["map"].clone());
                }
                if let Some(pages) = p.get("pages").and_then(|v| v.as_f64()) {
                    source.pages = cn_model::state::number_or(Some(pages), 1, 1, 5);
                }
                if let Some(enabled) = p.get("enabled").and_then(|v| v.as_bool()) {
                    source.enabled = enabled;
                }
                if let Some(send) = p.get("sendToLlm").and_then(|v| v.as_bool()) {
                    source.send_to_llm = send;
                }
                self.save_ok()
            }
            "source.remove" => {
                let id = str_field(p, "id");
                self.store.sources.retain(|s| s.id != id);
                self.save_ok()
            }
            "source.test" => {
                let id = str_field(p, "id");
                self.test_source(&id).await
            }
            "settings.save" => self.settings_save(p),
            "notify.test" => {
                let fired = true; // onNotify 由事件通道承载
                self.emit_pub(crate::service::AppEvent::Reminder {
                    title: "测试通知".to_string(),
                    label: "如果你看到这条，说明桌面提醒可用。".to_string(),
                });
                self.result_with_message(fired, None)
            }
            // ---- 凭据类 op（server.mjs:249-315）----
            "llm.key" => {
                let api_key = str_field(p, "apiKey").trim().to_string();
                self.secrets
                    .set(SECRET_LLM_KEY, &api_key)
                    .map_err(|e| e.to_string())
                    .expect("secret set");
                self.result_with_message(
                    true,
                    Some(if api_key.is_empty() {
                        "已清空 API Key".to_string()
                    } else {
                        "已保存 API Key".to_string()
                    }),
                )
            }
            "llm.test" => self.llm_test().await,
            "account.save" => self.account_save(p).await,
            "account.test" => {
                let login = match self.auto_login().await {
                    Ok(l) => l,
                    Err(e) => crate::service::LoginOutcome::fail(e.to_string()),
                };
                self.result_with_message(login.ok, Some(login.reason))
            }
            "account.clear" => {
                self.config.account = None;
                let _ = self.secrets.delete(SECRET_ACCOUNT_PASSWORD);
                let _ = self.save_config();
                self.last_login = cn_model::StatusDto {
                    at: self.clock.now().to_rfc3339(),
                    ok: None,
                    message: "已清除保存的账号密码".to_string(),
                };
                self.result_with_message(true, Some("已清除保存的账号密码".to_string()))
            }
            other => self.result_err(format!("未知操作：{other}")),
        }
    }

    fn save_ok(&mut self) -> cn_model::ActionResult {
        let _ = self.save();
        self.result_with_message(true, None)
    }

    fn result_with_message(&self, ok: bool, message: Option<String>) -> cn_model::ActionResult {
        if ok {
            self.result_ok(message, None)
        } else {
            self.result_err(message.unwrap_or_default())
        }
    }

    async fn notice_summarize(&mut self, id: &str) -> cn_model::ActionResult {
        let Some(index) = self.store.notices.iter().position(|n| n.id == id) else {
            return self.result_err("找不到这条通知");
        };
        // §6.4：源级 LLM 开关同样约束手动摘要——正文不出设备。
        let source_opted_out = self
            .store
            .sources
            .iter()
            .find(|s| s.id == self.store.notices[index].source_id)
            .is_some_and(|s| !s.send_to_llm);
        if source_opted_out {
            return self.result_err("该数据源已关闭 AI 摘要（sendToLlm=false），正文不会发送给模型");
        }
        let notice = self.store.notices[index].clone();
        let cfg = resolve_llm_config(&self.config, self.secrets.as_ref());
        let now = self.clock.now();
        let batch = vec![cn_llm::LlmNotice {
            title: notice.title.clone(),
            published_at: notice.published_at.clone(),
            content: notice.content.clone(),
        }];
        self.llm_meter.set_budget(cfg.monthly_token_budget);
        let mut cache = self.llm_cache_guard().await;
        let result = cn_llm::summarize_batch(
            self.fetcher.as_ref(),
            &cfg,
            &batch,
            &mut cache,
            &self.llm_meter,
            now,
        )
        .await;
        drop(cache);
        self.last_llm = cn_model::StatusDto {
            at: now.to_rfc3339(),
            ok: Some(result.ok),
            message: if result.ok {
                String::new()
            } else {
                result.reason.clone()
            },
        };
        if !result.ok {
            return self.result_err(result.reason);
        }
        let Some(row) = result.map.get(&0) else {
            return self.result_err("模型没有返回这条通知的摘要");
        };
        let keywords = self.store.settings.keywords.clone();
        let notice = &mut self.store.notices[index];
        if !row.gist.is_empty() {
            notice.summary = cn_core::cut(&row.gist, 160);
        }
        notice.useful = row
            .useful
            .iter()
            .filter(|s| !s.is_empty())
            .take(4)
            .cloned()
            .collect();
        notice.tags = row
            .tags
            .iter()
            .filter(|s| !s.is_empty())
            .take(5)
            .cloned()
            .collect();
        if !row.deadline.is_empty() {
            notice.deadline = row.deadline.trim().to_string();
        }
        notice.relevant = row.relevant;
        notice.ai = true;
        notice.score = cn_core::score(&cn_model::ScoreInput {
            title: notice.title.clone(),
            content: format!("{} {}", notice.title, notice.content),
            tags: notice.tags.clone(),
            deadline: non_empty(&notice.deadline),
            keywords,
            now,
        });
        self.save_ok()
    }

    async fn notices_refill(&mut self, p: &Value) -> cn_model::ActionResult {
        let limit = cn_model::state::number_or(p.get("limit").and_then(|v| v.as_f64()), 40, 1, 200) as usize;
        let keywords = self.store.settings.keywords.clone();
        let now = self.clock.now();
        let targets: Vec<String> = self
            .store
            .notices
            .iter()
            .filter(|n| n.content.chars().count() < 40)
            .map(|n| n.id.clone())
            .collect();
        let mut filled = 0;
        for id in targets.iter().take(limit) {
            let Some(index) = self.store.notices.iter().position(|n| n.id == *id) else {
                continue;
            };
            let notice = self.store.notices[index].clone();
            let Some(source) = self
                .store
                .sources
                .iter()
                .find(|s| s.id == notice.source_id)
                .cloned()
            else {
                continue;
            };
            if source.detail_url_template.is_empty() {
                continue;
            }
            let detail =
                pipeline::fetch_detail_text(self.fetcher.as_ref(), &notice, &source, &self.webvpn_config())
                    .await;
            if detail.is_empty() {
                continue;
            }
            let notice = &mut self.store.notices[index];
            notice.content = cn_core::strip_red_header(&detail);
            filled += 1;
            let body_text = notice.content.clone();
            let text = format!("{} {}", notice.title, body_text);
            let tags = cn_core::pick_tags(&text, &keywords);
            let deadline = cn_core::find_deadline(&text, now)
                .map(|d| d.iso)
                .unwrap_or_default();
            let contact = cn_core::find_contact(&body_text);
            let rule = cn_core::rule_summary(&cn_model::SummaryInput {
                title: notice.title.clone(),
                content: body_text.clone(),
                tags: tags.clone(),
                deadline: non_empty(&deadline),
                contact: non_empty(&contact),
                now,
            });
            notice.tags = tags.clone();
            notice.deadline = deadline.clone();
            notice.contact = contact;
            notice.summary = rule.summary;
            notice.useful = rule.useful;
            notice.score = cn_core::score(&cn_model::ScoreInput {
                title: notice.title.clone(),
                content: text,
                tags,
                deadline: non_empty(&deadline),
                keywords: keywords.clone(),
                now,
            });
            // 正文补回来才发现的截止时间也要能自动落成日程。
            let auto_due = if self.store.settings.auto_schedule && !notice.deadline.is_empty() {
                let due = cn_core::parse_due_local(&notice.deadline)
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(i64::MIN);
                let exists = self.store.items.iter().any(|i| i.notice_id == notice.id);
                if due != i64::MIN && due > now.timestamp_millis() - 3_600_000 && !exists {
                    Some(ItemInput {
                        title: cn_core::cut(&notice.title, 60),
                        note: format!("来自 {}：{}", notice.source_name, notice.summary),
                        due_at: notice.deadline.clone(),
                        remind_before: self.store.settings.auto_remind_before.clone(),
                        source: "notice".to_string(),
                        notice_id: Some(notice.id.clone()),
                        url: Some(notice.url.clone()),
                    })
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(input) = auto_due {
                let item = self.make_item(input);
                self.store.items.insert(0, item);
            }
            self.store.trim_storage();
        }
        let _ = self.save();
        let remaining = targets.len().saturating_sub(limit);
        self.result_with_message(
            true,
            Some(format!(
                "补齐 {filled} 条正文{}",
                if remaining > 0 {
                    format!("，还有 {remaining} 条可再点一次")
                } else {
                    String::new()
                }
            )),
        )
    }

    fn settings_save(&mut self, p: &Value) -> cn_model::ActionResult {
        let patch = p.get("settings").cloned().unwrap_or(Value::Null);
        // 日期先于任何字段赋值校验：失败直接返回带 state 的错误，供前端回滚。
        let today = self.clock.now().format("%Y-%m-%d").to_string();
        let candidate_start = patch
            .get("dateRangeStart")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| self.store.settings.date_range_start.clone());
        let candidate_end = patch
            .get("dateRangeEnd")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| self.store.settings.date_range_end.clone());
        if let Err(message) = cn_core::validate_range(&candidate_start, &candidate_end, &today) {
            return self.result_err(message);
        }
        let s = &mut self.store.settings;
        if let Some(v) = patch.get("pollMinutes").and_then(|v| v.as_f64()) {
            s.poll_minutes = cn_model::state::number_or(Some(v), 5, 5, 1440);
        }
        if let Some(list) = patch.get("keywords").and_then(|v| v.as_array()) {
            s.keywords = list
                .iter()
                .filter_map(|v| v.as_str())
                .map(|w| w.trim().to_string())
                .filter(|w| !w.is_empty())
                .take(200)
                .collect();
        }
        for key in ["llm", "autoSchedule", "fetchDetail", "notify", "fetchAttachments"] {
            if let Some(v) = patch.get(key).and_then(|v| v.as_bool()) {
                match key {
                    "llm" => s.llm = v,
                    "autoSchedule" => s.auto_schedule = v,
                    "fetchDetail" => s.fetch_detail = v,
                    "notify" => s.notify = v,
                    "fetchAttachments" => s.fetch_attachments = v,
                    _ => {}
                }
            }
        }
        if let Some(v) = patch.get("llmMaxPerPoll").and_then(|v| v.as_f64()) {
            s.llm_max_per_poll = cn_model::state::number_or(Some(v), 8, 1, 40);
        }
        if let Some(v) = patch.get("detailLimit").and_then(|v| v.as_f64()) {
            s.detail_limit = cn_model::state::number_or(Some(v), 25, 5, 60);
        }
        if let Some(v) = patch.get("maxItemsPerSource").and_then(|v| v.as_f64()) {
            // 已知缺陷统一：fallback 100（JS settings.save 回退 25，这里统一为 100）。
            s.max_items_per_source = cn_model::state::number_or(
                Some(v),
                cn_model::state::MAX_ITEMS_PER_SOURCE_DEFAULT,
                1,
                200,
            );
        }
        if patch.get("dateRangeStart").is_some() || patch.get("dateRangeEnd").is_some() {
            // 原样保存用户输入（trim 后）；end > today 的 clamp 在每次抓取时做，
            // 否则结束日期会被冻结在保存当天，之后发布的通知会被全部过滤。
            s.date_range_start = candidate_start.trim().to_string();
            s.date_range_end = candidate_end.trim().to_string();
        }
        if let Some(list) = patch.get("autoRemindBefore").and_then(|v| v.as_array()) {
            if !list.is_empty() {
                s.auto_remind_before = list
                    .iter()
                    .map(|v| cn_model::state::number_or(v.as_f64(), 60, 0, 100_000))
                    .collect();
            }
        }
        if let Some(vpn) = patch.get("webvpn").and_then(|v| v.as_object()) {
            let enabled = vpn
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(s.webvpn.enabled);
            let mut cookie = s.webvpn.cookie.clone();
            if let Some(v) = vpn.get("cookie").and_then(|v| v.as_str()) {
                if !v.trim().is_empty() {
                    cookie = v.trim().to_string();
                }
            }
            if vpn.get("clearCookie").and_then(|v| v.as_bool()) == Some(true) {
                cookie = String::new();
            }
            s.webvpn = crate::state::WebvpnSettings {
                enabled,
                cookie,
                ..crate::state::WebvpnSettings::default()
            };
        }
        let _ = self.save();
        self.result_with_message(true, None)
    }

    async fn llm_test(&mut self) -> cn_model::ActionResult {
        let sample = self.store.notices.first().cloned().unwrap_or_else(|| {
            crate::state::NoticeRecord {
                title: "测试通知：请于2026年11月5日前提交材料".to_string(),
                content: "联系人：张老师".to_string(),
                ..crate::state::NoticeRecord::default()
            }
        });
        let cfg = resolve_llm_config(&self.config, self.secrets.as_ref());
        let now = self.clock.now();
        let batch = vec![cn_llm::LlmNotice {
            title: sample.title.clone(),
            published_at: sample.published_at.clone(),
            content: sample.content.clone(),
        }];
        self.llm_meter.set_budget(cfg.monthly_token_budget);
        let mut cache = self.llm_cache_guard().await;
        let result = cn_llm::summarize_batch(
            self.fetcher.as_ref(),
            &cfg,
            &batch,
            &mut cache,
            &self.llm_meter,
            now,
        )
        .await;
        drop(cache);
        self.last_llm = cn_model::StatusDto {
            at: now.to_rfc3339(),
            ok: Some(result.ok),
            message: if result.ok {
                String::new()
            } else {
                result.reason.clone()
            },
        };
        self.result_with_message(
            result.ok,
            Some(if result.ok {
                "模型可用，返回正常".to_string()
            } else {
                result.reason.clone()
            }),
        )
    }

    async fn account_save(&mut self, p: &Value) -> cn_model::ActionResult {
        let username = str_field(p, "username").trim().to_string();
        let password = str_field(p, "password");
        let cas_base = str_field(p, "casBase").trim().to_string();
        let service = str_field(p, "service").trim().to_string();
        if username.is_empty() {
            return self.result_err("账号不能为空");
        }
        let is_http = |u: &str| {
            u.to_lowercase().starts_with("http://") || u.to_lowercase().starts_with("https://")
        };
        if !is_http(&cas_base) || !is_http(&service) {
            return self.result_err(
                "「认证入口地址」和「登录后返回的地址」都要填 http(s) 开头的完整网址",
            );
        }
        let has_old = self
            .secrets
            .get(SECRET_ACCOUNT_PASSWORD)
            .map(|p| !p.is_empty())
            .unwrap_or(false);
        if !password.trim().is_empty() {
            // 凭据入 SecretStore（P4 换 OS 密钥库）。
            if let Err(e) = self.secrets.set(SECRET_ACCOUNT_PASSWORD, &password) {
                return self.result_err(format!("密码加密失败（{e}）"));
            }
        } else if !has_old {
            return self.result_err("第一次保存需要填写密码");
        }
        self.config.account = Some(AccountConfig {
            cas_base,
            service,
            username,
        });
        let _ = self.save_config();
        let login = match self.auto_login().await {
            Ok(l) => l,
            Err(e) => crate::service::LoginOutcome::fail(e.to_string()),
        };
        self.result_with_message(login.ok, Some(login.reason))
    }

}

fn str_field(p: &Value, key: &str) -> String {
    p.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn u32_list(p: &Value, key: &str) -> Vec<u32> {
    p.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|v| cn_model::state::number_or(v.as_f64(), 30, 0, 100_000))
                .collect()
        })
        .unwrap_or_default()
}

fn non_empty(text: &str) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// PreviewItem 便捷构造（source.test 用）。
pub fn preview_of(items: &[cn_model::RawItem]) -> Vec<PreviewItem> {
    items
        .iter()
        .take(5)
        .map(|i| PreviewItem {
            title: i.title.clone(),
            url: i.url.clone(),
            date: i.published_at.clone(),
        })
        .collect()
}
