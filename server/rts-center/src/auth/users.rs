//! 账号表。
//!
//! 账号存放在 PostgreSQL 的 `auth_user` 表里（见 `docs/database/rts-schema.sql`），
//! 服务端只持有**口令哈希**（argon2id PHC 串），明文口令在收到请求时哈希后即被丢弃。
//! 仓库里**不存放任何口令或哈希字面量**，凭据一律由部署方通过环境变量提供或经
//! 账号管理接口写入。
//!
//! 取账号的方式被收敛到 [`AccountStore`]：生产路径是 [`super::db::PgAccountStore`]（查库），
//! 测试路径是 [`MemoryAccountStore`]（进程内存）。这样接口级测试不需要真实数据库，
//! 而生产代码只有一条取数路径。

use std::fmt;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use super::db::PgAccountStore;

/// 用户公开信息（会随登录响应返回）。
#[derive(Debug, Clone)]
pub struct User {
    /// 用户 ID（数据库主键的十进制形式）。
    pub id: String,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
}

/// 账号状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    /// 正常：可登录，令牌有效。
    Active,
    /// 停用：登录被拒，已签发的令牌也随即失效。
    Disabled,
}

impl AccountStatus {
    /// 落库 / 出库的字符串形式。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    /// 解析字符串形式；不是合法取值时返回 `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// 运行期账号记录：用户信息 + 口令哈希 + 状态。
#[derive(Debug, Clone)]
pub struct UserRecord {
    /// 用户公开信息。
    pub user: User,
    /// PHC 格式的 argon2id 哈希。
    pub password_hash: String,
    /// 账号状态。
    pub status: AccountStatus,
    /// 建号时间。
    pub created_at: DateTime<Utc>,
    /// 最后变更时间。
    pub updated_at: DateTime<Utc>,
}

/// 新建账号的入参。
#[derive(Debug, Clone)]
pub struct NewAccount {
    /// 登录名。
    pub username: String,
    /// **已经哈希过的**口令（PHC 串）。
    pub password_hash: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
}

/// 账号存储出错。
#[derive(Debug)]
pub enum StoreError {
    /// 数据库出错（连接失败、SQL 失败等）。
    Database(String),
    /// 用户名已存在。
    UsernameTaken,
    /// 目标账号不存在。
    NotFound,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(message) => write!(f, "账号存储出错: {message}"),
            Self::UsernameTaken => write!(f, "用户名已存在"),
            Self::NotFound => write!(f, "账号不存在"),
        }
    }
}

impl std::error::Error for StoreError {}

/// 账号来源。
///
/// 用枚举而不是 trait 对象，是因为异步 trait 方法不满足对象安全；
/// 两个变体也把"生产走查库、测试走内存"这件事写在了类型里。
#[derive(Debug)]
pub enum AccountStore {
    /// 生产路径：查 PostgreSQL 的 `auth_user` 表。
    Db(PgAccountStore),
    /// 测试与本地开发路径：进程内存里的账号表。
    Memory(MemoryAccountStore),
}

impl AccountStore {
    /// 按登录名查账号（**不区分大小写**，与库里的唯一索引一致）。
    pub async fn find_by_username(&self, username: &str) -> Result<Option<UserRecord>, StoreError> {
        match self {
            Self::Db(store) => store.find_by_username(username).await,
            Self::Memory(store) => store.find_by_username(username).await,
        }
    }

    /// 新建账号；用户名已存在时返回 [`StoreError::UsernameTaken`]。
    pub async fn create(&self, account: NewAccount) -> Result<UserRecord, StoreError> {
        match self {
            Self::Db(store) => store.create(account).await,
            Self::Memory(store) => store.create(account).await,
        }
    }

    /// 列出全部账号（按 ID 升序）。
    pub async fn list(&self) -> Result<Vec<UserRecord>, StoreError> {
        match self {
            Self::Db(store) => store.list().await,
            Self::Memory(store) => store.list().await,
        }
    }

