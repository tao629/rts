//! 账号管理接口。
//!
//! 面向超管的账号维护入口：新增账号、列出账号、启停账号、重置口令。
//!
//! 两条硬约束：
//! 1. **权限门槛**：必须带有效令牌，且令牌 `roles` 含 `super_admin`；
//! 2. **口令只进不出**：请求体里的明文口令在服务端哈希后即弃，
//!    响应体、日志、审计记录里都不出现口令与口令哈希。
//!
//! 契约见 `intents/20261031_ui-login-real-backend/spec.md` §3.3。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::auth::bearer::{self, BearerToken};
use crate::auth::users::{AccountStatus, NewAccount, StoreError, UserRecord, hash_password};
use crate::error::ApiError;
use crate::state::AppState;

/// 用户名最小长度。
const MIN_USERNAME_LEN: usize = 3;
/// 用户名最大长度。
const MAX_USERNAME_LEN: usize = 64;
/// 口令最小长度。
const MIN_PASSWORD_LEN: usize = 8;
/// 口令最大长度。
const MAX_PASSWORD_LEN: usize = 128;
/// 显示名最大长度。
const MAX_DISPLAY_NAME_LEN: usize = 64;
/// 单个角色名最大长度。
const MAX_ROLE_LEN: usize = 32;
/// 角色数量上限。
const MAX_ROLES: usize = 16;

/// 新增账号的请求体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    /// 登录名。
    pub username: String,
    /// 明文口令：只在这一次请求里出现，服务端哈希后即弃。
    pub password: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表，可省略。
    #[serde(default)]
    pub roles: Vec<String>,
}

/// 修改账号状态的请求体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusRequest {
    /// 目标状态：`active` 或 `disabled`。
    pub status: String,
}

/// 重置口令的请求体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    /// 明文口令：只在这一次请求里出现，服务端哈希后即弃。
    pub password: String,
}

/// 账号视图：**不含口令哈希**。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    /// 账号 ID。
    pub id: String,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
    /// 账号状态。
    pub status: String,
    /// 建号时间（ISO 8601 UTC）。
    pub created_at: String,
    /// 最后变更时间（ISO 8601 UTC）。
    pub updated_at: String,
}

impl From<&UserRecord> for UserView {
    fn from(record: &UserRecord) -> Self {
        Self {
            id: record.user.id.clone(),
            username: record.user.username.clone(),
            display_name: record.user.display_name.clone(),
            roles: record.user.roles.clone(),
            status: record.status.as_str().to_owned(),
            created_at: crate::auth::token::format_iso8601(record.created_at),
            updated_at: crate::auth::token::format_iso8601(record.updated_at),
        }
    }
}

/// 账号列表响应体。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserListResponse {
    /// 账号列表。
    pub users: Vec<UserView>,
}

/// `POST /api/v1/admin/users`：新增账号。
pub async fn create_user_handler(
    State(state): State<Arc<AppState>>,
    bearer: BearerToken,
    payload: Result<Json<CreateUserRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<(StatusCode, Json<UserView>), ApiError> {
    let actor = bearer::authenticate_admin(&state.config.jwt_secret, &bearer.0)?;

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            // 不回显请求体内容，避免口令进入错误信息与日志。
            tracing::warn!(error = %rejection, "新增账号请求体解析失败");
            return Err(ApiError::validation("请求体格式非法"));
        }
    };

    let account = validate_new_account(request)?;

    // 口令在这里哈希：明文不进数据库、不进日志。
    let password_hash = hash_password(&account.password).map_err(|err| {
        tracing::error!(error = %err, "计算口令哈希失败");
        ApiError::Server("口令处理失败".to_owned())
    })?;

    let record = state
        .store
        .create(NewAccount {
            username: account.username,
            password_hash,
            display_name: account.display_name,
            roles: account.roles,
        })
        .await
        .map_err(store_error)?;

    tracing::info!(
        actor = %actor.username,
        target = %record.user.username,
        action = "create",
        "账号管理：新增账号"
    );

    Ok((StatusCode::CREATED, Json(UserView::from(&record))))
}

/// `GET /api/v1/admin/users`：列出账号。
pub async fn list_users_handler(
    State(state): State<Arc<AppState>>,
    bearer: BearerToken,
) -> Result<Json<UserListResponse>, ApiError> {
    let actor = bearer::authenticate_admin(&state.config.jwt_secret, &bearer.0)?;

    let records = state.store.list().await.map_err(store_error)?;

    tracing::info!(actor = %actor.username, count = records.len(), "账号管理：列出账号");

    Ok(Json(UserListResponse {
        users: records.iter().map(UserView::from).collect(),
    }))
}

