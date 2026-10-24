//! 敏感关键字规则：规则文件解析 + 逐行匹配判定。
//!
//! 规则文件格式（见 `toolkit/f_matcher/sensitive_rule.txt`）：
//!
//! ```text
//! 关键字: 部队,军队,海军,...
//!
//! 上述结果中，不包括“路”、“医院”、“宾馆”，...
//! ```
//!
//! - 关键字表：取 `关键字` 标签所在行的内容，按半角/全角逗号切分。
//! - 排除词表：取全文中成对引号包裹的片段。
//! - 排除语义为**严格紧邻**：关键字后面紧接的字符构成排除词时，该次匹配不计为命中。
//!
//! 匹配实现采用 **Aho-Corasick 多模式自动机**：整张关键字表一次性编译成自动机，
//! 每行文本只被扫描**一遍**即可得到所有关键字的候选命中（而非逐关键字重复扫描），
//! 这是文件量大时最主要的性能收益点。

use std::fmt;

use aho_corasick::AhoCorasick;

/// 关键字标签行前缀。
const KEYWORD_LABEL: &str = "关键字";

/// 关键字规则：关键字表 + 排除词表 + 预编译的多模式匹配器。
#[derive(Debug, Clone)]
pub struct Rule {
    /// 关键字表，保持规则文件中的出现顺序并去重。
    pub keywords: Vec<String>,
    /// 排除词表：关键字后紧跟这些词时，该次匹配不算命中。
    pub exclusions: Vec<String>,
    /// 预编译的 Aho-Corasick 自动机（构造后只读，可跨线程共享）。
    matcher: AhoCorasick,
    /// 排除词首字节集合：命中位置之后的字节不在其中时，可直接跳过排除判定。
    exclusion_starts: [bool; 256],
}

impl PartialEq for Rule {
    fn eq(&self, other: &Self) -> bool {
        self.keywords == other.keywords && self.exclusions == other.exclusions
    }
}

impl Eq for Rule {}

/// 一行文本中的命中位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 命中的关键字。
    pub keyword: String,
    /// 命中关键字在行内的字节起始位置。
    pub position: usize,
}

/// 规则解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    /// 规则文件中找不到 `关键字` 标签行。
    MissingKeywordSection,
    /// 找到关键字行，但切分后没有任何有效关键字。
    EmptyKeywords,
    /// 关键字无法编译成匹配器。
    InvalidKeyword(String),
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::MissingKeywordSection => {
                write!(f, "规则文件中找不到 `{}` 标签行", KEYWORD_LABEL)
            }
            RuleError::EmptyKeywords => write!(f, "规则文件中的关键字表为空"),
            RuleError::InvalidKeyword(word) => {
                write!(f, "规则文件中的关键字无法用于匹配: {word}")
            }
        }
    }
}

impl std::error::Error for RuleError {}

impl Rule {
    /// 解析规则文件内容。
    ///
    /// 找不到关键字行与关键字表为空是两种不同的错误，便于给出可诊断的提示。
    pub fn parse(text: &str) -> Result<Self, RuleError> {
        let mut found_section = false;
        let mut keywords: Vec<String> = Vec::new();

        for raw_line in text.lines() {
            let line = raw_line.trim();
            let Some(rest) = strip_label(line, KEYWORD_LABEL) else {
                continue;
            };
            found_section = true;
            for part in rest.split([',', '，']) {
                let word = part.trim();
                if !word.is_empty() && !keywords.iter().any(|k| k == word) {
                    keywords.push(word.to_string());
                }
            }
        }

        if !found_section {
            return Err(RuleError::MissingKeywordSection);
        }
        if keywords.is_empty() {
            return Err(RuleError::EmptyKeywords);
        }

        let exclusions = parse_exclusions(text);

        let matcher = AhoCorasick::new(&keywords)
            .map_err(|err| RuleError::InvalidKeyword(err.to_string()))?;

        let mut exclusion_starts = [false; 256];
        for exclusion in &exclusions {
            if let Some(first) = exclusion.as_bytes().first() {
                exclusion_starts[*first as usize] = true;
            }
        }

        Ok(Rule {
            keywords,
            exclusions,
            matcher,
            exclusion_starts,
        })
    }

