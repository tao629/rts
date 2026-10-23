//! 目录遍历、文件读取与编码识别、逐行匹配编排。
//!
//! - 只处理 `.txt` / `.csv`（大小写不敏感），递归遍历子目录，不跟随目录符号链接。
//! - 编码识别：先按 UTF-8 校验整个文件，合法则按 UTF-8 处理，否则回退 GB18030
//!   （兼容 GBK/GB2312）。匹配始终在**解码后的 Unicode 文本**上进行，
//!   避免多字节编码下关键字边界错位。
//! - 逐行匹配，一旦命中即终止该文件的继续匹配。

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use encoding_rs::{CoderResult, Decoder, GB18030};

use crate::rule::Rule;

/// 待扫描的文件扩展名（小写比较）。
pub const TARGET_EXTENSIONS: [&str; 2] = ["txt", "csv"];

/// 读块大小。
const READ_CHUNK: usize = 8 * 1024;

/// 解码输出缓冲大小。
const DECODE_BUFFER: usize = 16 * 1024;

/// 扫描过程中的告警（不中断整体扫描）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanIssue {
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
    Unsupported { path: PathBuf, encoding: &'static str },
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
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext = ext.to_ascii_lowercase();
            TARGET_EXTENSIONS.contains(&ext.as_str())
        })
        .unwrap_or(false)
}

/// 递归收集目录下所有待扫描文件，并收集遍历过程中的告警。
///
/// 返回的文件按路径排序，保证多次运行输出完全一致。
pub fn collect_files(root: &Path, exclusions: &Exclusions) -> (Vec<PathBuf>, Vec<ScanIssue>) {
    let mut files = Vec::new();
    let mut issues = Vec::new();
    walk(root, exclusions, &mut files, &mut issues);
    files.sort();
    (files, issues)
}

fn walk(
    dir: &Path,
    exclusions: &Exclusions,
    files: &mut Vec<PathBuf>,
    issues: &mut Vec<ScanIssue>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            issues.push(ScanIssue {
                message: format!("无法读取目录 {}: {}", dir.display(), err),
            });
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                issues.push(ScanIssue {
                    message: format!("无法读取目录项 {}: {}", dir.display(), err),
                });
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                issues.push(ScanIssue {
                    message: format!("无法读取文件类型 {}: {}", path.display(), err),
                });
                continue;
            }
        };

        if file_type.is_dir() {
            // 真实目录才递归；目录符号链接不跟随，避免环路。
            walk(&path, exclusions, files, issues);
            continue;
        }

        // 符号链接：指向普通文件的按目标内容扫描，指向目录的不跟随。
        let is_candidate = if file_type.is_symlink() {
            path.is_file()
        } else {
            file_type.is_file()
        };
        if is_candidate && has_target_extension(&path) && !exclusions.contains(&path) {
            files.push(path);
        }
    }
}

/// 扫描单个文件，返回命中的行号（从 1 开始）；未命中返回 `None`。
pub fn scan_file(path: &Path, rule: &Rule) -> Result<Option<usize>, ScanError> {
    let head = read_head(path)?;
    if is_utf16_bom(&head) {
        return Err(ScanError::Unsupported {
            path: path.to_path_buf(),
            encoding: "UTF-16",
        });
    }

    if is_valid_utf8(path)? {
        scan_utf8(path, rule)
    } else {
        scan_gb18030(path, rule)
    }
}

/// 读取文件开头最多 3 个字节，用于识别 BOM。
fn read_head(path: &Path) -> Result<[u8; 3], ScanError> {
    let mut file = File::open(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
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
                })
            }
        }
    }
    Ok(head)
}

/// 判断是否以 UTF-16 BOM 开头（本次不支持该编码）。
fn is_utf16_bom(head: &[u8; 3]) -> bool {
    (head[0] == 0xFF && head[1] == 0xFE) || (head[0] == 0xFE && head[1] == 0xFF)
}

