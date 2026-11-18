//! 登录状态机（§5.2：7 主态 + 4 异常态）与会话保持（§5.4：单飞 / 退避 / 过期预测）。
//! 纯逻辑 + Clock 注入，单测自证 11 态可达与退避表。

use std::collections::HashMap;

use chrono::{DateTime, Duration, Local};
use crate::{AuthErrorCode, Clock};
use serde::Serialize;

/// 登录模式（§5.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    WebView,
    Password,
    ImportCookie,
}

/// 11 个登录状态（§5.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AuthState {
    Unconfigured,
    Ready,
    Launching,
    AwaitingUser,
    Verifying,
    Active { expires_at: Option<String> },
    Expiring,
    Expired,
    Cancelled,
    Failed { kind: AuthErrorCode },
    Blocked,
}

impl AuthState {
    /// 是否可取消（§5.2 表）。
    pub fn cancellable(&self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Launching | Self::AwaitingUser | Self::Expired | Self::Blocked
        )
    }

    /// UI 标题（§5.2 映射表）。
    pub fn title(&self) -> &'static str {
        match self {
            Self::Unconfigured => "还没登录学校门户",
            Self::Ready => "准备登录",
            Self::Launching => "正在打开登录页",
            Self::AwaitingUser => "请在窗口中完成登录",
            Self::Verifying => "正在确认登录状态",
            Self::Active { .. } => "已登录",
            Self::Expiring => "登录即将过期",
            Self::Expired => "登录已过期",
            Self::Cancelled => "已取消登录",
            Self::Failed { .. } => "登录失败",
            Self::Blocked => "需要人工验证",
        }
    }

    /// UI 说明（§5.2 映射表）。
    pub fn hint(&self) -> String {
        match self {
            Self::Unconfigured => "登录后才会抓取到通知".to_string(),
            Self::Ready => "选择登录方式（窗口登录 / 账号密码）".to_string(),
            Self::Launching => "若 5 秒未加载，可改用粘贴 Cookie".to_string(),
            Self::AwaitingUser => "需要验证码/短信时也在这里完成".to_string(),
            Self::Verifying => "校验门户是否下发了会话 Cookie".to_string(),
            Self::Active { expires_at } => match expires_at {
                Some(at) => format!("已登录 · 有效期至 {at}，下次抓取将自动使用"),
                None => "已登录 · 下次抓取将自动使用".to_string(),
            },
            Self::Expiring => "已提前续期，无需操作".to_string(),
            Self::Expired => "正在自动重登".to_string(),
            Self::Cancelled => "不影响已抓到的通知".to_string(),
            Self::Failed { kind } => {
                format!("登录失败：{}。下一步：{}", kind.as_str(), kind.next_action())
            }
            Self::Blocked => "出现验证码，自动登录不可用".to_string(),
        }
    }

    /// 主操作（§5.2 表）。
    pub fn primary_action(&self) -> &'static str {
        match self {
            Self::Unconfigured => "去登录",
            Self::Ready => "开始",
            Self::Launching => "取消",
            Self::AwaitingUser => "我已完成",
            Self::Verifying => "",
            Self::Active { .. } => "重新登录",
            Self::Expiring => "立即续期",
            Self::Expired => "手动登录",
            Self::Failed { .. } => "重试",
            Self::Cancelled => "重新登录",
            Self::Blocked => "改用窗口登录",
        }
    }
}

/// auth://progress 事件载荷。
#[derive(Debug, Clone, Serialize)]
pub struct AuthProgress {
    pub scope: String,
    pub state: AuthState,
    /// 3 步指示：打开登录页 → 完成验证 → 确认会话。
    pub step_index: u8,
    pub step_total: u8,
    pub hint: String,
    pub at: String,
}

/// 会话记录（§5.4 过期预测：无 Max-Age 用 last_verified + 8h）。
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub last_verified_at: DateTime<Local>,
    pub expires_at: Option<DateTime<Local>>,
}

impl SessionInfo {
    /// 会话 Cookie 无过期属性时按 8 小时估计。
    pub const SESSION_ESTIMATE_HOURS: i64 = 8;

    pub fn predicted_expiry(&self) -> DateTime<Local> {
        self.expires_at.unwrap_or_else(|| {
            self.last_verified_at + Duration::hours(Self::SESSION_ESTIMATE_HOURS)
        })
    }
}

/// 退避（§5.4：1min → 5min → 15min → 1h；1h 内 ≤4 次；连续 3 次失败置 Failed 停自动重试）。
#[derive(Debug, Clone, Default)]
pub struct Backoff {
    pub consecutive_failures: u32,
    pub attempts: Vec<DateTime<Local>>,
    pub next_allowed: Option<DateTime<Local>>,
    pub stopped: bool,
}

