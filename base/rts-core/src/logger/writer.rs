//! 按天 + 按大小滚动的日志写入器。
//!
//! 活动文件写在 `<dir>/<file_name>`。滚动时先把活动文件 `rename` 到
//! `<dir>/<archive_dir>/<file_name>.<日期>.<序号>`（rename 只是元数据操作，
//! 不额外占磁盘空间，磁盘写满时也能成功），再压缩成同名的 `.gz`，最后删除
//! 未压缩的中间文件。
//!
//! - 压缩失败时保留未压缩的归档，并打印告警——宁可少压缩，也不丢日志；
//! - 同一天内归档序号从 0 开始递增，跨天归零；
//! - 进程重启后从归档目录里已有的最大序号继续，不会覆盖既有归档；
//! - 超过保留期的归档在启动时与每次滚动后清理。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate};
use flate2::Compression;
use flate2::write::GzEncoder;

use super::config::ResolvedConfig;

/// 日期刷新间隔：避免每写一条日志都取一次时钟。
const DATE_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// 触发滚动的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotateReason {
    /// 单个文件超过大小上限。
    Size,
    /// 跨过了自然日。
    DayChanged,
}

/// 滚动写入器。
///
/// 内部状态由 `Mutex` 保护；实际写入发生在 `tracing-appender` 的工作线程上，
/// 因此同一时刻只有一个写入者。
#[derive(Debug, Clone)]
pub struct RollingWriter {
    state: Arc<Mutex<RollingState>>,
}

impl RollingWriter {
    /// 创建写入器：建目录 → 清理过期归档 → 打开（必要时归档）活动文件。
    pub fn new(config: &ResolvedConfig) -> io::Result<Self> {
        fs::create_dir_all(&config.dir)?;
        let archive_dir = config.archive_path();
        fs::create_dir_all(&archive_dir)?;
        let file_path = config.file_path();

        let today = Local::now().date_naive();

        // 上一进程遗留的活动文件：按 mtime 判定它属于哪一天。
        let file_date = today;
        let mut file_size = 0u64;
        if let Ok(meta) = fs::metadata(&file_path)
            && meta.len() > 0
        {
            let mtime_date = meta
                .modified()
                .ok()
                .map(|time| DateTime::<Local>::from(time).date_naive())
                .unwrap_or(today);
            if mtime_date == today {
                // 今天的文件：追加。
                file_size = meta.len();
            } else {
                // 跨天未滚动就退出了：先归档到它所属的那一天，再新建今天的。
                match archive_existing(&archive_dir, &config.file_name, &file_path, mtime_date) {
                    Ok(()) => {}
                    Err(err) => {
                        eprintln!(
                            "[rts-log] 归档遗留活动文件 {} 失败（{}），将继续写入该文件",
                            file_path.display(),
                            err
                        );
                        // 归档失败意味着文件还在原处，按追加处理。
                        file_size = meta.len();
                    }
                }
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        let state = RollingState {
            archive_dir,
            file_path,
            file_name: config.file_name.clone(),
            max_size: config.max_size,
            max_age_days: config.max_age_days,
            file,
            file_date,
            file_size,
            next_seq: next_seq_for(&config.archive_path(), &config.file_name, file_date),
            roll_blocked: false,
            date_checked_at: Instant::now(),
        };

        // 启动时清理一次过期归档。
        let state = Arc::new(Mutex::new(state));
        {
            let guard = state.lock().unwrap_or_else(|err| err.into_inner());
            guard.cleanup_expired();
        }

        Ok(Self { state })
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut RollingState) -> R) -> R {
        let mut guard = self.state.lock().unwrap_or_else(|err| err.into_inner());
        f(&mut guard)
    }
}

impl Write for RollingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.with_state(|state| state.write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.with_state(|state| state.file.flush())
    }
}

/// 写入器的内部状态。
#[derive(Debug)]
struct RollingState {
    archive_dir: PathBuf,
    file_path: PathBuf,
    file_name: String,
    max_size: u64,
    max_age_days: u32,
    file: File,
    /// 当前活动文件内容所属的自然日。
    file_date: NaiveDate,
    /// 当前活动文件已写入的字节数。
    file_size: u64,
    /// `file_date` 当天下一个可用的归档序号。
    ///
    /// 启动时扫描归档目录得到，之后在内存里递增；跨天时重新扫描。
    next_seq: u32,
    /// 归档目录不可用时置位：停止按大小滚动，避免每写一条日志都重试并刷屏。
    roll_blocked: bool,
    /// 上次刷新"今天"这个日期的时刻。
    date_checked_at: Instant,
}

impl RollingState {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.refresh_date();

