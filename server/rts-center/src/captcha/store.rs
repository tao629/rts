//! 验证码内存存储。
//!
//! 进程内保存「captchaId → 答案 + 过期时刻」，供登录时一次性消费。
//!
//! 三个关键语义：
//! - **一次性**：[`CaptchaStore::consume`] 取走即删除，无论后续校验是否通过；
//! - **惰性清理**：过期条目在读写时顺带清掉，避免后台定时任务；
//! - **容量上限**：条目数超过上限时淘汰最旧的一条，防止内存被撑爆。

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use rand::Rng;

/// 默认容量上限。
pub const DEFAULT_CAPACITY: usize = 10_000;

/// 一条验证码记录。
#[derive(Debug, Clone)]
struct CaptchaEntry {
    /// 答案（大写）。
    answer: String,
    /// 过期时刻。
    expires_at: Instant,
}

/// 验证码存储。
#[derive(Debug)]
pub struct CaptchaStore {
    /// captchaId → 记录。
    items: HashMap<String, CaptchaEntry>,
    /// captchaId 的写入顺序，用于淘汰最旧条目。
    order: VecDeque<String>,
    /// 有效期。
    ttl: Duration,
    /// 容量上限。
    capacity: usize,
}

impl CaptchaStore {
    /// 新建存储。
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            items: HashMap::new(),
            order: VecDeque::new(),
            ttl,
            capacity: capacity.max(1),
        }
    }

    /// 存入一个验证码，返回其 captchaId。
    pub fn issue(&mut self, answer: &str) -> String {
        self.cleanup_expired();

        // 容量已满时先淘汰最旧的一条，给新条目腾位置。
        while self.items.len() >= self.capacity {
            if !self.evict_oldest() {
                break;
            }
        }

        let id = new_id();
        self.items.insert(
            id.clone(),
            CaptchaEntry {
                answer: answer.to_owned(),
                expires_at: Instant::now() + self.ttl,
            },
        );
        self.order.push_back(id.clone());
        id
    }

    /// 取出并作废一个验证码。
    ///
    /// 返回 `None` 表示该 captchaId 不存在或已过期；返回 `Some` 时该条目**一定已被删除**，
    /// 因此调用方拿到答案后无论比对成功与否，都必须按"验证码已消费"处理。
    pub fn consume(&mut self, id: &str) -> Option<String> {
        self.cleanup_expired();
        self.items.remove(id).map(|entry| entry.answer)
    }

    /// 当前条目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 惰性清理过期条目。
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        let before = self.items.len();
        self.items.retain(|_, entry| entry.expires_at > now);
        if self.items.len() != before {
            // 顺序表里对应条目也要一并剔除，否则会一直堆积。
            self.order.retain(|id| self.items.contains_key(id));
        }
    }

    /// 淘汰最旧的一条，返回是否确实淘汰掉了一条。
    fn evict_oldest(&mut self) -> bool {
        while let Some(id) = self.order.pop_front() {
            // 已被消费掉的 id 只是顺序表里的残留，直接跳过。
            if self.items.remove(&id).is_some() {
                return true;
            }
        }
        false
    }
}

/// 生成一个随机的 captchaId（32 位十六进制字符串）。
fn new_id() -> String {
    let mut rng = rand::rng();
    let bytes: [u8; 16] = rng.random();
    let mut id = String::with_capacity(32);
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consume_is_one_shot() {
        let mut store = CaptchaStore::new(Duration::from_secs(300), 100);
        let id = store.issue("ABCD");
        assert_eq!(store.consume(&id).as_deref(), Some("ABCD"));
        assert_eq!(store.consume(&id), None);
    }

    #[test]
    fn expired_entry_is_not_returned() {
        let mut store = CaptchaStore::new(Duration::from_millis(1), 100);
        let id = store.issue("ABCD");
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(store.consume(&id), None);
        assert!(store.is_empty());
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut store = CaptchaStore::new(Duration::from_secs(300), 2);
        let first = store.issue("AAAA");
        let second = store.issue("BBBB");
        let third = store.issue("CCCC");
        assert_eq!(store.len(), 2);
        assert_eq!(store.consume(&first), None);
        assert_eq!(store.consume(&second).as_deref(), Some("BBBB"));
        assert_eq!(store.consume(&third).as_deref(), Some("CCCC"));
    }
}
