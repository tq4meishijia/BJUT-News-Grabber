//! 桌面入口：WebView2 探测 → 启动 Tauri。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebView2 缺失时给引导文案而非白屏（§7.2 / G6）。
    if !cn_app::webview2_installed() {
        cn_app::show_webview2_hint();
        return;
    }
    cn_app::run();
}