        // 写入前判定：文件已有内容且写入后会超限，则先滚动。
        if !self.roll_blocked
            && self.file_size > 0
            && self.file_size.saturating_add(buf.len() as u64) > self.max_size
        {
            self.rotate(RotateReason::Size);
        }

        let written = self.file.write(buf)?;
        self.file_size += written as u64;
        Ok(written)
    }

    /// 刷新当前日期，必要时触发跨天滚动。
    fn refresh_date(&mut self) {
        if self.date_checked_at.elapsed() < DATE_CHECK_INTERVAL {
            return;
        }
        self.date_checked_at = Instant::now();

        let today = Local::now().date_naive();
        if today != self.file_date {
            if self.file_size > 0 {
                self.rotate(RotateReason::DayChanged);
            }
            self.file_date = today;
            // 新的一天，序号从 0 重新开始。
            self.next_seq = next_seq_for(&self.archive_dir, &self.file_name, today);
        }
    }

    /// 滚动：归档当前活动文件，然后新建一个空的活动文件。
    fn rotate(&mut self, reason: RotateReason) {
        let date = self.file_date;
        let seq = self.next_seq;
        let plain_name = format!("{}.{}.{seq}", self.file_name, date.format("%Y-%m-%d"));
        let plain_path = self.archive_dir.join(&plain_name);
        let gz_path = self.archive_dir.join(format!("{plain_name}.gz"));

        if let Err(err) = self.file.flush() {
            eprintln!("[rts-log] 滚动前 flush 失败（{err}）");
        }

        // 先 rename 到归档目录：只是改目录项，不额外占空间，磁盘写满时也能成功。
        if let Err(err) = fs::rename(&self.file_path, &plain_path) {
            eprintln!(
                "[rts-log] 归档活动文件 {} 失败（{err}），继续写入原文件",
                self.file_path.display()
            );
            // 归档目录不可用时不再反复重试，否则每写一条日志都会刷一次错误。
            self.roll_blocked = true;
            self.cleanup_expired();
            return;
        }

        // 归档成功，这个序号已被占用。
        self.next_seq = seq.saturating_add(1);
        self.roll_blocked = false;

        // 压缩成 .gz；失败则保留未压缩的归档。
        match compress_file(&plain_path, &gz_path) {
            Ok(()) => {
                if let Err(err) = fs::remove_file(&plain_path) {
                    eprintln!(
                        "[rts-log] 删除未压缩归档 {} 失败（{err}）",
                        plain_path.display()
                    );
                }
            }
            Err(err) => eprintln!(
                "[rts-log] 压缩归档 {} 失败（{err}），保留未压缩文件",
                plain_path.display()
            ),
        }

        // 新建活动文件。
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            Ok(file) => {
                self.file = file;
                self.file_size = 0;
            }
            Err(err) => eprintln!(
                "[rts-log] 创建新的活动文件 {} 失败（{err}）",
                self.file_path.display()
            ),
        }

        eprintln!(
            "[rts-log] 日志滚动（{reason:?}）：{} -> {}",
            self.file_path.display(),
            gz_path.display()
        );

        self.cleanup_expired();
    }

    /// 清理超过保留期的归档。
    ///
    /// 只处理归档目录内、文件名能解析出日期的文件；其余文件原样保留。
    fn cleanup_expired(&self) {
        let today = Local::now().date_naive();
        let keep_days = i64::from(self.max_age_days) - 1;
        let Some(cutoff) = today.checked_sub_signed(ChronoDuration::days(keep_days)) else {
            return;
        };

        let Ok(entries) = fs::read_dir(&self.archive_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some((date, _seq)) = parse_archive(name, &self.file_name) else {
                continue;
            };
            if date < cutoff
                && let Err(err) = fs::remove_file(&path)
            {
                eprintln!("[rts-log] 删除过期归档 {} 失败（{err}）", path.display());
            }
        }
    }
}

/// 把活动文件归档到归档目录（先 rename，再压缩）。
fn archive_existing(
    archive_dir: &Path,
    file_name: &str,
    file_path: &Path,
    date: NaiveDate,
) -> io::Result<()> {
    let seq = next_seq_for(archive_dir, file_name, date);
    let plain_name = format!("{}.{}.{seq}", file_name, date.format("%Y-%m-%d"));
    let plain_path = archive_dir.join(&plain_name);
    let gz_path = archive_dir.join(format!("{plain_name}.gz"));

    fs::rename(file_path, &plain_path)?;

    match compress_file(&plain_path, &gz_path) {
        Ok(()) => {
            let _ = fs::remove_file(&plain_path);
        }
        Err(err) => eprintln!(
            "[rts-log] 压缩归档 {} 失败（{err}），保留未压缩文件",
            plain_path.display()
        ),
    }
    Ok(())
}

