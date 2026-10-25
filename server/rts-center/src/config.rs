//! 服务端配置。
//!
//! 全部配置项来自环境变量，取值非法时**回退到默认值并打印告警**（而不是直接退出），
//! 避免因为一个笔误的环境变量让整个服务起不来；只有监听地址、账号配置这类
//! "错了就没法正常工作"的配置才会以错误结束启动。
//!
//! | 环境变量 | 含义 | 默认值 |
//! |---------|------|-------|
//! | `RTS_CENTER_ADDR` | 监听地址 | `127.0.0.1:8080` |
//! | `RTS_JWT_SECRET` | 令牌签名密钥 | 未配置时**随机生成**（令牌在进程重启后失效） |
//! | `RTS_CAPTCHA_TTL_SECS` | 验证码有效期（秒） | `300` |
//! | `RTS_LOGIN_MAX_FAILURES` | 触发锁定的连续失败次数 | `5` |
//! | `RTS_LOGIN_LOCK_SECS` | 锁定时长（秒） | `60` |
//! | `RTS_AUTH_USERS` | 账号表（`用户名:口令:显示名[:角色]` 多条用 `;` 分隔） | 未配置则账号表为空 |
//!
//! **仓库里不存放任何凭据**：口令与签名密钥都由部署方通过环境变量注入。

use std::env;
use std::fmt;
use std::net::SocketAddr;

use crate::auth::users::{AccountSpec, parse_accounts};

/// 监听地址环境变量名。
pub const ENV_ADDR: &str = "RTS_CENTER_ADDR";
/// 令牌签名密钥环境变量名。
pub const ENV_JWT_SECRET: &str = "RTS_JWT_SECRET";
/// 验证码有效期环境变量名。
pub const ENV_CAPTCHA_TTL_SECS: &str = "RTS_CAPTCHA_TTL_SECS";
/// 登录失败锁定阈值环境变量名。
pub const ENV_LOGIN_MAX_FAILURES: &str = "RTS_LOGIN_MAX_FAILURES";
/// 登录失败锁定时长环境变量名。
pub const ENV_LOGIN_LOCK_SECS: &str = "RTS_LOGIN_LOCK_SECS";
/// 账号表环境变量名。
pub const ENV_AUTH_USERS: &str = "RTS_AUTH_USERS";

/// 监听地址默认值：只监听回环地址，服务默认不对外暴露。
pub const DEFAULT_ADDR: &str = "127.0.0.1:8080";
/// 验证码有效期默认值：5 分钟。
pub const DEFAULT_CAPTCHA_TTL_SECS: u64 = 300;
/// 登录失败锁定阈值默认值。
pub const DEFAULT_LOGIN_MAX_FAILURES: u32 = 5;
/// 登录失败锁定时长默认值：60 秒。
pub const DEFAULT_LOGIN_LOCK_SECS: u64 = 60;
/// 随机签名密钥的字节数。
pub const RANDOM_SECRET_BYTES: usize = 32;

/// 服务端配置。
#[derive(Debug, Clone)]
pub struct Config {
    /// HTTP 监听地址。
    pub addr: SocketAddr,
    /// JWT 签名密钥（HS256）。
    pub jwt_secret: String,
    /// 签名密钥是否为进程内随机生成的临时密钥。
    ///
    /// 为 `true` 时令牌在进程重启后即失效。
    pub jwt_secret_ephemeral: bool,
    /// 验证码有效期（秒）。
    pub captcha_ttl_secs: u64,
    /// 触发锁定的连续失败次数。
    pub login_max_failures: u32,
    /// 锁定时长（秒）。
    pub login_lock_secs: u64,
    /// 账号表（口令为明文，仅启动期存在）。
    pub auth_accounts: Vec<AccountSpec>,
}

/// 配置装载失败。
#[derive(Debug)]
pub struct ConfigError {
    /// 出错的配置项名称。
    pub key: &'static str,
    /// 出错原因。
    pub reason: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "配置项 {} 无效: {}", self.key, self.reason)
    }
}

impl std::error::Error for ConfigError {}

impl Default for Config {
    fn default() -> Self {
        // 默认配置同样不携带任何固定凭据：签名密钥随机生成，账号表为空。
        Self {
            addr: DEFAULT_ADDR.parse().expect("默认监听地址必须是合法的"),
            jwt_secret: random_secret(),
            jwt_secret_ephemeral: true,
            captcha_ttl_secs: DEFAULT_CAPTCHA_TTL_SECS,
            login_max_failures: DEFAULT_LOGIN_MAX_FAILURES,
            login_lock_secs: DEFAULT_LOGIN_LOCK_SECS,
            auth_accounts: Vec::new(),
        }
    }
}

impl Config {
    /// 从环境变量装载配置。
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut config = Config::default();

