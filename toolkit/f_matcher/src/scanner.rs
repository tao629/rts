//! 目录遍历、文件读取与编码识别、逐行匹配编排。
//!
//! - 只处理 `.txt` / `.csv`（大小写不敏感），递归遍历子目录，不跟随目录符号链接。
//! - 编码识别：按 UTF-8 校验文件，合法则按 UTF-8 处理，否则回退 GB18030
//!   （兼容 GBK/GB2312）。匹配始终在**解码后的 Unicode 文本**上进行，
//!   避免多字节编码下关键字边界错位。
//! - 逐行匹配，一旦命中即终止该文件的继续匹配。
//!
//! 性能要点：
//! - 目录遍历**并行**进行（按层级 BFS，层内并行）。
//! - 每个文件**只打开一次**；UTF-8 文件只**读一遍**——读取与 UTF-8 合法性校验、
//!   逐行匹配在同一次流式读取中完成。
//! - GB18030 文件在发现首个非法字节后才回退重扫（主流 UTF-8 文件不受影响）。

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use encoding_rs::{CoderResult, Decoder, GB18030};
use memchr::memchr;
use rayon::ThreadPool;
use rayon::prelude::*;

use crate::rule::LineMatcher;

/// 待扫描的文件扩展名（小写比较）。
pub const TARGET_EXTENSIONS: [&str; 2] = ["txt", "csv"];

/// 读块大小。
const READ_CHUNK: usize = 64 * 1024;

/// 解码输出缓冲大小。
const DECODE_BUFFER: usize = 64 * 1024;

/// 扫描过程中的告警（不中断整体扫描）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanIssue {
    /// 告警涉及的文件或目录。
    pub path: PathBuf,
    /// 告警描述。
    pub message: String,
}

impl fmt::Display for ScanIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// 单个文件扫描失败。
#[derive(Debug)]
pub enum ScanError {
    /// 读取文件失败。
    Io { path: PathBuf, source: io::Error },
    /// 文件编码本次不支持。
    Unsupported {
        path: PathBuf,
        encoding: &'static str,
    },
    /// 解码失败。
    Decode { path: PathBuf, message: String },
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanError::Io { path, source } => {
                write!(f, "无法读取文件 {}: {}", path.display(), source)
            }
            ScanError::Unsupported { path, encoding } => {
                write!(f, "跳过文件 {}: 不支持 {} 编码", path.display(), encoding)
            }
            ScanError::Decode { path, message } => {
                write!(f, "无法解码文件 {}: {}", path.display(), message)
            }
        }
    }
}

impl std::error::Error for ScanError {}

/// 排除项：需要跳过的文件（规则文件自身）。
#[derive(Debug, Clone, Default)]
pub struct Exclusions {
    paths: Vec<PathBuf>,
    canonical: Vec<PathBuf>,
}

impl Exclusions {
    /// 构造排除项，`path` 会被规范化用于比对不同路径写法指向同一文件的情况。
    pub fn new(paths: &[PathBuf]) -> Self {
        let mut exclusions = Exclusions {
            paths: paths.to_vec(),
            canonical: Vec::new(),
        };
        exclusions.canonical = paths.iter().filter_map(|p| p.canonicalize().ok()).collect();
        exclusions
    }

    /// 判断路径是否被排除。
    pub fn contains(&self, path: &Path) -> bool {
        if self.paths.iter().any(|p| p == path) {
            return true;
        }
        // 文件名与所有排除项都不一致 → 不可能是被排除的文件，避免逐文件做规范化开销。
        if !self.paths.iter().any(|p| p.file_name() == path.file_name()) {
            return false;
        }
        match path.canonicalize() {
            Ok(canonical) => self.canonical.contains(&canonical),
            Err(_) => false,
        }
    }
}

/// 判断路径的扩展名是否为待扫描类型。
pub fn has_target_extension(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => TARGET_EXTENSIONS
            .iter()
            .any(|target| ext.eq_ignore_ascii_case(target)),
        None => false,
    }
}

/// 单个目录的遍历结果。
#[derive(Debug, Default)]
struct DirScan {
    /// 子目录（下一层待遍历）。
    subdirs: Vec<PathBuf>,
    /// 本层收集到的待扫描文件。
    files: Vec<PathBuf>,
    /// 遍历过程中的告警。
    issues: Vec<ScanIssue>,
}

