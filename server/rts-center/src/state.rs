//! 应用状态。
//!
//! 单实例、内存态的最小实现：验证码存储、登录失败计数、账号表都在进程内存里，
//! 多实例部署时验证码不共享（intent 已声明本次不引入外部存储）。
//!
//! 账号来自环境变量注入，启动时对口令做一次 argon2id 哈希，明文随即丢弃。

use argon2::Argon2;
use argon2::password_hash::{PasswordVerifier, phc::PasswordHash};
use tokio::sync::Mutex;

use crate::auth::guard::LoginGuard;
use crate::auth::users::{UserRecord, build_records};
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
    /// 账号表（口令只以哈希形式驻留）。
    pub users: Vec<UserRecord>,
    /// 诱饵口令哈希：账号不存在时也做一次等价的 argon2 运算，
    /// 抹平"账号不存在"与"口令错误"之间的耗时差异。
    dummy_hash: String,
}

impl AppState {
    /// 按配置初始化状态。
    ///
    /// 账号表来自配置里的注入值；口令在这里做一次 argon2id 哈希，明文随即被丢弃，
    /// 运行期内存里只保留 PHC 哈希串。
    pub fn new(config: Config) -> Self {
        let mut config = config;
        // `take` 把注入的账号配置（含明文口令）从配置里搬走，哈希后即随临时值一起释放。
        let users = build_records(std::mem::take(&mut config.auth_accounts));
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
            users,
            dummy_hash,
        }
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
