//! 配置文件解析。
//!
//! 配置文件按行处理：
//!
//! ```text
//! sftp                       <- 第 1 行：协议
//! 192.168.1.10:2222          <- 第 2 行：连接信息 host:port
//! root,Passw0rd             <- 第 3 行：账号密码 user,password
//! uname -a                   <- 第 4 行起：待执行指令
//! ```
//!
//! 解析容错：空行与 `#` 注释行忽略；头部三行允许 `协议:` / `连接:` / `账号:` 前缀。

use std::fmt;

/// 连接协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// SFTP（本次与 SSH 行为一致，指令走 SSH exec 通道）
    Sftp,
    /// SSH
    Ssh,
}

impl Protocol {
    /// 解析协议名，大小写不敏感。
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "sftp" => Ok(Protocol::Sftp),
            "ssh" => Ok(Protocol::Ssh),
            other => Err(ConfigError::UnsupportedProtocol(other.to_string())),
        }
    }

    /// 协议名的规范写法。
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Sftp => "sftp",
            Protocol::Ssh => "ssh",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 解析后的配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 协议。
    pub protocol: Protocol,
    /// 远端主机地址。
    pub host: String,
    /// 远端端口。
    pub port: u16,
    /// 登录用户名。
    pub user: String,
    /// 登录密码；`None` 表示配置中未提供密码，需要交互式补输。
    pub password: Option<String>,
    /// 待执行的指令，按配置顺序排列。
    pub commands: Vec<String>,
}

/// 配置解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// 头部行缺失。
    MissingHeader {
        /// 缺失的是第几个头部行（1=协议，2=连接信息，3=账号密码）。
        index: usize,
    },
    /// 协议取值不支持。
    UnsupportedProtocol(String),
    /// 连接信息格式错误。
    InvalidConnection(String),
    /// 主机地址为空。
    EmptyHost,
    /// 端口非法。
    InvalidPort(String),
    /// 账号密码行格式错误。
    InvalidCredential(String),
    /// 用户名为空。
    EmptyUser,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingHeader { index } => {
                let what = match index {
                    1 => "协议",
                    2 => "连接信息",
                    _ => "账号密码",
                };
                write!(f, "配置文件缺少第 {index} 行（{what}）")
            }
            ConfigError::UnsupportedProtocol(v) => {
                write!(f, "不支持的协议 \"{v}\"，仅支持 sftp / ssh")
            }
            ConfigError::InvalidConnection(v) => {
                write!(f, "连接信息格式错误: \"{v}\"，应为 host:port")
            }
            ConfigError::EmptyHost => write!(f, "主机地址为空"),
            ConfigError::InvalidPort(v) => {
                write!(f, "端口非法: \"{v}\"，应为 1-65535 的整数")
            }
            ConfigError::InvalidCredential(v) => {
                write!(f, "账号密码格式错误: \"{v}\"，应为 user,password")
            }
            ConfigError::EmptyUser => write!(f, "用户名为空"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// 允许出现在头部行的可选标签。
const HEADER_LABELS: [&str; 3] = ["协议", "连接", "账号"];

/// 解析配置文件内容。
pub fn parse(content: &str) -> Result<Config, ConfigError> {
    let effective: Vec<&str> = content
        .lines()
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();

    if effective.len() < 3 {
        return Err(ConfigError::MissingHeader {
            index: effective.len() + 1,
        });
    }

    let protocol = Protocol::parse(strip_label(effective[0]))?;
    let (host, port) = parse_connection(strip_label(effective[1]))?;
    let (user, password) = parse_credential(strip_label(effective[2]))?;

    Ok(Config {
        protocol,
        host,
        port,
        user,
        password,
        commands: effective[3..].iter().map(|line| line.to_string()).collect(),
    })
}

/// 剥除可选的 `标签:` 前缀。
fn strip_label(line: &str) -> &str {
    for label in HEADER_LABELS {
        let Some(rest) = line.strip_prefix(label) else {
            continue;
        };
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix(':')
            .or_else(|| rest.strip_prefix('：'))
            .unwrap_or(rest);
        return rest.trim();
    }
    line
}

/// 解析连接信息，返回 (主机, 端口)。
fn parse_connection(raw: &str) -> Result<(String, u16), ConfigError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ConfigError::EmptyHost);
    }

    // [addr]:port 形式（IPv6）
    if let Some(rest) = raw.strip_prefix('[') {
        let Some((host, after)) = rest.split_once(']') else {
            return Err(ConfigError::InvalidConnection(raw.to_string()));
        };
        let after = after.trim();
        let port = if after.is_empty() {
            22
        } else if let Some(port) = after.strip_prefix(':') {
            parse_port(port)?
        } else {
            return Err(ConfigError::InvalidConnection(raw.to_string()));
        };
        if host.is_empty() {
            return Err(ConfigError::EmptyHost);
        }
        return Ok((host.to_string(), port));
    }

    // host:port 形式；不带方括号且含多个冒号时按 IPv6 处理（端口取默认 22）
    match raw.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            if host.is_empty() {
                return Err(ConfigError::EmptyHost);
            }
            Ok((host.to_string(), parse_port(port)?))
        }
        _ => Ok((raw.to_string(), 22)),
    }
}

/// 解析端口。
fn parse_port(raw: &str) -> Result<u16, ConfigError> {
    let raw = raw.trim();
    raw.parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| ConfigError::InvalidPort(raw.to_string()))
}

