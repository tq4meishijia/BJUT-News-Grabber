//! CampusService：store 状态 + 流水线编排 + 自动登录 + StateDto 视图。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cn_model::Clock;
use cn_core::DateRange;
use cn_llm::{LlmCache, LlmNotice, TokenMeter};
use cn_model::{
    AccountDto, ActionResult, AttachmentDto, NoticeDto, PreviewItem, ReminderDto, SettingsDto,
    SourceDto, StateDto, StateItemDto, StatusDto, WebvpnDto,
};

use crate::error::StoreError;
use crate::pipeline::{self, AiRow};
use crate::secrets::{MemSecretStore, SecretStore};
use crate::state::{
    new_id, resolve_llm_config, AppConfig, ItemRecord, NoticeRecord, SourceRecord, StoreData,
    SECRET_ACCOUNT_PASSWORD, SECRET_LLM_KEY,
};

/// 事件处理器。
pub type EventHandler = Box<dyn Fn(&AppEvent) + Send + Sync>;

/// 广播事件（trait 注入，不直接依赖 tauri）。
#[derive(Debug, Clone)]
pub enum AppEvent {
    StateChanged { version: u64 },
    Reminder { title: String, label: String },
    Log { text: String },
    /// 附件下载进度（T04 桥接成 `attachment://progress`）。
    AttachmentProgress {
        done: usize,
        total: usize,
        file_name: String,
    },
}

/// 流水线时序参数（测试可调零，生产用默认值）。
#[derive(Debug, Clone)]
pub struct PipelineOptions {
    pub source_gap: std::time::Duration,
    pub same_host_gap: std::time::Duration,
    pub detail_gap: std::time::Duration,
    pub total_timeout: std::time::Duration,
}

impl Default for PipelineOptions {
    fn default() -> Self {
        Self {
            source_gap: std::time::Duration::from_millis(250),
            same_host_gap: std::time::Duration::from_millis(300),
            detail_gap: std::time::Duration::from_millis(120),
            total_timeout: std::time::Duration::from_secs(5 * 60),
        }
    }
}

/// 服务主体。网络 / 时钟 / 凭据 / 通知全部注入。
pub struct CampusService {
    pub store: StoreData,
    pub config: AppConfig,
    pub(crate) store_path: PathBuf,
    pub(crate) config_path: PathBuf,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) fetcher: Arc<dyn cn_net::Fetcher>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) options: PipelineOptions,
    polling: AtomicBool,
    pub last_llm: StatusDto,
    pub last_login: StatusDto,
    pub port: u32,
    version: Mutex<u64>,
    pub(crate) counter: Mutex<u64>,
    pub(crate) llm_cache: tokio::sync::Mutex<LlmCache>,
    pub(crate) llm_meter: TokenMeter,
    on_event: Option<EventHandler>,
}

impl CampusService {
    /// 组装服务；store/config 从磁盘加载。
    pub fn new(
        store_path: PathBuf,
        config_path: PathBuf,
        secrets: Arc<dyn SecretStore>,
        fetcher: Arc<dyn cn_net::Fetcher>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let store = crate::persist::load_store(&store_path);
        let config = load_config(&config_path);
        let port = config.port;
        Self {
            store,
            config,
            store_path,
            config_path,
            secrets,
            fetcher,
            clock,
            options: PipelineOptions::default(),
            polling: AtomicBool::new(false),
            last_llm: StatusDto::default(),
            last_login: StatusDto::default(),
            port,
            version: Mutex::new(1),
            counter: Mutex::new(0),
            llm_cache: tokio::sync::Mutex::new(LlmCache::new()),
            llm_meter: TokenMeter::new(0),
            on_event: None,
        }
    }

    /// 测试快捷组装（内存凭据 + 调好的时序）。
    pub fn new_for_test(
        store_path: PathBuf,
        fetcher: Arc<dyn cn_net::Fetcher>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let mut svc = Self::new(
            store_path,
            PathBuf::from("config.json"),
            Arc::new(MemSecretStore::new()),
            fetcher,
            clock,
        );
        svc.options = PipelineOptions {
            source_gap: std::time::Duration::ZERO,
            same_host_gap: std::time::Duration::ZERO,
            detail_gap: std::time::Duration::ZERO,
            total_timeout: std::time::Duration::from_secs(60),
        };
        svc
    }

    /// 覆盖时序参数（测试用）。
    pub fn with_options(mut self, options: PipelineOptions) -> Self {
        self.options = options;
        self
    }

    /// 注册广播回调。
    pub fn set_event_handler(&mut self, handler: EventHandler) {
        self.on_event = Some(handler);
    }

    /// 注入凭据（测试/启动时）。
    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    fn emit(&self, event: AppEvent) {
        if let Some(handler) = &self.on_event {
            handler(&event);
        }
    }

    /// 单调版本号（每次 save 递增；前端只应用更大版本）。
    pub fn state_version(&self) -> u64 {
        *self.version.lock().expect("version")
    }

    fn next_counter(&self) -> u64 {
        let mut c = self.counter.lock().expect("counter");
        *c += 1;
        *c
    }