impl Backoff {
    /// 退避档位（分钟）。
    pub const STEPS_MIN: [i64; 4] = [1, 5, 15, 60];

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.attempts.clear();
        self.next_allowed = None;
        self.stopped = false;
    }

    /// 记录失败并返回本次退避时长；连续 3 次失败后停止自动重试。
    pub fn record_failure(&mut self, now: DateTime<Local>) -> Option<Duration> {
        self.consecutive_failures += 1;
        self.attempts.push(now);
        // 1h 窗口外的尝试丢弃。
        let window_start = now - Duration::hours(1);
        self.attempts.retain(|t| *t >= window_start);
        if self.consecutive_failures >= 3 {
            self.stopped = true;
            self.next_allowed = None;
            return None;
        }
        if self.attempts.len() > 4 {
            self.stopped = true;
            return None;
        }
        let step = self.consecutive_failures.saturating_sub(1) as usize;
        let minutes = Self::STEPS_MIN[step.min(Self::STEPS_MIN.len() - 1)];
        let delay = Duration::minutes(minutes);
        self.next_allowed = Some(now + delay);
        Some(delay)
    }

    /// 现在是否允许发起续期。
    pub fn allow(&self, now: DateTime<Local>) -> bool {
        if self.stopped {
            return false;
        }
        match self.next_allowed {
            Some(t) => now >= t,
            None => true,
        }
    }
}

/// 每 scope 的登录上下文。
#[derive(Debug, Clone)]
pub struct ScopeAuth {
    pub state: AuthState,
    pub session: Option<SessionInfo>,
    pub backoff: Backoff,
}

impl Default for ScopeAuth {
    fn default() -> Self {
        Self {
            state: AuthState::Unconfigured,
            session: None,
            backoff: Backoff::default(),
        }
    }
}

/// 登录管理器：状态机 + 会话 + 单飞（10s 内不重复登录）。
#[derive(Debug, Default)]
pub struct AuthManager {
    scopes: HashMap<String, ScopeAuth>,
    /// 单飞：scope → 最近一次 ensure() 开始时刻。
    inflight: HashMap<String, DateTime<Local>>,
}

impl AuthManager {
    pub const SINGLE_FLIGHT_WINDOW_SECS: i64 = 10;

    pub fn scope(&mut self, scope: &str) -> &mut ScopeAuth {
        self.scopes.entry(scope.to_string()).or_default()
    }

    pub fn state(&mut self, scope: &str) -> &AuthState {
        &self.scope(scope).state
    }

    /// 迁移状态并产出 progress 事件载荷。
    pub fn transition(
        &mut self,
        scope: &str,
        next: AuthState,
        clock: &dyn Clock,
    ) -> AuthProgress {
        let now = clock.now();
        let step_index = match &next {
            AuthState::Ready | AuthState::Unconfigured => 0,
            AuthState::Launching | AuthState::AwaitingUser | AuthState::Blocked => 1,
            AuthState::Verifying => 2,
            _ => 3,
        };
        let entry = self.scope(scope);
        entry.state = next.clone();
        AuthProgress {
            scope: scope.to_string(),
            state: next,
            step_index,
            step_total: 3,
            hint: entry.state.hint(),
            at: now.to_rfc3339(),
        }
    }

    /// 会话是否进入 Expiring 窗口（预测过期前 30 分钟）。
    pub fn session_expiring(&self, scope: &str, clock: &dyn Clock) -> bool {
        match self.scopes.get(scope).and_then(|s| s.session.as_ref()) {
            Some(session) => {
                let now = clock.now();
                session.predicted_expiry() <= now + Duration::minutes(30)
            }
            None => false,
        }
    }

    /// 主动探测（§5.4）：每次 poll 前最轻请求判定。
    pub fn mark_verified(&mut self, scope: &str, clock: &dyn Clock) {
        let now = clock.now();
        let entry = self.scope(scope);
        entry.session = Some(SessionInfo {
            last_verified_at: now,
            expires_at: None,
        });
        entry.backoff.record_success();
    }

    /// 单飞判定：10s 内已有在途登录则拒绝。
    pub fn try_begin_single_flight(&mut self, scope: &str, clock: &dyn Clock) -> bool {
        let now = clock.now();
        if let Some(at) = self.inflight.get(scope) {
            if (now - *at).num_seconds() < Self::SINGLE_FLIGHT_WINDOW_SECS {
                return false;
            }
        }
        self.inflight.insert(scope.to_string(), now);
        true
    }

    pub fn end_single_flight(&mut self, scope: &str) {
        self.inflight.remove(scope);
    }