/// `PATCH /api/v1/admin/users/{username}/status`：启用 / 停用账号。
pub async fn update_status_handler(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    bearer: BearerToken,
    payload: Result<Json<UpdateStatusRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<UserView>, ApiError> {
    let actor = bearer::authenticate_admin(&state.config.jwt_secret, &bearer.0)?;

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            tracing::warn!(error = %rejection, "修改账号状态请求体解析失败");
            return Err(ApiError::validation("请求体格式非法"));
        }
    };

    let status = AccountStatus::parse(request.status.trim())
        .ok_or_else(|| ApiError::validation("状态取值非法，只能是 active 或 disabled"))?;

    let record = state
        .store
        .set_status(&username, status)
        .await
        .map_err(store_error)?;

    tracing::info!(
        actor = %actor.username,
        target = %record.user.username,
        status = status.as_str(),
        action = "set-status",
        "账号管理：修改账号状态"
    );

    Ok(Json(UserView::from(&record)))
}

/// `POST /api/v1/admin/users/{username}/password`：重置口令。
pub async fn reset_password_handler(
    State(state): State<Arc<AppState>>,
    Path(username): Path<String>,
    bearer: BearerToken,
    payload: Result<Json<ResetPasswordRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let actor = bearer::authenticate_admin(&state.config.jwt_secret, &bearer.0)?;

    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            tracing::warn!(error = %rejection, "重置口令请求体解析失败");
            return Err(ApiError::validation("请求体格式非法"));
        }
    };

    let password = validate_password(&request.password)?;

    let password_hash = hash_password(&password).map_err(|err| {
        tracing::error!(error = %err, "计算口令哈希失败");
        ApiError::Server("口令处理失败".to_owned())
    })?;

    state
        .store
        .set_password(&username, &password_hash)
        .await
        .map_err(store_error)?;

    tracing::info!(
        actor = %actor.username,
        target = %username,
        action = "reset-password",
        "账号管理：重置口令"
    );

    Ok(StatusCode::NO_CONTENT)
}

/// 校验并归一化新增账号的入参。
fn validate_new_account(request: CreateUserRequest) -> Result<ValidatedAccount, ApiError> {
    let username = request.username.trim().to_owned();
    if username.chars().count() < MIN_USERNAME_LEN || username.chars().count() > MAX_USERNAME_LEN {
        return Err(ApiError::validation("用户名长度不合法"));
    }
    if !username
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(ApiError::validation("用户名只能包含字母、数字与 . _ -"));
    }

    let display_name = request.display_name.trim().to_owned();
    if display_name.is_empty() || display_name.chars().count() > MAX_DISPLAY_NAME_LEN {
        return Err(ApiError::validation("显示名不合法"));
    }

    let password = validate_password(&request.password)?;

    let roles = normalize_roles(request.roles)?;

    Ok(ValidatedAccount {
        username,
        password,
        display_name,
        roles,
    })
}

/// 校验口令长度。
fn validate_password(password: &str) -> Result<String, ApiError> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(ApiError::validation("口令至少 8 个字符"));
    }
    if len > MAX_PASSWORD_LEN {
        return Err(ApiError::validation("口令不能超过 128 个字符"));
    }
    Ok(password.to_owned())
}

/// 归一化角色列表：去空白、去空项、去重，并限制数量与长度。
fn normalize_roles(roles: Vec<String>) -> Result<Vec<String>, ApiError> {
    let mut normalized: Vec<String> = Vec::new();
    for role in roles {
        let role = role.trim().to_owned();
        if role.is_empty() {
            continue;
        }
        if role.chars().count() > MAX_ROLE_LEN {
            return Err(ApiError::validation("角色名过长"));
        }
        if !normalized.contains(&role) {
            normalized.push(role);
        }
    }
    if normalized.len() > MAX_ROLES {
        return Err(ApiError::validation("角色数量过多"));
    }
    Ok(normalized)
}

/// 校验通过后的账号入参。
struct ValidatedAccount {
    username: String,
    password: String,
    display_name: String,
    roles: Vec<String>,
}

/// 把存储错误映射成接口错误。
fn store_error(err: StoreError) -> ApiError {
    match err {
        StoreError::UsernameTaken => ApiError::UsernameTaken,
        StoreError::NotFound => ApiError::not_found("目标账号不存在"),
        // 存储层错误信息里不含口令，可以直接落到服务端错误里。
        StoreError::Database(message) => {
            tracing::error!(error = %message, "账号存储操作失败");
            ApiError::Server("账号服务暂不可用".to_owned())
        }
    }
}
