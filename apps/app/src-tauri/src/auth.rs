//! 登录状态机已移至 `cn_model::auth`（纯逻辑、零 tauri 依赖，测试可在无 WebView2
//! 的环境运行）。此处仅作壳层内部的再导出，保持 `crate::auth::*` 引用路径不变。

pub(crate) use cn_model::auth::*;
