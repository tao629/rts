//! 统一错误类型与响应体。
//!
//! 契约见 `sdlc/20261025_登录验证码/spec.md` §3.5：所有失败响应统一为
//! `{ "code": "<错误码>", "message": "<可读文案>" }`，错误码与 HTTP 状态一一对应。
//!
//! 错误文案刻意保持"中性"：验证码相关失败与口令相关失败使用不同文案，
//! 但都不透露"账号是否存在"，避免登录接口被用来枚举账号。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// 错误响应体。
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// 机器可读错误码。
    pub code: &'static str,
    /// 人类可读文案。
    pub message: String,
}

/// 业务错误。
#[derive(Debug, Clone)]
pub enum ApiError {
    /// 参数校验失败（必填缺失、超长、请求体非法）。
    Validation(String),
    /// 验证码缺失 / 不匹配 / 过期 / 已使用。
    InvalidCaptcha(String),
    /// 用户名或口令错误。
    InvalidCredentials,
    /// 令牌缺失 / 非法 / 过期 / 验签失败。
    TokenInvalid(String),
    /// 账号已停用。
    AccountDisabled,
    /// 已登录但没有权限。
    Forbidden(String),
    /// 目标账号不存在。
    NotFound(String),
    /// 用户名已存在。
    UsernameTaken,
    /// 账号处于锁定期。
    AccountLocked,
    /// 触发限流。
    RateLimited,
    /// 服务端内部异常。
    Server(String),
}

impl ApiError {
    /// 错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::InvalidCaptcha(_) => "INVALID_CAPTCHA",
            Self::InvalidCredentials => "INVALID_CREDENTIALS",
            Self::TokenInvalid(_) => "TOKEN_INVALID",
            Self::AccountDisabled => "ACCOUNT_DISABLED",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound(_) => "NOT_FOUND",
            Self::UsernameTaken => "USERNAME_TAKEN",
            Self::AccountLocked => "ACCOUNT_LOCKED",
            Self::RateLimited => "RATE_LIMITED",
            Self::Server(_) => "SERVER_ERROR",
        }
    }

    /// 对应的 HTTP 状态码。
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Validation(_) | Self::InvalidCaptcha(_) => StatusCode::BAD_REQUEST,
            Self::InvalidCredentials | Self::TokenInvalid(_) => StatusCode::UNAUTHORIZED,
            Self::AccountDisabled | Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::UsernameTaken => StatusCode::CONFLICT,
            Self::AccountLocked => StatusCode::LOCKED,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Server(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 可读文案。
    pub fn message(&self) -> &str {
        match self {
            Self::Validation(message)
            | Self::InvalidCaptcha(message)
            | Self::TokenInvalid(message)
            | Self::Forbidden(message)
            | Self::NotFound(message)
            | Self::Server(message) => message,
            Self::InvalidCredentials => "用户名或密码错误",
            Self::AccountDisabled => "账号已停用，请联系管理员",
            Self::UsernameTaken => "用户名已存在",
            Self::AccountLocked => "登录失败次数过多，账号已临时锁定，请稍后再试",
            Self::RateLimited => "请求过于频繁，请稍后再试",
        }
    }

    /// 参数校验失败的便捷构造。
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// 验证码失败的便捷构造。
    pub fn invalid_captcha(message: impl Into<String>) -> Self {
        Self::InvalidCaptcha(message.into())
    }

    /// 令牌无效的便捷构造。
    pub fn token_invalid(message: impl Into<String>) -> Self {
        Self::TokenInvalid(message.into())
    }

    /// 权限不足的便捷构造。
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    /// 目标不存在的便捷构造。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Json(ErrorBody {
            code: self.code(),
            message: self.message().to_owned(),
        });
        (status, body).into_response()
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        // 只记录解析位置，不回显请求体内容，避免把口令写进日志。
        tracing::error!(error = %err, "解析请求体失败");
        Self::validation("请求体格式非法")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_matches_spec() {
        assert_eq!(ApiError::validation("x").status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            ApiError::invalid_captcha("x").status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ApiError::InvalidCredentials.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(ApiError::AccountLocked.status(), StatusCode::LOCKED);
        assert_eq!(
            ApiError::RateLimited.status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            ApiError::Server("x".into()).status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
