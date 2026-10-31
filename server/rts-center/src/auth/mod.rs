//! 认证接口：取验证码、登录、登出、确认登录态。
//!
//! 登录的校验顺序是本模块的核心（spec §1）：
//! **验证码 → 口令 → 失败锁定**。
//!
//! 之所以把验证码放在最前面，有两个原因：
//! 1. 验证码是与"人"绑定的挑战因子，脚本拿不到就无从自动化；
//! 2. 验证码先校验可以保证"账号不存在"与"口令错误"的对外表现完全一致，
//!    不会因为先查账号再判验证码而泄露账号是否存在。

pub mod bearer;
pub mod db;
pub mod guard;
pub mod token;
pub mod users;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::header::{CACHE_CONTROL, HeaderMap};
use axum::http::request::Parts;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::captcha;
use crate::error::ApiError;
use crate::state::AppState;

/// 用户名最大长度。
const MAX_USERNAME_LEN: usize = 64;
/// 口令最大长度。
const MAX_PASSWORD_LEN: usize = 128;

/// 获取验证码的响应体。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptchaResponse {
    /// 验证码唯一标识，登录时原样回传。
    pub captcha_id: String,
    /// `data:image/svg+xml;base64,` 前缀的 SVG 图片。
    pub image: String,
    /// 过期时刻（ISO 8601 UTC）。
    pub expires_at: String,
}

/// 登录请求体。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    /// 登录名。
    pub username: String,
    /// 口令。
    pub password: String,
    /// 是否"记住我"：决定令牌有效期（24 小时 / 2 小时）。
    #[serde(default)]
    pub remember: bool,
    /// 验证码 ID。
    ///
    /// 缺省为空串而不是解析失败：验证码缺失要报 `INVALID_CAPTCHA`（spec §3.5），
    /// 不能退化成"请求体非法"。
    #[serde(default, alias = "captcha_id")]
    pub captcha_id: String,
    /// 验证码内容。
    #[serde(default, alias = "captcha_code")]
    pub captcha_code: String,
}

/// 登录响应中的用户信息。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    /// 用户 ID。
    pub id: String,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
}

impl From<&users::User> for UserInfo {
    fn from(user: &users::User) -> Self {
        Self {
            id: user.id.clone(),
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            roles: user.roles.clone(),
        }
    }
}

/// 登录成功响应体。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    /// JWT 令牌。
    pub token: String,
    /// 令牌类型，固定 `Bearer`。
    pub token_type: String,
    /// 令牌过期时刻（ISO 8601 UTC）。
    pub expires_at: String,
    /// 用户信息。
    pub user: UserInfo,
}

/// 客户端来源 IP 提取器。
///
/// 优先取反向代理写入的 `X-Forwarded-For` / `X-Real-IP`，
/// 其次取 TCP 连接的对端地址（由 `ConnectInfo` 注入），都没有时退化为 `unknown`。
///
/// 注意：转发头在没有"可信代理白名单"的前提下是可以伪造的，
/// 因此它只能作为锁定维度的辅助信息，不能当作安全边界。
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ip = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(|value| value.trim().to_owned())
            .or_else(|| {
                parts
                    .headers
                    .get("x-real-ip")
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.trim().to_owned())
            })
            .or_else(|| {
                parts
                    .extensions
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|info| info.0.ip().to_string())
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown".to_owned());
        Ok(ClientIp(ip))
    }
}

/// `GET /api/v1/auth/captcha`：生成验证码。
///
/// 响应体只含 `captchaId` / `image` / `expiresAt`——**答案绝不回传**。
pub async fn captcha_handler(
    State(state): State<Arc<AppState>>,
) -> Result<(HeaderMap, Json<CaptchaResponse>), ApiError> {
    let generated = captcha::generate();
    let expires_at = Utc::now() + Duration::seconds(state.config.captcha_ttl_secs as i64);
    let captcha_id = {
        let mut store = state.captcha.lock().await;
        store.issue(&generated.answer)
    };

    // 只记录 ID 与过期时刻，不记录答案。
    tracing::info!(
        captcha_id = %captcha_id,
        expires_at = %expires_at,
        "已生成图形验证码"
    );

    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, "no-store".parse().expect("常量头值合法"));

    Ok((
        headers,
        Json(CaptchaResponse {
            captcha_id,
            image: generated.image,
            expires_at: token::format_iso8601(expires_at),
        }),
    ))
}

