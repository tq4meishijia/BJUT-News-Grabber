//! 命令层 8 个（§3.4）：get_state / act / auth_begin / auth_cancel /
//! auth_import_cookie / llm_test / export_data / import_data。
//! 失败也必须带 state（前端断网改设置要能回滚）。


use serde::Serialize;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use cn_model::{ActionResult, AuthErrorCode, StateDto};
use cn_net::{FetchRequest, Fetcher, ScopeId, WEBVPN_GATEWAY};
use cn_store::ops::ActRequest;

use crate::auth::{AuthMode, AuthProgress, AuthState};
use crate::{emit_state_changed, log_line, AppState};

/// AuthTicket 返回形状。
#[derive(Debug, Clone, Serialize)]
pub struct AuthTicketDto {
    pub id: String,
    pub scope: String,
    pub mode: String,
}

fn err_with_state(state: &StateDto, message: &str) -> ActionResult {
    ActionResult {
        ok: false,
        message: None,
        error: Some(message.to_string()),
        preview: None,
        id: None,
        state: state.clone(),
    }
}

/// `get_state`：启动与重连用。
#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> Result<StateDto, String> {
    let svc = state.service.lock().await;
    Ok(svc.state())
}

/// `act`：23 个 op 的统一入口（透传 cn-store）。
#[tauri::command]
pub async fn act(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: ActRequest,
) -> Result<ActionResult, String> {
    let op = req.op.clone();
    let mut svc = state.service.lock().await;
    let result = svc.act(req).await;
    drop(svc);
    log_line(&app, "info", &format!("act {op} -> ok={}", result.ok));
    emit_state_changed(&app, "act");
    Ok(result)
}

/// `auth_begin`：三模式登录（WebView / Password / ImportCookie）。
#[tauri::command]
pub async fn auth_begin(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    scope: String,
    mode: String,
) -> Result<AuthTicketDto, String> {
    let auth_mode = match mode.as_str() {
        "WebView" | "webview" => AuthMode::WebView,
        "Password" | "password" => AuthMode::Password,
        "ImportCookie" | "importCookie" | "import" => AuthMode::ImportCookie,
        // 用户在 WebView 流程里点「我已完成」：对登录窗口 Cookie 做一次确认。
        "VerifyDone" | "verifyDone" => {
            if is_gateway_scope(&scope) {
                // 网关 scope 的 Cookie 在 webvpn.bjut.edu.cn 域下，不能走门户 service。
                spawn_verify_gateway_cookies(app.clone(), scope.clone());
            } else {
                let service = {
                    let svc = state.service.lock().await;
                    svc.config.account.clone().unwrap_or_default().service
                };
                spawn_verify_webview_cookies(app.clone(), scope.clone(), service);
            }
            return Ok(AuthTicketDto {
                id: format!("t-{}", chrono::Utc::now().timestamp_millis()),
                scope,
                mode: "VerifyDone".to_string(),
            });
        }
        other => return Err(format!("未知登录模式：{other}")),
    };
    let ticket = AuthTicketDto {
        id: format!("t-{}", chrono::Utc::now().timestamp_millis()),
        scope: scope.clone(),
        mode: format!("{auth_mode:?}"),
    };

    match auth_mode {
        AuthMode::ImportCookie => {
            // 等待 auth_import_cookie 携带 Cookie；先到 AwaitingUser。
            let progress = {
                let mut auth = state.auth.lock().await;
                auth.transition(&scope, AuthState::AwaitingUser, &FixedNow)
            };
            let _ = app.emit("auth://progress", &progress);
        }
        AuthMode::Password => {
            spawn_password_login(app.clone(), scope.clone());
        }
        AuthMode::WebView => {
            if is_gateway_scope(&scope) {
                // 网关登录页不是 CAS，走独立的 WebVPN 流程。
                let ticket = spawn_webvpn_login(app.clone(), &state, scope.clone()).await?;
                return Ok(ticket);
            }
            spawn_webview_login(app.clone(), scope.clone()).await?;
        }
    }
    Ok(ticket)
}

