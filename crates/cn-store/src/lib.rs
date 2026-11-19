//! cn-store — Store 状态机、18 个 op、抓取流水线、日程与提醒引擎、原子落盘。
//!
//! 硬约束（§2.2 / §3.3 / §4 / §10.3）：
//! - 不依赖 tauri；网络经 cn_net::Fetcher、时间经 cn_model::Clock 注入。
//! - `act(op, payload) -> ActionResult`，StateDto 与 store.mjs:1025-1078 逐字段对齐。
//! - 行为保留 B1–B12（详见 docs/impl-notes-p2-p3.md 对照表）。

pub mod error;
pub mod attachments;
pub mod ops;
pub mod persist;
pub mod pipeline;
pub mod reminders;
pub mod secrets;
pub mod service;
pub mod state;

pub use attachments::{cascade_remove, retry_attachment, AttachmentStats};
pub use error::StoreError;
pub use ops::ActRequest;
pub use persist::{load_store, save_store, save_store_crashable};
pub use secrets::{FileSecretStore, MemSecretStore, SecretStore};
pub use service::{AppEvent, CampusService, ItemInput, LoginOutcome, PipelineOptions, PollOutcome};
pub use state::{
    AppConfig, AttachmentRecord, ItemRecord, NoticeRecord, ReminderRecord, SourceRecord, StoreData,
    StoreMeta, StoreSettings, WebvpnSettings,
};
