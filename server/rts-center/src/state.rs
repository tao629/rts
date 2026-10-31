//! 应用状态。
//!
//! 验证码存储、登录失败计数在单进程内存里（多实例部署时"取码"与"登录"必须落在同一实例）；
//! **账号表在数据库里**，通过 [`AccountStore`] 访问，多实例共享同一份账号。

use argon2::Argon2;
use argon2::password_hash::{PasswordVerifier, phc::PasswordHash};
use tokio::sync::Mutex;

use crate::auth::guard::LoginGuard;
use crate::auth::users::{AccountStore, UserRecord};
use crate::captcha::store::CaptchaStore;
use crate::config::Config;

/// 全局应用状态。
#[derive(Debug)]
pub struct AppState {
    /// 服务配置。
    pub config: Config,
    /// 验证码存储。
    pub captcha: Mutex<CaptchaStore>,
    /// 登录失败计数与锁定。
    pub guard: Mutex<LoginGuard>,
    /// 账号来源（生产查库，测试走内存）。
    pub store: AccountStore,
    /// 诱饵口令哈希：账号不存在时也做一次等价的 argon2 运算，
    /// 抹平"账号不存在"与"口令错误"之间的耗时差异。
    dummy_hash: String,
}

impl AppState {
    /// 按配置与账号来源初始化状态。
    pub fn new(config: Config, store: AccountStore) -> Self {
        // 诱饵口令用随机值，永远不会匹配成功。
        let dummy_hash =
            crate::auth::users::hash_password(&random_dummy_secret()).unwrap_or_default();

        let captcha = CaptchaStore::new(
            std::time::Duration::from_secs(config.captcha_ttl_secs),
            crate::captcha::store::DEFAULT_CAPACITY,
        );
        let guard = LoginGuard::new(
            config.login_max_failures,
            std::time::Duration::from_secs(config.login_lock_secs),
        );

        Self {
            config,
            captcha: Mutex::new(captcha),
            guard: Mutex::new(guard),
            store,
            dummy_hash,
        }
    }

    /// 按登录名查账号。
    ///
    /// 数据库出错时**不**降级成"账号不存在"——那会把服务端故障说成认证失败，
    /// 让运维分不清是库挂了还是口令错了。
    pub async fn find_account(&self, username: &str) -> Result<Option<UserRecord>, String> {
        self.store
            .find_by_username(username)
            .await
            .map_err(|err| err.to_string())
    }

    /// 用诱饵哈希校验口令：结果恒为 `false`，但耗时与真实校验相当。
    pub fn verify_dummy_password(&self, password: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(&self.dummy_hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }
}

/// 生成一个随机的诱饵口令。
fn random_dummy_secret() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: [u8; 16] = rng.random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