/// 压缩文件为 gzip；失败时删除半成品。
fn compress_file(src: &Path, dst: &Path) -> io::Result<()> {
    let mut input = File::open(src)?;
    let output = File::create(dst)?;
    let mut encoder = GzEncoder::new(output, Compression::default());

    if let Err(err) = io::copy(&mut input, &mut encoder) {
        let _ = fs::remove_file(dst);
        return Err(err);
    }
    if let Err(err) = encoder.finish() {
        let _ = fs::remove_file(dst);
        return Err(err);
    }
    Ok(())
}

/// 取某一天已用归档序号的下一个值。
///
/// 每次滚动都重新扫描归档目录，因此进程重启后不会覆盖既有归档。
fn next_seq_for(archive_dir: &Path, file_name: &str, date: NaiveDate) -> u32 {
    let mut max_seq: Option<u32> = None;

    let Ok(entries) = fs::read_dir(archive_dir) else {
        return 0;
    };

    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some((archive_date, seq)) = parse_archive(&name, file_name) else {
            continue;
        };
        if archive_date != date {
            continue;
        }
        max_seq = Some(max_seq.map_or(seq, |current: u32| current.max(seq)));
    }

    max_seq.map_or(0, |max| max.saturating_add(1))
}

/// 解析归档文件名，返回（日期, 序号）。
///
/// 形如 `app.log.2026-10-23.0.gz`；压缩失败降级留下的未压缩归档
/// `app.log.2026-10-23.0` 同样能解析。
fn parse_archive(file_name: &str, base: &str) -> Option<(NaiveDate, u32)> {
    let rest = file_name.strip_prefix(base)?.strip_prefix('.')?;
    // 先剥掉 gzip 后缀，否则 `rsplit_once` 会切在 ".gz" 的那个点上。
    let rest = rest.strip_suffix(".gz").unwrap_or(rest);
    let (date_str, seq_str) = rest.rsplit_once('.')?;
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let seq = seq_str.parse::<u32>().ok()?;
    Some((date, seq))
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::time::SystemTime;

    use flate2::read::GzDecoder;

    use super::*;

    fn test_config(dir: &Path) -> ResolvedConfig {
        ResolvedConfig {
            dir: dir.to_path_buf(),
            file_name: "app.log".to_owned(),
            archive_dir: "archive".to_owned(),
            max_size: 1024,
            max_age_days: 7,
            level: tracing::level_filters::LevelFilter::INFO,
            console: false,
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rts-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 读出归档目录里的归档文件名，按序号排序。
    fn archives(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir.join("archive"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("app.log."))
            .collect();
        names.sort();
        names
    }

    /// 解压归档内容。
    fn gunzip(path: &Path) -> String {
        let file = File::open(path).unwrap();
        let mut decoder = GzDecoder::new(file);
        let mut content = String::new();
        decoder.read_to_string(&mut content).unwrap();
        content
    }

    #[test]
    fn rolls_by_size_and_numbers_archives_from_zero() {
        let dir = temp_dir("roll-by-size");
        let mut writer = RollingWriter::new(&test_config(&dir)).unwrap();

        let line = "x".repeat(200);
        for _ in 0..20 {
            writer.write_all(line.as_bytes()).unwrap();
            writer.write_all(b"\n").unwrap();
        }
        writer.flush().unwrap();

        let names = archives(&dir);
        assert!(names.len() >= 2, "应当至少产生两个归档，实际: {names:?}");

        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        assert_eq!(names[0], format!("app.log.{today}.0.gz"), "序号应从 0 开始");
        assert_eq!(names[1], format!("app.log.{today}.1.gz"), "序号应连续递增");

        // 活动文件仍然存在，且不超过上限太多。
        assert!(dir.join("app.log").exists());
        drop(writer);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_is_valid_gzip_keeping_original_content() {
        let dir = temp_dir("gzip-roundtrip");
        let mut writer = RollingWriter::new(&test_config(&dir)).unwrap();

        let payload = "第一行日志\n第二行日志\n";
        for _ in 0..100 {
            writer.write_all(payload.as_bytes()).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);

        let names = archives(&dir);
        assert!(!names.is_empty(), "应当产生归档");

        let content = gunzip(&dir.join("archive").join(&names[0]));
        assert!(
            content.contains("第一行日志"),
            "解压后应保留原始内容: {content}"
        );
        assert!(
            content.contains("第二行日志"),
            "解压后应保留原始内容: {content}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_removes_expired_archives_and_keeps_recent_ones() {
        let dir = temp_dir("cleanup");
        let archive_dir = dir.join("archive");
        fs::create_dir_all(&archive_dir).unwrap();

        let today = Local::now().date_naive();
        let expired = today - ChronoDuration::days(10);
        let recent = today - ChronoDuration::days(1);

        fs::write(
            archive_dir.join(format!("app.log.{}.0.gz", expired.format("%Y-%m-%d"))),
            b"old",
        )
        .unwrap();
        fs::write(
            archive_dir.join(format!("app.log.{}.0.gz", recent.format("%Y-%m-%d"))),
            b"new",
        )
        .unwrap();
        // 无法解析日期的文件不应被删除。
        fs::write(archive_dir.join("unrelated.txt"), b"keep").unwrap();

        let _writer = RollingWriter::new(&test_config(&dir)).unwrap();

        assert!(
            !archive_dir
                .join(format!("app.log.{}.0.gz", expired.format("%Y-%m-%d")))
                .exists()
        );
        assert!(
            archive_dir
                .join(format!("app.log.{}.0.gz", recent.format("%Y-%m-%d")))
                .exists()
        );
        assert!(archive_dir.join("unrelated.txt").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn startup_archives_stale_active_file_from_previous_day() {
        let dir = temp_dir("stale-active");
        let archive_dir = dir.join("archive");
        fs::create_dir_all(&archive_dir).unwrap();

        // 造一个两天前的活动文件。
        let active = dir.join("app.log");
        fs::write(&active, b"yesterday's log\n").unwrap();
        let file = OpenOptions::new().append(true).open(&active).unwrap();
        let two_days_ago = SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
        file.set_times(fs::FileTimes::new().set_modified(two_days_ago))
            .unwrap();
        drop(file);

        let _writer = RollingWriter::new(&test_config(&dir)).unwrap();

        let stale_date = (Local::now().date_naive() - ChronoDuration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        let archived = archive_dir.join(format!("app.log.{stale_date}.0.gz"));
        assert!(archived.exists(), "遗留的活动文件应被归档到 {archived:?}");

        let content = gunzip(&archived);
        assert!(content.contains("yesterday's log"), "{content}");

        // 今天的活动文件重新开始。
        assert!(active.exists());
        assert_eq!(fs::metadata(&active).unwrap().len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restart_continues_sequence_instead_of_overwriting() {
        let dir = temp_dir("restart-seq");
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();

        // 模拟上次运行留下的归档。
        let archive_dir = dir.join("archive");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(archive_dir.join(format!("app.log.{today}.0.gz")), b"a").unwrap();
        fs::write(archive_dir.join(format!("app.log.{today}.1.gz")), b"b").unwrap();

        let mut writer = RollingWriter::new(&test_config(&dir)).unwrap();
        let line = "y".repeat(200);
        for _ in 0..20 {
            writer.write_all(line.as_bytes()).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);

        let names = archives(&dir);
        assert!(
            names.contains(&format!("app.log.{today}.2.gz")),
            "重启后应接着用序号 2，而不是覆盖已有归档: {names:?}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_archive_handles_gz_and_plain() {
        let (date, seq) = parse_archive("app.log.2026-10-23.0.gz", "app.log").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 10, 23).unwrap());
        assert_eq!(seq, 0);

        let (date, seq) = parse_archive("app.log.2026-10-23.12.gz", "app.log").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 10, 23).unwrap());
        assert_eq!(seq, 12);

        let (_, seq) = parse_archive("app.log.2026-10-23.3", "app.log").unwrap();
        assert_eq!(seq, 3);
    }

    #[test]
    fn parse_archive_rejects_unrelated_names() {
        assert!(parse_archive("other.log.2026-10-23.0.gz", "app.log").is_none());
        assert!(parse_archive("app.log.not-a-date.0.gz", "app.log").is_none());
        assert!(parse_archive("random.txt", "app.log").is_none());
    }

    #[test]
    fn next_seq_starts_at_zero_and_increments() {
        let dir = temp_dir("seq");
        let archive_dir = dir.join("archive");
        fs::create_dir_all(&archive_dir).unwrap();

        let date = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        assert_eq!(next_seq_for(&archive_dir, "app.log", date), 0);

        fs::write(archive_dir.join("app.log.2026-10-23.0.gz"), b"x").unwrap();
        assert_eq!(next_seq_for(&archive_dir, "app.log", date), 1);

        fs::write(archive_dir.join("app.log.2026-10-23.1.gz"), b"x").unwrap();
        assert_eq!(next_seq_for(&archive_dir, "app.log", date), 2);

        // 另一天的归档不影响当天的序号。
        fs::write(archive_dir.join("app.log.2026-10-22.7.gz"), b"x").unwrap();
        assert_eq!(next_seq_for(&archive_dir, "app.log", date), 2);

        let _ = fs::remove_dir_all(&dir);
    }
}
