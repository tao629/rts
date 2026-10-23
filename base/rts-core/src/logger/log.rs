//! 全局日志初始化。
//!
//! 基于 `ftail` 实现，级别可通过环境变量 `RTS_LOG_LEVEL` 覆盖
//! （取值：`off` / `error` / `warn` / `info` / `debug` / `trace`）。

use ftail::Ftail;
use log::LevelFilter;

/// 环境变量名：用于覆盖日志级别。
pub const LEVEL_ENV: &str = "RTS_LOG_LEVEL";

/// 初始化全局日志器。
///
/// 级别取 `RTS_LOG_LEVEL` 环境变量的值，未设置或取值非法时使用 `default_level`。
pub fn init(default_level: LevelFilter) -> Result<(), ftail::error::FtailError> {
    Ftail::new().console(level_from_env(default_level)).init()
}

/// 解析日志级别名称。
pub fn parse_level(raw: &str) -> Option<LevelFilter> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

/// 从环境变量读取日志级别。
pub fn level_from_env(default_level: LevelFilter) -> LevelFilter {
    std::env::var(LEVEL_ENV)
        .ok()
        .and_then(|raw| parse_level(&raw))
        .unwrap_or(default_level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_level_ignores_case_and_padding() {
        assert_eq!(parse_level("INFO"), Some(LevelFilter::Info));
        assert_eq!(parse_level("  Warn "), Some(LevelFilter::Warn));
        assert_eq!(parse_level("off"), Some(LevelFilter::Off));
    }

    #[test]
    fn parse_level_rejects_unknown_value() {
        assert_eq!(parse_level("verbose"), None);
        assert_eq!(parse_level(""), None);
    }
}