        if let Ok(raw) = env::var(ENV_ADDR) {
            let raw = raw.trim();
            if !raw.is_empty() {
                config.addr = raw.parse().map_err(|err| ConfigError {
                    key: ENV_ADDR,
                    reason: format!("无法解析为地址（{err}）"),
                })?;
            }
        }

        let (secret, ephemeral) = resolve_jwt_secret(env::var(ENV_JWT_SECRET).ok().as_deref());
        config.jwt_secret = secret;
        config.jwt_secret_ephemeral = ephemeral;
        if ephemeral {
            tracing::warn!("未配置 {ENV_JWT_SECRET}，本次使用随机密钥，令牌在进程重启后失效");
        }

        config.captcha_ttl_secs = read_u64(ENV_CAPTCHA_TTL_SECS, config.captcha_ttl_secs);
        config.login_max_failures =
            read_u64(ENV_LOGIN_MAX_FAILURES, u64::from(config.login_max_failures)) as u32;
        config.login_lock_secs = read_u64(ENV_LOGIN_LOCK_SECS, config.login_lock_secs);

        config.auth_accounts = load_accounts()?;

        Ok(config)
    }
}

/// 解析 JWT 签名密钥。
///
/// 环境变量存在且非空时直接采用；否则随机生成一个 32 字节密钥。
/// 返回 `(密钥, 是否为临时随机密钥)`。
pub fn resolve_jwt_secret(env_value: Option<&str>) -> (String, bool) {
    match env_value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(secret) => (secret.to_owned(), false),
        None => (random_secret(), true),
    }
}

/// 装载账号表。
///
/// 未配置或配置为空串时返回空账号表（服务正常启动，但所有登录都会失败）。
fn load_accounts() -> Result<Vec<AccountSpec>, ConfigError> {
    let raw = match env::var(ENV_AUTH_USERS) {
        Ok(raw) => raw,
        Err(_) => {
            tracing::warn!("未配置账号，所有登录都会失败：环境变量 {ENV_AUTH_USERS} 未设置");
            return Ok(Vec::new());
        }
    };
    if raw.trim().is_empty() {
        tracing::warn!("未配置账号，所有登录都会失败：环境变量 {ENV_AUTH_USERS} 为空");
        return Ok(Vec::new());
    }

    parse_accounts(&raw).map_err(|err| ConfigError {
        key: ENV_AUTH_USERS,
        reason: err.to_string(),
    })
}

/// 生成一个随机的十六进制密钥（默认 32 字节）。
pub fn random_secret() -> String {
    random_secret_with_len(RANDOM_SECRET_BYTES)
}

/// 生成指定字节数的随机十六进制密钥。
pub fn random_secret_with_len(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..len).map(|_| rng.random::<u8>()).collect();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 读取一个正整数型环境变量，非法时回退默认值并告警。
fn read_u64(key: &str, fallback: u64) -> u64 {
    let Ok(raw) = env::var(key) else {
        return fallback;
    };
    match raw.trim().parse::<u64>() {
        Ok(value) if value > 0 => value,
        Ok(_) => {
            tracing::warn!(config = key, value = %raw, "配置项必须为正整数，回退到默认值");
            fallback
        }
        Err(_) => {
            tracing::warn!(config = key, value = %raw, "配置项无法解析为整数，回退到默认值");
            fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_spec() {
        let config = Config::default();
        assert_eq!(config.addr.to_string(), DEFAULT_ADDR);
        assert_eq!(config.captcha_ttl_secs, 300);
        assert_eq!(config.login_max_failures, 5);
        assert_eq!(config.login_lock_secs, 60);
        assert!(config.auth_accounts.is_empty());
    }

    #[test]
    fn random_secret_is_hex_and_unpredictable() {
        let first = random_secret();
        let second = random_secret();
        assert_eq!(first.len(), RANDOM_SECRET_BYTES * 2);
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first, second, "随机密钥不应重复");
    }

    #[test]
    fn resolve_jwt_secret_prefers_env_value() {
        let (secret, ephemeral) = resolve_jwt_secret(Some("from-env"));
        assert_eq!(secret, "from-env");
        assert!(!ephemeral);
    }

    #[test]
    fn resolve_jwt_secret_falls_back_to_random() {
        let (secret, ephemeral) = resolve_jwt_secret(None);
        assert!(ephemeral);
        assert_eq!(secret.len(), RANDOM_SECRET_BYTES * 2);
        assert!(secret.chars().all(|ch| ch.is_ascii_hexdigit()));
        // 空串等同于未配置。
        let (secret, ephemeral) = resolve_jwt_secret(Some("   "));
        assert!(ephemeral);
        assert_eq!(secret.len(), RANDOM_SECRET_BYTES * 2);
    }
}