/// 递归收集目录下所有待扫描文件，并收集遍历过程中的告警。
///
/// 遍历按层级 BFS 进行，同一层的目录并行读取。
///
/// 返回的文件按路径排序，保证多次运行输出完全一致。
pub fn collect_files(
    root: &Path,
    exclusions: &Exclusions,
    pool: &ThreadPool,
) -> (Vec<PathBuf>, Vec<ScanIssue>) {
    let mut files = Vec::new();
    let mut issues = Vec::new();
    let mut level = vec![root.to_path_buf()];

    while !level.is_empty() {
        let scanned: Vec<DirScan> = pool.install(|| {
            level
                .par_iter()
                .map(|dir| scan_dir(dir, exclusions))
                .collect::<Vec<_>>()
        });

        let mut next = Vec::new();
        for scan in scanned {
            next.extend(scan.subdirs);
            files.extend(scan.files);
            issues.extend(scan.issues);
        }
        level = next;
    }

    files.sort();
    issues.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.message.cmp(&b.message)));
    (files, issues)
}

/// 读取一个目录，收集其下的子目录与待扫描文件。
fn scan_dir(dir: &Path, exclusions: &Exclusions) -> DirScan {
    let mut scan = DirScan::default();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            scan.issues.push(ScanIssue {
                path: dir.to_path_buf(),
                message: format!("无法读取目录 {}: {}", dir.display(), err),
            });
            return scan;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                scan.issues.push(ScanIssue {
                    path: dir.to_path_buf(),
                    message: format!("无法读取目录项 {}: {}", dir.display(), err),
                });
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                scan.issues.push(ScanIssue {
                    path: path.clone(),
                    message: format!("无法读取文件类型 {}: {}", path.display(), err),
                });
                continue;
            }
        };

        if file_type.is_dir() {
            // 真实目录才递归；目录符号链接不跟随，避免环路。
            scan.subdirs.push(path);
            continue;
        }

        // 符号链接：指向普通文件的按目标内容扫描，指向目录的不跟随。
        let is_candidate = if file_type.is_symlink() {
            path.is_file()
        } else {
            file_type.is_file()
        };
        if is_candidate && has_target_extension(&path) && !exclusions.contains(&path) {
            scan.files.push(path);
        }
    }

    scan
}

/// UTF-8 扫描的结果。
enum Utf8Scan {
    /// 扫描完成，`Some(line_no)` 表示命中行号。
    Hit(Option<usize>),
    /// 文件不是合法 UTF-8，需要回退 GB18030 重新扫描。
    NotUtf8,
}

/// 扫描单个文件，返回命中的行号（从 1 开始）；未命中返回 `None`。
///
/// `matcher` 持有跨文件复用的匹配状态，由调用方（每个扫描线程一份）提供。
pub fn scan_file(path: &Path, matcher: &mut LineMatcher<'_>) -> Result<Option<usize>, ScanError> {
    let mut file = File::open(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    // 读取文件开头最多 3 个字节，用于识别 BOM。
    let mut head = [0u8; 3];
    let mut filled = 0;
    while filled < head.len() {
        match file.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) => {
                return Err(ScanError::Io {
                    path: path.to_path_buf(),
                    source: err,
                });
            }
        }
    }

    if is_utf16_bom(&head) {
        return Err(ScanError::Unsupported {
            path: path.to_path_buf(),
            encoding: "UTF-16",
        });
    }

    // 回到文件开头：后续扫描复用同一个文件句柄，只打开一次。
    file.seek(SeekFrom::Start(0))
        .map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    match scan_utf8(&mut file, path, matcher)? {
        Utf8Scan::Hit(line_no) => Ok(line_no),
        Utf8Scan::NotUtf8 => {
            // 不是合法 UTF-8：回到开头按 GB18030 重新解码扫描。
            file.seek(SeekFrom::Start(0))
                .map_err(|source| ScanError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
            scan_gb18030(&mut file, path, matcher)
        }
    }
}

/// 判断是否以 UTF-16 BOM 开头（本次不支持该编码）。
fn is_utf16_bom(head: &[u8; 3]) -> bool {
    (head[0] == 0xFF && head[1] == 0xFE) || (head[0] == 0xFE && head[1] == 0xFF)
}

