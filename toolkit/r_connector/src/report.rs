//! 执行结果回显格式。
//!
//! 每条指令的回显结构：
//!
//! ```text
//! >>> [1/3] uname -a
//! <远端 stdout 原样输出>
//! <远端 stderr 原样输出（写到 stderr）>
//! <<< exit=0
//! ```

use std::io::Write;

/// 打印一行到 stdout。
///
/// 下游管道被关闭（如 `| head`）时静默退出，不 panic。
pub fn print_line(line: &str) {
    let mut out = std::io::stdout();
    match writeln!(out, "{line}") {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("警告: 输出失败: {e}");
        }
    }
}

/// 指令开始。
pub fn header(index: usize, total: usize, command: &str) {
    print_line(&format!(">>> [{index}/{total}] {command}"));
}

/// 指令结束（带退出码）。
pub fn footer_exit(exit_code: Option<u32>) {
    match exit_code {
        Some(code) => print_line(&format!("<<< exit={code}")),
        None => print_line("<<< exit=<远端未提供退出状态>"),
    }
}

/// 指令结束（执行出错）。
pub fn footer_error(message: &str) {
    print_line(&format!("<<< error: {message}"));
}

/// 指令结束（超时）。
pub fn footer_timeout() {
    print_line("<<< error: 执行超时");
}

/// 汇总行。
pub fn summary(total: usize, succeeded: usize, failed: usize) {
    print_line(&format!(
        "执行完成: 共 {total} 条指令, 成功 {succeeded} 条, 失败 {failed} 条"
    ));
}
