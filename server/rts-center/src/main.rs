//! rts-center 进程入口。
//!
//! 启动顺序：初始化日志 → 读配置 → 组装路由 → 监听端口 → 等待退出信号。
//!
//! 日志先于配置初始化：配置加载过程本身会产生告警（例如"未配置账号"、
//! "未配置签名密钥"），这些告警要能落进日志文件。

use std::net::SocketAddr;
use std::sync::Arc;

use rts_center::config::Config;
use rts_center::routes::build_router;
use rts_center::state::AppState;
use rts_core::logger::LogConfig;
use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 日志由平台公共包 rts-core 统一落盘（按天/按大小滚动）。
    // guard 必须持有到进程结束，否则异步写入的工作线程会提前退出。
    let _log_guard = match rts_core::logger::init(LogConfig::default()) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("初始化日志失败: {err}");
            std::process::exit(1);
        }
    };

    // 配置非法（监听地址、账号格式）时直接退出，不带着半截配置启动。
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => {
            tracing::error!(error = %err, "加载配置失败");
            eprintln!("加载配置失败: {err}");
            std::process::exit(1);
        }
    };

    tracing::info!(
        addr = %config.addr,
        captcha_ttl_secs = config.captcha_ttl_secs,
        login_max_failures = config.login_max_failures,
        login_lock_secs = config.login_lock_secs,
        account_count = config.auth_accounts.len(),
        ephemeral_jwt_secret = config.jwt_secret_ephemeral,
        "rts-center 启动中"
    );

    let addr: SocketAddr = config.addr;
    let app = build_router(Arc::new(AppState::new(config)));

    let listener = TcpListener::bind(addr).await?;
    tracing::info!("rts-center 已监听 {addr}");

    // 注入对端地址，供登录失败锁定按「用户名 + 来源 IP」维度计数。
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    tracing::info!("rts-center 已退出");
    Ok(())
}

/// 等待退出信号（Ctrl+C / SIGTERM）。
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("注册 Ctrl+C 处理器失败");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("注册 SIGTERM 处理器失败")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("收到退出信号，开始优雅停机");
}
