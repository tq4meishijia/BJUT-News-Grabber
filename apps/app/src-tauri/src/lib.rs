//! cn-app — Tauri 2 桌面壳（唯一允许 `use tauri::` 的地方）。
//!
//! 职责（§2.3）：8 个命令、5 个事件、托盘/通知/自启动/单实例、WebView2 探测、
//! OS 密钥库、日志脱敏。任何业务规则都在 cn-store / cn-core / cn-net。

// 模块私有：cdylib 只应导出入口符号，pub 模块会把整棵依赖图的泛型符号
// 单态化导出（实测 129k > PE 上限 65535，链接失败）。仅 `run()` 保持 pub。
mod auth;
mod commands;
mod redact;
mod secrets;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager, RunEvent, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;

use cn_model::SystemClock;
use cn_net::ReqwestFetcher;
use cn_store::{AppEvent, CampusService};

use crate::auth::{AuthManager, AuthState};
use crate::commands::FixedNow;
use crate::secrets::SecretBackend;

/// 全局应用状态（命令层共享）。
pub struct AppState {
    pub service: tokio::sync::Mutex<CampusService>,
    pub auth: tokio::sync::Mutex<AuthManager>,
    pub secrets: Arc<dyn cn_store::SecretStore>,
    /// 凭据后端名（诊断用，构造时捕获）。
    pub secret_backend_name: &'static str,
    pub data_dir: PathBuf,
}

/// 数据目录：沿用 Node 版 `%APPDATA%\campus-notice`（升级免迁移）。
pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("campus-notice")
}

/// WebView2 运行时探测（缺失给引导文案而非白屏）。
pub fn webview2_installed() -> bool {
    #[cfg(windows)]
    {
        let candidates = [
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application",
            r"C:\Program Files\Microsoft\EdgeWebView\Application",
        ];
        candidates.iter().any(|p| std::path::Path::new(p).exists())
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// WebView2 缺失时的引导（Evergreen 引导器链接）。
pub fn show_webview2_hint() {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        let text = "本机缺少 Microsoft Edge WebView2 运行时，无法显示界面。\n\n\
            请安装 WebView2 Evergreen 运行时后重新打开本应用：\n\
            https://developer.microsoft.com/microsoft-edge/webview2/\n\n\
            （安装包约 2 MB，微软官方签名）";
        let caption = "校园通知 · 需要 WebView2 运行时";
        let wide_text: Vec<u16> = std::ffi::OsStr::new(text).encode_wide().chain(Some(0)).collect();
        let wide_caption: Vec<u16> =
            std::ffi::OsStr::new(caption).encode_wide().chain(Some(0)).collect();
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            wide_text.as_ptr(),
            wide_caption.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
        );
    }
    #[cfg(not(windows))]
    {
        eprintln!("缺少 WebView2 运行时");
    }
}

/// 关窗二选一（用户确认的「弹窗二选一」行为）：true = 完全退出，false = 最小化到托盘。
/// 默认按钮为「否」（最小化），避免误按回车杀掉后台提醒。
fn confirm_quit() -> bool {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        let text = "完全退出「校园通知」？\n\n\
            是 — 退出应用（停止后台抓取与到点提醒）\n\
            否 — 最小化到托盘（继续在后台接收提醒）";
        let caption = "校园通知 · 退出确认";
        let wide_text: Vec<u16> = std::ffi::OsStr::new(text).encode_wide().chain(Some(0)).collect();
        let wide_caption: Vec<u16> =
            std::ffi::OsStr::new(caption).encode_wide().chain(Some(0)).collect();
        let answer = windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            wide_text.as_ptr(),
            wide_caption.as_ptr(),
            windows_sys::Win32::UI::WindowsAndMessaging::MB_YESNO
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONQUESTION
                | windows_sys::Win32::UI::WindowsAndMessaging::MB_DEFBUTTON2,
        );
        answer == windows_sys::Win32::UI::WindowsAndMessaging::IDYES
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// 日志输出（统一走脱敏，S3）。
pub fn log_line(app: &tauri::AppHandle, level: &str, text: &str) {
    let line = redact::LogLine::new(level, text);
    let _ = app.emit("log://line", &line);
}