    /// ops 模块访问 LLM 缓存（tokio Mutex，可跨 await 持有）。
    pub(crate) async fn llm_cache_guard(&self) -> tokio::sync::MutexGuard<'_, LlmCache> {
        self.llm_cache.lock().await
    }

    /// ops 模块递增计数器。
    pub(crate) fn next_counter_pub(&self) -> u64 {
        self.next_counter()
    }

    /// ops 模块发事件。
    pub(crate) fn emit_pub(&self, event: AppEvent) {
        self.emit(event);
    }

    /// 附件落盘目录：`store.json` 同级的 `attachments/`（生产即 data_dir/attachments）。
    pub fn attachments_dir(&self) -> PathBuf {
        self.store_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("attachments")
    }

    /// 删除通知并级联删除最后一个引用所对应的本地附件与记录（B12 / B8）。
    /// 文件 IO 失败只记日志，不影响 op 成功。
    pub fn remove_notice_and_attachments(&mut self, id: &str) {
        let dir = self.attachments_dir();
        let mut logs: Vec<String> = Vec::new();
        crate::attachments::cascade_remove(&mut self.store, &dir, id, &mut logs);
        self.store.notices.retain(|n| n.id != id);
        for text in logs {
            self.emit(AppEvent::Log { text });
        }
        let _ = self.save();
    }

    /// 保存（原子落盘）+ 版本递增 + 广播。
    pub fn save(&self) -> Result<(), StoreError> {
        crate::persist::save_store(&self.store_path, &self.store)?;
        let mut v = self.version.lock().expect("version");
        *v += 1;
        let version = *v;
        drop(v);
        self.emit(AppEvent::StateChanged { version });
        Ok(())
    }

    /// `store.mjs::makeItem`。
    pub fn make_item(&self, input: ItemInput) -> ItemRecord {
        let now = self.clock.now();
        let remind_before: Vec<u32> = if input.remind_before.is_empty() {
            self.store.settings.auto_remind_before.clone()
        } else {
            input
                .remind_before
                .iter()
                .map(|n| cn_model::state::number_or(Some(*n as f64), 30, 0, 100_000))
                .collect()
        };
        ItemRecord {
            id: new_id("s", &input.title, now.timestamp_millis(), self.next_counter()),
            title: input.title.trim().chars().take(200).collect(),
            note: input.note.chars().take(600).collect(),
            due_at: cn_core::normalize_due(&input.due_at).unwrap_or_default(),
            remind_before,
            fired: Vec::new(),
            done: false,
            done_at: String::new(),
            created_at: now.to_rfc3339(),
            source: if input.source.is_empty() {
                "manual".to_string()
            } else {
                input.source
            },
            notice_id: input.notice_id.unwrap_or_default(),
            url: input.url.unwrap_or_default(),
        }
    }

    /// `store.mjs::state()` —— StateDto 视图（B9/B12）。
    pub fn state(&self) -> StateDto {
        let vpn = &self.store.settings.webvpn;
        let webvpn = WebvpnDto {
            enabled: vpn.enabled,
            gateway: cn_net::WEBVPN_GATEWAY.to_string(),
            has_cookie: !vpn.cookie.trim().is_empty(),
        };
        let settings = SettingsDto {
            poll_minutes: self.store.settings.poll_minutes,
            keywords: self.store.settings.keywords.clone(),
            llm: self.store.settings.llm,
            llm_max_per_poll: self.store.settings.llm_max_per_poll,
            auto_schedule: self.store.settings.auto_schedule,
            auto_remind_before: self.store.settings.auto_remind_before.clone(),
            max_items_per_source: self.store.settings.max_items_per_source,
            fetch_detail: self.store.settings.fetch_detail,
            detail_limit: self.store.settings.detail_limit,
            notify: self.store.settings.notify,
            webvpn,
            date_range_start: self.store.settings.date_range_start.clone(),
            date_range_end: self.store.settings.date_range_end.clone(),
            fetch_attachments: self.store.settings.fetch_attachments,
        };
        let sources = self
            .store
            .sources
            .iter()
            .map(|s| SourceDto {
                id: s.id.clone(),
                name: s.name.clone(),
                r#type: s.r#type.clone(),
                url: s.url.clone(),
                enabled: s.enabled,
                note: s.note.clone(),
                has_cookie: !s.cookie.trim().is_empty(),
                login_expired: s.login_expired,
                resolve_ip: s.resolve_ip.clone(),
                link_pattern: s.link_pattern.clone(),
                items_path: s.items_path.clone(),
                pages: s.pages,
                page_url: s.page_url.clone(),
                url_template: s.url_template.clone(),
                detail_url_template: s.detail_url_template.clone(),
                content_path: s.content_path.clone(),
                map: s.map.clone(),
                send_to_llm: s.send_to_llm,
                status: s.status.clone(),
                status_text: s.status_text.clone(),
                last_fetch_at: s.last_fetch_at.clone(),
                last_count: s.last_count,
            })
            .collect();
        let notices: Vec<NoticeDto> = self
            .store
            .notices
            .iter()
            .take(200)
            .map(not_to_dto)
            .collect();
        let items: Vec<StateItemDto> = self
            .store
            .items
            .iter()
            .take(300)
            .map(|i| StateItemDto {
                id: i.id.clone(),
                title: i.title.clone(),
                note: i.note.clone(),
                due_at: i.due_at.clone(),
                remind_before: i.remind_before.clone(),
                fired: i.fired.clone(),
                done: i.done,
                done_at: i.done_at.clone(),
                created_at: i.created_at.clone(),
                source: i.source.clone(),
                notice_id: i.notice_id.clone(),
                url: i.url.clone(),
            })
            .collect();
        let reminders: Vec<ReminderDto> = self
            .store
            .meta
            .reminders
            .iter()
            .take(20)
            .map(|r| ReminderDto {
                id: r.id.clone(),
                item_id: r.item_id.clone(),
                title: r.title.clone(),
                note: r.note.clone(),
                due_at: r.due_at.clone(),
                at: r.at.clone(),
            })
            .collect();
        let account = self.config.account.clone().unwrap_or_default();
        let configured = self.can_auto_login();
        let (llm_used, llm_budget) = self.llm_meter.usage(self.clock.now());
        let attachments_dir = self.attachments_dir();
        let attachments: Vec<AttachmentDto> = self
            .store
            .attachments
            .iter()
            .take(300)
            .map(|a| AttachmentDto {
                id: a.id.clone(),
                notice_ids: a.notice_ids.clone(),
                source_url: a.source_url.clone(),
                file_name: a.file_name.clone(),
                status: a.status,
                size_bytes: a.size_bytes,
                fetched_at: a.fetched_at.clone(),
                error: a.error.clone(),
                local_path: attachments_dir
                    .join(&a.local_path)
                    .to_string_lossy()
                    .into_owned(),
            })
            .collect();
        StateDto {
            settings,
            sources,
            notices,
            items,
            attachments,
            meta: cn_model::MetaDto {
                last_poll_at: self.store.meta.last_poll_at.clone(),
                last_poll_error: self.store.meta.last_poll_error.clone(),
                poll_count: self.store.meta.poll_count,
                reminders,
                llm: self.last_llm.clone(),
                account: AccountDto {
                    username: account.username,
                    cas_base: account.cas_base,
                    service: account.service,
                    configured,
                    last_login: self.last_login.clone(),
                },
                polling: self.polling.load(Ordering::SeqCst),
                data_path: self.store_path.to_string_lossy().into_owned(),
                port: self.port,
                llm_key_set: self
                    .secrets
                    .get(SECRET_LLM_KEY)
                    .map(|k| !k.trim().is_empty())
                    .unwrap_or(false),
                llm_used,
                llm_budget,
            },
        }
    }

    /// `store.mjs::webvpnConfig`。
    pub fn webvpn_config(&self) -> cn_net::WebvpnConfig {
        self.store.settings.webvpn.to_net_config()
    }

    /// `store.mjs::canAutoLogin`。
    pub fn can_auto_login(&self) -> bool {
        match &self.config.account {
            Some(a) => {
                !a.cas_base.is_empty()
                    && !a.service.is_empty()
                    && !a.username.is_empty()
                    && self
                        .secrets
                        .get(SECRET_ACCOUNT_PASSWORD)
                        .map(|p| !p.is_empty())
                        .unwrap_or(false)
            }
            None => false,
        }
    }

    /// `store.mjs::autoLogin`（含回调地址候选自动纠正）。
    pub async fn auto_login(&mut self) -> Result<LoginOutcome, StoreError> {
        let now = self.clock.now();
        let Some(account) = self.config.account.clone() else {
            return Ok(LoginOutcome::fail("还没保存统一身份认证账号密码（设置页可以填）"));
        };
        if account.username.is_empty() {
            return Ok(LoginOutcome::fail("还没保存统一身份认证账号密码（设置页可以填）"));
        }
        if account.cas_base.is_empty() || account.service.is_empty() {
            return Ok(LoginOutcome::fail(
                "还没填「认证入口地址」和「登录后返回的地址」（设置页可以填）",
            ));
        }
        let Some(password) = self.secrets.get(SECRET_ACCOUNT_PASSWORD) else {
            let message = "密码解密失败：换了 Windows 用户或换了电脑，需要重新保存一次密码";
            self.last_login = StatusDto {
                at: now.to_rfc3339(),
                ok: Some(false),
                message: message.to_string(),
            };
            return Ok(LoginOutcome::fail(message));
        };
        let vpn = self.webvpn_config();
        let effective_service = if vpn.enabled {
            cn_net::restore_url(&account.service, &vpn)
        } else {
            account.service.clone()
        };

        let mut result = cn_net::cas_login(
            self.fetcher.as_ref(),
            &account.cas_base,
            &account.service,
            &account.username,
            &password,
            Some(&vpn),
        )
        .await;

        // CAS 成功 ≠ 门户认你：用新 Cookie 探测同域数据源。
        if result.is_ok() {
            let portal_host = host_of(&effective_service);
            let alive = if portal_host.is_empty() {
                true
            } else {
                let cookie = result
                    .as_ref()
                    .map(|s| s.cookie.clone())
                    .unwrap_or_default();
                self.probe_login_cookie(&cookie, &portal_host).await
            };
            if !alive {
                result = Err(cn_model::AuthError::new(
                    cn_model::AuthErrorCode::NoPortalSession,
                    cn_net::NO_PORTAL_SESSION,
                ));
            }
        }

        // 回调地址候选自动纠正。
        if let Err(err) = &result {
            if err.code == cn_model::AuthErrorCode::NoPortalSession
                && !account.service.contains("actionCasLogin")
                && !account.service.contains("casLogin")
                && !account.service.contains("redirect_url")
            {
                for candidate in cn_net::cas_callback_candidates(&effective_service) {
                    let fixed = cn_net::cas_login(
                        self.fetcher.as_ref(),
                        &account.cas_base,
                        &candidate,
                        &account.username,
                        &password,
                        Some(&vpn),
                    )
                    .await;
                    match fixed {
                        Ok(success) => {
                            let host = host_of(&candidate);
                            let ok = if host.is_empty() {
                                true
                            } else {
                                self.probe_login_cookie(&success.cookie, &host).await
                            };
                            if ok {
                                result = Ok(success);
                                self.emit(AppEvent::Log {
                                    text: format!("自动换用认证回调地址登录成功：{candidate}"),
                                });
                                if let Some(a) = self.config.account.as_mut() {
                                    a.service = candidate;
                                }
                                let _ = self.save_config();
                                break;
                            }
                        }
                        Err(fixed_err) => {
                            if fixed_err.code != cn_model::AuthErrorCode::NoPortalSession {
                                result = Err(fixed_err);
                                break;
                            }
                        }
                    }
                }
            }
        }

        let outcome = match result {
            Ok(success) => {
                // 只更新同一门户域名下的数据源。
                let portal_host = host_of(&effective_service);
                let mut updated = 0u32;
                for source in self.store.sources.iter_mut() {
                    let host = host_of(&source.url);
                    let match_host = !portal_host.is_empty()
                        && (host == portal_host
                            || host.ends_with(&format!(".{portal_host}"))
                            || host.contains(&portal_host));
                    if match_host {
                        source.cookie = success.cookie.clone();
                        source.login_expired = false;
                        if source.status == "error" {
                            source.status = "idle".to_string();
                            source.status_text = "登录态已更新，等待下次抓取".to_string();
                        }
                        updated += 1;
                    }
                }
                let _ = self.save();
                let message = format!("自动登录成功，已更新 {updated} 个数据源的登录态");
                self.last_login = StatusDto {
                    at: self.clock.now().to_rfc3339(),
                    ok: Some(true),
                    message: message.clone(),
                };
                LoginOutcome {
                    ok: true,
                    updated,
                    reason: message,
                }
            }
            Err(err) => {
                let reason = err.detail.clone();
                self.last_login = StatusDto {
                    at: self.clock.now().to_rfc3339(),
                    ok: Some(false),
                    message: reason.clone(),
                };
                LoginOutcome::fail(reason)
            }
        };
        Ok(outcome)
    }

    async fn probe_login_cookie(&mut self, cookie: &str, portal_host: &str) -> bool {
        // 找同域名数据源，用新 Cookie 探测一次。
        let Some(index) = self.store.sources.iter().position(|s| {
            host_of(&s.url) == portal_host
        }) else {
            return true;
        };
        let mut source = self.store.sources[index].clone();
        let backup = source.cookie.clone();
        source.cookie = cookie.to_string();
        let vpn = self.webvpn_config();
        let result = pipeline::fetch_page(self.fetcher.as_ref(), &source, &source.url, &vpn).await;
        self.store.sources[index].cookie = backup;
        match result {
            Ok(_) => true,
            Err(err) => {
                let text = err.to_string();
                !(text.contains("登录态失效")
                    || text.contains("需要登录")
                    || text.contains("请登录")
                    || text.contains("WebVPN 网关会话已过期"))
            }
        }
    }

    /// `store.mjs::poll`（§4.1 十二步流水线，B1/B2/B3/B4/B5/B7/B11）。
    /// 托盘 / 定时器入口：使用设置页的默认时间范围；校验失败降级为不过滤（绝不 panic）。
    pub async fn poll(&mut self, reason: &str) -> PollOutcome {
        let today = self.clock.now().format("%Y-%m-%d").to_string();
        let start = self.store.settings.date_range_start.clone();
        let end = self.store.settings.date_range_end.clone();
        let range = match cn_core::validate_range(&start, &end, &today) {
            Ok(active) if !(active.start.is_empty() && active.end.is_empty()) => Some(active),
            _ => None,
        };
        self.poll_with_range(reason, range).await
    }

    /// 带本次抓取时间范围（不写回 settings）的抓取入口。
    pub async fn poll_with_range(&mut self, reason: &str, range: Option<DateRange>) -> PollOutcome {
        let _ = reason;
        if self.polling.swap(true, Ordering::SeqCst) {
            return PollOutcome::fail("正在抓取中");
        }
        let outcome = self.poll_inner(range).await;
        self.polling.store(false, Ordering::SeqCst);
        outcome
    }

    async fn poll_inner(&mut self, range: Option<DateRange>) -> PollOutcome {
        let started_at = self.clock.now().to_rfc3339();
        let deadline_instant = self.clock.now() + chrono::Duration::from_std(self.options.total_timeout).unwrap_or_default();
        let enabled: Vec<SourceRecord> = self
            .store
            .sources
            .iter()
            .filter(|s| s.enabled && !s.url.is_empty())
            .cloned()
            .collect();
        if enabled.is_empty() {
            self.store.meta.last_poll_at = started_at.clone();
            self.store.meta.last_poll_error = "没有启用任何数据源".to_string();
            let _ = self.save();
            return PollOutcome::fail("没有启用任何数据源");
        }

        let known: std::collections::HashSet<String> = self
            .store
            .notices
            .iter()
            .map(|n| n.dedupe_key.clone())
            .collect();
        let mut known = known;
        let mut fresh: Vec<NoticeRecord> = Vec::new();
        // 本次抓取统计（不写回 settings）。
        let mut date_skipped = 0usize;
        let mut attachments_downloaded = 0usize;
        let mut attachments_failed = 0usize;
        let mut attachments_skipped = 0usize;

        // B2：登录态过期时自动登录重试一轮，只重跑失败的那几个源。
        let attempts = if self.can_auto_login() { 2 } else { 1 };
        let mut pending = enabled.clone();
        let mut prev_host = String::new();
        for attempt in 0..attempts {
            let mut login_failures: Vec<SourceRecord> = Vec::new();
            for source in &pending {
                // 单轮总超时：按部分成功返回。
                if self.clock.now() > deadline_instant {
                    break;
                }
                // 同 host 最小间隔 300ms；异源 250ms。
                let host = host_of(&source.url);
                let gap = if !prev_host.is_empty() && host == prev_host {
                    self.options.same_host_gap
                } else {
                    self.options.source_gap
                };
                sleep(gap).await;
                prev_host = host;

                match pipeline::fetch_source(
                    self.fetcher.as_ref(),
                    source,
                    &self.webvpn_config(),
                    self.clock.now(),
                )
                .await
                {
                    Ok(items) => {
                        let total = items.len();
                        let sliced: Vec<_> = items
                            .into_iter()
                            .take(self.store.settings.max_items_per_source as usize)
                            .collect();
                        let mut added = 0u32;
                        for item in sliced {
                            // 时间过滤必须在最前面：范围外通知不建去重键、不进 known，
                            // 也就不会进详情 / LLM / 建日程（A3）。
                            if let Some(active) = range.as_ref() {
                                if !cn_core::in_date_range(&item.published_at, active) {
                                    date_skipped += 1;
                                    continue;
                                }
                            }
                            let dedupe_key = pipeline::build_dedupe_key(&source.id, &item.title);
                            if known.contains(&dedupe_key) {
                                continue;
                            }
                            known.insert(dedupe_key.clone());
                            let counter = self.next_counter();
                            fresh.push(NoticeRecord {
                                id: new_id("n", &dedupe_key, self.clock.now().timestamp_millis(), counter),
                                dedupe_key,
                                source_id: source.id.clone(),
                                source_name: source.name.clone(),
                                source_item_id: item.item_id,
                                title: item.title,
                                url: item.url,
                                published_at: item.published_at,
                                context: item.context,
                                fetched_at: started_at.clone(),
                                ..NoticeRecord::default()
                            });
                            added += 1;
                        }
                        if let Some(s) = self.store.sources.iter_mut().find(|s| s.id == source.id) {
                            s.status = "ok".to_string();
                            s.status_text = format!("抓到 {total} 条，新增 {added} 条");
                            s.last_fetch_at = started_at.clone();
                            s.last_count = added;
                            s.login_expired = false;
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        if let Some(s) = self.store.sources.iter_mut().find(|s| s.id == source.id) {
                            s.status = "error".to_string();
                            s.status_text = message.clone();
                            s.last_fetch_at = started_at.clone();
                            if message.contains("登录态失效") || message.contains("需要登录") {
                                s.login_expired = true;
                                login_failures.push(source.clone());
                            }
                        }
                    }
                }
            }
            if login_failures.is_empty() || attempt == attempts - 1 {
                break;
            }
            self.emit(AppEvent::Log {
                text: format!(
                    "检测到 {} 个数据源登录态失效，尝试自动登录…",
                    login_failures.len()
                ),
            });
            let login = match self.auto_login().await {
                Ok(l) => l,
                Err(e) => LoginOutcome::fail(e.to_string()),
            };
            if !login.ok {
                self.emit(AppEvent::Log {
                    text: format!("自动登录失败：{}", login.reason),
                });
                break;
            }
            pending = login_failures;
        }

        // 详情跟进（B3）：按标题命中关键词数排序，限 10–40。
        // 附件发现复用这一循环（一次请求既取正文又提取链接），不额外翻倍请求。
        // T06 附件追溯补抓：附件开关开启时，从已入库通知中挑「尚无附件记录」的补位
        // （按 published_at 倒序，新的优先），总数仍受 limit 约束，避免一次几百条
        // 历史通知把抓取拖死。关闭附件开关时候选集只含 fresh，行为与改动前完全一致。
        let detail_enabled = self.store.settings.fetch_detail;
        let attachments_enabled = self.store.settings.fetch_attachments;
        let mut attachment_events: Vec<(usize, usize, String)> = Vec::new();
        let mut attachment_logs: Vec<String> = Vec::new();
        let mut attempted_attachments: std::collections::HashSet<String> = std::collections::HashSet::new();
        if detail_enabled || attachments_enabled {
            // 本轮处理对象：fresh 用下标（尚未并入 store.notices），历史通知用 id 定位
            // （候选来源不同，下标会错位）。
            enum Candidate {
                Fresh(usize),
                History(String),
            }
            let keywords = self.store.settings.keywords.clone();
            let limit = self.store.settings.detail_limit.clamp(10, 40) as usize;
            let vpn_for_attachments = self.webvpn_config();
            let dir_for_attachments = self.attachments_dir();

            // 1) fresh 候选：保持原有「关键词命中数倒序」排序。
            let mut ranked: Vec<(usize, usize)> = fresh
                .iter()
                .enumerate()
                .map(|(idx, n)| {
                    let hits = keywords
                        .iter()
                        .filter(|w| !w.is_empty() && n.title.contains(w.as_str()))
                        .count();
                    (idx, hits)
                })
                .collect();
            ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
            let mut candidates: Vec<Candidate> = ranked
                .into_iter()
                .take(limit)
                .map(|(idx, _)| Candidate::Fresh(idx))
                .collect();

            // 2) 历史补抓候选：仅附件开关开启且有剩余名额时补位（T06）。
            //    快照「已有附件的通知 id」集合，避免在 filter 闭包里反复扫描 attachments。
            let mut history_added = 0usize;
            if attachments_enabled && candidates.len() < limit {
                let fresh_ids: std::collections::HashSet<String> =
                    fresh.iter().map(|n| n.id.clone()).collect();
                let attached_ids: std::collections::HashSet<String> = self
                    .store
                    .attachments
                    .iter()
                    .flat_map(|a| a.notice_ids.iter().cloned())
                    .collect();
                let mut history: Vec<(String, String)> = self
                    .store
                    .notices
                    .iter()
                    .filter(|n| {
                        !fresh_ids.contains(&n.id)
                            && !attached_ids.contains(&n.id)
                            // T06b 负缓存：只挑「从未扫描过附件」的通知，避免无附件的
                            // 老通知每轮重复占用补抓名额、其他通知永远轮不到。
                            && n.attachments_scanned_at.is_empty()
                    })
                    .map(|n| {
                        // 排序键：有发布日期用日期，否则退回抓取时间（同为 ISO 串，字典序即时间序）。
                        let sort_key = if n.published_at.is_empty() {
                            n.fetched_at.clone()
                        } else {
                            n.published_at.clone()
                        };
                        (sort_key, n.id.clone())
                    })
                    .collect();
                history.sort_by(|a, b| b.0.cmp(&a.0));
                for (_, id) in history.into_iter().take(limit - candidates.len()) {
                    candidates.push(Candidate::History(id));
                    history_added += 1;
                }
                if history_added > 0 {
                    attachment_logs.push(format!("附件补抓 {history_added} 条历史通知"));
                }
            }

            // 3) 逐条处理：fresh 与历史补抓都走一次详情请求（正文 + 附件 HTML）。
            for candidate in candidates {
                if self.clock.now() > deadline_instant {
                    break;
                }
                match candidate {
                    Candidate::Fresh(idx) => {
                        let source_id = fresh[idx].source_id.clone();
                        let Some(source) = self
                            .store
                            .sources
                            .iter()
                            .find(|s| s.id == source_id)
                            .cloned()
                        else {
                            continue;
                        };
                        if fresh[idx].url.is_empty() && source.detail_url_template.is_empty() {
                            continue;
                        }
                        if !fresh[idx].url.is_empty() && fresh[idx].url == source.url {
                            continue;
                        }
                        let bundle = pipeline::fetch_detail_bundle(
                            self.fetcher.as_ref(),
                            &fresh[idx],
                            &source,
                            &vpn_for_attachments,
                        )
                        .await;
                        if detail_enabled && !bundle.text.is_empty() {
                            fresh[idx].content = cn_core::strip_red_header(&bundle.text);
                        }
                        if attachments_enabled {
                            let id_seed = self.next_counter();
                            let stats = crate::attachments::discover_and_download(
                                self.fetcher.as_ref(),
                                &vpn_for_attachments,
                                &dir_for_attachments,
                                &mut self.store,
                                &source,
                                &fresh[idx],
                                &bundle,
                                self.clock.as_ref(),
                                &mut attempted_attachments,
                                id_seed,
                                &mut attachment_logs,
                                |done, total, file_name| {
                                    attachment_events.push((done, total, file_name));
                                },
                            )
                            .await;
                            attachments_downloaded += stats.downloaded;
                            attachments_failed += stats.failed;
                            attachments_skipped += stats.skipped;
                            // T06b：fresh 通知同样记扫描时间（与历史补抓口径一致，
                            // 便于诊断与未来扩展；fresh 本身靠去重不会重复入选）。
                            fresh[idx].attachments_scanned_at = self.clock.now().to_rfc3339();
                        }
                        sleep(self.options.detail_gap).await;
                    }
                    Candidate::History(id) => {
                        // 快照通知与数据源：随后既要 &mut self.store 写回正文，又要把它
                        // 交给附件发现函数，克隆可彻底规避「同时借 notices 与 &mut store」。
                        let Some(notice) = self.store.notices.iter().find(|n| n.id == id).cloned()
                        else {
                            continue;
                        };
                        let Some(source) = self
                            .store
                            .sources
                            .iter()
                            .find(|s| s.id == notice.source_id)
                            .cloned()
                        else {
                            continue;
                        };
                        if notice.url.is_empty() && source.detail_url_template.is_empty() {
                            continue;
                        }
                        if !notice.url.is_empty() && notice.url == source.url {
                            continue;
                        }
                        // 附件链接只能从详情 HTML 提取，不能用已存的纯文本 content。
                        let bundle = pipeline::fetch_detail_bundle(
                            self.fetcher.as_ref(),
                            &notice,
                            &source,
                            &vpn_for_attachments,
                        )
                        .await;
                        // 顺带收益：原正文为空且用户开启详情跟进时补正文；
                        // 关闭详情跟进时只取 HTML 供附件解析，不写 content（尊重设置）。
                        if detail_enabled && notice.content.is_empty() && !bundle.text.is_empty() {
                            let content = cn_core::strip_red_header(&bundle.text);
                            if let Some(record) = self.store.notices.iter_mut().find(|n| n.id == id) {
                                record.content = content;
                            }
                        }
                        if attachments_enabled {
                            let id_seed = self.next_counter();
                            let stats = crate::attachments::discover_and_download(
                                self.fetcher.as_ref(),
                                &vpn_for_attachments,
                                &dir_for_attachments,
                                &mut self.store,
                                &source,
                                &notice,
                                &bundle,
                                self.clock.as_ref(),
                                &mut attempted_attachments,
                                id_seed,
                                &mut attachment_logs,
                                |done, total, file_name| {
                                    attachment_events.push((done, total, file_name));
                                },
                            )
                            .await;
                            attachments_downloaded += stats.downloaded;
                            attachments_failed += stats.failed;
                            attachments_skipped += stats.skipped;
                        }
                        // T06b 负缓存：扫描完成即记账——无论是否发现附件、下载成功或失败、
                        // 甚至详情请求网络失败（bundle 为空）都写入，避免同一条老通知每轮
                        // 重复占用补抓名额、把其它通知永远挡在门外。
                        let scanned_at = self.clock.now().to_rfc3339();
                        if let Some(record) = self.store.notices.iter_mut().find(|n| n.id == id) {
                            record.attachments_scanned_at = scanned_at;
                        }
                        sleep(self.options.detail_gap).await;
                    }
                }
            }
        }
        for (done, total, file_name) in attachment_events {
            self.emit(AppEvent::AttachmentProgress {
                done,
                total,
                file_name,
            });
        }
        for text in attachment_logs {
            self.emit(AppEvent::Log { text });
        }

        // LLM 批量 + 逐条补全（B11）。
        let keywords = self.store.settings.keywords.clone();
        let ai_rows = self.summarize(&fresh).await;
        for (idx, notice) in fresh.iter_mut().enumerate() {
            let source = self
                .store
                .sources
                .iter()
                .find(|s| s.id == notice.source_id);
            let ai = ai_rows.as_ref().and_then(|rows| rows.get(&idx)).map(|row| AiRow {
                gist: row.gist.clone(),
                useful: row.useful.clone(),
                tags: row.tags.clone(),
                deadline: row.deadline.clone(),
                contact: row.contact.clone(),
                relevant: row.relevant,
            });
            pipeline::enrich_notice(notice, source, &keywords, ai.as_ref(), self.clock.now());
            notice.updated_at = started_at.clone();
        }

        // 排序：publishedAt 倒序，同日按 score 倒序；并入头部。
        fresh.sort_by(|a, b| {
            let da = if a.published_at.is_empty() { &a.fetched_at } else { &a.published_at };
            let db = if b.published_at.is_empty() { &b.fetched_at } else { &b.published_at };
            db.cmp(da).then(b.score.cmp(&a.score))
        });
        let mut merged = fresh.clone();
        merged.extend(self.store.notices.iter().cloned());
        self.store.notices = merged;
        self.store.trim_storage();

        // 自动建日程（B7）。
        if self.store.settings.auto_schedule {
            for notice in &fresh {
                if notice.deadline.is_empty() || !notice.relevant {
                    continue;
                }
                let due = cn_core::parse_due_local(&notice.deadline)
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(i64::MIN);
                if due == i64::MIN || due < self.clock.now().timestamp_millis() - 3_600_000 {
                    continue;
                }
                if self.store.items.iter().any(|i| i.notice_id == notice.id) {
                    continue;
                }
                let item = self.make_item(ItemInput {
                    title: cn_core::cut(&notice.title, 60),
                    note: format!("来自 {}：{}", notice.source_name, notice.summary),
                    due_at: notice.deadline.clone(),
                    remind_before: self.store.settings.auto_remind_before.clone(),
                    source: "notice".to_string(),
                    notice_id: Some(notice.id.clone()),
                    url: Some(notice.url.clone()),
                });
                self.store.items.insert(0, item);
            }
            self.store.trim_storage();
        }

        // meta + 落盘。
        self.store.meta.last_poll_at = started_at;
        self.store.meta.poll_count += 1;
        let failures: Vec<String> = self
            .store
            .sources
            .iter()
            .filter(|s| s.enabled && s.status == "error")
            .map(|s| format!("{}: {}", s.name, s.status_text))
            .collect();
        self.store.meta.last_poll_error = if failures.is_empty() {
            String::new()
        } else {
            failures.join("；")
        };
        let _ = self.save();

        let added = fresh.len();
        let mut details: Vec<String> = Vec::new();
        if date_skipped > 0 {
            details.push(format!("时间范围跳过 {date_skipped} 条"));
        }
        if attachments_downloaded > 0 || attachments_failed > 0 || attachments_skipped > 0 {
            details.push(format!(
                "附件成功 {attachments_downloaded}、失败 {attachments_failed}、跳过 {attachments_skipped}"
            ));
        }
        if !failures.is_empty() {
            details.push(format!("{} 个数据源失败", failures.len()));
        }
        let message = if details.is_empty() {
            format!("新增 {added} 条通知")
        } else {
            format!("新增 {added} 条通知（{}）", details.join("；"))
        };
        PollOutcome {
            ok: true,
            message,
            added,
            date_skipped,
            attachments_downloaded,
            attachments_failed,
            attachments_skipped,
        }
    }

    /// `store.mjs::summarize`：按关键词命中排序取 llmMaxPerPoll 条走 AI。
    async fn summarize(
        &mut self,
        notices: &[NoticeRecord],
    ) -> Option<std::collections::HashMap<usize, cn_llm::LlmRow>> {
        if !self.store.settings.llm || notices.is_empty() {
            return None;
        }
        let max = self.store.settings.llm_max_per_poll.clamp(1, 40) as usize;
        let keywords = self.store.settings.keywords.clone();
        // §6.4：sendToLlm=false 的源不进 LLM 批量（正文不出设备），规则兜底仍在。
        // 源已删除的旧通知不在禁用名单内，维持原行为（可送 LLM）。
        let llm_blocked: std::collections::HashSet<&str> = self
            .store
            .sources
            .iter()
            .filter(|s| !s.send_to_llm)
            .map(|s| s.id.as_str())
            .collect();
        let mut ranked: Vec<(usize, usize)> = notices
            .iter()
            .enumerate()
            .filter(|(_, n)| !llm_blocked.contains(n.source_id.as_str()))
            .map(|(idx, n)| {
                let hits = keywords
                    .iter()
                    .filter(|w| !w.is_empty() && n.title.contains(w.as_str()))
                    .count();
                (idx, hits)
            })
            .collect();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        let chosen: Vec<usize> = ranked.into_iter().take(max).map(|(i, _)| i).collect();
        let batch: Vec<LlmNotice> = chosen
            .iter()
            .map(|&i| LlmNotice {
                title: notices[i].title.clone(),
                published_at: notices[i].published_at.clone(),
                content: notices[i].content.clone(),
            })
            .collect();
        let cfg = resolve_llm_config(&self.config, self.secrets.as_ref());
        let now = self.clock.now();
        self.llm_meter.set_budget(cfg.monthly_token_budget);
        let mut cache = self.llm_cache.lock().await;
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
        self.last_llm = StatusDto {
            at: now.to_rfc3339(),
            ok: Some(result.ok),
            message: if result.ok {
                String::new()
            } else {
                result.reason.clone()
            },
        };
        if !result.ok {
            return None;
        }
        // 把批次下标映射回 notices 下标。
        let mut mapped = std::collections::HashMap::new();
        for (batch_idx, row) in result.map {
            if let Some(&orig) = chosen.get(batch_idx) {
                mapped.insert(orig, row);
            }
        }
        Some(mapped)
    }

    /// 提醒检查（§4.2）。
    pub fn check_reminders(&mut self) -> bool {
        let mut counter = self.counter.lock().expect("counter");
        let mut notified: Vec<(String, String)> = Vec::new();
        let mut logs: Vec<String> = Vec::new();
        let notify_enabled = self.store.settings.notify;
        let changed = crate::reminders::check_reminders(
            &mut self.store.items,
            &mut self.store.meta.reminders,
            notify_enabled,
            self.clock.as_ref(),
            &mut counter,
            &mut |title: &str, label: &str| {
                notified.push((title.to_string(), label.to_string()));
                true
            },
            &mut |text: &str| logs.push(text.to_string()),
        );
        drop(counter);
        for (title, label) in notified {
            self.emit(AppEvent::Reminder {
                title,
                label,
            });
        }
        for text in logs {
            self.emit(AppEvent::Log { text });
        }
        if changed {
            let _ = self.save();
        }
        changed
    }

    /// 保存 config.json。
    pub fn save_config(&self) -> Result<(), StoreError> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.config)?;
        let tmp = self.config_path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.config_path)?;
        Ok(())
    }

    /// 便捷 ActionResult 构造。
    pub fn result_ok(&self, message: Option<String>, id: Option<String>) -> ActionResult {
        ActionResult {
            ok: true,
            message,
            error: None,
            preview: None,
            id,
            state: self.state(),
        }
    }

    pub fn result_err(&self, error: impl Into<String>) -> ActionResult {
        ActionResult {
            ok: false,
            message: None,
            error: Some(error.into()),
            preview: None,
            id: None,
            state: self.state(),
        }
    }

    pub fn result_preview(
        &self,
        message: Option<String>,
        preview: Vec<PreviewItem>,
    ) -> ActionResult {
        ActionResult {
            ok: true,
            message,
            error: None,
            preview: Some(preview),
            id: None,
            state: self.state(),
        }
    }

    /// LLM 运行时状态（llm.test 用）。
    pub fn last_llm(&self) -> &StatusDto {
        &self.last_llm
    }

    /// 数据源测试（source.test）。
    pub async fn test_source(&mut self, source_id: &str) -> ActionResult {
        let now_iso = self.clock.now().to_rfc3339();
        let Some(index) = self.store.sources.iter().position(|s| s.id == source_id) else {
            return self.result_err("找不到这个数据源");
        };
        let mut relogged = false;
        for _attempt in 0..2 {
            let source = self.store.sources[index].clone();
            match pipeline::fetch_source(
                self.fetcher.as_ref(),
                &source,
                &self.webvpn_config(),
                self.clock.now(),
            )
            .await
            {
                Ok(items) => {
                    let count = items.len();
                    let status_text = if relogged {
                        format!("已自动重新登录，解析出 {count} 条")
                    } else {
                        format!("测试成功：解析出 {count} 条")
                    };
                    let preview: Vec<PreviewItem> = items
                        .iter()
                        .take(5)
                        .map(|i| PreviewItem {
                            title: i.title.clone(),
                            url: i.url.clone(),
                            date: i.published_at.clone(),
                        })
                        .collect();
                    let s = &mut self.store.sources[index];
                    s.status = "ok".to_string();
                    s.status_text = status_text.clone();
                    s.last_fetch_at = now_iso.clone();
                    s.last_count = count as u32;
                    s.login_expired = false;
                    let _ = self.save();
                    return self.result_preview(Some(status_text), preview);
                }
                Err(error) => {
                    let text = error.to_string();
                    let login_expired = text.contains("登录态失效") || text.contains("需要登录");
                    {
                        let s = &mut self.store.sources[index];
                        s.status = "error".to_string();
                        s.status_text = text.clone();
                        s.login_expired = login_expired;
                    }
                    if login_expired && !relogged && self.can_auto_login() {
                        let login = match self.auto_login().await {
                            Ok(l) => l,
                            Err(e) => LoginOutcome::fail(e.to_string()),
                        };
                        if login.ok {
                            relogged = true;
                            let _ = self.save();
                            continue;
                        }
                        let s = &mut self.store.sources[index];
                        s.status_text = format!("{text}（自动登录失败：{}）", login.reason);
                    }
                    let error_text = self.store.sources[index].status_text.clone();
                    let _ = self.save();
                    return self.result_err(error_text);
                }
            }
        }
        let error_text = self.store.sources[index].status_text.clone();
        self.result_err(error_text)
    }
}