    /// 记录登录失败并按退避表决定下次重试。
    pub fn record_failure(
        &mut self,
        scope: &str,
        kind: AuthErrorCode,
        clock: &dyn Clock,
    ) -> AuthProgress {
        let now = clock.now();
        let entry = self.scope(scope);
        entry.backoff.record_failure(now);
        let next = AuthState::Failed { kind };
        self.transition(scope, next, clock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    struct Fixed(DateTime<Local>);
    impl Clock for Fixed {
        fn now(&self) -> DateTime<Local> {
            self.0
        }
    }

    fn clock_at(minute: u32) -> Fixed {
        Fixed(
            Local
                .with_ymd_and_hms(2026, 11, 18, 10, minute, 0)
                .single()
                .unwrap(),
        )
    }

    #[test]
    fn eleven_states_reachable() {
        let clock = clock_at(0);
        let mut mgr = AuthManager::default();
        let all = [
            AuthState::Unconfigured,
            AuthState::Ready,
            AuthState::Launching,
            AuthState::AwaitingUser,
            AuthState::Verifying,
            AuthState::Active {
                expires_at: Some("2026-11-18T18:00".into()),
            },
            AuthState::Expiring,
            AuthState::Expired,
            AuthState::Cancelled,
            AuthState::Failed {
                kind: AuthErrorCode::Credential,
            },
            AuthState::Blocked,
        ];
        assert_eq!(all.len(), 11);
        for state in &all {
            let p = mgr.transition("portal:my.bjut.edu.cn", state.clone(), &clock);
            assert_eq!(&p.state, state);
            assert_eq!(p.step_total, 3);
            assert!(!p.hint.is_empty());
        }
    }

    #[test]
    fn state_copy_covers_11_error_codes() {
        // §5.3：11 个错误码各有用户文案与下一步动作，且 Failed 态 hint 携带下一步。
        for code in AuthErrorCode::all() {
            assert!(!code.user_message().is_empty());
            assert!(!code.next_action().is_empty());
            let failed = AuthState::Failed { kind: *code };
            assert!(failed.hint().contains(code.next_action()));
        }
        assert_eq!(AuthErrorCode::all().len(), 11);
    }

    #[test]
    fn step_index_maps_to_wizard() {
        let clock = clock_at(0);
        let mut mgr = AuthManager::default();
        assert_eq!(
            mgr.transition("s", AuthState::Ready, &clock).step_index,
            0
        );
        assert_eq!(
            mgr.transition("s", AuthState::Launching, &clock).step_index,
            1
        );
        assert_eq!(
            mgr.transition("s", AuthState::AwaitingUser, &clock).step_index,
            1
        );
        assert_eq!(
            mgr.transition("s", AuthState::Verifying, &clock).step_index,
            2
        );
    }

    #[test]
    fn backoff_schedule_1_5_15_60() {
        // §5.4：失败按 1→5→15→60 min 退避，但「连续 3 次失败停自动重试」——
        // 第 3 次连续失败即停止、不再给出下一次自动重试时间（等用户操作）。
        let mut b = Backoff::default();
        let t0 = Local.with_ymd_and_hms(2026, 11, 18, 10, 0, 0).single().unwrap();
        let d1 = b.record_failure(t0).expect("first backoff");
        assert_eq!(d1, Duration::minutes(1));
        let d2 = b.record_failure(t0).expect("second");
        assert_eq!(d2, Duration::minutes(5));
        assert!(b.record_failure(t0).is_none(), "第 3 次连续失败后停自动重试");
        assert!(b.stopped);
    }

    #[test]
    fn backoff_hourly_cap_and_reset() {
        let mut b = Backoff::default();
        let t0 = Local.with_ymd_and_hms(2026, 11, 18, 10, 0, 0).single().unwrap();
        assert!(b.allow(t0));
        let _ = b.record_failure(t0);
        assert!(!b.allow(t0), "退避窗口内不允许");
        assert!(b.allow(t0 + Duration::minutes(2)), "1 分钟后允许");
        b.record_success();
        assert!(b.allow(t0 + Duration::minutes(2)), "成功后重置");
        assert_eq!(b.consecutive_failures, 0);
    }

    #[test]
    fn session_expiry_prediction_8h() {
        let clock = clock_at(0);
        let mut mgr = AuthManager::default();
        mgr.mark_verified("s", &clock);
        // 7h40m 后进入 Expiring 窗口（过期前 30 分钟）
        assert!(!mgr.session_expiring("s", &clock));
        let later = Fixed(clock.0 + Duration::hours(7) + Duration::minutes(40));
        assert!(mgr.session_expiring("s", &later));
    }

    #[test]
    fn single_flight_10s_window() {
        let mut mgr = AuthManager::default();
        let c0 = clock_at(0);
        assert!(mgr.try_begin_single_flight("s", &c0));
        assert!(!mgr.try_begin_single_flight("s", &c0), "10s 内拒绝");
        mgr.end_single_flight("s");
        assert!(mgr.try_begin_single_flight("s", &c0));
        let c1 = clock_at(0);
        let later = Fixed(c1.0 + Duration::seconds(11));
        assert!(mgr.try_begin_single_flight("s", &later), "10s 后放行");
    }

    #[test]
    fn cancellable_matrix() {
        assert!(AuthState::Launching.cancellable());
        assert!(AuthState::AwaitingUser.cancellable());
        assert!(!AuthState::Verifying.cancellable(), "Verifying 不可取消");
        assert!(!AuthState::Active { expires_at: None }.cancellable());
    }
}