    /// 修改账号状态；账号不存在时返回 [`StoreError::NotFound`]。
    pub async fn set_status(
        &self,
        username: &str,
        status: AccountStatus,
    ) -> Result<UserRecord, StoreError> {
        match self {
            Self::Db(store) => store.set_status(username, status).await,
            Self::Memory(store) => store.set_status(username, status).await,
        }
    }

    /// 修改口令哈希；账号不存在时返回 [`StoreError::NotFound`]。
    pub async fn set_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<UserRecord, StoreError> {
        match self {
            Self::Db(store) => store.set_password(username, password_hash).await,
            Self::Memory(store) => store.set_password(username, password_hash).await,
        }
    }
}

/// 进程内存里的账号表：测试替身与本地开发用。
#[derive(Debug, Default)]
pub struct MemoryAccountStore {
    records: RwLock<Vec<UserRecord>>,
}

impl MemoryAccountStore {
    /// 用给定记录初始化。
    pub fn new(records: Vec<UserRecord>) -> Self {
        Self {
            records: RwLock::new(records),
        }
    }

    /// 用账号规格初始化（明文口令在此处哈希，明文随即被丢弃）。
    pub fn from_specs(specs: Vec<AccountSpec>) -> Self {
        Self::new(build_records(specs))
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<UserRecord>, StoreError> {
        let records = self.records.read().await;
        Ok(records
            .iter()
            .find(|record| record.user.username.eq_ignore_ascii_case(username))
            .cloned())
    }

    async fn create(&self, account: NewAccount) -> Result<UserRecord, StoreError> {
        let mut records = self.records.write().await;
        if records
            .iter()
            .any(|record| record.user.username.eq_ignore_ascii_case(&account.username))
        {
            return Err(StoreError::UsernameTaken);
        }
        let next_id = records.len() as i64 + 1;
        let now = Utc::now();
        let record = UserRecord {
            user: User {
                id: next_id.to_string(),
                username: account.username,
                display_name: account.display_name,
                roles: account.roles,
            },
            password_hash: account.password_hash,
            status: AccountStatus::Active,
            created_at: now,
            updated_at: now,
        };
        records.push(record.clone());
        Ok(record)
    }

    async fn list(&self) -> Result<Vec<UserRecord>, StoreError> {
        let records = self.records.read().await;
        let mut list = records.clone();
        list.sort_by_key(|record| record.user.id.parse::<i64>().unwrap_or_default());
        Ok(list)
    }

    async fn set_status(
        &self,
        username: &str,
        status: AccountStatus,
    ) -> Result<UserRecord, StoreError> {
        let mut records = self.records.write().await;
        let record = records
            .iter_mut()
            .find(|record| record.user.username.eq_ignore_ascii_case(username))
            .ok_or(StoreError::NotFound)?;
        record.status = status;
        record.updated_at = Utc::now();
        Ok(record.clone())
    }

    async fn set_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<UserRecord, StoreError> {
        let mut records = self.records.write().await;
        let record = records
            .iter_mut()
            .find(|record| record.user.username.eq_ignore_ascii_case(username))
            .ok_or(StoreError::NotFound)?;
        record.password_hash = password_hash.to_owned();
        record.updated_at = Utc::now();
        Ok(record.clone())
    }
}

/// 一个账号的构造规格（仅含明文口令，用于启动期或测试期构造账号）。
///
/// 与 `AccountSpec` 不同的是：这里**不负责解析环境变量串**，只描述一个账号。
#[derive(Debug, Clone)]
pub struct AccountSpec {
    /// 登录名。
    pub username: String,
    /// 明文口令（构造时哈希，随即被丢弃）。
    pub password: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
}

/// 把账号规格转换成运行期账号记录（口令哈希后丢弃明文）。
///
/// 哈希运算有成本（argon2 默认参数约几十毫秒），因此在构造阶段一次性完成，
/// 而不是每次登录时再算。
pub fn build_records(accounts: Vec<AccountSpec>) -> Vec<UserRecord> {
    accounts
        .into_iter()
        .enumerate()
        .map(|(index, account)| {
            let password_hash =
                hash_password(&account.password).expect("口令哈希不应失败：argon2 参数为默认值");
            let now = Utc::now();
            UserRecord {
                user: User {
                    id: (index as i64 + 1).to_string(),
                    username: account.username,
                    display_name: account.display_name,
                    roles: account.roles,
                },
                password_hash,
                status: AccountStatus::Active,
                created_at: now,
                updated_at: now,
            }
        })
        .collect()
}

/// 用 argon2id 计算口令哈希。
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    // argon2 默认使用 Argon2id v19 参数，随机盐由库内部生成。
    Ok(Argon2::default()
        .hash_password(password.as_bytes())?
        .to_string())
}

