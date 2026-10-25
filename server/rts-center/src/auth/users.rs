//! 账号表。
//!
//! 本次不做用户持久化（intent 约束），账号全部来自**环境变量注入**，
//! 仅用于打通"取码 → 登录 → 拿令牌"的闭环。
//!
//! 口令**不明文存储**：进程启动时用 argon2id 对注入的明文口令做一次哈希，
//! 之后内存里只保留 PHC 字符串，校验时用 argon2 verify。
//! 仓库里**不存放任何口令或密钥字面量**，凭据一律由部署方通过环境变量提供。
//!
//! 账号配置格式（环境变量 `RTS_AUTH_USERS`）：
//!
//! ```text
//! <用户名>:<口令>:<显示名>[:<角色1>,<角色2>][;<下一个账号>...]
//! ```
//!
//! 规则：
//! - 多个账号用 `;` 分隔；单个账号用 `:` 分隔"用户名 / 口令 / 显示名 / 角色列表"；
//! - 角色列表可省略（该账号无角色），多个角色用 `,` 分隔；
//! - 口令**不得**包含 `:` 与 `;`（会被当作字段/条目分隔符）；
//! - 任一条目格式非法 → 启动时报错退出，不做静默跳过。

use std::collections::HashSet;
use std::fmt;

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};

/// 用户公开信息（会随登录响应返回）。
#[derive(Debug, Clone)]
pub struct User {
    /// 用户 ID。
    pub id: String,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
}

/// 一个账号的原始配置（仅在启动期存在，含明文口令）。
///
/// 哈希完成后即被丢弃，运行期内存里不留明文。
#[derive(Debug, Clone)]
pub struct AccountSpec {
    /// 登录名。
    pub username: String,
    /// 明文口令（启动期使用）。
    pub password: String,
    /// 显示名。
    pub display_name: String,
    /// 角色列表。
    pub roles: Vec<String>,
}

/// 账号配置解析错误。
#[derive(Debug)]
pub struct AccountSpecError {
    /// 出错的条目序号（从 1 开始）。
    pub index: usize,
    /// 出错原因。
    ///
    /// **不得包含口令**，否则错误信息与日志会泄露凭据。
    pub reason: String,
}

impl fmt::Display for AccountSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "第 {} 个账号条目非法: {}", self.index, self.reason)
    }
}

impl std::error::Error for AccountSpecError {}