/// 组装并启动。
pub fn run() {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).ok();
    let secret_backend = SecretBackend::open(&dir);
    let secret_backend_name = secret_backend.backend_name();
    let secrets: Arc<dyn cn_store::SecretStore> = Arc::new(secret_backend);

    let service = CampusService::new(
        dir.join("store.json"),
        dir.join("config.json"),
        secrets.clone(),
        Arc::new(ReqwestFetcher::new()),
        Arc::new(SystemClock),
    );

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            service: tokio::sync::Mutex::new(service),
            auth: tokio::sync::Mutex::new(AuthManager::default()),
            secrets,
            secret_backend_name,
            data_dir: dir.clone(),
        })
        .setup(|app| {
            let handle = app.handle().clone();
            // 事件桥接：cn-store 的 AppEvent → Tauri 事件（设计 §3.3）。
            // 逐项核对：
            // - AttachmentProgress → `attachment://progress`（前端状态行「正在下载附件 x/y」）；
            // - Reminder           → `reminder://fired`（前端重拉 state 并重绘提醒）；
            // - Log                → `log://line`（脱敏后转发，与 crate::log_line 同格式）；
            // - StateChanged       → **刻意跳过**：commands.rs 的 emit_state_changed 已在 act 后
            //   广播完整 state（版本号单调），若此处再发同名事件会双发且 handler 内无法安全
            //   再锁 service 组装 StateDto（死锁），故由既有路径唯一负责。
            // 注：handler 在 CampusService 持锁期间被调用，只能做 app.emit，绝不回读 service。
            {
                let emitter = app.handle().clone();
                let event_state = app.state::<AppState>();
                let mut svc = event_state.service.blocking_lock();
                svc.set_event_handler(Box::new(move |event| match event {
                    AppEvent::AttachmentProgress {
                        done,
                        total,
                        file_name,
                    } => {
                        // 最后一条（done == total）发 done：前端据此清除状态行与进度条，
                        // 否则状态行会停在「正在下载附件 5/5」。
                        let phase = if *total > 0 && *done >= *total {
                            "done"
                        } else {
                            "tick"
                        };
                        let _ = emitter.emit(
                            "attachment://progress",
                            serde_json::json!({
                                "phase": phase,
                                "done": done,
                                "total": total,
                                "fileName": file_name,
                            }),
                        );
                    }
                    AppEvent::Reminder { title, label } => {
                        let _ = emitter.emit(
                            "reminder://fired",
                            serde_json::json!({ "title": title, "label": label }),
                        );
                    }
                    AppEvent::Log { text } => {
                        let line = redact::LogLine::new("info", text);
                        let _ = emitter.emit("log://line", &line);
                    }
                    AppEvent::StateChanged { .. } => {}
                }));
            }
            // 托盘（#[cfg(desktop)] 语义：本期只做 Windows 桌面）。
            setup_tray(app)?;
            // 启动后台调度：15s 提醒心跳 + 按 pollMinutes 抓取 + 状态广播。
            spawn_scheduler(handle);
            let backend = {
                let state = app.state::<AppState>();
                state.secret_backend_name
            };
            log_line(
                app.handle(),
                "info",
                &format!("应用已启动（凭据后端：{backend}）"),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::act,
            commands::auth_begin,
            commands::auth_webvpn_login,
            commands::auth_cancel,
            commands::auth_import_cookie,
            commands::llm_test,
            commands::open_attachment,
            commands::export_data,
            commands::import_data,
        ])
        .build(tauri::generate_context!())
        .expect("tauri app build");

    app.run(|app_handle, event| {
        if let RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } = event
        {
            match label.as_str() {
                // 主窗口：关窗 = 询问「最小化到托盘 / 完全退出」（§8 桌面行为）。
                "main" => {
                    api.prevent_close();
                    if confirm_quit() {
                        app_handle.exit(0);
                    } else if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
                // 登录窗：用户手动关闭 = 放行关闭，并把登录状态收尾为 Cancelled。
                // 程序化关闭一律走 destroy()（不发 CloseRequested），不会进到这里，
                // 因此本分支只会由用户操作触发，无与 mark_verified 的竞态。
                "auth-login" => {
                    let state = app_handle.state::<AppState>();
                    if let Ok(mut auth) = state.auth.try_lock() {
                        let progress = auth.transition("portal", AuthState::Cancelled, &FixedNow);
                        let _ = app_handle.emit("auth://progress", &progress);
                    };
                }
                _ => {}
            }
        }
    });
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)?;
    let poll = MenuItem::with_id(app, "poll", "立即抓取", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &poll, &quit])?;
    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("校园通知 · 日程")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
            "poll" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    let mut svc = state.service.lock().await;
                    let outcome = svc.poll("tray").await;
                    log_line(&handle, "info", &outcome.message);
                    emit_state_changed(&handle, "poll");
                });
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// 广播 state://changed（版本号单调，前端只应用更大版本）。
pub fn emit_state_changed(app: &tauri::AppHandle, origin: &str) {
    if let Some(payload) = build_state_payload(app, origin) {
        let _ = app.emit("state://changed", payload);
    }
}

/// 组装载荷（try_lock 避免与命令层死锁；拿不到就跳过，下一次变化会补发）。
fn build_state_payload(app: &tauri::AppHandle, origin: &str) -> Option<serde_json::Value> {
    let state = app.state::<AppState>();
    let svc = state.service.try_lock().ok()?;
    let snapshot = svc.state();
    let version = svc.state_version();
    Some(serde_json::json!({
        "version": version,
        "state": snapshot,
        "origin": origin,
    }))
}

/// 后台调度（§4.2 心跳 15s 提醒；poll 按 settings.pollMinutes）。
fn spawn_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
        let mut poll_tick = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut last_poll = std::time::Instant::now();
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    let state = app.state::<AppState>();
                    let mut svc = state.service.lock().await;
                    let changed = svc.check_reminders();
                    if changed {
                        drop(svc);
                        emit_state_changed(&app, "reminder");
                    }
                }
                _ = poll_tick.tick() => {
                    let state = app.state::<AppState>();
                    let poll_minutes = {
                        let svc = state.service.lock().await;
                        svc.store.settings.poll_minutes.max(5) as u64
                    };
                    if last_poll.elapsed().as_secs() >= poll_minutes * 60 {
                        last_poll = std::time::Instant::now();
                        let mut svc = state.service.lock().await;
                        let outcome = svc.poll("timer").await;
                        drop(svc);
                        log_line(&app, "info", &outcome.message);
                        emit_state_changed(&app, "poll");
                    }
                }
            }
        }
    });
}