/// 按 UTF-8 流式扫描：读取、合法性校验与逐行匹配在同一次读取中完成。
///
/// 命中后仍会把整个文件读完，以满足"整文件 UTF-8 合法性"语义；
/// 若中途发现非法字节，则本次结果作废（由调用方回退 GB18030 重扫）。
fn scan_utf8(
    file: &mut File,
    path: &Path,
    matcher: &mut LineMatcher<'_>,
) -> Result<Utf8Scan, ScanError> {
    let mut reader = BufReader::with_capacity(READ_CHUNK, file);
    let mut chunk = vec![0u8; READ_CHUNK];
    // 跨块延续的行内容。
    let mut line: Vec<u8> = Vec::new();
    let mut line_no = 0usize;
    let mut hit: Option<usize> = None;
    let mut first_line = true;

    loop {
        let read = reader.read(&mut chunk).map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }

        let mut data = &chunk[..read];
        while let Some(position) = memchr(b'\n', data) {
            line.extend_from_slice(&data[..position]);
            data = &data[position + 1..];

            line_no += 1;
            let bytes = strip_line_markers(&line, first_line);
            first_line = false;
            let text = match std::str::from_utf8(bytes) {
                Ok(text) => text,
                // 不是合法 UTF-8，交由 GB18030 路径处理。
                Err(_) => return Ok(Utf8Scan::NotUtf8),
            };
            if hit.is_none() && matcher.find_hit(text).is_some() {
                hit = Some(line_no);
            }
            line.clear();
        }
        line.extend_from_slice(data);
    }

    // 文件末尾没有换行时，最后一段也是一行。
    if !line.is_empty() {
        line_no += 1;
        let bytes = strip_line_markers(&line, first_line);
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return Ok(Utf8Scan::NotUtf8),
        };
        if hit.is_none() && matcher.find_hit(text).is_some() {
            hit = Some(line_no);
        }
    }

    Ok(Utf8Scan::Hit(hit))
}

