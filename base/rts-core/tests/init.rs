//! 日志模块的集成测试。
//!
//! `tracing` 的全局 subscriber 只能注册一次，因此本文件只放一个用例；
//! 滚动、归档、保留策略的细粒度用例在 `logger::writer` 的单元测试里。

use std::fs;

use rts_core::logger::{LevelFilter, LogConfig};

#[test]
fn writes_to_file_and_rolls_into_gz_archives() {
    let dir = std::env::temp_dir().join(format!("rts-log-init-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);

    let guard = rts_core::logger::log::init(
        LogConfig::default()
            .dir(&dir)
            .max_size(1024)
            .level(LevelFilter::INFO)
            .console(false),
    )
    .expect("初始化日志失败");

    for index in 0..50 {
        tracing::info!("tracing 日志 {index}");
        log::info!("log facade 日志 {index}");
    }

    // 释放 guard 会 flush 异步缓冲并停止后台工作线程。
    drop(guard);

    let active = fs::read_to_string(dir.join("app.log")).expect("读取活动日志文件");
    assert!(
        active.contains("tracing 日志"),
        "tracing 事件应落盘: {active}"
    );
    assert!(
        active.contains("log facade 日志"),
        "log facade 应落盘: {active}"
    );

    let mut archives: Vec<String> = fs::read_dir(dir.join("archive"))
        .expect("读取归档目录")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    archives.sort();
    assert!(!archives.is_empty(), "应当产生归档: {archives:?}");

    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(
        archives[0],
        format!("app.log.{today}.0.gz"),
        "归档序号应从 0 开始: {archives:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}
