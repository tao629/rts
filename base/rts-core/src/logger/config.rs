//! 日志配置。
//!
//! [`LogConfig`] 的每个字段都是 `Option`：`Some` 表示调用方显式设置（优先级最高），
//! `None` 表示未设置，依次回退到环境变量、最后回退到内置默认值。
//!
//! | 环境变量 | 含义 | 默认值 |
//! |---------|------|-------|
//! | `RTS_LOG_DIR` | 日志根目录 | `logs`（相对当前工作目录） |
//! | `RTS_LOG_MAX_SIZE` | 单个活动文件的大小上限，支持 `300M` / `1G` 这类带单位写法 | `300M`（300 MiB） |
//! | `RTS_LOG_MAX_AGE_DAYS` | 归档文件保留天数 | `7` |
//! | `RTS_LOG_LEVEL` | 日志级别：`off` / `error` / `warn` / `info` / `debug` / `trace` | `info` |
//! | `RTS_LOG_CONSOLE` | 是否同时输出到标准错误 | `true` |
//!
//! 环境变量取值非法时回退到默认值并打印告警，不影响启动。

use std::path::PathBuf;

use tracing::level_filters::LevelFilter;

/// 日志根目录默认值（相对当前工作目录）。
pub const DEFAULT_DIR: &str = "logs";
/// 活动日志文件名默认值。
pub const DEFAULT_FILE_NAME: &str = "app.log";
/// 归档子目录名默认值。
pub const DEFAULT_ARCHIVE_DIR: &str = "archive";
/// 单个活动文件的大小上限默认值：300 MiB。
pub const DEFAULT_MAX_SIZE: u64 = 300 * 1024 * 1024;
/// 归档文件保留天数默认值：一周。
pub const DEFAULT_MAX_AGE_DAYS: u32 = 7;
/// 日志级别默认值。
pub const DEFAULT_LEVEL: LevelFilter = LevelFilter::INFO;
/// 是否默认同时输出到控制台。
pub const DEFAULT_CONSOLE: bool = true;

/// 环境变量名：日志根目录。
pub const ENV_DIR: &str = "RTS_LOG_DIR";
/// 环境变量名：单文件大小上限。
pub const ENV_MAX_SIZE: &str = "RTS_LOG_MAX_SIZE";
/// 环境变量名：归档保留天数。
pub const ENV_MAX_AGE_DAYS: &str = "RTS_LOG_MAX_AGE_DAYS";
/// 环境变量名：日志级别。
pub const ENV_LEVEL: &str = "RTS_LOG_LEVEL";
/// 环境变量名：是否输出到控制台。
pub const ENV_CONSOLE: &str = "RTS_LOG_CONSOLE";

/// 日志配置。
///
/// 所有字段默认 `None`（即"未显式设置"），可直接 [`LogConfig::default`] 后按需覆盖。
#[derive(Debug, Clone, Default)]
pub struct LogConfig {
    /// 日志根目录。
    pub dir: Option<PathBuf>,
    /// 活动日志文件名。
    pub file_name: Option<String>,
    /// 归档子目录名（相对 [`LogConfig::dir`]）。
    pub archive_dir: Option<String>,
    /// 单个活动文件的大小上限（字节）。
    pub max_size: Option<u64>,
    /// 归档文件保留天数。
    pub max_age_days: Option<u32>,
    /// 日志级别。
    pub level: Option<LevelFilter>,
    /// 是否同时输出到控制台（标准错误）。
    pub console: Option<bool>,
}

impl LogConfig {
    /// 设置日志根目录。
    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// 设置活动日志文件名。
    pub fn file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    /// 设置归档子目录名。
    pub fn archive_dir(mut self, archive_dir: impl Into<String>) -> Self {
        self.archive_dir = Some(archive_dir.into());
        self
    }

    /// 设置单个活动文件的大小上限（字节）。
    pub fn max_size(mut self, max_size: u64) -> Self {
        self.max_size = Some(max_size);
        self
    }

    /// 设置归档文件保留天数。
    pub fn max_age_days(mut self, max_age_days: u32) -> Self {
        self.max_age_days = Some(max_age_days);
        self
    }

