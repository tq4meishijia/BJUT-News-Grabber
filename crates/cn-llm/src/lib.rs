//! cn-llm — OpenAI 兼容客户端 + Schema 校验 + 修复重试 + 结果缓存。
//!
//! 硬约束（§6）：
//! - 三任务合并为一次调用（摘要/分类/结构化抽取，§6.1）。
//! - 提示词约束 + 本地严格校验 + 一次修复，不赌服务端 json_schema（§6.2）。
//! - 降级顺序：AI → 规则（cn-core）→ 空；任何情况下通知必须有摘要与「对我有什么用」（B11）。
//! - 缓存键 `ai_hash = sha1(title + content[0..2000] + model + PROMPT_VERSION)`。
//! - 不依赖 tauri；HTTP 经 cn_net::Fetcher 注入。

pub mod cache;
pub mod client;
pub mod prompt;
pub mod schema;

pub use cache::{ai_hash, LlmCache};
pub use client::{
    summarize_batch, summarize_with_fallback, BatchMode, BatchResult, LlmConfig, LlmNotice,
    LlmRow, TokenMeter,
};
pub use prompt::{build_prompt, system_prompt, PROMPT_VERSION};
pub use schema::{repair_rows, validate_rows, CATEGORIES};