/// 剥掉首行的 UTF-8 BOM 与行尾的 `\r`（CRLF 兼容）。
fn strip_line_markers(line: &[u8], is_first_line: bool) -> &[u8] {
    let mut bytes = line;
    if is_first_line {
        // 去掉 UTF-8 BOM。
        if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            bytes = rest;
        }
    }
    // 兼容 CRLF。
    if bytes.last() == Some(&b'\r') {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

/// 按 GB18030（兼容 GBK/GB2312）逐行扫描。
fn scan_gb18030(
    file: &mut File,
    path: &Path,
    matcher: &mut LineMatcher<'_>,
) -> Result<Option<usize>, ScanError> {
    let mut reader = BufReader::with_capacity(READ_CHUNK, file);
    let mut decoder: Decoder = GB18030.new_decoder();
    let mut out_buffer = vec![0u8; DECODE_BUFFER];
    let mut chunk = vec![0u8; READ_CHUNK];
    // 解码后的文本累积区，按换行切分后整体回收，避免逐行分配。
    let mut text: Vec<u8> = Vec::new();
    let mut line_no = 0usize;
    let mut hit: Option<usize> = None;

    loop {
        let read = reader.read(&mut chunk).map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let last = read == 0;
        decode_into(
            &mut decoder,
            &mut out_buffer,
            &mut text,
            &chunk[..read],
            last,
        );

        let mut start = 0usize;
        for position in memchr::memchr_iter(b'\n', &text) {
            let line = &text[start..position];
            start = position + 1;
            line_no += 1;
            let bytes = strip_cr(line);
            let line = std::str::from_utf8(bytes).map_err(|err| ScanError::Decode {
                path: path.to_path_buf(),
                message: err.to_string(),
            })?;
            if hit.is_none() && matcher.find_hit(line).is_some() {
                hit = Some(line_no);
            }
        }
        text.drain(..start);

        if last {
            break;
        }
    }

    // 文件末尾没有换行时，最后一段也是一行。
    if !text.is_empty() {
        line_no += 1;
        let bytes = strip_cr(&text);
        let line = std::str::from_utf8(bytes).map_err(|err| ScanError::Decode {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        if hit.is_none() && matcher.find_hit(line).is_some() {
            hit = Some(line_no);
        }
    }

    Ok(hit)
}

/// 去掉行尾的 `\r`（CRLF 兼容）。
fn strip_cr(line: &[u8]) -> &[u8] {
    match line.last() {
        Some(b'\r') => &line[..line.len() - 1],
        _ => line,
    }
}

/// 用解码器把一段字节解码成文本并追加到 `text`，自动处理输出缓冲写满的情况。
fn decode_into(
    decoder: &mut Decoder,
    out_buffer: &mut Vec<u8>,
    text: &mut Vec<u8>,
    src: &[u8],
    last: bool,
) {
    let mut input = src;

    loop {
        let (result, read, written, _) =
            decoder.decode_to_utf8(input, out_buffer.as_mut_slice(), last);
        // encoding_rs 保证写入的是合法 UTF-8。
        text.extend_from_slice(&out_buffer[..written]);
        input = &input[read..];

        match result {
            CoderResult::InputEmpty => break,
            CoderResult::OutputFull => {
                if written == 0 {
                    // 没有进展说明缓冲过小，扩容后重试。
                    out_buffer.resize(out_buffer.len() * 2, 0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rule::Rule;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 创建一个临时目录，测试结束后自动清理。
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir()
                .join(format!("f_matcher_test_{tag}_{}_{id}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("应能创建临时目录");
            TempDir { path }
        }

        fn write(&self, relative: &str, content: &[u8]) -> PathBuf {
            let path = self.path.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("应能创建父目录");
            }
            fs::write(&path, content).expect("应能写入文件");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn rule(text: &str) -> Rule {
        Rule::parse(text).expect("规则应可解析")
    }

    /// 在单线程池中扫描（测试不需要并行）。
    fn scan(path: &Path, rule: &Rule) -> Result<Option<usize>, ScanError> {
        let mut matcher = rule.matcher();
        scan_file(path, &mut matcher)
    }

    fn collect(root: &Path) -> (Vec<PathBuf>, Vec<ScanIssue>) {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .expect("应能创建线程池");
        collect_files(root, &Exclusions::default(), &pool)
    }

    #[test]
    fn collects_only_txt_and_csv_recursively() {
        let temp = TempDir::new("collect");
        temp.write("a.txt", b"x");
        temp.write("b.csv", b"x");
        temp.write("c.log", b"x");
        temp.write("noext", b"x");
        temp.write("d.TXT", b"x");
        temp.write("e.Csv", b"x");
        temp.write("sub/f.txt", b"x");
        temp.write("sub/deep/g.csv", b"x");

        let (files, issues) = collect(&temp.path);
        assert!(issues.is_empty(), "不应有告警: {issues:?}");

        let names: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&temp.path)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "a.txt".to_string(),
                "b.csv".to_string(),
                "d.TXT".to_string(),
                "e.Csv".to_string(),
                "sub/deep/g.csv".to_string(),
                "sub/f.txt".to_string(),
            ]
        );
    }

    #[test]
    fn collects_in_stable_sorted_order() {
        let temp = TempDir::new("sorted");
        temp.write("z.txt", b"x");
        temp.write("a.txt", b"x");
        temp.write("m/b.txt", b"x");

        let (files, _) = collect(&temp.path);
        let names: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&temp.path)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "a.txt".to_string(),
                "m/b.txt".to_string(),
                "z.txt".to_string()
            ]
        );
    }

    #[test]
    fn skips_excluded_rule_file() {
        let temp = TempDir::new("exclude");
        let rule_path = temp.write("sensitive_rule.txt", "关键字: 部队".as_bytes());
        temp.write("a.txt", b"x");

        let exclusions = Exclusions::new(std::slice::from_ref(&rule_path));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .expect("应能创建线程池");
        let (files, _) = collect_files(&temp.path, &exclusions, &pool);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("a.txt"));
    }

    #[test]
    fn finds_hit_in_utf8_file() {
        let temp = TempDir::new("utf8");
        let path = temp.write("a.txt", "第一行\n第二行有部队\n第三行\n".as_bytes());

        let line = scan(&path, &rule("关键字: 部队")).expect("应扫描成功");
        assert_eq!(line, Some(2));
    }

    #[test]
    fn finds_hit_in_gbk_file() {
        let temp = TempDir::new("gbk");
        let (encoded, _, _) = GB18030.encode("第一行\n第二行有导弹\n");
        let path = temp.write("c.csv", &encoded);

        let line = scan(&path, &rule("关键字: 导弹")).expect("应扫描成功");
        assert_eq!(line, Some(2));
    }

    #[test]
    fn handles_utf8_bom() {
        let temp = TempDir::new("bom");
        let mut content = vec![0xEF, 0xBB, 0xBF];
        content.extend_from_slice("第一行\n第二行有部队\n".as_bytes());
        let path = temp.write("a.txt", &content);

        let line = scan(&path, &rule("关键字: 部队")).expect("应扫描成功");
        assert_eq!(line, Some(2));
    }

    #[test]
    fn reports_utf16_as_unsupported() {
        let temp = TempDir::new("utf16");
        let mut content = vec![0xFF, 0xFE];
        content.extend_from_slice(&[0x2D, 0x4E, 0x00, 0x00]);
        let path = temp.write("a.txt", &content);

        let err = scan(&path, &rule("关键字: 部队")).expect_err("应报不支持");
        assert!(matches!(err, ScanError::Unsupported { .. }));
    }

    #[test]
    fn stops_at_first_hit_in_file() {
        let temp = TempDir::new("stop");
        let path = temp.write("a.txt", "第一行\n第二行有部队\n第三行有导弹\n".as_bytes());

        let rule = rule("关键字: 部队,导弹");
        let line = scan(&path, &rule).expect("应扫描成功");
        assert_eq!(line, Some(2), "命中后应停止，不再看第三行");
    }

    #[test]
    fn excludes_keyword_followed_by_exclusion_word() {
        let temp = TempDir::new("exclusion");
        let path = temp.write("a.txt", "第一行\n部队医院\n".as_bytes());

        let rule = rule("关键字: 部队\n不包括“医院”");
        assert_eq!(scan(&path, &rule).expect("应扫描成功"), None);
    }

    #[test]
    fn counts_lines_without_trailing_newline() {
        let temp = TempDir::new("noeol");
        let path = temp.write("a.txt", "第一行\n第二行有部队".as_bytes());

        assert_eq!(
            scan(&path, &rule("关键字: 部队")).expect("应扫描成功"),
            Some(2)
        );
    }

    #[test]
    fn counts_lines_with_crlf() {
        let temp = TempDir::new("crlf");
        let path = temp.write("a.txt", b"first\r\nsecond \xe9\x83\xa8\xe9\x98\x9f\r\n");

        assert_eq!(
            scan(&path, &rule("关键字: 部队")).expect("应扫描成功"),
            Some(2)
        );
    }

    #[test]
    fn reports_missing_file_as_io_error() {
        let missing = std::env::temp_dir().join("f_matcher_definitely_missing_file.txt");
        let err = scan(&missing, &rule("关键字: 部队")).expect_err("应报错");
        assert!(matches!(err, ScanError::Io { .. }));
    }

    #[test]
    fn gbk_file_without_trailing_newline_counts_last_line() {
        let temp = TempDir::new("gbk_noeol");
        let (encoded, _, _) = GB18030.encode("第一行\n第二行有导弹");
        let path = temp.write("c.csv", &encoded);

        assert_eq!(
            scan(&path, &rule("关键字: 导弹")).expect("应扫描成功"),
            Some(2)
        );
    }

    #[test]
    fn gbk_file_with_crlf_counts_lines() {
        let temp = TempDir::new("gbk_crlf");
        let (encoded, _, _) = GB18030.encode("第一行\r\n第二行有导弹\r\n");
        let path = temp.write("c.csv", &encoded);

        assert_eq!(
            scan(&path, &rule("关键字: 导弹")).expect("应扫描成功"),
            Some(2)
        );
    }

    #[test]
    fn mixed_encoding_file_falls_back_to_gb18030() {
        // 前半段是合法 UTF-8，后半段含非法字节 → 整体按 GB18030 解读。
        let temp = TempDir::new("mixed");
        let (encoded, _, _) = GB18030.encode("第一行有导弹\n");
        let mut content = "ASCII 前缀行\n".as_bytes().to_vec();
        content.extend_from_slice(&encoded);
        let path = temp.write("c.csv", &content);

        let line = scan(&path, &rule("关键字: 导弹")).expect("应扫描成功");
        assert_eq!(line, Some(2), "回退 GB18030 后仍应命中第 2 行");
    }

    #[test]
    fn hit_before_invalid_bytes_is_discarded_on_gb18030_fallback() {
        // UTF-8 前缀里出现关键字，但整体不是合法 UTF-8 → 必须按 GB18030 重新判定。
        let temp = TempDir::new("fallback");
        let mut content = "部队\n".as_bytes().to_vec();
        content.push(0xFF);
        let path = temp.write("c.csv", &content);

        // GB18030 解读下 "部队" 的字节不是同两个字符，故不应命中。
        let line = scan(&path, &rule("关键字: 部队")).expect("应扫描成功");
        assert_eq!(line, None, "非 UTF-8 文件按 GB18030 判定，不应命中");
    }

    #[test]
    fn line_numbering_is_stable_across_chunk_boundaries() {
        // 造一个跨越 64KB 读块边界的文件，验证行号不漂移。
        let temp = TempDir::new("chunk");
        let mut content = String::new();
        for index in 0..5000 {
            content.push_str(&format!("第{index}行普通内容\n"));
        }
        content.push_str("出现部队的一行\n");
        let path = temp.write("big.txt", content.as_bytes());

        assert_eq!(
            scan(&path, &rule("关键字: 部队")).expect("应扫描成功"),
            Some(5001)
        );
    }
}
