//! 全局日志初始化。
//!
//! 基于 `tracing` 实现：日志既写入按天/按大小滚动的文件，也（可选）输出到
//! 标准错误。文件滚动、GZ 归档、过期清理的细节见 [`super::writer`]。
//!
//! 同时安装了 `log` facade 的兼容桥接，因此既有的 `log::info!` 等宏调用
//! 会写入同一份日志文件。
//!
//! ```no_run
//! use rts_core::logger::{LevelFilter, LogConfig};
//!
//! let _guard = rts_core::logger::log::init(LogConfig::default().level(LevelFilter::INFO))?;
//! tracing::info!("hello");
//! # Ok::<(), rts_core::logger::log::LoggerError>(())
//! ```

use std::error::Error;
use std::fmt;
use std::io;

use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

use super::config::LogConfig;
use super::writer::RollingWriter;

pub use super::config::parse_level;

/// 异步写入的缓冲行数上限。
///
/// 缓冲满时丢弃最旧的待写日志（`lossy`），保证业务线程不被日志写入阻塞。
const BUFFERED_LINES_LIMIT: usize = 1024;

/// 日志初始化失败的原因。
#[derive(Debug)]
pub enum LoggerError {
    /// 日志目录或文件操作失败。
    Io(io::Error),
    /// 全局 subscriber 已被注册（`tracing` 的全局 subscriber 只能设置一次）。
    GlobalSubscriber(String),
    /// `log` facade 桥接初始化失败。
    LogTracer(String),
}

impl fmt::Display for LoggerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "日志文件操作失败: {err}"),
            Self::GlobalSubscriber(err) => write!(f, "全局日志 subscriber 注册失败: {err}"),
            Self::LogTracer(err) => write!(f, "log facade 桥接初始化失败: {err}"),
        }
    }
}

impl Error for LoggerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::GlobalSubscriber(_) | Self::LogTracer(_) => None,
        }
    }
}

/// 日志工作线程的守卫。
///
/// 异步写入依赖后台工作线程；guard 被释放时工作线程会 flush 缓冲并退出，
/// 之后的日志将不再落盘。因此调用方必须持有它（例如 `let _guard = init(..)?;`）。
#[derive(Debug)]
pub struct LoggerGuard {
    _worker: WorkerGuard,
    _writer: NonBlocking,
}

/// 初始化全局日志。
///
/// - 日志目录与归档目录不存在时自动创建；
/// - 启动时清理超过保留期的归档；
/// - 级别由 [`LogConfig`] 决定，未显式设置时读 `RTS_LOG_LEVEL` 环境变量，
///   再回退到 `info`；
/// - 保留 `log` facade 兼容桥接，`log::info!` 等宏同样会落盘。
///
/// 失败时返回 [`LoggerError`]，不 panic。
pub fn init(config: LogConfig) -> Result<LoggerGuard, LoggerError> {
    let resolved = config.resolve();

    let writer = RollingWriter::new(&resolved).map_err(LoggerError::Io)?;

    let (non_blocking, worker) = NonBlockingBuilder::default()
        .lossy(true)
        .buffered_lines_limit(BUFFERED_LINES_LIMIT)
        .finish(writer);

    // 文件层：纯文本（不带 ANSI 颜色），保留 target / 文件名 / 行号 / 线程号。
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking.clone())
        .with_timer(local_timer())
        .with_ansi(false);

    // 控制台层：输出到标准错误，带 ANSI 颜色。
    let console_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_timer(local_timer())
        .with_ansi(true);

    let subscriber = Registry::default()
        .with(resolved.level)
        .with(file_layer)
        .with(if resolved.console {
            Some(console_layer)
        } else {
            None
        });

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|err| LoggerError::GlobalSubscriber(err.to_string()))?;

    // 安装 log facade 桥接：既有的 log::info! 等宏写入同一份日志。
    tracing_log::LogTracer::builder()
        .with_max_level(level_to_log(resolved.level))
        .init()
        .map_err(|err| LoggerError::LogTracer(err.to_string()))?;

    Ok(LoggerGuard {
        _worker: worker,
        _writer: non_blocking,
    })
}

/// 本地时区的时间格式化器。
///
/// 归档文件名用本地日期，这里也用本地时间，两者才对得上；默认的 UTC 时间戳
/// 在跨时区排查时容易和归档日期错位。
fn local_timer() -> tracing_subscriber::fmt::time::ChronoLocal {
    tracing_subscriber::fmt::time::ChronoLocal::new("%Y-%m-%dT%H:%M:%S%.6f%:z".to_owned())
}

/// `tracing` 的级别过滤换算成 `log` 的级别过滤。
fn level_to_log(level: LevelFilter) -> log::LevelFilter {
    match level {
        LevelFilter::TRACE => log::LevelFilter::Trace,
        LevelFilter::DEBUG => log::LevelFilter::Debug,
        LevelFilter::INFO => log::LevelFilter::Info,
        LevelFilter::WARN => log::LevelFilter::Warn,
        LevelFilter::ERROR => log::LevelFilter::Error,
        LevelFilter::OFF => log::LevelFilter::Off,
    }
}

/// `log` 的级别过滤换算成 `tracing` 的级别过滤。
///
/// 供仍在使用 `log::LevelFilter` 的调用方迁移使用。
pub fn level_from_log(level: log::LevelFilter) -> LevelFilter {
    match level {
        log::LevelFilter::Off => LevelFilter::OFF,
        log::LevelFilter::Error => LevelFilter::ERROR,
        log::LevelFilter::Warn => LevelFilter::WARN,
        log::LevelFilter::Info => LevelFilter::INFO,
        log::LevelFilter::Debug => LevelFilter::DEBUG,
        log::LevelFilter::Trace => LevelFilter::TRACE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_conversion_round_trips() {
        for level in [
            LevelFilter::OFF,
            LevelFilter::ERROR,
            LevelFilter::WARN,
            LevelFilter::INFO,
            LevelFilter::DEBUG,
            LevelFilter::TRACE,
        ] {
            assert_eq!(level_from_log(level_to_log(level)), level);
        }
    }

    #[test]
    fn level_to_log_maps_off() {
        assert_eq!(level_to_log(LevelFilter::OFF), log::LevelFilter::Off);
        assert_eq!(level_to_log(LevelFilter::WARN), log::LevelFilter::Warn);
    }
}