/// `auth_webvpn_login`：专用入口——打开 WebVPN 网关登录窗，登录后自动写网关 Cookie。
/// scope 固定 gateway；前端「窗口登录 WebVPN」按钮与统一 auth_begin 入口均可触发同一逻辑。
#[tauri::command]
pub async fn auth_webvpn_login(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AuthTicketDto, String> {
    spawn_webvpn_login(app, &state, "gateway".to_string()).await
}

/// `auth_cancel`：用户中途取消。
#[tauri::command]
pub async fn auth_cancel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ticket: String,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("auth-login") {
        // destroy：不发 CloseRequested，与 run 回调的用户关闭分支互不干扰。
        let _ = window.destroy();
    }
    let _ = ticket;
    // 票据短命：按全部 scope 终止进行中的登录（单飞窗口 10s 自然过期）。
    let progress = {
        let mut auth = state.auth.lock().await;
        auth.transition("portal", AuthState::Cancelled, &FixedNow)
    };
    let _ = app.emit("auth://progress", &progress);
    Ok(())
}

/// `auth_import_cookie`：粘贴 Cookie 兜底（桌面）。
#[tauri::command]
pub async fn auth_import_cookie(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    scope: String,
    cookie: String,
) -> Result<ActionResult, String> {
    let cookie = cookie.trim().to_string();
    if cookie.is_empty() {
        let svc = state.service.lock().await;
        return Ok(err_with_state(&svc.state(), "Cookie 不能为空"));
    }
    let mut svc = state.service.lock().await;
    if is_gateway_scope(&scope) {
        svc.store.settings.webvpn.cookie = cookie.clone();
    } else {
        // 门户 scope：写到同 host 的数据源。
        let host = scope.clone();
        let mut updated = 0;
        for source in svc.store.sources.iter_mut() {
            let source_host = url_host(&source.url);
            if !host.is_empty() && (source_host == host || source_host.ends_with(&format!(".{host}"))) {
                source.cookie = cookie.clone();
                source.login_expired = false;
                updated += 1;
            }
        }
        if updated == 0 {
            // 没有同域源：写给全部启用源（与"粘贴 Cookie 兜底"语义一致）。
            for source in svc.store.sources.iter_mut() {
                if source.enabled {
                    source.cookie = cookie.clone();
                    source.login_expired = false;
                }
            }
        }
    }
    let _ = svc.save();
    let mut result = svc.result_ok(Some("已导入登录态 Cookie".to_string()), None);
    drop(svc);
    {
        let mut auth = state.auth.lock().await;
        auth.mark_verified(&scope, &FixedNow);
        let progress = auth.transition(&scope, AuthState::Active { expires_at: None }, &FixedNow);
        let _ = app.emit("auth://progress", &progress);
    }
    emit_state_changed(&app, "auth");
    result.state = state.service.lock().await.state();
    Ok(result)
}

/// `llm_test`：测试模型（不覆盖已存 Key）。
#[tauri::command]
pub async fn llm_test(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<ActionResult, String> {
    let mut svc = state.service.lock().await;
    let result = svc
        .act(ActRequest {
            op: "llm.test".to_string(),
            payload: serde_json::json!({}),
        })
        .await;
    drop(svc);
    log_line(&app, "info", &format!("llm.test -> ok={}", result.ok));
    emit_state_changed(&app, "llm");
    Ok(result)
}

/// `open_attachment`：打开本地附件（仅允许数据目录下的文件，防任意路径打开）。
#[tauri::command]
pub fn open_attachment(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let target = std::path::PathBuf::from(&path);
    // 规范化后必须仍在 <data_dir>/attachments 之下，防止越权打开任意文件。
    let base = state.data_dir.join("attachments");
    let canonical_target = target
        .canonicalize()
        .map_err(|_| "附件不存在或已被移动".to_string())?;
    let canonical_base = base
        .canonicalize()
        .unwrap_or_else(|_| base.clone());
    if !canonical_target.starts_with(&canonical_base) {
        return Err("只允许打开附件目录下的文件".to_string());
    }
    tauri_plugin_opener::open_path(canonical_target.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("打开失败：{e}"))?;
    log_line(&app, "info", "已打开本地附件");
    Ok(())
}

/// `export_data`：导出 store.json（剥离 Cookie/密钥——S1/S2 口径的"不含密钥"）。
#[tauri::command]
pub async fn export_data(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let svc = state.service.lock().await;
    let mut exported = svc.store.clone();
    // 剥离登录态：网关 Cookie 与各源 Cookie 都不落导出文件（S1）。
    exported.settings.webvpn.cookie = String::new();
    for source in exported.sources.iter_mut() {
        source.cookie = String::new();
    }
    let text = serde_json::to_string_pretty(&exported).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("导出失败：{e}"))?;
    drop(svc);
    log_line(&app, "info", &format!("已导出数据到 {path}（不含登录态）"));
    Ok(())
}