/// 校验口令是否与记录的哈希匹配。
pub fn verify_password(record: &UserRecord, password: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(&record.password_hash) else {
        // 哈希串本身损坏，按校验失败处理。
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 运行时随机口令：测试里不出现任何口令字面量。
    fn random_password() -> String {
        crate::config::random_secret_with_len(24)
    }

    #[test]
    fn records_keep_only_password_hash() {
        let password = random_password();
        let accounts = vec![AccountSpec {
            username: "alice".to_owned(),
            password: password.clone(),
            display_name: "显示名甲".to_owned(),
            roles: vec!["role_a".to_owned()],
        }];
        let records = build_records(accounts);
        assert_eq!(records.len(), 1);
        let alice = &records[0];
        assert!(alice.password_hash.starts_with("$argon2id$"));
        assert!(!alice.password_hash.contains(&password));
        assert_eq!(alice.user.display_name, "显示名甲");
        assert_eq!(alice.status, AccountStatus::Active);
    }

    #[test]
    fn verify_password_accepts_correct_and_rejects_wrong() {
        let password = random_password();
        let accounts = vec![AccountSpec {
            username: "alice".to_owned(),
            password: password.clone(),
            display_name: "显示名甲".to_owned(),
            roles: vec![],
        }];
        let records = build_records(accounts);
        let alice = &records[0];
        assert!(verify_password(alice, &password));
        assert!(!verify_password(alice, &format!("{password}x")));
    }

    #[test]
    fn account_status_parses_only_known_values() {
        assert_eq!(AccountStatus::parse("active"), Some(AccountStatus::Active));
        assert_eq!(
            AccountStatus::parse("disabled"),
            Some(AccountStatus::Disabled)
        );
        assert_eq!(AccountStatus::parse("unknown"), None);
        assert_eq!(AccountStatus::parse(""), None);
    }

    #[tokio::test]
    async fn memory_store_finds_case_insensitively() {
        let store = MemoryAccountStore::from_specs(vec![AccountSpec {
            username: "Alice".to_owned(),
            password: random_password(),
            display_name: "显示名甲".to_owned(),
            roles: vec![],
        }]);
        assert!(
            store
                .find_by_username("alice")
                .await
                .expect("查询应成功")
                .is_some()
        );
        assert!(
            store
                .find_by_username("ALICE")
                .await
                .expect("查询应成功")
                .is_some()
        );
        assert!(
            store
                .find_by_username("nobody")
                .await
                .expect("查询应成功")
                .is_none()
        );
    }

    #[tokio::test]
    async fn memory_store_rejects_duplicated_username() {
        let store = MemoryAccountStore::default();
        let account = NewAccount {
            username: "alice".to_owned(),
            password_hash: hash_password("whatever").expect("哈希应成功"),
            display_name: "显示名甲".to_owned(),
            roles: vec![],
        };
        store.create(account.clone()).await.expect("首次创建应成功");
        let error = store.create(account).await.expect_err("重复用户名应失败");
        assert!(matches!(error, StoreError::UsernameTaken));
    }
}
