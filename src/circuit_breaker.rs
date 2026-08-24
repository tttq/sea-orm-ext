//! 简单熔断器实现。
//!
//! 提供 三态熔断器：
//! - `Closed`：正常状态，所有请求放行；失败累计达阈值后切换到 `Open`。
//! - `Open`：熔断状态，所有请求快速失败；超时后切换到 `HalfOpen`。
//! - `HalfOpen`：半开状态，允许单个探针请求；成功则切换回 `Closed`，失败则回到 `Open`。
//!
//! # 用法
//!
//! ```ignore
//! use sea_orm_ext::circuit_breaker::CircuitBreaker;
//! use std::time::Duration;
//!
//! let breaker = CircuitBreaker::new(5, Duration::from_secs(30));
//! if breaker.allow() {
//!     match do_db_work().await {
//!         Ok(v) => { breaker.record_success(); Ok(v) }
//!         Err(e) => { breaker.record_failure(); Err(e) }
//!     }
//! } else {
//!     Err(DbErr::Custom("circuit breaker open".to_owned()))
//! }
//! ```

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 熔断器状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    /// 关闭（正常工作）。
    Closed,
    /// 打开（熔断中，快速失败）。
    Open,
    /// 半开（允许探针请求）。
    HalfOpen,
}

const STATE_CLOSED: u8 = 0;
const STATE_OPEN: u8 = 1;
const STATE_HALF_OPEN: u8 = 2;

fn state_from_u8(v: u8) -> BreakerState {
    match v {
        STATE_CLOSED => BreakerState::Closed,
        STATE_OPEN => BreakerState::Open,
        STATE_HALF_OPEN => BreakerState::HalfOpen,
        _ => BreakerState::Closed,
    }
}

/// 简单熔断器（无锁，基于原子操作）。
///
/// 失败计数和状态切换均通过原子操作完成，适合高并发场景。
pub struct CircuitBreaker {
    /// 触发熔断的连续失败次数阈值。
    failure_threshold: u64,
    /// 熔断持续时间，超过后进入 HalfOpen。
    reset_timeout: Duration,
    /// 当前状态（编码为 u8）。
    state: AtomicU8,
    /// 连续失败次数。
    consecutive_failures: AtomicU64,
    /// 进入 Open 状态的时刻（自进程启动以来的纳秒数）。
    opened_at_ns: AtomicU64,
}

impl CircuitBreaker {
    /// 创建新的熔断器。
    ///
    /// - `failure_threshold`：连续失败多少次后熔断。
    /// - `reset_timeout`：熔断持续时间，超过后进入 HalfOpen 尝试恢复。
    pub fn new(failure_threshold: u64, reset_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            reset_timeout,
            state: AtomicU8::new(STATE_CLOSED),
            consecutive_failures: AtomicU64::new(0),
            opened_at_ns: AtomicU64::new(0),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> BreakerState {
        let s = self.state.load(Ordering::Acquire);
        let st = state_from_u8(s);
        if st == BreakerState::Open {
            // 检查是否已过 reset_timeout，若是则切换到 HalfOpen
            let opened_at = self.opened_at_ns.load(Ordering::Acquire);
            let elapsed = elapsed_since_ns(opened_at);
            if elapsed >= self.reset_timeout.as_nanos() as u64 {
                // 尝试切换到 HalfOpen
                let _ = self.state.compare_exchange(
                    STATE_OPEN,
                    STATE_HALF_OPEN,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return BreakerState::HalfOpen;
            }
        }
        st
    }

    /// 是否允许请求通过。
    ///
    /// - `Closed` → 允许。
    /// - `Open` 且未过 reset_timeout → 拒绝。
    /// - `Open` 且已过 reset_timeout → 切换到 `HalfOpen`，允许。
    /// - `HalfOpen` → 允许（仅一个探针请求；其他并发请求会被拒绝）。
    pub fn allow(&self) -> bool {
        match self.state() {
            BreakerState::Closed => true,
            BreakerState::Open => false,
            BreakerState::HalfOpen => true,
        }
    }

    /// 记录一次成功。
    ///
    /// 重置失败计数，并将状态从 `HalfOpen` 切换回 `Closed`。
    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Release);
        let prev = self.state.swap(STATE_CLOSED, Ordering::AcqRel);
        if prev != STATE_CLOSED {
            tracing::info!(
                "CIRCUIT-BREAKER: state {:?} -> Closed (success recorded)",
                state_from_u8(prev)
            );
        }
    }