/// `import_data`：导入 store.json（合并后落盘）。
#[tauri::command]
pub async fn import_data(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ActionResult, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取失败：{e}"))?;
    let raw: cn_store::StoreData =
        serde_json::from_str(&text).map_err(|e| format!("文件不是合法 store.json：{e}"))?;
    let mut svc = state.service.lock().await;
    let merged = svc.store.clone().merged_with(raw);
    svc.store = merged;
    svc.store.trim_storage();
    let _ = svc.save();
    let result = svc.result_ok(Some("已导入数据".to_string()), None);
    drop(svc);
    log_line(&app, "info", &format!("已从 {path} 导入数据"));
    emit_state_changed(&app, "import");
    Ok(result)
}

/* ------------------------------ 内部：登录流程 ------------------------------ */

use chrono::{DateTime, Local};

/// 轻量 Clock：命令层的事件时间戳。
pub struct FixedNow;

impl cn_model::Clock for FixedNow {
    fn now(&self) -> DateTime<Local> {
        Local::now()
    }
}

fn url_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default()
}

fn emit_progress(app: &tauri::AppHandle, progress: &AuthProgress) {
    let _ = app.emit("auth://progress", progress);
}

/// Password 模式：cn-net CAS 登录（静默续期同路径）。
fn spawn_password_login(app: tauri::AppHandle, scope: String) {
    tauri::async_runtime::spawn(async move {
        {
            let state = app.state::<AppState>();
            let mut auth = state.auth.lock().await;
            if !auth.try_begin_single_flight(&scope, &FixedNow) {
                return;
            }
            let p = auth.transition(&scope, AuthState::Launching, &FixedNow);
            emit_progress(&app, &p);
        }
        let (cas_base, service, username, password) = {
            let state = app.state::<AppState>();
            let svc = state.service.lock().await;
            let account = svc.config.account.clone().unwrap_or_default();
            let password = state.secrets.get("account.password").unwrap_or_default();
            (
                account.cas_base.clone(),
                account.service.clone(),
                account.username.clone(),
                password,
            )
        };
        if cas_base.is_empty() || service.is_empty() || username.is_empty() || password.is_empty() {
            let state = app.state::<AppState>();
            let mut auth = state.auth.lock().await;
            auth.end_single_flight(&scope);
            let p = auth.transition(
                &scope,
                AuthState::Failed {
                    kind: AuthErrorCode::Form,
                },
                &FixedNow,
            );
            emit_progress(&app, &p);
            return;
        }
        {
            let state = app.state::<AppState>();
            let mut auth = state.auth.lock().await;
            let p = auth.transition(&scope, AuthState::Verifying, &FixedNow);
            emit_progress(&app, &p);
        }
        let fetcher = ReqwestFetcherWrapper;
        let result = cn_net::cas_login(
            &fetcher,
            &cas_base,
            &service,
            &username,
            &password,
            None,
        )
        .await;
        let state = app.state::<AppState>();
        let mut auth = state.auth.lock().await;
        auth.end_single_flight(&scope);
        match result {
            Ok(success) => {
                auth.mark_verified(&scope, &FixedNow);
                // 新 Cookie 写回同门户源。
                drop(auth);
                {
                    let mut svc = state.service.lock().await;
                    let portal_host = url_host(&service);
                    for source in svc.store.sources.iter_mut() {
                        let host = url_host(&source.url);
                        if !portal_host.is_empty()
                            && (host == portal_host
                                || host.ends_with(&format!(".{portal_host}"))
                                || host.contains(&portal_host))
                        {
                            source.cookie = success.cookie.clone();
                            source.login_expired = false;
                        }
                    }
                    let _ = svc.save();
                }
                let mut auth = state.auth.lock().await;
                let p = auth.transition(
                    &scope,
                    AuthState::Active {
                        expires_at: Some(
                            (Local::now() + chrono::Duration::hours(8)).to_rfc3339(),
                        ),
                    },
                    &FixedNow,
                );
                emit_progress(&app, &p);
            }
            Err(err) => {
                let p = auth.record_failure(&scope, err.code, &FixedNow);
                emit_progress(&app, &p);
                log_line(&app, "warn", &format!("自动登录失败：{}", err.detail));
            }
        }
        emit_state_changed(&app, "auth");
    });
}

