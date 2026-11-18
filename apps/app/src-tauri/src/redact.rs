//! 日志脱敏（S3）已移至 `cn_core::redact`（纯逻辑，测试可在无 WebView2 的环境运行）。
//! 此处仅作壳层内部的再导出，保持 `crate::redact::*` 引用路径不变。

pub(crate) use cn_core::redact::*;
