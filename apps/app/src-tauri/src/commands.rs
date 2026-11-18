//! 命令层 8 个（§3.4）：get_state / act / auth_begin / auth_cancel /
//! auth_import_cookie / llm_test / export_data / import_data。
//! 失败也必须带 state（前端断网改设置要能回滚）。


use serde::Serialize;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use cn_model::{ActionResult, AuthErrorCode, StateDto};
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
            let service = {
                let svc = state.service.lock().await;
                svc.config.account.clone().unwrap_or_default().service
            };
            spawn_verify_webview_cookies(app.clone(), scope.clone(), service);
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
            spawn_webview_login(app.clone(), scope.clone()).await?;
        }
    }
    Ok(ticket)
}

/// `auth_cancel`：用户中途取消。
#[tauri::command]
pub async fn auth_cancel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ticket: String,
) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("auth-login") {
        let _ = window.close();
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
    let is_gateway = scope.eq_ignore_ascii_case("gateway") || scope.eq_ignore_ascii_case("webvpn");
    if is_gateway {
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
                    let _ = window.close();
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
            let _ = window.close();
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