/// WebView 模式：应用内窗口登录，导航回 service 后取 Cookie。
async fn spawn_webview_login(
    app: tauri::AppHandle,
    scope: String,
) -> Result<(), String> {
    let (cas_base, service) = {
        let state = app.state::<AppState>();
        let svc = state.service.lock().await;
        let account = svc.config.account.clone().unwrap_or_default();
        (account.cas_base.clone(), account.service.clone())
    };
    if cas_base.is_empty() || service.is_empty() {
        return Err("还没配置「认证入口地址」和「登录后返回的地址」（设置页可以填）".to_string());
    }
    let login_url = format!(
        "{}/login?service={}",
        cas_base.trim_end_matches('/'),
        cn_core::encode_uri_component(&service)
    );
    {
        let state = app.state::<AppState>();
        let mut auth = state.auth.lock().await;
        let p = auth.transition(&scope, AuthState::Launching, &FixedNow);
        emit_progress(&app, &p);
    }
    let url: url::Url = login_url.parse().map_err(|e| format!("登录页地址不合法：{e}"))?;
    // 导航监听：回到 service 即视为完成，随后取 Cookie 验证（builder 期挂载）。
    let app_handle = app.clone();
    let scope_clone = scope.clone();
    let service_clone = service.clone();
    let _window = WebviewWindowBuilder::new(
        &app,
        "auth-login",
        WebviewUrl::External(url),
    )
    .title("登录学校门户")
    .inner_size(480.0, 720.0)
    .on_navigation(move |nav_url| {
        let url_text = nav_url.to_string();
        if url_text.starts_with(service_clone.trim_end_matches('/'))
            || url_text.contains("actionCasLogin")
            || url_text.contains("casLogin")
        {
            let app = app_handle.clone();
            let scope = scope_clone.clone();
            let service = service_clone.clone();
            tauri::async_runtime::spawn(async move {
                {
                    let state = app.state::<AppState>();
                    let mut auth = state.auth.lock().await;
                    let p = auth.transition(&scope, AuthState::Verifying, &FixedNow);
                    emit_progress(&app, &p);
                }
                // 取登录窗口会话 Cookie（tauri 2 cookie API）。
                let cookies = app
                    .get_webview_window("auth-login")
                    .map(|w| {
                        w.cookies_for_url(url::Url::parse(&service).unwrap_or_else(|_| {
                            url::Url::parse("https://localhost").unwrap()
                        }))
                        .map(|list| {
                            list.iter()
                                .map(|c| format!("{}={}", c.name(), c.value()))
                                .collect::<Vec<_>>()
                                .join("; ")
                        })
                        .unwrap_or_default()
                    })
                    .unwrap_or_default();
                if let Some(window) = app.get_webview_window("auth-login") {
                    let _ = window.destroy();
                }
                if cookies.is_empty() {
                    let state = app.state::<AppState>();
                    let mut auth = state.auth.lock().await;
                    let p = auth.record_failure(&scope, AuthErrorCode::NoPortalSession, &FixedNow);
                    emit_progress(&app, &p);
                } else {
                    let state = app.state::<AppState>();
                    let portal_host = url_host(&service);
                    {
                        let mut svc = state.service.lock().await;
                        for source in svc.store.sources.iter_mut() {
                            let host = url_host(&source.url);
                            if !portal_host.is_empty()
                                && (host == portal_host
                                    || host.ends_with(&format!(".{portal_host}")))
                            {
                                source.cookie = cookies.clone();
                                source.login_expired = false;
                            }
                        }
                        let _ = svc.save();
                    }
                    let mut auth = state.auth.lock().await;
                    auth.mark_verified(&scope, &FixedNow);
                    let p = auth.transition(
                        &scope,
                        AuthState::Active {
                            expires_at: Some(
                                (Local::now() + chrono::Duration::hours(8)).to_rfc3339(),
                            ),
                        },
                        &FixedNow,
                    );
                    emit_progress(&app, &p);
                }
                emit_state_changed(&app, "auth");
            });
        }
        true
    })
    .build()
    .map_err(|e| format!("打开登录窗口失败：{e}"))?;
    {
        let state = app.state::<AppState>();
        let mut auth = state.auth.lock().await;
        let p = auth.transition(&scope, AuthState::AwaitingUser, &FixedNow);
        emit_progress(&app, &p);
    }
    Ok(())
}

