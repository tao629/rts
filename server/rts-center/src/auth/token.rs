//! JWT 令牌签发与校验。
//!
//! 算法固定 HS256（对称），密钥来自 `RTS_JWT_SECRET`。
//! 载荷里带上用户名、显示名与角色，前端后续调接口时可以直接取用，
//! 也便于服务端做粗粒度的权限判断。

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use super::users::User;

/// 令牌载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// 主题：用户 ID。
    pub sub: String,
    /// 过期时间（Unix 秒）。
    pub exp: u64,
    /// 签发时间（Unix 秒）。
    pub iat: u64,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
}

/// 令牌相关错误。
#[derive(Debug)]
pub enum TokenError {
    /// 签发失败。
    Encode(jsonwebtoken::errors::Error),
    /// 解析或校验失败。
    Decode(jsonwebtoken::errors::Error),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(err) => write!(f, "签发令牌失败: {err}"),
            Self::Decode(err) => write!(f, "解析令牌失败: {err}"),
        }
    }
}

impl std::error::Error for TokenError {}

/// 签发令牌，返回 `(令牌字符串, 过期时刻)`。
pub fn issue(
    secret: &str,
    user: &User,
    ttl: Duration,
) -> Result<(String, DateTime<Utc>), TokenError> {
    let issued_at = Utc::now();
    let expires_at = issued_at + ttl;
    let claims = Claims {
        sub: user.id.clone(),
        exp: expires_at.timestamp().max(0) as u64,
        iat: issued_at.timestamp().max(0) as u64,
        username: user.username.clone(),
        display_name: user.display_name.clone(),
        roles: user.roles.clone(),
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(TokenError::Encode)?;
    Ok((token, expires_at))
}

/// 校验令牌并返回载荷。
pub fn verify(secret: &str, token: &str) -> Result<Claims, TokenError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    // jsonwebtoken 默认允许 60 秒的时钟偏移，会让"刚过期"的令牌仍然可用；
    // 令牌有效期是安全边界，这里不留宽限。
    validation.leeway = 0;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(TokenError::Decode)
}

/// 把时间格式化为 ISO 8601 UTC（形如 `2026-10-25T02:15:30Z`）。
pub fn format_iso8601(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::users::{AccountSpec, build_records};

    /// 运行时随机密钥：测试里不出现任何固定密钥字面量。
    fn random_secret() -> String {
        crate::config::random_secret()
    }

    /// 构造一个测试账号（口令运行时随机生成）。
    fn test_user() -> crate::auth::users::User {
        let accounts = vec![AccountSpec {
            username: "admin".to_owned(),
            password: crate::config::random_secret_with_len(24),
            display_name: "系统管理员".to_owned(),
            roles: vec!["super_admin".to_owned()],
        }];
        build_records(accounts).remove(0).user
    }

    #[test]
    fn issue_and_verify_round_trip() {
        let admin = test_user();
        let secret = random_secret();
        let (token, expires_at) = issue(&secret, &admin, Duration::hours(2)).expect("应当签发成功");
        let claims = verify(&secret, &token).expect("应当校验通过");
        assert_eq!(claims.username, "admin");
        assert_eq!(claims.roles, vec!["super_admin".to_owned()]);
        assert_eq!(claims.exp, expires_at.timestamp().max(0) as u64);
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let admin = test_user();
        let secret = random_secret();
        let (token, _) = issue(&secret, &admin, Duration::hours(2)).unwrap();
        assert!(verify(&random_secret(), &token).is_err());
    }

    #[test]
    fn expired_token_is_rejected() {
        let admin = test_user();
        let secret = random_secret();
        let (token, _) = issue(&secret, &admin, Duration::seconds(-10)).unwrap();
        assert!(verify(&secret, &token).is_err());
    }

    #[test]
    fn iso8601_format() {
        let time = DateTime::from_timestamp(1_700_000_000, 0).expect("合法时间戳");
        assert_eq!(format_iso8601(time), "2023-11-14T22:13:20Z");
    }
}
