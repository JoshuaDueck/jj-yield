//! Parser for Jujutsu *snapshot* conflict markers.
//!
//! The grammar is line-oriented. A conflict region opens with a run of `<` (>= 7
//! chars), contains a sequence of `+++++++` / `-------` terms each followed by
//! their content lines, and closes with a matching run of `>`. Everything else
//! is context.
//!
//! Robustness note: Jujutsu lengthens the marker run when file content would
//! otherwise collide with a 7-char marker, so within a region we only treat a
//! line as a structural marker if its run length *exactly* matches the opening
//! `<<<<<<<`. Content that merely starts with `+++++++` of a different length is
//! left untouched.

use crate::conflict::{ConflictRegion, ParsedFile, Segment, SideLabel, Term, TermKind};

const MARKER_CHARS: [char; 5] = ['<', '>', '+', '-', '%'];
const MIN_MARKER_LEN: usize = 7;

/// If `line` is a conflict marker, return `(marker_char, run_len, label_rest)`.
/// `label_rest` is the trailing text after a single separating space (trimmed),
/// or empty if the line is just the marker run.
fn marker(line: &str) -> Option<(char, usize, &str)> {
    let first = line.chars().next()?;
    if !MARKER_CHARS.contains(&first) {
        return None;
    }
    // Marker chars are all ASCII, so char count == byte count for the run.
    let run = line.chars().take_while(|&c| c == first).count();
    if run < MIN_MARKER_LEN {
        return None;
    }
    let rest = &line[run..];
    match rest.chars().next() {
        None => Some((first, run, "")),
        Some(' ') => Some((first, run, rest[1..].trim_end())),
        // e.g. "+++++++x" — a run immediately followed by a non-space is content.
        Some(_) => None,
    }
}

/// Parse a `conflict n of m` header into `(n, m)`.
fn parse_header(rest: &str) -> Option<(usize, usize)> {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    if toks.len() >= 4 && toks[0] == "conflict" && toks[2] == "of" {
        if let (Ok(n), Ok(m)) = (toks[1].parse(), toks[3].parse()) {
            return Some((n, m));
        }
    }
    None
}

/// Parse a full materialized file into [`ParsedFile`].
pub fn parse(text: &str) -> ParsedFile {
    let trailing_newline = text.ends_with('\n');
    // Drop the single trailing newline so `split('\n')` doesn't yield a phantom
    // empty final line.
    let body = if trailing_newline {
        &text[..text.len() - 1]
    } else {
        text
    };
    let lines: Vec<&str> = if body.is_empty() {
        Vec::new()
    } else {
        body.split('\n').collect()
    };

    let mut segments: Vec<Segment> = Vec::new();
    let mut context: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        match marker(lines[i]) {
            Some(('<', len, rest)) => {
                if !context.is_empty() {
                    segments.push(Segment::Context(std::mem::take(&mut context)));
                }
                let (region, next) = parse_region(&lines, i, len, rest);
                segments.push(Segment::Conflict(region));
                i = next;
            }
            _ => {
                context.push(lines[i].to_string());
                i += 1;
            }
        }
    }
    if !context.is_empty() {
        segments.push(Segment::Context(context));
    }

    ParsedFile { segments, trailing_newline }
}

/// Parse one conflict region starting at `lines[start]` (the `<<<<<<<` line).
/// Returns the region and the index of the line after its `>>>>>>>` close.
fn parse_region(
    lines: &[&str],
    start: usize,
    marker_len: usize,
    header_rest: &str,
) -> (ConflictRegion, usize) {
    let header = parse_header(header_rest);
    let mut raw: Vec<String> = vec![lines[start].to_string()];
    let mut terms: Vec<Term> = Vec::new();
    let mut current: Option<(TermKind, SideLabel, Vec<String>)> = None;
    let mut i = start + 1;

    while i < lines.len() {
        let line = lines[i];
        match marker(line) {
            // A new term, but only if the run length matches this region's.
            Some((ch, len, rest)) if len == marker_len && (ch == '+' || ch == '-') => {
                if let Some((kind, label, content)) = current.take() {
                    terms.push(Term { kind, label, content });
                }
                let kind = if ch == '+' { TermKind::Add } else { TermKind::Remove };
                current = Some((kind, SideLabel::parse(rest), Vec::new()));
                raw.push(line.to_string());
            }
            // End of the region.
            Some(('>', len, _)) if len == marker_len => {
                if let Some((kind, label, content)) = current.take() {
                    terms.push(Term { kind, label, content });
                }
                raw.push(line.to_string());
                i += 1;
                return (ConflictRegion { header, marker_len, terms, raw }, i);
            }
            // Anything else is content for the current term.
            _ => {
                if let Some((_, _, content)) = current.as_mut() {
                    content.push(line.to_string());
                }
                raw.push(line.to_string());
            }
        }
        i += 1;
    }

    // Unterminated region (malformed input): flush what we have.
    if let Some((kind, label, content)) = current.take() {
        terms.push(Term { kind, label, content });
    }
    (ConflictRegion { header, marker_len, terms, raw }, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_detection() {
        assert_eq!(marker("<<<<<<< conflict 1 of 1"), Some(('<', 7, "conflict 1 of 1")));
        assert_eq!(marker("+++++++ rpulyxsl 46d0a2ab \"x\""), Some(('+', 7, "rpulyxsl 46d0a2ab \"x\"")));
        assert_eq!(marker(">>>>>>>"), Some(('>', 7, "")));
        assert_eq!(marker("------- base"), Some(('-', 7, "base")));
        assert_eq!(marker("++++++"), None); // only 6 — too short
        assert_eq!(marker("+++++++x"), None); // run followed by non-space
        assert_eq!(marker("hello"), None);
    }

    #[test]
    fn longer_markers_are_respected() {
        // jj uses 8-char markers when content would collide with 7-char ones.
        // A 7-char `+++++++` line inside an 8-char region must stay content.
        let text = "ctx\n\
                    <<<<<<<< conflict 1 of 1\n\
                    ++++++++ a 1 \"A\"\n\
                    +++++++\n\
                    -------- b 2 \"base\"\n\
                    base line\n\
                    ++++++++ c 3 \"B\"\n\
                    bee\n\
                    >>>>>>>> conflict 1 of 1 ends\n\
                    end\n";
        let parsed = parse(text);
        assert_eq!(parsed.region_count(), 1);
        let region = parsed.region(0).unwrap();
        assert_eq!(region.marker_len, 8);
        // The 7-char `+++++++` is content of the first term, not a new term.
        assert_eq!(region.terms[0].content, vec!["+++++++".to_string()]);
        assert_eq!(region.sides(), 2);
        // Re-emitting unresolved must reproduce the input exactly.
        assert_eq!(parsed.render(&[None]), text);
    }
}