/// 解析账号配置串。
///
/// 失败即返回错误，由调用方在启动阶段报错退出；不做静默跳过。
pub fn parse_accounts(spec: &str) -> Result<Vec<AccountSpec>, AccountSpecError> {
    let mut accounts: Vec<AccountSpec> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (offset, raw) in spec.split(';').enumerate() {
        let index = offset + 1;
        let entry = raw.trim();
        if entry.is_empty() {
            return Err(AccountSpecError {
                index,
                reason: "条目为空，请检查 ';' 分隔符".to_owned(),
            });
        }

        let fields: Vec<&str> = entry.split(':').collect();
        if fields.len() < 3 || fields.len() > 4 {
            return Err(AccountSpecError {
                index,
                reason: format!(
                    "字段数应为 3 或 4（用户名:口令:显示名[:角色列表]），实际 {} 个（口令不得包含 ':' 或 ';'）",
                    fields.len()
                ),
            });
        }

        let username = fields[0].trim();
        // 口令按字面量取用，不做 trim：口令里的首尾空格是有效字符。
        let password = fields[1];
        let display_name = fields[2].trim();

        if username.is_empty() {
            return Err(AccountSpecError {
                index,
                reason: "用户名不能为空".to_owned(),
            });
        }
        if password.is_empty() {
            return Err(AccountSpecError {
                index,
                reason: format!("账号 {username} 的口令不能为空"),
            });
        }
        if display_name.is_empty() {
            return Err(AccountSpecError {
                index,
                reason: format!("账号 {username} 的显示名不能为空"),
            });
        }
        if !seen.insert(username.to_owned()) {
            return Err(AccountSpecError {
                index,
                reason: format!("用户名 {username} 重复"),
            });
        }

        // 角色列表可省略；空角色元素直接忽略。
        let roles: Vec<String> = fields
            .get(3)
            .map(|raw_roles| {
                raw_roles
                    .split(',')
                    .map(str::trim)
                    .filter(|role| !role.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        accounts.push(AccountSpec {
            username: username.to_owned(),
            password: password.to_owned(),
            display_name: display_name.to_owned(),
            roles,
        });
    }

    Ok(accounts)
}

/// 把账号配置转换成运行期账号记录（口令哈希后丢弃明文）。
///
/// 哈希运算有成本（argon2 默认参数约几十毫秒），因此在启动阶段一次性完成，
/// 而不是每次登录时再算。
pub fn build_records(accounts: Vec<AccountSpec>) -> Vec<UserRecord> {
    accounts
        .into_iter()
        .enumerate()
        .map(|(index, account)| {
            let password_hash =
                hash_password(&account.password).expect("口令哈希不应失败：argon2 参数为默认值");
            UserRecord {
                user: User {
                    id: format!("u-{}", 1001 + index),
                    username: account.username,
                    display_name: account.display_name,
                    roles: account.roles,
                },
                password_hash,
            }
        })
        .collect()
}

/// 一条账号记录：用户信息 + 口令哈希。
#[derive(Debug, Clone)]
pub struct UserRecord {
    /// 用户信息。
    pub user: User,
    /// PHC 格式的 argon2id 哈希。
    pub password_hash: String,
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

/// 按用户名查找账号记录。
pub fn find_user<'a>(users: &'a [UserRecord], username: &str) -> Option<&'a UserRecord> {
    users
        .iter()
        .find(|record| record.user.username.eq_ignore_ascii_case(username))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 运行时随机口令：测试里不出现任何口令字面量。
    fn random_password() -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let bytes: [u8; 24] = rng.random();
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn parse_accounts_reads_fields_and_roles() {
        let accounts = parse_accounts("alice:<口令>:显示名甲:role_a,role_b;bob:<口令>:显示名乙")
            .expect("应当解析成功");
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].username, "alice");
        assert_eq!(accounts[0].password, "<口令>");
        assert_eq!(accounts[0].display_name, "显示名甲");
        assert_eq!(
            accounts[0].roles,
            vec!["role_a".to_owned(), "role_b".to_owned()]
        );
        assert_eq!(accounts[1].username, "bob");
        assert!(accounts[1].roles.is_empty());
    }

    #[test]
    fn parse_accounts_rejects_wrong_field_count() {
        assert!(parse_accounts("alice:<口令>").is_err());
        assert!(parse_accounts("alice:<口令>:显示名:role:extra").is_err());
        assert!(parse_accounts("alice:<口令>:显示名;").is_err());
        assert!(parse_accounts("").is_err());
    }

    #[test]
    fn parse_accounts_rejects_empty_fields_and_duplicates() {
        assert!(parse_accounts(":<口令>:显示名").is_err());
        assert!(parse_accounts("alice::显示名").is_err());
        assert!(parse_accounts("alice:<口令>:").is_err());
        assert!(parse_accounts("alice:<口令>:显示名;alice:<口令>:显示名").is_err());
    }

    #[test]
    fn parse_accounts_error_never_contains_password() {
        let error = parse_accounts("alice:secret-value:显示名:role:extra").unwrap_err();
        let message = error.to_string();
        assert!(
            !message.contains("secret-value"),
            "错误信息泄露了口令: {message}"
        );
    }

    #[test]
    fn records_keep_only_password_hash() {
        let password = random_password();
        let accounts = parse_accounts(&format!("alice:{password}:显示名甲")).expect("应当解析成功");
        let records = build_records(accounts);
        assert_eq!(records.len(), 1);
        let alice = find_user(&records, "alice").expect("应有 alice 账号");
        assert!(alice.password_hash.starts_with("$argon2id$"));
        assert!(!alice.password_hash.contains(&password));
        assert_eq!(alice.user.display_name, "显示名甲");
    }

    #[test]
    fn verify_password_accepts_correct_and_rejects_wrong() {
        let password = random_password();
        let accounts = parse_accounts(&format!("alice:{password}:显示名甲")).expect("应当解析成功");
        let records = build_records(accounts);
        let alice = find_user(&records, "alice").expect("应有 alice 账号");
        assert!(verify_password(alice, &password));
        assert!(!verify_password(alice, &format!("{password}x")));
    }

    #[test]
    fn find_user_is_case_insensitive() {
        let password = random_password();
        let accounts = parse_accounts(&format!("alice:{password}:显示名甲")).expect("应当解析成功");
        let records = build_records(accounts);
        assert!(find_user(&records, "ALICE").is_some());
        assert!(find_user(&records, "nobody").is_none());
    }
}
