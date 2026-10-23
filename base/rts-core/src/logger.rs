//! 日志模块。
//!
//! 基于 `tracing` 封装的文件日志能力：
//!
//! - **按天 + 按大小滚动**：跨自然日或单文件超过上限（默认 300 MiB）即滚动；
//! - **GZ 归档**：滚动下来的文件压缩成 gzip，放进 `archive/` 子目录，
//!   命名为 `app.log.2026-10-23.0.gz`——同一天内序号从 0 开始递增，跨天归零；
//! - **保留一周**：超过保留天数（默认 7 天）的归档自动清理；
//! - **兼容 `log` facade**：既有的 `log::info!` 等宏同样会写入日志文件。
//!
//! 用法：
//!
//! ```no_run
//! use rts_core::logger::{LevelFilter, LogConfig};
//!
//! // guard 必须持有到进程结束，否则异步写入的工作线程会退出。
//! let _guard = rts_core::logger::log::init(LogConfig::default().level(LevelFilter::INFO))?;
//! tracing::info!("任务开始");
//! # Ok::<(), rts_core::logger::log::LoggerError>(())
//! ```

pub mod config;
pub mod log;
pub mod writer;

pub use config::{LogConfig, ResolvedConfig};
pub use log::{LoggerError, LoggerGuard, init, level_from_log, parse_level};
pub use writer::RollingWriter;

/// 日志级别过滤类型（`tracing` 的 `LevelFilter`，含 `OFF`）。
pub use tracing::level_filters::LevelFilter;