    /// 创建逐行匹配器：持有跨行复用的中间状态，避免每行重新分配。
    pub fn matcher(&self) -> LineMatcher<'_> {
        LineMatcher {
            rule: self,
            // 每个关键字上一次被采纳匹配的结束位置，用于非重叠归一。
            last_end: vec![0; self.keywords.len()],
        }
    }

    /// 判断命中结束位置之后是否紧邻某个排除词。
    fn is_excluded(&self, line: &str, end: usize) -> bool {
        let rest = &line[end..];
        let Some(first) = rest.as_bytes().first() else {
            return false;
        };
        // 排除词首字节都不匹配时，无需逐词比较。
        if !self.exclusion_starts[*first as usize] {
            return false;
        }
        self.exclusions
            .iter()
            .any(|exclusion| rest.starts_with(exclusion.as_str()))
    }
}

/// 逐行匹配器：复用自动机扫描所需的中间状态。
///
/// 每个扫描线程持有一个实例，跨文件、跨行复用，避免逐行分配。
pub struct LineMatcher<'a> {
    rule: &'a Rule,
    /// 每个关键字上一次被采纳匹配的结束位置。
    last_end: Vec<usize>,
}

impl LineMatcher<'_> {
    /// 在一行文本中查找命中，返回起始位置最靠前的有效命中。
    ///
    /// 排除词按**严格紧邻**判定：关键字后面紧接的字符若构成某个排除词，
    /// 则该次匹配不计为命中（例：`部队医院` 不命中，`解放军总医院` 命中）。
    pub fn find_hit(&mut self, line: &str) -> Option<Hit> {
        self.reset();
        let mut best: Option<(usize, usize)> = None;

        for mat in self.rule.matcher.find_overlapping_iter(line.as_bytes()) {
            let keyword = mat.pattern().as_usize();
            // 同一关键字的重叠出现按非重叠处理（等价 `str::match_indices` 语义）。
            if mat.start() < self.last_end[keyword] {
                continue;
            }
            self.last_end[keyword] = mat.end();
            if self.rule.is_excluded(line, mat.end()) {
                continue;
            }
            let better = match best {
                None => true,
                // 位置相同时取关键字表中靠前的那个，与逐关键字扫描的旧行为一致。
                Some((position, current)) => {
                    mat.start() < position || (mat.start() == position && keyword < current)
                }
            };
            if better {
                best = Some((mat.start(), keyword));
            }
        }

        best.map(|(position, keyword)| Hit {
            keyword: self.rule.keywords[keyword].clone(),
            position,
        })
    }

    /// 清空每行独立的状态。
    fn reset(&mut self) {
        self.last_end.iter_mut().for_each(|end| *end = 0);
    }
}

/// 剥掉行首的标签与紧随其后的冒号、空白，返回标签之后的内容。
fn strip_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(label)?;
    Some(rest.trim_start_matches(|c: char| c == ':' || c == '：' || c.is_whitespace()))
}

