//! Diff a conflict side against the base, with intra-line word emphasis.
//!
//! This powers the VSCode-style highlighting: each present side is diffed
//! against the merge base so the UI can colour added lines green, removed lines
//! red, and emphasise the exact words that changed within a modified line.

use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTag {
    /// Unchanged relative to the base.
    Equal,
    /// Present in the side but not the base (added).
    Insert,
    /// Present in the base but not the side (removed).
    Delete,
}

/// One run within a line. `emphasized` marks the sub-spans that actually changed
/// (used for intra-line highlighting on modified lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    pub emphasized: bool,
}

/// One line of a side-vs-base diff.
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub tag: DiffTag,
    /// Full line text (no trailing newline).
    pub text: String,
    /// Segments that concatenate to `text`, carrying per-span emphasis.
    pub words: Vec<Word>,
    /// Index into the *side* content for `Insert`/`Equal` lines; `None` for
    /// `Delete` (those lines exist only in the base).
    pub side_line: Option<usize>,
}

/// Diff `side` against `base`, line by line, with inline word emphasis.
pub fn diff_side(base: &[String], side: &[String]) -> Vec<DiffLine> {
    // Terminate every line with '\n' so the final line tokenizes the same as the
    // rest — otherwise `"a"` (base) and `"a\n"` (side) compare as different.
    let base_text = join_lines(base);
    let side_text = join_lines(side);
    let diff = TextDiff::from_lines(&base_text, &side_text);

    let mut out = Vec::new();
    let mut side_idx = 0usize;
    for op in diff.ops() {
        for change in diff.iter_inline_changes(op) {
            let tag = match change.tag() {
                ChangeTag::Equal => DiffTag::Equal,
                ChangeTag::Insert => DiffTag::Insert,
                ChangeTag::Delete => DiffTag::Delete,
            };
            let mut words = Vec::new();
            let mut text = String::new();
            for (emphasized, value) in change.iter_strings_lossy() {
                // Line tokens carry a trailing '\n'; strip it for display.
                let cleaned = value.replace('\n', "");
                if cleaned.is_empty() {
                    continue;
                }
                text.push_str(&cleaned);
                words.push(Word { text: cleaned, emphasized });
            }
            let side_line = match tag {
                DiffTag::Delete => None,
                _ => {
                    let i = side_idx;
                    side_idx += 1;
                    Some(i)
                }
            };
            out.push(DiffLine { tag, text, words, side_line });
        }
    }
    out
}

fn join_lines(lines: &[String]) -> String {
    let mut s = String::new();
    for line in lines {
        s.push_str(line);
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn pure_insert() {
        let d = diff_side(&s(&["a"]), &s(&["a", "b"]));
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].tag, DiffTag::Equal);
        assert_eq!(d[0].side_line, Some(0));
        assert_eq!(d[1].tag, DiffTag::Insert);
        assert_eq!(d[1].text, "b");
        assert_eq!(d[1].side_line, Some(1));
    }

    #[test]
    fn pure_delete() {
        let d = diff_side(&s(&["a", "b"]), &s(&["a"]));
        let del: Vec<_> = d.iter().filter(|l| l.tag == DiffTag::Delete).collect();
        assert_eq!(del.len(), 1);
        assert_eq!(del[0].text, "b");
        assert_eq!(del[0].side_line, None);
    }

    #[test]
    fn empty_side_is_all_deletes() {
        let d = diff_side(&s(&["x", "y"]), &[]);
        assert!(d.iter().all(|l| l.tag == DiffTag::Delete));
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn modification_emphasizes_changed_words() {
        let d = diff_side(&s(&["foo bar"]), &s(&["foo baz"]));
        // The inserted (side) line should carry an emphasized sub-span.
        let ins = d.iter().find(|l| l.tag == DiffTag::Insert).expect("an insert line");
        assert_eq!(ins.text, "foo baz");
        assert!(ins.words.iter().any(|w| w.emphasized), "a word should be emphasized");
        // The unchanged prefix should not be wholly emphasized.
        assert!(ins.words.iter().any(|w| !w.emphasized));
    }
}
