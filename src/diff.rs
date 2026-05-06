#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineDiff {
    pub line_index: usize,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WordDiff {
    pub line_index: usize,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

pub fn diff_lines(before: &[String], after: &[String]) -> Vec<LineDiff> {
    let max_len = before.len().max(after.len());
    let mut diffs = Vec::new();

    for idx in 0..max_len {
        let before_line = before.get(idx).cloned().unwrap_or_default();
        let after_line = after.get(idx).cloned().unwrap_or_default();

        if before_line != after_line {
            diffs.push(LineDiff {
                line_index: idx,
                before: before_line,
                after: after_line,
            });
        }
    }

    diffs
}

pub fn diff_words(before: &str, after: &str, line_index: usize) -> WordDiff {
    let before_words: Vec<String> = before.split_whitespace().map(str::to_string).collect();
    let after_words: Vec<String> = after.split_whitespace().map(str::to_string).collect();

    let removed = before_words
        .iter()
        .filter(|word| !after_words.contains(word))
        .cloned()
        .collect();
    let added = after_words
        .iter()
        .filter(|word| !before_words.contains(word))
        .cloned()
        .collect();

    WordDiff {
        line_index,
        removed,
        added,
    }
}

#[cfg(test)]
mod tests {
    use super::{LineDiff, WordDiff, diff_lines, diff_words};

    #[test]
    fn detects_changed_lines() {
        let before = vec!["alpha".to_string(), "beta".to_string()];
        let after = vec!["alpha".to_string(), "gamma".to_string()];

        assert_eq!(
            diff_lines(&before, &after),
            vec![LineDiff {
                line_index: 1,
                before: "beta".to_string(),
                after: "gamma".to_string(),
            }]
        );
    }

    #[test]
    fn reports_added_and_removed_words() {
        assert_eq!(
            diff_words("alpha beta", "alpha gamma", 0),
            WordDiff {
                line_index: 0,
                removed: vec!["beta".to_string()],
                added: vec!["gamma".to_string()],
            }
        );
    }
}
