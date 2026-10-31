//! 账号表的数据库实现。
//!
//! 生产路径的账号来源：PostgreSQL 的 `auth_user` 表（见 `docs/database/rts-schema.sql`）。
//!
//! 这里刻意**不用 sqlx 的编译期宏**（`query!`）：宏会在编译期连库校验 SQL，
//! 让"没有数据库的机器"无法编译。用运行时查询 + `Row::try_get` 取值，
//! 代价是失去编译期类型检查，收益是构建过程与运行环境解耦。

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::Row;
use sqlx::postgres::PgRow;

use super::users::{AccountStatus, NewAccount, StoreError, User, UserRecord};

/// 查询账号的列清单。
const ACCOUNT_COLUMNS: &str =
    "id, username, password_hash, display_name, roles, status, created_at, updated_at";

/// 基于 PostgreSQL 的账号存储。
#[derive(Debug, Clone)]
pub struct PgAccountStore {
    pool: PgPool,
}

impl PgAccountStore {
    /// 用连接池构造。
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 连接池（供健康检查等场景使用）。
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn find_by_username(&self, username: &str) -> Result<Option<UserRecord>, StoreError> {
        let sql = format!(
            "SELECT {ACCOUNT_COLUMNS} FROM auth_user WHERE lower(username) = lower($1) LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(row_to_record).transpose()
    }

    pub async fn create(&self, account: NewAccount) -> Result<UserRecord, StoreError> {
        let sql = format!(
            "INSERT INTO auth_user (username, password_hash, display_name, roles) \
             VALUES ($1, $2, $3, $4) RETURNING {ACCOUNT_COLUMNS}"
        );
        let result = sqlx::query(&sql)
            .bind(&account.username)
            .bind(&account.password_hash)
            .bind(&account.display_name)
            .bind(&account.roles)
            .fetch_one(&self.pool)
            .await;
        match result {
            Ok(row) => row_to_record(row),
            // 用户名撞上唯一索引 → 409，而不是笼统的 500。
            Err(err) if is_unique_violation(&err) => Err(StoreError::UsernameTaken),
            Err(err) => Err(db_error(err)),
        }
    }

    pub async fn list(&self) -> Result<Vec<UserRecord>, StoreError> {
        let sql = format!("SELECT {ACCOUNT_COLUMNS} FROM auth_user ORDER BY id");
        let rows = sqlx::query(&sql)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.into_iter().map(row_to_record).collect()
    }

    pub async fn set_status(
        &self,
        username: &str,
        status: AccountStatus,
    ) -> Result<UserRecord, StoreError> {
        let sql = format!(
            "UPDATE auth_user SET status = $2, updated_at = now() \
             WHERE lower(username) = lower($1) RETURNING {ACCOUNT_COLUMNS}"
        );
        let row = sqlx::query(&sql)
            .bind(username)
            .bind(status.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(row_to_record)
            .transpose()?
            .ok_or(StoreError::NotFound)
    }

    pub async fn set_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<UserRecord, StoreError> {
        let sql = format!(
            "UPDATE auth_user SET password_hash = $2, updated_at = now() \
             WHERE lower(username) = lower($1) RETURNING {ACCOUNT_COLUMNS}"
        );
        let row = sqlx::query(&sql)
            .bind(username)
            .bind(password_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(row_to_record)
            .transpose()?
            .ok_or(StoreError::NotFound)
    }
}

/// 把一行查询结果转成账号记录。
fn row_to_record(row: PgRow) -> Result<UserRecord, StoreError> {
    let id: i64 = row.try_get("id").map_err(db_error)?;
    let username: String = row.try_get("username").map_err(db_error)?;
    let password_hash: String = row.try_get("password_hash").map_err(db_error)?;
    let display_name: String = row.try_get("display_name").map_err(db_error)?;
    let roles: Vec<String> = row.try_get("roles").map_err(db_error)?;
    let status: String = row.try_get("status").map_err(db_error)?;
    let created_at: DateTime<Utc> = row.try_get("created_at").map_err(db_error)?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at").map_err(db_error)?;
    let status = AccountStatus::parse(&status)
        .ok_or_else(|| StoreError::Database(format!("账号 {username} 的状态取值非法: {status}")))?;

    Ok(UserRecord {
        user: User {
            id: id.to_string(),
            username,
            display_name,
            roles,
        },
        password_hash,
        status,
        created_at,
        updated_at,
    })
}

/// 把 sqlx 错误转成存储错误。
///
/// **不把口令带进错误信息**：这里的错误只会包含 SQL 与 PG 错误码。
fn db_error(err: sqlx::Error) -> StoreError {
    StoreError::Database(err.to_string())
}

/// 判断是不是唯一索引冲突。
fn is_unique_violation(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db_err) => db_err.is_unique_violation(),
        _ => false,
    }
}