/// 流式校验文件是否为合法 UTF-8。
fn is_valid_utf8(path: &Path) -> Result<bool, ScanError> {
    let mut file = File::open(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut chunk = vec![0u8; READ_CHUNK];
    // 跨块可能截断多字节序列，最多保留 3 个字节到下一块。
    let mut carry: Vec<u8> = Vec::with_capacity(4);

    loop {
        let read = file.read(&mut chunk).map_err(|source| ScanError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }

        let mut combined = Vec::with_capacity(carry.len() + read);
        combined.extend_from_slice(&carry);
        combined.extend_from_slice(&chunk[..read]);

        match std::str::from_utf8(&combined) {
            Ok(_) => carry.clear(),
            Err(err) => {
                if err.error_len().is_some() {
                    // 出现了确定的非法字节序列。
                    return Ok(false);
                }
                // 结尾处是不完整的多字节序列，留到下一块继续判断。
                carry.clear();
                carry.extend_from_slice(&combined[err.valid_up_to()..]);
            }
        }
    }

    // 文件以不完整的多字节序列结尾 → 不是合法 UTF-8。
    Ok(carry.is_empty())
}

/// 按 UTF-8 逐行扫描。
fn scan_utf8(path: &Path, rule: &Rule) -> Result<Option<usize>, ScanError> {
    let file = File::open(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::with_capacity(READ_CHUNK, file);
    let mut raw: Vec<u8> = Vec::new();
    let mut line_no = 0usize;
    let mut first_line = true;

    loop {
        raw.clear();
        let read = reader
            .read_until(b'\n', &mut raw)
            .map_err(|source| ScanError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        line_no += 1;

        let mut bytes = raw.as_slice();
        if first_line {
            first_line = false;
            // 去掉 UTF-8 BOM。
            if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
                bytes = rest;
            }
        }
        // 兼容 CRLF。
        if bytes.last() == Some(&b'\r') {
            bytes = &bytes[..bytes.len() - 1];
        }

        let line = std::str::from_utf8(bytes).map_err(|err| ScanError::Decode {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        if rule.find_hit(line).is_some() {
            return Ok(Some(line_no));
        }
    }

    Ok(None)
}

/// 按 GB18030（兼容 GBK/GB2312）逐行扫描。
fn scan_gb18030(path: &Path, rule: &Rule) -> Result<Option<usize>, ScanError> {
    let file = File::open(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::with_capacity(READ_CHUNK, file);
    let mut decoder: Decoder = GB18030.new_decoder();
    let mut out_buffer = vec![0u8; DECODE_BUFFER];
    let mut chunk = vec![0u8; READ_CHUNK];
    let mut pending = String::new();
    let mut line_no = 0usize;

    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|source| ScanError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        let last = read == 0;
        let text = decode_chunk(&mut decoder, &mut out_buffer, &chunk[..read], last);
        pending.push_str(&text);

        while let Some(position) = pending.find('\n') {
            let line: String = pending.drain(..=position).collect();
            line_no += 1;
            let line = line.strip_suffix('\n').unwrap_or(&line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            if rule.find_hit(line).is_some() {
                return Ok(Some(line_no));
            }
        }

        if last {
            break;
        }
    }

    // 文件末尾没有换行时，最后一段也是一行。
    if !pending.is_empty() {
        line_no += 1;
        let line = pending.strip_suffix('\r').unwrap_or(&pending);
        if rule.find_hit(line).is_some() {
            return Ok(Some(line_no));
        }
    }

    Ok(None)
}

/// 用解码器把一段字节解码成文本，自动处理输出缓冲写满的情况。
fn decode_chunk(
    decoder: &mut Decoder,
    out_buffer: &mut Vec<u8>,
    src: &[u8],
    last: bool,
) -> String {
    let mut text = String::new();
    let mut input = src;

    loop {
        let (result, read, written, _) = decoder.decode_to_utf8(input, out_buffer, last);
        // encoding_rs 保证写入的是合法 UTF-8。
        if let Ok(decoded) = std::str::from_utf8(&out_buffer[..written]) {
            text.push_str(decoded);
        }
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

    text
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 创建一个临时目录，测试结束后自动清理。
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "f_matcher_test_{tag}_{}_{id}",
                std::process::id()
            ));
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

        let (files, issues) = collect_files(&temp.path, &Exclusions::default());
        assert!(issues.is_empty(), "不应有告警: {issues:?}");

        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(&temp.path).unwrap().to_string_lossy().into_owned())
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

        let (files, _) = collect_files(&temp.path, &Exclusions::default());
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(&temp.path).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["a.txt".to_string(), "m/b.txt".to_string(), "z.txt".to_string()]
        );
    }

    #[test]
    fn skips_excluded_rule_file() {
        let temp = TempDir::new("exclude");
        let rule_path = temp.write("sensitive_rule.txt", "关键字: 部队".as_bytes());
        temp.write("a.txt", b"x");

        let exclusions = Exclusions::new(std::slice::from_ref(&rule_path));
        let (files, _) = collect_files(&temp.path, &exclusions);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("a.txt"));
    }

    #[test]
    fn finds_hit_in_utf8_file() {
        let temp = TempDir::new("utf8");
        let path = temp.write("a.txt", "第一行\n第二行有部队\n第三行\n".as_bytes());

        let line = scan_file(&path, &rule("关键字: 部队")).expect("应扫描成功");
        assert_eq!(line, Some(2));
    }

    #[test]
    fn finds_hit_in_gbk_file() {
        let temp = TempDir::new("gbk");
        let (encoded, _, _) = GB18030.encode("第一行\n第二行有导弹\n");
        let path = temp.write("c.csv", &encoded);

        let line = scan_file(&path, &rule("关键字: 导弹")).expect("应扫描成功");
        assert_eq!(line, Some(2));
    }

    #[test]
    fn handles_utf8_bom() {
        let temp = TempDir::new("bom");
        let mut content = vec![0xEF, 0xBB, 0xBF];
        content.extend_from_slice("第一行\n第二行有部队\n".as_bytes());
        let path = temp.write("a.txt", &content);

        let line = scan_file(&path, &rule("关键字: 部队")).expect("应扫描成功");
        assert_eq!(line, Some(2));
    }

    #[test]
    fn reports_utf16_as_unsupported() {
        let temp = TempDir::new("utf16");
        let mut content = vec![0xFF, 0xFE];
        content.extend_from_slice(&[0x2D, 0x4E, 0x00, 0x00]);
        let path = temp.write("a.txt", &content);

        let err = scan_file(&path, &rule("关键字: 部队")).expect_err("应报不支持");
        assert!(matches!(err, ScanError::Unsupported { .. }));
    }

    #[test]
    fn stops_at_first_hit_in_file() {
        let temp = TempDir::new("stop");
        let path = temp.write("a.txt", "第一行\n第二行有部队\n第三行有导弹\n".as_bytes());

        let rule = rule("关键字: 部队,导弹");
        let line = scan_file(&path, &rule).expect("应扫描成功");
        assert_eq!(line, Some(2), "命中后应停止，不再看第三行");
    }

    #[test]
    fn excludes_keyword_followed_by_exclusion_word() {
        let temp = TempDir::new("exclusion");
        let path = temp.write("a.txt", "第一行\n部队医院\n".as_bytes());

        let rule = rule("关键字: 部队\n不包括“医院”");
        assert_eq!(scan_file(&path, &rule).expect("应扫描成功"), None);
    }

    #[test]
    fn counts_lines_without_trailing_newline() {
        let temp = TempDir::new("noeol");
        let path = temp.write("a.txt", "第一行\n第二行有部队".as_bytes());

        assert_eq!(
            scan_file(&path, &rule("关键字: 部队")).expect("应扫描成功"),
            Some(2)
        );
    }

    #[test]
    fn counts_lines_with_crlf() {
        let temp = TempDir::new("crlf");
        let path = temp.write("a.txt", b"first\r\nsecond \xe9\x83\xa8\xe9\x98\x9f\r\n");

        assert_eq!(
            scan_file(&path, &rule("关键字: 部队")).expect("应扫描成功"),
            Some(2)
        );
    }

    #[test]
    fn reports_missing_file_as_io_error() {
        let missing = std::env::temp_dir().join("f_matcher_definitely_missing_file.txt");
        let err = scan_file(&missing, &rule("关键字: 部队")).expect_err("应报错");
        assert!(matches!(err, ScanError::Io { .. }));
    }
}
