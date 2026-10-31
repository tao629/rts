//! `rts-user` —— 账号引导与救援工具。
//!
//! 存在的理由只有一个：**库里一个超管都没有时，账号管理接口调不了**
//! （管理接口本身要求 `super_admin` 令牌）。这是一个先有鸡还是先有蛋的问题，
//! 所以留了这个命令行入口。
//!
//! 它不是另一套实现：所有动作都走 [`rts_center::auth::users::AccountStore`]，
//! 与 `/api/v1/admin/users` 系列接口共用同一套账号逻辑。
//!
//! ```text
//! cargo run -p rts-center --bin rts-user -- add --username admin --display-name 系统管理员 --roles super_admin
//! cargo run -p rts-center --bin rts-user -- list
//! cargo run -p rts-center --bin rts-user -- disable --username someone
//! ```
//!
//! **口令只从标准输入读**，不接受命令行参数——命令行会进 shell 历史。

use std::io::Read;
use std::process::ExitCode;

use rts_center::auth::db::PgAccountStore;
use rts_center::auth::users::{AccountStatus, AccountStore, NewAccount, hash_password};
use rts_center::config::Config;
use sqlx::postgres::PgPoolOptions;

/// 口令最小长度。
const MIN_PASSWORD_LEN: usize = 8;
/// 口令最大长度。
const MAX_PASSWORD_LEN: usize = 128;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_usage();
        return ExitCode::from(2);
    };

    let options = match parse_options(&args[1..]) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("参数错误：{message}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("创建异步运行时失败：{err}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(command, options)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("错误：{message}");
            ExitCode::FAILURE
        }
    }
}

/// 执行具体命令。
async fn run(command: &str, options: Options) -> Result<(), String> {
    let config = Config::from_env().map_err(|err| err.to_string())?;
    let pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .connect(&config.database_url)
        .await
        .map_err(|err| format!("连接数据库失败：{err}"))?;
    let store = AccountStore::Db(PgAccountStore::new(pool));

    match command {
        "add" => add_account(&store, &options).await,
        "list" => list_accounts(&store).await,
        "disable" => set_status(&store, &options, AccountStatus::Disabled).await,
        "enable" => set_status(&store, &options, AccountStatus::Active).await,
        "reset-password" => reset_password(&store, &options).await,
        other => Err(format!("未知命令 {other}")),
    }
}

/// 新增账号。
async fn add_account(store: &AccountStore, options: &Options) -> Result<(), String> {
    let username = options.require("username")?;
    let display_name = options.require("display-name")?;
    let roles = options
        .get("roles")
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|role| !role.is_empty())
                .map(str::to_owned)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let password = read_password_from_stdin()?;
    let password_hash =
        hash_password(&password).map_err(|err| format!("计算口令哈希失败：{err}"))?;

    let record = store
        .create(NewAccount {
            username: username.clone(),
            password_hash,
            display_name: display_name.clone(),
            roles: roles.clone(),
        })
        .await
        .map_err(|err| err.to_string())?;

    println!(
        "已创建账号 {}（id={}，角色：{}）",
        record.user.username,
        record.user.id,
        if roles.is_empty() {
            "无".to_owned()
        } else {
            roles.join(", ")
        }
    );
    Ok(())
}

/// 列出账号。
async fn list_accounts(store: &AccountStore) -> Result<(), String> {
    let records = store.list().await.map_err(|err| err.to_string())?;
    if records.is_empty() {
        println!("账号表为空");
        return Ok(());
    }
    println!(
        "{:<6} {:<20} {:<16} {:<10} 角色",
        "ID", "用户名", "显示名", "状态"
    );
    for record in records {
        println!(
            "{:<6} {:<20} {:<16} {:<10} {}",
            record.user.id,
            record.user.username,
            record.user.display_name,
            record.status.as_str(),
            record.user.roles.join(",")
        );
    }
    Ok(())
}

/// 修改账号状态。
async fn set_status(
    store: &AccountStore,
    options: &Options,
    status: AccountStatus,
) -> Result<(), String> {
    let username = options.require("username")?;
    let record = store
        .set_status(&username, status)
        .await
        .map_err(|err| err.to_string())?;
    println!(
        "账号 {} 的状态已改为 {}",
        record.user.username,
        record.status.as_str()
    );
    Ok(())
}

/// 重置口令。
async fn reset_password(store: &AccountStore, options: &Options) -> Result<(), String> {
    let username = options.require("username")?;
    let password = read_password_from_stdin()?;
    let password_hash =
        hash_password(&password).map_err(|err| format!("计算口令哈希失败：{err}"))?;
    store
        .set_password(&username, &password_hash)
        .await
        .map_err(|err| err.to_string())?;
    println!("账号 {username} 的口令已重置");
    Ok(())
}

/// 从标准输入读取口令。
///
/// 只去掉结尾的换行（口令里的首尾空格是有效字符）。
fn read_password_from_stdin() -> Result<String, String> {
    eprintln!("请从标准输入提供口令（不会出现在命令行历史里）：");
    let mut raw = String::new();
    std::io::stdin()
        .read_to_string(&mut raw)
        .map_err(|err| format!("读取标准输入失败：{err}"))?;
    let password = raw.strip_suffix('\n').unwrap_or(&raw);
    let password = password.strip_suffix('\r').unwrap_or(password);

    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(format!("口令至少 {MIN_PASSWORD_LEN} 个字符"));
    }
    if password.chars().count() > MAX_PASSWORD_LEN {
        return Err(format!("口令不能超过 {MAX_PASSWORD_LEN} 个字符"));
    }
    Ok(password.to_owned())
}

/// 解析 `--key value` 形式的选项。
fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let Some(key) = arg.strip_prefix("--") else {
            return Err(format!("无法识别的参数 {arg}"));
        };
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("参数 --{key} 缺少取值"))?;
        options.insert(key.to_owned(), value.to_owned());
        index += 2;
    }
    Ok(options)
}

/// 解析后的选项。
#[derive(Debug, Default)]
struct Options {
    values: Vec<(String, String)>,
}

impl Options {
    /// 记录一个选项。
    fn insert(&mut self, key: String, value: String) {
        self.values.push((key, value));
    }

    /// 取选项值。
    fn get(&self, key: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    /// 取必填选项。
    fn require(&self, key: &str) -> Result<String, String> {
        self.get(key).ok_or_else(|| format!("缺少必填参数 --{key}"))
    }
}

/// 打印用法。
fn print_usage() {
    eprintln!(
        "\
用法：
  rts-user add --username <用户名> --display-name <显示名> [--roles <角色1,角色2>]
  rts-user list
  rts-user enable  --username <用户名>
  rts-user disable --username <用户名>
  rts-user reset-password --username <用户名>

口令从标准输入读取，例如：
  printf '%s\\n' '你的口令' | rts-user add --username admin --display-name 系统管理员 --roles super_admin

需要环境变量 DATABASE_URL 指向账号表所在库。"
    );
}