/* ------------------------------ 内部：WebVPN 网关登录 ------------------------------ */

use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// gateway / webvpn 都视作网关 scope（与 auth_import_cookie 口径一致）。
fn is_gateway_scope(scope: &str) -> bool {
    scope.eq_ignore_ascii_case("gateway") || scope.eq_ignore_ascii_case("webvpn")
}

/// 打开 WebVPN 网关登录窗（与 CAS 门户窗口完全独立，不复用 cas_base）。
/// 导航离开网关 host 即视为登录完成；同时保留前端「我已完成」兜底路径。
async fn spawn_webvpn_login(
    app: tauri::AppHandle,
    state: &State<'_, AppState>,
    scope: String,
) -> Result<AuthTicketDto, String> {
    // 单飞：已有的 auth-login 先销毁，避免两个窗口抢 Cookie。
    if let Some(window) = app.get_webview_window("auth-login") {
        let _ = window.destroy();
    }
    let login_url = format!("{}/login", WEBVPN_GATEWAY.trim_end_matches('/'));
    let url: url::Url = login_url
        .parse()
        .map_err(|e| format!("登录页地址不合法：{e}"))?;
    let app_handle = app.clone();
    let scope_clone = scope.clone();
    let gateway_host = url::Url::parse(WEBVPN_GATEWAY)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .unwrap_or_else(|| "webvpn.bjut.edu.cn".to_string());
    let finished = Arc::new(AtomicBool::new(false));
    let finished_clone = finished.clone();
    let _window = WebviewWindowBuilder::new(&app, "auth-login", WebviewUrl::External(url))
        .title("登录学校 VPN 网关")
        .inner_size(480.0, 720.0)
        .on_navigation(move |nav_url| {
            let host = nav_url.host_str().unwrap_or_default().to_ascii_lowercase();
            // 离开网关登录页（跳转门户/其他站点）→ 登录完成；只触发一次。
            if !host.is_empty()
                && host != gateway_host
                && !host.ends_with(&format!(".{gateway_host}"))
                && !finished_clone.swap(true, Ordering::SeqCst)
            {
                let app = app_handle.clone();
                let scope = scope_clone.clone();
                tauri::async_runtime::spawn(async move {
                    {
                        let state = app.state::<AppState>();
                        let mut auth = state.auth.lock().await;
                        let p = auth.transition(&scope, AuthState::Verifying, &FixedNow);
                        emit_progress(&app, &p);
                    }
                    let state = app.state::<AppState>();
                    apply_gateway_cookies(&app, &state, &scope).await;
                });
            }
            true
        })
        .build()
        .map_err(|e| format!("打开登录窗口失败：{e}"))?;
    {
        let mut auth = state.auth.lock().await;
        let p = auth.transition(&scope, AuthState::AwaitingUser, &FixedNow);
        emit_progress(&app, &p);
    }
    Ok(AuthTicketDto {
        id: format!("t-{}", chrono::Utc::now().timestamp_millis()),
        scope,
        mode: "WebView".to_string(),
    })
}

