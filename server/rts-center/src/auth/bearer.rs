//! Bearer 令牌提取与校验。
//!
//! 需要登录态的接口（`/auth/me`、账号管理接口）统一从这里取令牌：
//! `Authorization: Bearer <token>`。提取失败一律返回 401 `TOKEN_INVALID`，
//! 不区分"没带令牌"和"令牌不合法"，避免给攻击者反馈。

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use super::token::{Claims, verify};
use crate::error::ApiError;

/// 从请求头里提取出来的 Bearer 令牌。
#[derive(Debug, Clone)]
pub struct BearerToken(pub String);

impl<S> FromRequestParts<S> for BearerToken
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| ApiError::token_invalid("缺少访问令牌"))?;

        // 方案名大小写不敏感（RFC 7235），但只接受 Bearer 这一种。
        let token = header
            .split_once(' ')
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, token)| token.trim())
            .filter(|token| !token.is_empty())
            .ok_or_else(|| ApiError::token_invalid("访问令牌格式非法"))?;

        Ok(Self(token.to_owned()))
    }
}

/// 校验令牌并返回载荷。
///
/// 令牌无效（格式、签名、过期）一律映射为 401 `TOKEN_INVALID`。
pub fn authenticate(secret: &str, token: &str) -> Result<Claims, ApiError> {
    verify(secret, token).map_err(|err| {
        // 只记录失败原因，不记录令牌本身。
        tracing::warn!(error = %err, "令牌校验失败");
        ApiError::token_invalid("令牌无效或已过期")
    })
}

/// 校验令牌并要求具备 `super_admin` 角色。
pub fn authenticate_admin(secret: &str, token: &str) -> Result<Claims, ApiError> {
    let claims = authenticate(secret, token)?;
    if !claims.roles.iter().any(|role| role == "super_admin") {
        tracing::warn!(username = %claims.username, "非超管访问管理接口");
        return Err(ApiError::forbidden("没有权限执行该操作"));
    }
    Ok(claims)
}
