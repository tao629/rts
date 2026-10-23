//! 日志模块演示：写入若干条日志，观察按天/按大小滚动与 GZ 归档。
//!
//! ```text
//! cargo run -p rts-core --example log_demo
//! RTS_LOG_DIR=/tmp/rts-log-demo RTS_LOG_MAX_SIZE=10K cargo run -p rts-core --example log_demo
//! ```

use rts_core::logger::LogConfig;

fn main() {
    let dir = std::env::var("RTS_LOG_DIR").unwrap_or_else(|_| "logs".to_owned());

    // guard 必须持有到进程结束，否则异步写入的工作线程会退出。
    let guard = match rts_core::logger::log::init(LogConfig::default().dir(&dir)) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("初始化日志失败: {err}");
            std::process::exit(1);
        }
    };

    for index in 0..500 {
        tracing::info!("这是第 {index} 条日志，用于演示滚动与归档");
    }
    // 演示 log facade 兼容：同样的宏写法也能落盘。
    log::info!("这条日志走 log facade");

    // 释放 guard 会 flush 缓冲并停止后台工作线程。
    drop(guard);

    println!("日志目录: {dir}");
    if let Ok(entries) = std::fs::read_dir(std::path::Path::new(&dir).join("archive")) {
        for entry in entries.flatten() {
            println!("归档: {}", entry.path().display());
        }
    }
}
