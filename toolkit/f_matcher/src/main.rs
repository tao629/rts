//! f_matcher —— 批量扫描目录下命中敏感关键字的文件。
//!
//! 遍历指定目录下的所有 `.txt` / `.csv` 文件（含子目录），按规则文件中的
//! 关键字规则匹配；一旦某文件命中 1 个关键字即终止该文件的继续匹配；
//! 输出命中文件的绝对路径与命中行号。

mod cli;
mod rule;
mod scanner;

use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use cli::{Cli, CliOutcome};
use rule::Rule;
use scanner::Exclusions;

/// 退出码：扫描完成，无命中。
const EXIT_NO_MATCH: u8 = 0;
/// 退出码：参数 / 目录 / 规则文件错误。
const EXIT_USAGE: u8 = 1;
/// 退出码：扫描完成但存在文件读取失败（覆盖不完整）。
const EXIT_INCOMPLETE: u8 = 2;
/// 退出码：扫描完成，至少命中一个文件。
const EXIT_MATCHED: u8 = 3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let outcome = match cli::parse_args(&args) {
        Ok(outcome) => outcome,
        Err(err) => {
            eprintln!("错误: {err}");
            eprint!("{}", cli::usage());
            return ExitCode::from(EXIT_USAGE);
        }
    };

    match outcome {
        CliOutcome::Help => {
            print!("{}", cli::usage());
            ExitCode::from(EXIT_NO_MATCH)
        }
        CliOutcome::Version => {
            println!("f_matcher {}", env!("CARGO_PKG_VERSION"));
            ExitCode::from(EXIT_NO_MATCH)
        }
        CliOutcome::Run(cli) => run(cli),
    }
}

/// 执行扫描，返回退出码。
fn run(cli: Cli) -> ExitCode {
    // 1. 定位并解析规则文件。
    let rule_path = match resolve_rule_path(&cli) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("错误: {message}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let rule_text = match fs::read_to_string(&rule_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("错误: 无法读取规则文件 {}: {err}", rule_path.display());
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let rule = match Rule::parse(&rule_text) {
        Ok(rule) => rule,
        Err(err) => {
            eprintln!("错误: 规则文件解析失败 ({}): {err}", rule_path.display());
            return ExitCode::from(EXIT_USAGE);
        }
    };

    // 2. 定位待扫描目录。
    let dir = match std::path::absolute(&cli.dir) {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("错误: 无法解析目录 {}: {err}", cli.dir.display());
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match fs::metadata(&dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            eprintln!("错误: 不是目录: {}", dir.display());
            return ExitCode::from(EXIT_USAGE);
        }
        Err(err) => {
            eprintln!("错误: 无法访问目录 {}: {err}", dir.display());
            return ExitCode::from(EXIT_USAGE);
        }
    }

    // 3. 收集待扫描文件（规则文件自身不参与扫描）。
    let exclusions = Exclusions::new(std::slice::from_ref(&rule_path));
    let (files, issues) = scanner::collect_files(&dir, &exclusions);
    let mut incomplete = !issues.is_empty();
    for issue in issues {
        eprintln!("警告: {issue}");
    }

    // 4. 逐个文件扫描并输出命中结果。
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    let mut matched = 0usize;
    let mut broken_pipe = false;

    for file in files {
        match scanner::scan_file(&file, &rule) {
            Ok(Some(line_no)) => {
                matched += 1;
                if writeln!(writer, "{}:{line_no}", file.display()).is_err() {
                    // 下游管道提前关闭（如 `| head`）时静默结束。
                    broken_pipe = true;
                    break;
                }
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!("警告: {err}");
                incomplete = true;
            }
        }
    }

    if !broken_pipe {
        let _ = writer.flush();
    }

    if incomplete {
        ExitCode::from(EXIT_INCOMPLETE)
    } else if matched > 0 {
        ExitCode::from(EXIT_MATCHED)
    } else {
        ExitCode::from(EXIT_NO_MATCH)
    }
}

/// 定位规则文件：显式指定优先，否则按默认顺序查找。
fn resolve_rule_path(cli: &Cli) -> Result<PathBuf, String> {
    if let Some(path) = &cli.rule {
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        return Ok(absolute);
    }
    cli::default_rule_path().ok_or_else(|| {
        format!(
            "未找到默认规则文件 {}, 请用 --rule 指定规则文件路径",
            cli::RULE_FILE_NAME
        )
    })
}
