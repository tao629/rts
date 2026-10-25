//! 登录失败计数与锁定。
//!
//! 锁定维度是「用户名 + 来源 IP」：这样既限制了针对某个账号的在线猜解速率，
//! 又不会因为某个 IP 猜错一次就锁死所有账号。
//!
//! 计数规则：
//! - 一次失败计数 +1；
//! - 连续失败达到阈值即进入锁定期，锁定期内任何登录都返回 `ACCOUNT_LOCKED`；
//! - 登录成功清零。
//!
//! 不存在的用户名同样计入失败：这让"账号不存在"与"口令错误"的响应完全一致，
//! 攻击者无法借此枚举账号。

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 一条失败记录。
#[derive(Debug, Clone)]
struct FailureRecord {
    /// 连续失败次数。
    count: u32,
    /// 锁定截止时刻，`None` 表示当前未锁定。
    locked_until: Option<Instant>,
}

/// 登录失败守卫。
#[derive(Debug)]
pub struct LoginGuard {
    /// 锁定键 → 失败记录。
    records: HashMap<String, FailureRecord>,
    /// 触发锁定的连续失败次数。
    max_failures: u32,
    /// 锁定时长。
    lock_duration: Duration,
}

/// 锁定状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStatus {
    /// 未锁定。
    Allowed,
    /// 锁定中。
    Locked,
}

impl LoginGuard {
    /// 新建守卫。
    pub fn new(max_failures: u32, lock_duration: Duration) -> Self {
        Self {
            records: HashMap::new(),
            max_failures: max_failures.max(1),
            lock_duration,
        }
    }

    /// 查询当前是否处于锁定期。
    ///
    /// 锁定期已过时顺手清理记录，避免锁定状态残留。
    pub fn status(&mut self, key: &str) -> LockStatus {
        let now = Instant::now();
        let Some(record) = self.records.get_mut(key) else {
            return LockStatus::Allowed;
        };
        match record.locked_until {
            Some(until) if until > now => LockStatus::Locked,
            Some(_) => {
                // 锁定已到期：清掉锁定标记，失败计数也从零重新开始。
                record.locked_until = None;
                record.count = 0;
                LockStatus::Allowed
            }
            None => LockStatus::Allowed,
        }
    }

    /// 记录一次失败，返回记录之后的锁定状态。
    ///
    /// 失败次数**达到**阈值时会给账号上锁，但这一次尝试本身仍然算失败
    /// （返回 `INVALID_CREDENTIALS`）；从**下一次**尝试开始才返回 `ACCOUNT_LOCKED`。
    /// 这与 spec A7「连续 N 次失败后再试 → 423」一致。
    pub fn record_failure(&mut self, key: &str) -> LockStatus {
        let now = Instant::now();
        let max_failures = self.max_failures;
        let lock_duration = self.lock_duration;
        let record = self.records.entry(key.to_owned()).or_insert(FailureRecord {
            count: 0,
            locked_until: None,
        });
        record.count += 1;
        if record.count >= max_failures {
            record.locked_until = Some(now + lock_duration);
            return LockStatus::Locked;
        }
        LockStatus::Allowed
    }

    /// 登录成功后清零计数。
    pub fn reset(&mut self, key: &str) {
        self.records.remove(key);
    }
}

/// 拼装锁定键：用户名 + 来源 IP。
pub fn lock_key(username: &str, ip: &str) -> String {
    format!("{username}\u{0}{ip}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_triggers_at_threshold() {
        let mut guard = LoginGuard::new(3, Duration::from_secs(60));
        let key = lock_key("admin", "127.0.0.1");
        assert_eq!(guard.record_failure(&key), LockStatus::Allowed);
        assert_eq!(guard.record_failure(&key), LockStatus::Allowed);
        assert_eq!(guard.record_failure(&key), LockStatus::Locked);
        assert_eq!(guard.status(&key), LockStatus::Locked);
    }

    #[test]
    fn reset_clears_counter() {
        let mut guard = LoginGuard::new(2, Duration::from_secs(60));
        let key = lock_key("admin", "127.0.0.1");
        guard.record_failure(&key);
        guard.reset(&key);
        assert_eq!(guard.status(&key), LockStatus::Allowed);
        assert_eq!(guard.record_failure(&key), LockStatus::Allowed);
    }

    #[test]
    fn different_ip_has_independent_counter() {
        let mut guard = LoginGuard::new(2, Duration::from_secs(60));
        guard.record_failure(&lock_key("admin", "1.1.1.1"));
        assert_eq!(
            guard.status(&lock_key("admin", "2.2.2.2")),
            LockStatus::Allowed
        );
    }
}