/// 抓取 WebVPN 会话所需的全部校内 Cookie。
///
/// `cookies_for_url(网关根)` 只返回 path=/ 的 Cookie；而 iframe 里的 CAS 登录与
/// 门户回调常把会话 Cookie 设在 `/https/<hex-host>/` 等子路径下，根 URL 匹配不到。
/// 这里枚举 WebView2 Cookie 存储全量，按校内域筛选后按名称去重；只打印名称，不打印值。
fn collect_webvpn_cookies(app: &tauri::AppHandle) -> String {
    let fallback = || grab_login_cookies(app, WEBVPN_GATEWAY);
    let Some(window) = app.get_webview_window("auth-login") else {
        return fallback();
    };
    let Ok(all) = window.cookies() else {
        return fallback();
    };
    // (name, value, path)：路径更深（/https/...）的门户会话优先保留。
    let mut entries: Vec<(String, String, String)> = Vec::new();
    for cookie in all {
        let domain = cookie
            .domain()
            .unwrap_or_default()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        if !domain.contains("bjut.edu.cn") {
            continue;
        }
        let name = cookie.name().to_string();
        if name.is_empty() {
            continue;
        }
        entries.push((
            name,
            cookie.value().to_string(),
            cookie.path().unwrap_or_default().to_string(),
        ));
    }
    entries.sort_by(|a, b| {
        let weight = |path: &str| if path.starts_with("/https/") { 1u8 } else { 0u8 };
        weight(&b.2).cmp(&weight(&a.2))
    });
    let mut seen = HashSet::new();
    let mut parts: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for (name, value, path) in entries {
        if !seen.insert(name.clone()) {
            continue;
        }
        names.push(if path.starts_with("/https/") {
            format!("{name}*")
        } else {
            name.clone()
        });
        parts.push(format!("{name}={value}"));
    }
    let joined = parts.join("; ");
    log_line(
        app,
        "info",
        &format!(
            "网关会话抓取：{} 个片段（名称：{}；* 表示网关内子路径）",
            names.len(),
            names.join(", ")
        ),
    );
    if joined.is_empty() {
        fallback()
    } else {
        joined
    }
}

/// WebVPN 登录后的门户单点登录引导（实测必需）：
/// 依次访问「门户 /common/actionCasLogin → CAS /login → 带 ticket 回门户」，
/// 完成后同一份网关票据即可让门户接口返回 e=0；否则接口一直返回 10042「请登录」。
/// CAS 地址不写死：从 actionCasLogin 的 302 Location 里逐步取，全程走网关重写。
async fn bootstrap_portal_sso(cookie: &str, origin: &str) -> Result<(), String> {
    let cfg = cn_net::WebvpnConfig {
        enabled: true,
        gateway: WEBVPN_GATEWAY.to_string(),
        cookie: cookie.to_string(),
        login_page_pattern: cn_net::DEFAULT_LOGIN_PAGE_PATTERN.to_string(),
    };
    let service = format!(
        "{origin}/common/actionCasLogin?redirect_url={}",
        cn_core::encode_uri_component(&format!("{origin}/"))
    );
    let host = url_host(origin);
    let fetcher = ReqwestFetcherWrapper;
    let mut current = cn_net::rewrite_url(&service, &cfg);
    // 三步：门户入口 → CAS 登录（SIG 自动放行）→ 门户回调换会话。
    for step in 0..3 {
        let mut req = FetchRequest::get(&current, ScopeId::Portal(host.clone()));
        req.gateway_cookie = cookie.to_string();
        req.follow_redirects = 0;
        req.timeout = std::time::Duration::from_secs(20);
        let response = fetcher
            .fetch(req)
            .await
            .map_err(|e| format!("第 {} 步失败：{}", step + 1, e))?;
        if !(300..400).contains(&response.status) {
            return Ok(());
        }
        let Some(location) = response.header("location") else {
            return Ok(());
        };
        if location.is_empty() {
            return Ok(());
        }
        current = location.to_string();
    }
    Ok(())
}

/// 抓取网关域 Cookie → 写 settings.webvpn.cookie → 迁移状态。
/// 与门户 Cookie 逻辑分离：网关会话只存一处，不写数据源。
async fn apply_gateway_cookies(app: &tauri::AppHandle, state: &State<'_, AppState>, scope: &str) {
    let cookies = collect_webvpn_cookies(app);
    // Cookie 抓取后再销毁窗口；destroy 不发 CloseRequested，不会被误判为用户取消。
    if let Some(window) = app.get_webview_window("auth-login") {
        let _ = window.destroy();
    }
    if cookies.is_empty() {
        let mut auth = state.auth.lock().await;
        let p = auth.record_failure(scope, AuthErrorCode::NoPortalSession, &FixedNow);
        emit_progress(app, &p);
    } else {
        let first_url = {
            let svc = state.service.lock().await;
            svc.store
                .sources
                .iter()
                .find(|source| source.enabled)
                .map(|source| source.url.clone())
                .unwrap_or_default()
        };
        let origin = url::Url::parse(&first_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| format!("{}://{}", u.scheme(), h)))
            .unwrap_or_default();
        if origin.is_empty() {
            log_line(app, "warn", "没有启用的数据源，跳过门户单点登录引导");
        } else {
            match bootstrap_portal_sso(&cookies, &origin).await {
                Ok(()) => log_line(app, "info", "门户单点登录引导完成"),
                Err(err) => log_line(app, "warn", &format!("门户单点登录引导未完成：{err}")),
            }
        }
        {
            let mut svc = state.service.lock().await;
            svc.store.settings.webvpn.cookie = cookies;
            let _ = svc.save();
        }
        let mut auth = state.auth.lock().await;
        auth.mark_verified(scope, &FixedNow);
        let p = auth.transition(scope, AuthState::Active { expires_at: None }, &FixedNow);
        emit_progress(app, &p);
    }
    emit_state_changed(app, "auth");
}