/// make_item 入参。
#[derive(Debug, Clone, Default)]
pub struct ItemInput {
    pub title: String,
    pub note: String,
    pub due_at: String,
    pub remind_before: Vec<u32>,
    pub source: String,
    pub notice_id: Option<String>,
    pub url: Option<String>,
}

/// 登录结果。
#[derive(Debug, Clone)]
pub struct LoginOutcome {
    pub ok: bool,
    pub updated: u32,
    pub reason: String,
}

impl LoginOutcome {
    pub fn fail(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            updated: 0,
            reason: reason.into(),
        }
    }
}

/// poll 结果。
#[derive(Debug, Clone)]
pub struct PollOutcome {
    pub ok: bool,
    pub message: String,
    pub added: usize,
    /// 因时间范围被跳过的通知条数。
    pub date_skipped: usize,
    /// 本次成功下载的附件数。
    pub attachments_downloaded: usize,
    /// 本次下载失败的附件数。
    pub attachments_failed: usize,
    /// 本次跳过的附件数（已存在 / 过大）。
    pub attachments_skipped: usize,
}

impl PollOutcome {
    pub fn fail(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            added: 0,
            date_skipped: 0,
            attachments_downloaded: 0,
            attachments_failed: 0,
            attachments_skipped: 0,
        }
    }
}

fn not_to_dto(n: &NoticeRecord) -> NoticeDto {
    NoticeDto {
        id: n.id.clone(),
        dedupe_key: n.dedupe_key.clone(),
        source_id: n.source_id.clone(),
        source_name: n.source_name.clone(),
        source_item_id: n.source_item_id.clone(),
        title: n.title.clone(),
        url: n.url.clone(),
        published_at: n.published_at.clone(),
        fetched_at: n.fetched_at.clone(),
        updated_at: n.updated_at.clone(),
        content: n.content.clone(),
        context: n.context.clone(),
        tags: n.tags.clone(),
        deadline: n.deadline.clone(),
        contact: n.contact.clone(),
        summary: n.summary.clone(),
        useful: n.useful.clone(),
        relevant: n.relevant,
        ai: n.ai,
        read: n.read,
        score: n.score,
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default()
}

async fn sleep(duration: std::time::Duration) {
    if duration.is_zero() {
        return;
    }
    tokio::time::sleep(duration).await;
}

fn load_config(path: &std::path::Path) -> AppConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}