/// 解析账号密码行，返回 (用户名, 密码)。
///
/// 密码为空时返回 `None`，表示需要交互式补输。
fn parse_credential(raw: &str) -> Result<(String, Option<String>), ConfigError> {
    let raw = raw.trim();
    let Some((user, password)) = raw.split_once(',') else {
        return Err(ConfigError::InvalidCredential(raw.to_string()));
    };
    let user = user.trim();
    if user.is_empty() {
        return Err(ConfigError::EmptyUser);
    }
    let password = password.trim();
    let password = if password.is_empty() {
        None
    } else {
        Some(password.to_string())
    };
    Ok((user.to_string(), password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_config() {
        let cfg = parse("sftp\n192.168.1.10:2222\nroot,Passw0rd\nuname -a\nls -l\n").unwrap();
        assert_eq!(cfg.protocol, Protocol::Sftp);
        assert_eq!(cfg.host, "192.168.1.10");
        assert_eq!(cfg.port, 2222);
        assert_eq!(cfg.user, "root");
        assert_eq!(cfg.password.as_deref(), Some("Passw0rd"));
        assert_eq!(cfg.commands, vec!["uname -a", "ls -l"]);
    }

    #[test]
    fn protocol_is_case_insensitive() {
        assert_eq!(parse("SFTP\nh:22\nu,p\n").unwrap().protocol, Protocol::Sftp);
        assert_eq!(parse("Ssh\nh:22\nu,p\n").unwrap().protocol, Protocol::Ssh);
    }

    #[test]
    fn rejects_unknown_protocol() {
        let err = parse("telnet\nh:22\nu,p\n").unwrap_err();
        assert_eq!(err, ConfigError::UnsupportedProtocol("telnet".into()));
    }

    #[test]
    fn port_defaults_to_22() {
        let cfg = parse("sftp\n192.168.1.10\nroot,p\n").unwrap();
        assert_eq!(cfg.port, 22);
        assert_eq!(cfg.host, "192.168.1.10");
    }

    #[test]
    fn parses_bracketed_ipv6() {
        let cfg = parse("sftp\n[::1]:2222\nroot,p\n").unwrap();
        assert_eq!(cfg.host, "::1");
        assert_eq!(cfg.port, 2222);
    }

    #[test]
    fn bare_ipv6_uses_default_port() {
        let cfg = parse("sftp\n::1\nroot,p\n").unwrap();
        assert_eq!(cfg.host, "::1");
        assert_eq!(cfg.port, 22);
    }

    #[test]
    fn rejects_invalid_port() {
        assert_eq!(
            parse("sftp\nhost:0\nroot,p\n").unwrap_err(),
            ConfigError::InvalidPort("0".into())
        );
        assert_eq!(
            parse("sftp\nhost:70000\nroot,p\n").unwrap_err(),
            ConfigError::InvalidPort("70000".into())
        );
    }

    #[test]
    fn empty_password_means_interactive_prompt() {
        let cfg = parse("sftp\nhost:22\nroot,\n").unwrap();
        assert_eq!(cfg.password, None);
    }

    #[test]
    fn password_keeps_inner_comma() {
        let cfg = parse("sftp\nhost:22\nroot,pa,ss\n").unwrap();
        assert_eq!(cfg.password.as_deref(), Some("pa,ss"));
    }

    #[test]
    fn credential_without_comma_is_rejected() {
        assert_eq!(
            parse("sftp\nhost:22\nroot\n").unwrap_err(),
            ConfigError::InvalidCredential("root".into())
        );
    }

    #[test]
    fn empty_user_is_rejected() {
        assert_eq!(
            parse("sftp\nhost:22\n,pass\n").unwrap_err(),
            ConfigError::EmptyUser
        );
    }

    #[test]
    fn blank_and_comment_lines_are_skipped() {
        let cfg = parse("# 注释\n\nsftp\n\nhost:22\nroot,p\n\n# 尾注释\necho hi\n").unwrap();
        assert_eq!(cfg.commands, vec!["echo hi"]);
    }

    #[test]
    fn header_labels_are_stripped() {
        let cfg =
            parse("协议: sftp\n连接: 192.168.1.10:2222\n账号: root,Passw0rd\nuname -a\n").unwrap();
        assert_eq!(cfg.protocol, Protocol::Sftp);
        assert_eq!(cfg.host, "192.168.1.10");
        assert_eq!(cfg.port, 2222);
        assert_eq!(cfg.user, "root");
        assert_eq!(cfg.password.as_deref(), Some("Passw0rd"));
    }

    #[test]
    fn command_lines_keep_their_own_colons() {
        let cfg = parse("sftp\nhost:22\nroot,p\necho a:b\n").unwrap();
        assert_eq!(cfg.commands, vec!["echo a:b"]);
    }

    #[test]
    fn missing_headers_are_reported() {
        assert_eq!(
            parse("sftp\nhost:22\n").unwrap_err(),
            ConfigError::MissingHeader { index: 3 }
        );
        assert_eq!(
            parse("sftp\n").unwrap_err(),
            ConfigError::MissingHeader { index: 2 }
        );
        assert_eq!(
            parse("").unwrap_err(),
            ConfigError::MissingHeader { index: 1 }
        );
    }

    #[test]
    fn commands_may_be_empty() {
        let cfg = parse("sftp\nhost:22\nroot,p\n").unwrap();
        assert!(cfg.commands.is_empty());
    }
}
