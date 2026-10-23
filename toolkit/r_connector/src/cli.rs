//! 命令行参数解析。

use std::path::PathBuf;

/// 命令行动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 使用指定配置文件运行。
    Run(PathBuf),
    /// 打印用法。
    Help,
    /// 打印版本。
    Version,
}

/// 解析命令行参数（不含程序名）。
pub fn parse(args: &[String]) -> Result<Action, String> {
    let mut positional: Vec<&String> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("未知参数: {other}"));
            }
            _ => positional.push(arg),
        }
    }

    match positional.len() {
        0 => Err("缺少参数：需要指定配置文件路径".to_string()),
        1 => Ok(Action::Run(PathBuf::from(positional[0]))),
        _ => Err(format!(
            "参数过多：只接受一个配置文件路径，收到 {} 个",
            positional.len()
        )),
    }
}

/// 用法说明。
pub fn usage() -> String {
    "用法: r_connector <config-file>

按配置文件连接远端主机并依次执行其中的指令，执行结果回显到本地终端。

配置文件格式（按行处理）:
  第 1 行   协议        sftp 或 ssh
  第 2 行   连接信息    host:port（端口可省略，默认 22；IPv6 用 [addr]:port）
  第 3 行   账号密码    user,password（密码可留空，留空则交互式输入且不回显）
  第 4 行起 待执行指令  每行一条，按顺序执行，交给远端 shell 解释

示例:
  sftp
  192.168.1.10:2222
  root,Passw0rd
  uname -a
  systemctl status nginx

选项:
  -h, --help     打印本帮助
  -V, --version  打印版本号
"
    .to_string()
}

/// 版本号。
pub fn version() -> String {
    format!("r_connector {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_positional_is_config_path() {
        let action = parse(&args(&["/tmp/conf.txt"])).unwrap();
        assert_eq!(action, Action::Run(PathBuf::from("/tmp/conf.txt")));
    }

    #[test]
    fn missing_argument_is_rejected() {
        assert!(parse(&args(&[])).is_err());
    }

    #[test]
    fn too_many_arguments_are_rejected() {
        assert!(parse(&args(&["a.txt", "b.txt"])).is_err());
    }

    #[test]
    fn unknown_flag_is_rejected() {
        assert!(parse(&args(&["--nope"])).is_err());
    }

    #[test]
    fn help_and_version_are_recognized() {
        assert_eq!(parse(&args(&["--help"])).unwrap(), Action::Help);
        assert_eq!(parse(&args(&["-h"])).unwrap(), Action::Help);
        assert_eq!(parse(&args(&["--version"])).unwrap(), Action::Version);
        assert_eq!(parse(&args(&["-V"])).unwrap(), Action::Version);
    }
}