    /// 设置日志级别。
    pub fn level(mut self, level: LevelFilter) -> Self {
        self.level = Some(level);
        self
    }

    /// 设置是否同时输出到控制台。
    pub fn console(mut self, console: bool) -> Self {
        self.console = Some(console);
        self
    }

    /// 按 **显式配置 > 环境变量 > 默认值** 的优先级解析出最终配置。
    pub fn resolve(self) -> ResolvedConfig {
        let dir = self
            .dir
            .or_else(|| env_path(ENV_DIR))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DIR));

        let file_name = self
            .file_name
            .unwrap_or_else(|| DEFAULT_FILE_NAME.to_owned());

        let archive_dir = self
            .archive_dir
            .unwrap_or_else(|| DEFAULT_ARCHIVE_DIR.to_owned());

        let max_size = match self.max_size {
            Some(value) => value,
            None => env_max_size().unwrap_or(DEFAULT_MAX_SIZE),
        };

        let max_age_days = match self.max_age_days {
            Some(value) => value,
            None => env_max_age_days().unwrap_or(DEFAULT_MAX_AGE_DAYS),
        };

        let level = match self.level {
            Some(value) => value,
            None => env_level().unwrap_or(DEFAULT_LEVEL),
        };

        let console = match self.console {
            Some(value) => value,
            None => env_console().unwrap_or(DEFAULT_CONSOLE),
        };

        ResolvedConfig {
            dir,
            file_name,
            archive_dir,
            max_size,
            max_age_days,
            level,
            console,
        }
    }
}

/// 解析后的最终配置，字段全部为具体值。
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    /// 日志根目录。
    pub dir: PathBuf,
    /// 活动日志文件名。
    pub file_name: String,
    /// 归档子目录名。
    pub archive_dir: String,
    /// 单个活动文件的大小上限（字节）。
    pub max_size: u64,
    /// 归档文件保留天数。
    pub max_age_days: u32,
    /// 日志级别。
    pub level: LevelFilter,
    /// 是否同时输出到控制台。
    pub console: bool,
}

impl ResolvedConfig {
    /// 归档目录的完整路径。
    pub fn archive_path(&self) -> PathBuf {
        self.dir.join(&self.archive_dir)
    }

    /// 活动日志文件的完整路径。
    pub fn file_path(&self) -> PathBuf {
        self.dir.join(&self.file_name)
    }
}

/// 解析日志级别名称。
///
/// 取值不区分大小写：`off` / `error` / `warn` / `info` / `debug` / `trace`。
pub fn parse_level(raw: &str) -> Option<LevelFilter> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::OFF),
        "error" => Some(LevelFilter::ERROR),
        "warn" => Some(LevelFilter::WARN),
        "info" => Some(LevelFilter::INFO),
        "debug" => Some(LevelFilter::DEBUG),
        "trace" => Some(LevelFilter::TRACE),
        _ => None,
    }
}

/// 解析文件大小，支持带单位的写法。
///
/// - 无单位：字节，如 `314572800`；
/// - `K` / `KB` / `M` / `MB` / `G` / `GB`：按 1024 进制换算，如 `300M` = 300 MiB。
pub fn parse_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let upper = raw.to_ascii_uppercase();
    let (number, multiplier) = if let Some(rest) = upper.strip_suffix("GB") {
        (rest, 1024u64 * 1024 * 1024)
    } else if let Some(rest) = upper.strip_suffix("MB") {
        (rest, 1024 * 1024)
    } else if let Some(rest) = upper.strip_suffix("KB") {
        (rest, 1024)
    } else if let Some(rest) = upper.strip_suffix('G') {
        (rest, 1024 * 1024 * 1024)
    } else if let Some(rest) = upper.strip_suffix('M') {
        (rest, 1024 * 1024)
    } else if let Some(rest) = upper.strip_suffix('K') {
        (rest, 1024)
    } else {
        (upper.as_str(), 1)
    };

    let value: u64 = number.trim().parse().ok()?;
    value.checked_mul(multiplier)
}

