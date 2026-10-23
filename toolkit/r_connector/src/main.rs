//! r_connector：按配置文件连接远端主机并依次执行其中的指令。
//!
//! 用法：`r_connector <config-file>`

mod cli;
mod config;
mod hostkey;
mod report;
mod session;

use log::{LevelFilter, info};

use crate::hostkey::{HostKeyStore, Status};

/// 正常完成。
const EXIT_OK: i32 = 0;
/// 参数或配置文件错误。
const EXIT_USAGE: i32 = 1;

#[tokio::main]
async fn main() {
    let code = run().await;
    std::process::exit(code);
}

async fn run() -> i32 {
    if let Err(e) = rts_core::logger::log::init(LevelFilter::Warn) {
        eprintln!("警告: 初始化日志失败: {e}");
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = match cli::parse(&args) {
        Ok(action) => action,
        Err(message) => {
            eprintln!("错误: {message}\n");
            eprint!("{}", cli::usage());
            return EXIT_USAGE;
        }
    };

    let config_path = match action {
        cli::Action::Help => {
            print!("{}", cli::usage());
            return EXIT_OK;
        }
        cli::Action::Version => {
            println!("{}", cli::version());
            return EXIT_OK;
        }
        cli::Action::Run(path) => path,
    };

    let content = match std::fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("错误: 读取配置文件 {} 失败: {e}", config_path.display());
            return EXIT_USAGE;
        }
    };

    let config = match config::parse(&content) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("错误: 解析配置文件 {} 失败: {e}", config_path.display());
            return EXIT_USAGE;
        }
    };

    let password = match config.password.clone() {
        Some(password) => password,
        None => {
            let prompt = format!("请输入 {}@{} 的密码: ", config.user, config.host);
            match rpassword::prompt_password(prompt) {
                Ok(password) => password,
                Err(e) => {
                    eprintln!("错误: 读取密码失败: {e}");
                    return EXIT_USAGE;
                }
            }
        }
    };

    let store = HostKeyStore::default_path();
    info!("主机密钥信任文件: {}", store.path().display());

    let (mut session, status) = match session::connect(&config, &password, store).await {
        Ok(connected) => connected,
        Err(e) => {
            eprintln!("错误: {}", e.message);
            return e.exit_code();
        }
    };

    match status {
        Status::Trusted => info!("主机密钥校验通过"),
        Status::FirstTime { fingerprint } => {
            println!("首次连接该主机，已信任并记录主机密钥 {fingerprint}");
        }
    }

    let total = config.commands.len();
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for (index, command) in config.commands.iter().enumerate() {
        report::header(index + 1, total, command);

        let mut out = std::io::stdout();
        let mut err = std::io::stderr();

        match session.exec(command, &mut out, &mut err).await {
            Ok(outcome) if outcome.timed_out => {
                report::footer_timeout();
                failed += 1;
            }
            Ok(outcome) => {
                report::footer_exit(outcome.exit_code);
                if outcome.exit_code == Some(0) {
                    succeeded += 1;
                } else {
                    failed += 1;
                }
            }
            Err(e) => {
                report::footer_error(&e.message);
                failed += 1;
                if e.is_fatal() {
                    eprintln!("错误: 连接已不可用，停止后续指令: {}", e.message);
                    break;
                }
            }
        }
    }

    report::summary(total, succeeded, failed);
    session.disconnect().await;

    EXIT_OK
}