    /// 记录一次失败。
    ///
    /// - `Closed`：累计失败次数；达到阈值后切换到 `Open`。
    /// - `HalfOpen`：立即切换到 `Open`。
    /// - `Open`：保持 `Open`，重置 opened_at。
    pub fn record_failure(&self) {
        let prev_state = self.state.load(Ordering::Acquire);

        match state_from_u8(prev_state) {
            BreakerState::HalfOpen => {
                // 半开状态下失败，立即回到 Open
                self.opened_at_ns.store(now_ns(), Ordering::Release);
                self.state.store(STATE_OPEN, Ordering::Release);
                tracing::warn!(
                    "CIRCUIT-BREAKER: HalfOpen -> Open (probe request failed)"
                );
            }
            BreakerState::Closed => {
                let failures = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
                if failures >= self.failure_threshold {
                    self.opened_at_ns.store(now_ns(), Ordering::Release);
                    self.state.store(STATE_OPEN, Ordering::Release);
                    tracing::warn!(
                        "CIRCUIT-BREAKER: Closed -> Open (failures = {}, threshold = {})",
                        failures, self.failure_threshold
                    );
                }
            }
            BreakerState::Open => {
                // 已在 Open 状态，刷新 opened_at（延长熔断）
                self.opened_at_ns.store(now_ns(), Ordering::Release);
            }
        }
    }

    /// 重置熔断器到 Closed 状态（用于手动恢复）。
    pub fn reset(&self) {
        self.consecutive_failures.store(0, Ordering::Release);
        self.state.store(STATE_CLOSED, Ordering::Release);
        self.opened_at_ns.store(0, Ordering::Release);
    }

    /// 当前连续失败次数。
    pub fn consecutive_failures(&self) -> u64 {
        self.consecutive_failures.load(Ordering::Acquire)
    }

    /// 失败阈值。
    pub fn failure_threshold(&self) -> u64 {
        self.failure_threshold
    }

    /// 熔断持续时间。
    pub fn reset_timeout(&self) -> Duration {
        self.reset_timeout
    }
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field("failure_threshold", &self.failure_threshold)
            .field("reset_timeout", &self.reset_timeout)
            .field("state", &self.state())
            .field("consecutive_failures", &self.consecutive_failures())
            .finish()
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        // 默认：连续 5 次失败熔断，30 秒后尝试恢复
        Self::new(5, Duration::from_secs(30))
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn elapsed_since_ns(since_ns: u64) -> u64 {
    if since_ns == 0 {
        return 0;
    }
    let now = now_ns();
    now.saturating_sub(since_ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_closed() {
        let cb = CircuitBreaker::new(3, Duration::from_millis(100));
        assert_eq!(cb.state(), BreakerState::Closed);
        assert!(cb.allow());
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[test]
    fn test_failure_accumulation_below_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_millis(100));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), BreakerState::Closed);
        assert!(cb.allow());
        assert_eq!(cb.consecutive_failures(), 2);
    }

    #[test]
    fn test_open_after_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        cb.record_failure();
        // 已达阈值，应该切换到 Open
        assert_eq!(cb.state(), BreakerState::Open);
        assert!(!cb.allow());
    }

    #[test]
    fn test_success_resets_failures() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.consecutive_failures(), 0);
        assert_eq!(cb.state(), BreakerState::Closed);
    }

    #[test]
    fn test_reset_method() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), BreakerState::Open);
        cb.reset();
        assert_eq!(cb.state(), BreakerState::Closed);
        assert_eq!(cb.consecutive_failures(), 0);
    }

    #[test]
    fn test_default() {
        let cb = CircuitBreaker::default();
        assert_eq!(cb.failure_threshold(), 5);
        assert_eq!(cb.reset_timeout(), Duration::from_secs(30));
        assert_eq!(cb.state(), BreakerState::Closed);
    }
}