/// VerifyDone 的网关分支：先切 Verifying，再由 apply_gateway_cookies 抓 Cookie 收尾。
fn spawn_verify_gateway_cookies(app: tauri::AppHandle, scope: String) {
    tauri::async_runtime::spawn(async move {
        {
            let state = app.state::<AppState>();
            let mut auth = state.auth.lock().await;
            let p = auth.transition(&scope, AuthState::Verifying, &FixedNow);
            emit_progress(&app, &p);
        }
        let state = app.state::<AppState>();
        apply_gateway_cookies(&app, &state, &scope).await;
    });
}

/// 从登录窗口取 Cookie → 写回同门户源 → 迁移状态（WebView 流程共用）。
fn spawn_verify_webview_cookies(app: tauri::AppHandle, scope: String, service: String) {
    tauri::async_runtime::spawn(async move {
        {
            let state = app.state::<AppState>();
            let mut auth = state.auth.lock().await;
            let p = auth.transition(&scope, AuthState::Verifying, &FixedNow);
            emit_progress(&app, &p);
        }
        let cookies = grab_login_cookies(&app, &service);
        if let Some(window) = app.get_webview_window("auth-login") {
            let _ = window.destroy();
        }
        if cookies.is_empty() {
            let state = app.state::<AppState>();
            let mut auth = state.auth.lock().await;
            let p = auth.record_failure(&scope, AuthErrorCode::NoPortalSession, &FixedNow);
            emit_progress(&app, &p);
        } else {
            apply_portal_cookies(&app, &service, &cookies).await;
            let state = app.state::<AppState>();
            let mut auth = state.auth.lock().await;
            auth.mark_verified(&scope, &FixedNow);
            let p = auth.transition(
                &scope,
                AuthState::Active {
                    expires_at: Some((Local::now() + chrono::Duration::hours(8)).to_rfc3339()),
                },
                &FixedNow,
            );
            emit_progress(&app, &p);
        }
        emit_state_changed(&app, "auth");
    });
}

/// 登录窗口 Cookie 拼接为 Cookie 头。
fn grab_login_cookies(app: &tauri::AppHandle, service: &str) -> String {
    app.get_webview_window("auth-login")
        .and_then(|w| {
            let target = url::Url::parse(service)
                .or_else(|_| url::Url::parse("https://localhost"))
                .ok()?;
            w.cookies_for_url(target).ok().map(|list| {
                list.iter()
                    .map(|c| format!("{}={}", c.name(), c.value()))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
        })
        .unwrap_or_default()
}

/// Cookie 写回同门户 host 的数据源。
async fn apply_portal_cookies(app: &tauri::AppHandle, service: &str, cookies: &str) {
    let state = app.state::<AppState>();
    let portal_host = url_host(service);
    let mut svc = state.service.lock().await;
    for source in svc.store.sources.iter_mut() {
        let host = url_host(&source.url);
        if !portal_host.is_empty()
            && (host == portal_host || host.ends_with(&format!(".{portal_host}")))
        {
            source.cookie = cookies.to_string();
            source.login_expired = false;
        }
    }
    let _ = svc.save();
}

/// ReqwestFetcher 零大小包装（cas_login 需要 &dyn Fetcher）。
struct ReqwestFetcherWrapper;

#[async_trait::async_trait]
impl cn_net::Fetcher for ReqwestFetcherWrapper {
    async fn fetch(
        &self,
        req: cn_net::FetchRequest,
    ) -> Result<cn_net::FetchResponse, cn_net::NetError> {
        cn_net::ReqwestFetcher::new().fetch(req).await
    }
}