fn env_path(key: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(key)?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

fn env_max_size() -> Option<u64> {
    let raw = std::env::var(ENV_MAX_SIZE).ok()?;
    match parse_size(&raw) {
        Some(value) => Some(value),
        None => {
            eprintln!(
                "[rts-log] 环境变量 {ENV_MAX_SIZE} 的取值 `{raw}` 无法解析，回退默认值 {DEFAULT_MAX_SIZE}"
            );
            None
        }
    }
}

fn env_max_age_days() -> Option<u32> {
    let raw = std::env::var(ENV_MAX_AGE_DAYS).ok()?;
    match raw.trim().parse::<u32>() {
        Ok(value) => Some(value),
        Err(_) => {
            eprintln!(
                "[rts-log] 环境变量 {ENV_MAX_AGE_DAYS} 的取值 `{raw}` 无法解析，回退默认值 {DEFAULT_MAX_AGE_DAYS}"
            );
            None
        }
    }
}

fn env_level() -> Option<LevelFilter> {
    let raw = std::env::var(ENV_LEVEL).ok()?;
    match parse_level(&raw) {
        Some(value) => Some(value),
        None => {
            eprintln!(
                "[rts-log] 环境变量 {ENV_LEVEL} 的取值 `{raw}` 无法解析，回退默认值 {:?}",
                DEFAULT_LEVEL
            );
            None
        }
    }
}

fn env_console() -> Option<bool> {
    let raw = std::env::var(ENV_CONSOLE).ok()?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => {
            eprintln!(
                "[rts-log] 环境变量 {ENV_CONSOLE} 的取值 `{raw}` 无法解析，回退默认值 {DEFAULT_CONSOLE}"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_level_ignores_case_and_padding() {
        assert_eq!(parse_level("INFO"), Some(LevelFilter::INFO));
        assert_eq!(parse_level("  Warn "), Some(LevelFilter::WARN));
        assert_eq!(parse_level("off"), Some(LevelFilter::OFF));
        assert_eq!(parse_level("TRACE"), Some(LevelFilter::TRACE));
    }

    #[test]
    fn parse_level_rejects_unknown_value() {
        assert_eq!(parse_level("verbose"), None);
        assert_eq!(parse_level(""), None);
    }

    #[test]
    fn parse_size_supports_plain_bytes_and_units() {
        assert_eq!(parse_size("1024"), Some(1024));
        assert_eq!(parse_size("300M"), Some(300 * 1024 * 1024));
        assert_eq!(parse_size("300MB"), Some(300 * 1024 * 1024));
        assert_eq!(parse_size("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size(" 2k "), Some(2048));
    }

    #[test]
    fn parse_size_rejects_invalid_value() {
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size("12X"), None);
    }

    #[test]
    fn default_config_resolves_to_builtin_defaults() {
        let resolved = LogConfig::default().resolve();
        assert_eq!(resolved.dir, PathBuf::from(DEFAULT_DIR));
        assert_eq!(resolved.file_name, DEFAULT_FILE_NAME);
        assert_eq!(resolved.archive_dir, DEFAULT_ARCHIVE_DIR);
        assert_eq!(resolved.max_size, 300 * 1024 * 1024);
        assert_eq!(resolved.max_age_days, 7);
        assert_eq!(resolved.level, LevelFilter::INFO);
        assert!(resolved.console);
    }

    #[test]
    fn explicit_config_wins_over_defaults() {
        let resolved = LogConfig::default()
            .dir("/var/log/rts")
            .file_name("svc.log")
            .archive_dir("old")
            .max_size(1024)
            .max_age_days(3)
            .level(LevelFilter::ERROR)
            .console(false)
            .resolve();

        assert_eq!(resolved.dir, PathBuf::from("/var/log/rts"));
        assert_eq!(resolved.file_name, "svc.log");
        assert_eq!(resolved.archive_dir, "old");
        assert_eq!(resolved.max_size, 1024);
        assert_eq!(resolved.max_age_days, 3);
        assert_eq!(resolved.level, LevelFilter::ERROR);
        assert!(!resolved.console);
    }
}
