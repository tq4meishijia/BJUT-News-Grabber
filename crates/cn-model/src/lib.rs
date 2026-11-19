//! cn-model — 跨 crate 共享 DTO / 错误码 / 配置结构（serde）与 `Clock` 注入点。
//!
//! 依赖方向（强制）：`cn-model ← cn-core ← cn-net ← cn-store ← src-tauri`。
//! 本 crate 不做任何 IO，不读时钟（`SystemClock` 仅封装 `Local::now()`，由调用方选择实现）。

pub mod auth;
pub mod clock;
pub mod dto;
pub mod error;
pub mod state;

pub use clock::{Clock, FixedClock, SystemClock};
pub use dto::{
    Deadline, DueText, Extract, FieldMap, RawItem, RuleSummary, ScoreInput, SourceKind, SourceSpec,
    SummaryInput, TemplateKind,
};
pub use error::{AuthError, AuthErrorCode};
pub use state::{
    AccountDto, ActionResult, AttachmentDto, AttachmentStatus, MetaDto, NoticeDto, PreviewItem,
    ReminderDto, SettingsDto, SourceDto, StateDto, StateItemDto, StatusDto, WebvpnDto,
};
