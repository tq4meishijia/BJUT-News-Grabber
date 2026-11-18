//! 时钟注入：`cn-core` 的唯一时间来源，禁止在纯逻辑里读系统时钟。

use chrono::{DateTime, Local};

/// 时间提供者。golden 测试用 [`FixedClock`] 冻结时间，生产环境用 [`SystemClock`]。
pub trait Clock: Send + Sync {
    /// 当前本地时间。
    fn now(&self) -> DateTime<Local>;
}

/// 系统时钟（生产环境唯一实现；只允许在 IO 边界层构造）。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Local> {
        Local::now()
    }
}

/// 固定时钟（测试与 golden 对照用）。
#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    at: DateTime<Local>,
}

impl FixedClock {
    /// 在给定本地时间处冻结。
    pub const fn new(at: DateTime<Local>) -> Self {
        Self { at }
    }

    /// 取冻结时刻。
    pub const fn at(&self) -> DateTime<Local> {
        self.at
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Local> {
        self.at
    }
}
