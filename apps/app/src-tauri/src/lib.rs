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
use cn_store::CampusService;

use crate::auth::AuthManager;
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
            commands::auth_cancel,
            commands::auth_import_cookie,
            commands::llm_test,
            commands::export_data,
            commands::import_data,
        ])
        .build(tauri::generate_context!())
        .expect("tauri app build");

    app.run(|app_handle, event| {
        if let RunEvent::WindowEvent {
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } = event
        {
            // 关窗 = 隐藏到托盘，退出才结束进程（§8 桌面行为）。
            api.prevent_close();
            if let Some(window) = app_handle.get_webview_window("main") {
                let _ = window.hide();
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