/// 提取全文中成对引号包裹的片段作为排除词。
///
/// 支持全角弯引号 `“…”`、半角双引号 `"…"` 与半角单引号 `'…'`。
fn parse_exclusions(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut exclusions: Vec<String> = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let closing = match chars[index] {
            '“' => '”',
            '"' => '"',
            '\'' => '\'',
            _ => {
                index += 1;
                continue;
            }
        };
        match chars[index + 1..].iter().position(|c| *c == closing) {
            Some(offset) => {
                let end = index + 1 + offset;
                let word: String = chars[index + 1..end].iter().collect();
                let word = word.trim();
                if !word.is_empty() && !exclusions.iter().any(|e| e == word) {
                    exclusions.push(word.to_string());
                }
                index = end + 1;
            }
            None => index += 1,
        }
    }

    exclusions
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_RULE: &str = include_str!("../sensitive_rule.txt");

    /// 在一行文本中查找命中（测试用的便捷入口）。
    fn find_hit(rule: &Rule, line: &str) -> Option<Hit> {
        rule.matcher().find_hit(line)
    }

    #[test]
    fn parses_real_rule_file() {
        let rule = Rule::parse(REAL_RULE).expect("规则文件应可解析");
        assert_eq!(rule.keywords.len(), 25);
        assert_eq!(rule.keywords[0], "部队");
        assert_eq!(rule.keywords[1], "军队");
        assert_eq!(rule.keywords[24], "军区");
        assert_eq!(rule.exclusions.len(), 8);
        for expected in [
            "路",
            "医院",
            "宾馆",
            "学院",
            "家属区",
            "宿舍",
            "干休所",
            "疗养院",
        ] {
            assert!(
                rule.exclusions.iter().any(|e| e == expected),
                "排除词应包含 {expected}"
            );
        }
    }

    #[test]
    fn accepts_half_width_comma_and_colon() {
        let rule = Rule::parse("关键字: 部队,军队").expect("半角分隔符应可解析");
        assert_eq!(rule.keywords, vec!["部队", "军队"]);
        assert!(rule.exclusions.is_empty());
    }

    #[test]
    fn accepts_full_width_comma_and_colon() {
        let rule = Rule::parse("关键字：部队，军队").expect("全角分隔符应可解析");
        assert_eq!(rule.keywords, vec!["部队", "军队"]);
    }

    #[test]
    fn accepts_space_after_label() {
        let rule = Rule::parse("关键字  :  部队").expect("标签后的空白应被忽略");
        assert_eq!(rule.keywords, vec!["部队"]);
    }

    #[test]
    fn extracts_exclusions_from_quote_styles() {
        let rule = Rule::parse("关键字: 军区\n不包括“医院”和\"宾馆\"及'家属区'").expect("应可解析");
        assert_eq!(rule.exclusions, vec!["医院", "宾馆", "家属区"]);
    }

    #[test]
    fn deduplicates_keywords_and_exclusions() {
        let rule = Rule::parse("关键字: 军区,军区,部队\n不包括“医院”、“医院”").expect("应可解析");
        assert_eq!(rule.keywords, vec!["军区", "部队"]);
        assert_eq!(rule.exclusions, vec!["医院"]);
    }

    #[test]
    fn drops_empty_items() {
        let rule = Rule::parse("关键字: 军区,, ,部队,\n").expect("应可解析");
        assert_eq!(rule.keywords, vec!["军区", "部队"]);
    }

    #[test]
    fn reports_missing_keyword_section() {
        let err = Rule::parse("没有任何关键字标签").expect_err("应报错");
        assert_eq!(err, RuleError::MissingKeywordSection);
    }

    #[test]
    fn reports_empty_keyword_table() {
        let err = Rule::parse("关键字: ").expect_err("应报错");
        assert_eq!(err, RuleError::EmptyKeywords);
    }

    #[test]
    fn finds_plain_hit() {
        let rule = Rule::parse("关键字: 部队").expect("应可解析");
        let hit = find_hit(&rule, "今天有部队调动").expect("应命中");
        assert_eq!(hit.keyword, "部队");
        // 位置为字节偏移："今天有" 占 3 个汉字 = 9 字节。
        assert_eq!(hit.position, 9);
    }

    #[test]
    fn excludes_keyword_followed_by_exclusion_word() {
        let rule = Rule::parse("关键字: 部队\n不包括“医院”").expect("应可解析");
        assert!(
            find_hit(&rule, "部队医院").is_none(),
            "紧邻排除词应不算命中"
        );
    }

    #[test]
    fn keeps_hit_when_exclusion_word_is_not_adjacent() {
        let rule = Rule::parse("关键字: 解放军\n不包括“医院”").expect("应可解析");
        assert!(
            find_hit(&rule, "解放军总医院").is_some(),
            "排除词不紧邻时仍应命中"
        );
    }

    #[test]
    fn ignores_exclusion_word_before_keyword() {
        let rule = Rule::parse("关键字: 部队\n不包括“医院”").expect("应可解析");
        assert!(
            find_hit(&rule, "医院旁边的部队").is_some(),
            "排除词在前不应影响命中"
        );
    }

    #[test]
    fn reports_earliest_position_within_line() {
        let rule = Rule::parse("关键字: 导弹,部队").expect("应可解析");
        let hit = find_hit(&rule, "部队和导弹").expect("应命中");
        assert_eq!(hit.keyword, "部队");
        assert_eq!(hit.position, 0);
    }

    #[test]
    fn matches_keyword_at_end_of_line() {
        let rule = Rule::parse("关键字: 部队").expect("应可解析");
        assert!(find_hit(&rule, "末尾是部队").is_some());
    }

    #[test]
    fn exclusion_at_end_of_line_does_not_panic() {
        let rule = Rule::parse("关键字: 部队\n不包括“医院”").expect("应可解析");
        assert!(find_hit(&rule, "部队").is_some());
    }

    #[test]
    fn matches_every_keyword_of_real_rule() {
        let rule = Rule::parse(REAL_RULE).expect("规则文件应可解析");
        for keyword in &rule.keywords {
            let line = format!("前缀{keyword}后缀");
            let hit = find_hit(&rule, &line).expect("每个关键字都应能命中");
            assert_eq!(&hit.keyword, keyword);
        }
    }

    #[test]
    fn same_keyword_overlap_is_non_overlapping() {
        // "aaaa" 中 "aa" 的出现位置应与 `str::match_indices` 一致（0 与 2，而非 0 与 1）。
        let rule = Rule::parse("关键字: aa").expect("应可解析");
        let mut matcher = rule.matcher();
        let hit = matcher.find_hit("aaaa").expect("应命中");
        assert_eq!(hit.position, 0);

        let expected: Vec<usize> = "aaaa".match_indices("aa").map(|(pos, _)| pos).collect();
        assert_eq!(expected, vec![0, 2]);
    }

    #[test]
    fn keyword_prefix_of_another_keyword_still_matches() {
        let rule = Rule::parse("关键字: 军,军队").expect("应可解析");
        assert!(find_hit(&rule, "有军队").is_some());
        assert!(find_hit(&rule, "有军").is_some());
    }

    #[test]
    fn overlapping_keywords_both_detected() {
        // "部队" 与 "队医院" 重叠时两者都应被检出。
        let rule = Rule::parse("关键字: 部队,队医院").expect("应可解析");
        let mut matcher = rule.matcher();
        assert!(
            matcher.find_hit("部队医院").is_some(),
            "重叠的关键字都应命中"
        );
    }

    #[test]
    fn excluded_match_does_not_shadow_overlapping_valid_match() {
        // "abc" 命中后紧跟排除词 "d" → 该次不算命中；
        // 与之重叠的 "cd" 仍应被检出（不能因左优先而漏掉）。
        let rule = Rule::parse("关键字: abc,cd\n不包括“d”").expect("应可解析");
        let hit = find_hit(&rule, "abcd").expect("重叠的有效命中不应被漏掉");
        assert_eq!(hit.keyword, "cd");
        assert_eq!(hit.position, 2);
    }

    #[test]
    fn matcher_state_does_not_leak_between_lines() {
        let rule = Rule::parse("关键字: aa").expect("应可解析");
        let mut matcher = rule.matcher();
        assert!(matcher.find_hit("aaaa").is_some());
        // 上一行留下的状态不应影响下一行的判定。
        assert!(matcher.find_hit("bbb").is_none());
        assert!(matcher.find_hit("aaaa").is_some());
    }
}
