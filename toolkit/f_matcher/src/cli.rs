//! 命令行参数解析与规则文件路径解析。

use std::fmt;
use std::path::{Path, PathBuf};

/// 规则文件默认文件名。
pub const RULE_FILE_NAME: &str = "sensitive_rule.txt";

/// 解析后的命令行参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    /// 待扫描目录。
    pub dir: PathBuf,
    /// 关键字规则文件路径（未显式指定时为 `None`，由调用方按默认顺序查找）。
    pub rule: Option<PathBuf>,
}

/// 参数解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliOutcome {
    /// 正常执行扫描。
    Run(Cli),
    /// 打印帮助后退出。
    Help,
    /// 打印版本后退出。
    Version,
}

/// 参数错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// 缺少必填的目录参数。
    MissingDirectory,
    /// 给了多个目录参数。
    UnexpectedDirectory(String),
    /// 未知选项。
    UnknownOption(String),
    /// `--rule` 缺少取值。
    MissingRuleValue,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::MissingDirectory => write!(f, "缺少必填参数：待扫描目录"),
            CliError::UnexpectedDirectory(value) => {
                write!(f, "只支持一个待扫描目录，多余参数：{value}")
            }
            CliError::UnknownOption(option) => write!(f, "未知选项：{option}"),
            CliError::MissingRuleValue => write!(f, "选项 --rule 需要一个规则文件路径"),
        }
    }
}

impl std::error::Error for CliError {}

/// 解析命令行参数（不含程序名）。
pub fn parse_args(args: &[String]) -> Result<CliOutcome, CliError> {
    let mut dir: Option<PathBuf> = None;
    let mut rule: Option<PathBuf> = None;
    let mut index = 0;

    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--help" | "-h" => return Ok(CliOutcome::Help),
            "--version" | "-V" => return Ok(CliOutcome::Version),
            "--rule" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or(CliError::MissingRuleValue)?;
                rule = Some(PathBuf::from(value));
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--rule=") {
                    if value.is_empty() {
                        return Err(CliError::MissingRuleValue);
                    }
                    rule = Some(PathBuf::from(value));
                } else if arg.starts_with('-') && arg.len() > 1 {
                    return Err(CliError::UnknownOption(arg.to_string()));
                } else if dir.is_none() {
                    dir = Some(PathBuf::from(arg));
                } else {
                    return Err(CliError::UnexpectedDirectory(arg.to_string()));
                }
            }
        }
        index += 1;
    }

    match dir {
        Some(dir) => Ok(CliOutcome::Run(Cli { dir, rule })),
        None => Err(CliError::MissingDirectory),
    }
}

/// 查找默认规则文件：先找可执行文件同级目录，再找源码树内的同名文件。
pub fn default_rule_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(RULE_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = manifest_dir.join(RULE_FILE_NAME);
    if candidate.is_file() {
        return Some(candidate);
    }

    None
}

/// 用法说明。
pub fn usage() -> String {
    format!(
        "\
f_matcher {version} —— 批量扫描目录下命中敏感关键字的文件

用法：
  f_matcher <dir> [--rule <rule-file>]
  f_matcher --help
  f_matcher --version

参数：
  <dir>            待扫描目录（必填）。递归遍历其下所有层级。
  --rule <file>    关键字规则文件，默认为 {rule_name}
                   （先找可执行文件同级目录，再找源码树目录）。

扫描规则：
  * 只处理扩展名为 .txt / .csv 的文件（大小写不敏感）。
  * 关键字与排除词来自规则文件，不在代码中硬编码。
  * 排除语义为“严格紧邻”：关键字后紧接排除词（路/医院/宾馆/学院/
    家属区/宿舍/干休所/疗养院）时该次匹配不算命中。
  * 文件编码按 UTF-8 优先识别，失败回退 GB18030（兼容 GBK/GB2312）。
  * 某个文件命中 1 个关键字即终止该文件的继续匹配，每文件最多一条结果。

输出：
  标准输出每行一条：<文件绝对路径>:<命中行号>
  只输出路径与行号，不回显命中行内容。

退出码：
  0  扫描完成，无命中
  1  参数 / 目录 / 规则文件错误
  2  扫描完成但存在文件读取失败（覆盖不完整）
  3  扫描完成，至少命中 1 个文件
",
        version = env!("CARGO_PKG_VERSION"),
        rule_name = RULE_FILE_NAME,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn parses_directory() {
        let outcome = parse_args(&args(&["/data/export"])).expect("应解析成功");
        assert_eq!(
            outcome,
            CliOutcome::Run(Cli {
                dir: PathBuf::from("/data/export"),
                rule: None,
            })
        );
    }

    #[test]
    fn parses_rule_option_with_space() {
        let outcome = parse_args(&args(&["/data", "--rule", "/etc/rule.txt"])).expect("应解析成功");
        assert_eq!(
            outcome,
            CliOutcome::Run(Cli {
                dir: PathBuf::from("/data"),
                rule: Some(PathBuf::from("/etc/rule.txt")),
            })
        );
    }

    #[test]
    fn parses_rule_option_with_equals() {
        let outcome = parse_args(&args(&["/data", "--rule=/etc/rule.txt"])).expect("应解析成功");
        assert_eq!(
            outcome,
            CliOutcome::Run(Cli {
                dir: PathBuf::from("/data"),
                rule: Some(PathBuf::from("/etc/rule.txt")),
            })
        );
    }

    #[test]
    fn parses_rule_before_directory() {
        let outcome = parse_args(&args(&["--rule", "/etc/rule.txt", "/data"])).expect("应解析成功");
        assert_eq!(
            outcome,
            CliOutcome::Run(Cli {
                dir: PathBuf::from("/data"),
                rule: Some(PathBuf::from("/etc/rule.txt")),
            })
        );
    }

    #[test]
    fn recognizes_help_and_version() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), CliOutcome::Help);
        assert_eq!(parse_args(&args(&["-h"])).unwrap(), CliOutcome::Help);
        assert_eq!(
            parse_args(&args(&["--version"])).unwrap(),
            CliOutcome::Version
        );
        assert_eq!(parse_args(&args(&["-V"])).unwrap(), CliOutcome::Version);
    }

    #[test]
    fn rejects_missing_directory() {
        assert_eq!(parse_args(&args(&[])).unwrap_err(), CliError::MissingDirectory);
    }

    #[test]
    fn rejects_extra_directory() {
        let err = parse_args(&args(&["/a", "/b"])).unwrap_err();
        assert_eq!(err, CliError::UnexpectedDirectory("/b".to_string()));
    }

    #[test]
    fn rejects_unknown_option() {
        let err = parse_args(&args(&["--wat", "/a"])).unwrap_err();
        assert_eq!(err, CliError::UnknownOption("--wat".to_string()));
    }

    #[test]
    fn rejects_rule_without_value() {
        let err = parse_args(&args(&["/a", "--rule"])).unwrap_err();
        assert_eq!(err, CliError::MissingRuleValue);
        assert_eq!(
            parse_args(&args(&["/a", "--rule="])).unwrap_err(),
            CliError::MissingRuleValue
        );
    }
}