/// `POST /api/v1/auth/login`：校验验证码 → 校验口令 → 签发令牌。
pub async fn login_handler(
    State(state): State<Arc<AppState>>,
    client_ip: ClientIp,
    payload: Result<Json<LoginRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<LoginResponse>, ApiError> {
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            // 不回显请求体内容，避免口令进入错误信息与日志。
            tracing::warn!(error = %rejection, "登录请求体解析失败");
            return Err(ApiError::validation("请求体格式非法"));
        }
    };

    let username = request.username.trim();
    let password = request.password.as_str();
    let captcha_id = request.captcha_id.trim();
    let captcha_code = request.captcha_code.trim();

    // ① 验证码优先：缺失即拒绝，且不透露账号是否存在。
    if captcha_id.is_empty() || captcha_code.is_empty() {
        return Err(ApiError::invalid_captcha("验证码缺失，请重新获取验证码"));
    }

    // ② 账号口令字段的合法性检查（长度、是否为空）。
    if username.is_empty() {
        return Err(ApiError::validation("用户名不能为空"));
    }
    if username.chars().count() > MAX_USERNAME_LEN {
        return Err(ApiError::validation("用户名过长"));
    }
    if password.is_empty() {
        return Err(ApiError::validation("密码不能为空"));
    }
    if password.chars().count() > MAX_PASSWORD_LEN {
        return Err(ApiError::validation("密码过长"));
    }

    // ③ 消费验证码（一次性）：无论后续比对是否通过，该验证码都已作废。
    let expected = {
        let mut store = state.captcha.lock().await;
        store.consume(captcha_id)
    };
    let Some(expected) = expected else {
        return Err(ApiError::invalid_captcha("验证码错误或已失效，请重新输入"));
    };
    if !expected.eq_ignore_ascii_case(captcha_code) {
        return Err(ApiError::invalid_captcha("验证码错误或已失效，请重新输入"));
    }

    let lock_key = guard::lock_key(username, &client_ip.0);

    // ④ 锁定检查：锁定期内一律拒绝，即便口令正确。
    {
        let mut login_guard = state.guard.lock().await;
        if login_guard.status(&lock_key) == guard::LockStatus::Locked {
            tracing::warn!(username = %username, ip = %client_ip.0, "账号处于锁定期，拒绝登录");
            return Err(ApiError::AccountLocked);
        }
    }

    // ⑤ 取账号。数据库出错时返回 500 而不是"账号不存在"：
    //    把服务端故障说成认证失败，会让运维分不清是库挂了还是口令错了。
    let record = match state.find_account(username).await {
        Ok(record) => record,
        Err(error) => {
            tracing::error!(username = %username, error = %error, "读取账号失败");
            return Err(ApiError::Server("账号服务暂不可用".to_owned()));
        }
    };

    // ⑥ 口令校验。账号不存在时也走一次等价的 argon2 运算，
    //    让"账号不存在"与"口令错误"的耗时一致，避免被用来枚举账号。
    let authenticated = match record {
        Some(ref record) => users::verify_password(record, password),
        None => state.verify_dummy_password(password),
    };

    if !authenticated {
        let mut login_guard = state.guard.lock().await;
        let status = login_guard.record_failure(&lock_key);
        // 达到阈值时账号被上锁，但本次尝试仍然按"口令错误"返回；
        // 从下一次尝试开始才会命中 423 ACCOUNT_LOCKED（spec A7）。
        if status == guard::LockStatus::Locked {
            tracing::warn!(
                username = %username,
                ip = %client_ip.0,
                "登录失败次数达到阈值，账号已临时锁定"
            );
        } else {
            tracing::warn!(username = %username, ip = %client_ip.0, "登录失败");
        }
        return Err(ApiError::InvalidCredentials);
    }

    let record = record.expect("口令校验通过时账号必然存在");

    // ⑦ 停用账号：口令正确才提示"已停用"，避免不掌握口令的人
    //    靠这个错误码枚举出账号的存在与状态。
    if record.status == users::AccountStatus::Disabled {
        tracing::warn!(username = %username, ip = %client_ip.0, "账号已停用，拒绝登录");
        return Err(ApiError::AccountDisabled);
    }

    // 登录成功清零失败计数。
    {
        let mut login_guard = state.guard.lock().await;
        login_guard.reset(&lock_key);
    }

    // ⑧ 签发令牌。
    let ttl = if request.remember {
        Duration::hours(24)
    } else {
        Duration::hours(2)
    };
    let (token_value, expires_at) = token::issue(&state.config.jwt_secret, &record.user, ttl)
        .map_err(|err| {
            tracing::error!(error = %err, "签发令牌失败");
            ApiError::Server("令牌签发失败".to_owned())
        })?;

    tracing::info!(
        username = %username,
        remember = request.remember,
        expires_at = %token::format_iso8601(expires_at),
        "登录成功"
    );

    Ok(Json(LoginResponse {
        token: token_value,
        token_type: "Bearer".to_owned(),
        expires_at: token::format_iso8601(expires_at),
        user: UserInfo::from(&record.user),
    }))
}

/// `POST /api/v1/auth/logout`：登出。
///
/// 服务端不维护令牌黑名单（本次是单实例、内存态的最小实现），
/// 客户端本地清理凭证即可；接口恒定返回 204，失败也不影响前端登出。
pub async fn logout_handler() -> axum::http::StatusCode {
    axum::http::StatusCode::NO_CONTENT
}

/// `GET /api/v1/auth/me`：用令牌换回当前用户信息。
///
/// 前端在刷新页面时用它确认"登录态还有效"——登录态由服务端裁决，
/// 而不是浏览器自己说了算。响应体与登录响应的 `user` 同构，
/// 前端可以直接用它刷新本地会话里的用户信息。
pub async fn me_handler(
    State(state): State<Arc<AppState>>,
    bearer: bearer::BearerToken,
) -> Result<Json<UserInfo>, ApiError> {
    let claims = bearer::authenticate(&state.config.jwt_secret, &bearer.0)?;

    // 回库确认账号仍然存在且启用：停用即时生效，已签发的令牌也随之作废。
    let record = match state.find_account(&claims.username).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            tracing::warn!(username = %claims.username, "令牌对应的账号已不存在");
            return Err(ApiError::token_invalid("令牌无效或已过期"));
        }
        Err(error) => {
            tracing::error!(username = %claims.username, error = %error, "读取账号失败");
            return Err(ApiError::Server("账号服务暂不可用".to_owned()));
        }
    };

    if record.status == users::AccountStatus::Disabled {
        tracing::warn!(username = %claims.username, "账号已停用，令牌失效");
        return Err(ApiError::AccountDisabled);
    }

    Ok(Json(UserInfo::from(&record.user)))
}
