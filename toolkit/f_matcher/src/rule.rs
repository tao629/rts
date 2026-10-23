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

use std::fmt;

/// 关键字标签行前缀。
const KEYWORD_LABEL: &str = "关键字";

/// 关键字规则：关键字表 + 排除词表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// 关键字表，保持规则文件中的出现顺序并去重。
    pub keywords: Vec<String>,
    /// 排除词表：关键字后紧跟这些词时，该次匹配不算命中。
    pub exclusions: Vec<String>,
}

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
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::MissingKeywordSection => {
                write!(f, "规则文件中找不到 `{}` 标签行", KEYWORD_LABEL)
            }
            RuleError::EmptyKeywords => write!(f, "规则文件中的关键字表为空"),
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

        Ok(Rule {
            keywords,
            exclusions: parse_exclusions(text),
        })
    }

    /// 在一行文本中查找命中。
    ///
    /// 排除词按**严格紧邻**判定：关键字后面紧接的字符若构成某个排除词，
    /// 则该次匹配不计为命中（例：`部队医院` 不命中，`解放军总医院` 命中）。
    /// 同行存在多个有效命中时返回起始位置最靠前的一个。
    pub fn find_hit(&self, line: &str) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for keyword in &self.keywords {
            for (position, _) in line.match_indices(keyword.as_str()) {
                let rest = &line[position + keyword.len()..];
                if self
                    .exclusions
                    .iter()
                    .any(|exclusion| rest.starts_with(exclusion.as_str()))
                {
                    continue;
                }
                let better = match &best {
                    Some(current) => position < current.position,
                    None => true,
                };
                if better {
                    best = Some(Hit {
                        keyword: keyword.clone(),
                        position,
                    });
                }
            }
        }
        best
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

    #[test]
    fn parses_real_rule_file() {
        let rule = Rule::parse(REAL_RULE).expect("规则文件应可解析");
        assert_eq!(rule.keywords.len(), 25);
        assert_eq!(rule.keywords[0], "部队");
        assert_eq!(rule.keywords[1], "军队");
        assert_eq!(rule.keywords[24], "军区");
        assert_eq!(rule.exclusions.len(), 8);
        for expected in [
            "路", "医院", "宾馆", "学院", "家属区", "宿舍", "干休所", "疗养院",
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
        let hit = rule.find_hit("今天有部队调动").expect("应命中");
        assert_eq!(hit.keyword, "部队");
        // 位置为字节偏移："今天有" 占 3 个汉字 = 9 字节。
        assert_eq!(hit.position, 9);
    }

    #[test]
    fn excludes_keyword_followed_by_exclusion_word() {
        let rule = Rule::parse("关键字: 部队\n不包括“医院”").expect("应可解析");
        assert!(rule.find_hit("部队医院").is_none(), "紧邻排除词应不算命中");
    }

    #[test]
    fn keeps_hit_when_exclusion_word_is_not_adjacent() {
        let rule = Rule::parse("关键字: 解放军\n不包括“医院”").expect("应可解析");
        assert!(
            rule.find_hit("解放军总医院").is_some(),
            "排除词不紧邻时仍应命中"
        );
    }

    #[test]
    fn ignores_exclusion_word_before_keyword() {
        let rule = Rule::parse("关键字: 部队\n不包括“医院”").expect("应可解析");
        assert!(rule.find_hit("医院旁边的部队").is_some(), "排除词在前不应影响命中");
    }

    #[test]
    fn reports_earliest_position_within_line() {
        let rule = Rule::parse("关键字: 导弹,部队").expect("应可解析");
        let hit = rule.find_hit("部队和导弹").expect("应命中");
        assert_eq!(hit.keyword, "部队");
        assert_eq!(hit.position, 0);
    }

    #[test]
    fn matches_keyword_at_end_of_line() {
        let rule = Rule::parse("关键字: 部队").expect("应可解析");
        assert!(rule.find_hit("末尾是部队").is_some());
    }

    #[test]
    fn exclusion_at_end_of_line_does_not_panic() {
        let rule = Rule::parse("关键字: 部队\n不包括“医院”").expect("应可解析");
        assert!(rule.find_hit("部队").is_some());
    }
}
