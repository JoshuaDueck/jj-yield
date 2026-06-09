//! tree-sitter syntax highlighting with a small dark theme.
//!
//! A [`Highlighter`] holds one configured grammar per supported language, keyed
//! by file extension. [`Highlighter::highlight`] returns one styled line per
//! `'\n'`-delimited line of the input (matching `source.split('\n')`), so the UI
//! can index lines directly and overlay diff colours on top of syntax colours.
//!
//! Unknown languages (or any tree-sitter error) degrade gracefully to plain,
//! unstyled lines. Adding a language is one `add(...)` call plus the grammar
//! dependency in `Cargo.toml`.

use ratatui::style::{Color, Modifier, Style};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter as TsHighlighter};

/// Capture names we recognise. The parallel theme is built from these in
/// [`Highlighter::new`]; `configure` maps grammar captures onto these indices.
const NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "escape",
    "function",
    "function.builtin",
    "function.method",
    "keyword",
    "label",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.escape",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

/// A styled run of text within a single line.
#[derive(Debug, Clone)]
pub struct Seg {
    pub text: String,
    pub style: Style,
}

struct Lang {
    exts: &'static [&'static str],
    config: HighlightConfiguration,
}

pub struct Highlighter {
    langs: Vec<Lang>,
    /// Parallel to [`NAMES`].
    theme: Vec<Style>,
}

impl Highlighter {
    pub fn new() -> Self {
        let theme = NAMES.iter().map(|n| style_for(n)).collect();
        let mut langs: Vec<Lang> = Vec::new();

        let mut add = |exts: &'static [&'static str], cfg: Option<HighlightConfiguration>| {
            if let Some(c) = cfg {
                langs.push(Lang { exts, config: c });
            }
        };

        add(&["rs"], build(
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        ));

        // TypeScript/TSX inherit JavaScript, so prepend the JS queries; fall back
        // to the TS-only query if the combined query fails to compile.
        let ts_combined = format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        );
        add(&["ts", "mts", "cts"], build(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &ts_combined,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ).or_else(|| build(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        )));

        let tsx_combined = format!(
            "{}\n{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        );
        add(&["tsx"], build(
            "tsx",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            &tsx_combined,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ).or_else(|| build(
            "tsx",
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        )));

        let js_combined = format!(
            "{}\n{}",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
        );
        add(&["js", "mjs", "cjs", "jsx"], build(
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            &js_combined,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ).or_else(|| build(
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        )));

        add(&["rb", "rake", "gemspec"], build(
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ruby::LOCALS_QUERY,
        ));

        add(&["py", "pyi"], build(
            "python",
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        ));

        add(&["json", "jsonc"], build(
            "json",
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        ));

        Highlighter { langs, theme }
    }

    fn lang_for(&self, path: &str) -> Option<&Lang> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        self.langs.iter().find(|l| l.exts.contains(&ext.as_str()))
    }

    /// Highlight `source`, returning one styled line per `'\n'`-delimited line.
    pub fn highlight(&self, path: &str, source: &str) -> Vec<Vec<Seg>> {
        match self.lang_for(path) {
            Some(lang) => self.run(lang, source),
            None => plain(source),
        }
    }

    fn run(&self, lang: &Lang, source: &str) -> Vec<Vec<Seg>> {
        let mut ts = TsHighlighter::new();
        let events = match ts.highlight(&lang.config, source.as_bytes(), None, |_| {
            None::<&HighlightConfiguration>
        }) {
            Ok(ev) => ev,
            Err(_) => return plain(source),
        };

        // Fold the event stream into a contiguous tiling of (range, style).
        let mut stack: Vec<usize> = Vec::new();
        let mut spans: Vec<(usize, usize, Style)> = Vec::new();
        for event in events {
            match event {
                Ok(HighlightEvent::HighlightStart(h)) => stack.push(h.0),
                Ok(HighlightEvent::HighlightEnd) => {
                    stack.pop();
                }
                Ok(HighlightEvent::Source { start, end }) => {
                    let style = stack
                        .last()
                        .and_then(|&i| self.theme.get(i).copied())
                        .unwrap_or_default();
                    spans.push((start, end, style));
                }
                Err(_) => return plain(source),
            }
        }
        split_into_lines(source, &spans)
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

fn build(
    name: &str,
    language: tree_sitter::Language,
    highlights: &str,
    injections: &str,
    locals: &str,
) -> Option<HighlightConfiguration> {
    let mut cfg = HighlightConfiguration::new(language, name, highlights, injections, locals).ok()?;
    cfg.configure(NAMES);
    Some(cfg)
}

/// Split a contiguous `(start, end, style)` tiling of `source` into per-line
/// segments, breaking at every `'\n'`.
fn split_into_lines(source: &str, spans: &[(usize, usize, Style)]) -> Vec<Vec<Seg>> {
    let mut lines: Vec<Vec<Seg>> = vec![Vec::new()];
    for &(start, end, style) in spans {
        if start >= end || end > source.len() {
            continue;
        }
        let mut first = true;
        for piece in source[start..end].split('\n') {
            if !first {
                lines.push(Vec::new());
            }
            first = false;
            if !piece.is_empty() {
                lines.last_mut().unwrap().push(Seg { text: piece.to_string(), style });
            }
        }
    }
    lines
}

fn plain(source: &str) -> Vec<Vec<Seg>> {
    source
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                Vec::new()
            } else {
                vec![Seg { text: l.to_string(), style: Style::default() }]
            }
        })
        .collect()
}

/// One Dark-ish theme mapping a canonical capture name to a foreground style.
fn style_for(name: &str) -> Style {
    let rgb = |r, g, b| Style::default().fg(Color::Rgb(r, g, b));
    match name {
        "comment" => rgb(0x5c, 0x63, 0x70).add_modifier(Modifier::ITALIC),
        "keyword" | "label" => rgb(0xc6, 0x78, 0xdd),
        "string" | "string.special" => rgb(0x98, 0xc3, 0x79),
        "string.escape" | "escape" => rgb(0x56, 0xb6, 0xc2),
        "number" | "constant" | "constant.builtin" => rgb(0xd1, 0x9a, 0x66),
        "function" | "function.builtin" | "function.method" | "constructor" => {
            rgb(0x61, 0xaf, 0xef)
        }
        "type" | "type.builtin" => rgb(0xe5, 0xc0, 0x7b),
        "property" | "variable.member" | "tag" => rgb(0xe0, 0x6c, 0x75),
        "attribute" => rgb(0xd1, 0x9a, 0x66),
        "variable.builtin" => rgb(0xe0, 0x6c, 0x75),
        "module" => rgb(0xe5, 0xc0, 0x7b),
        "operator" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" => {
            rgb(0xab, 0xb2, 0xbf)
        }
        // "variable", "variable.parameter", and anything else → default fg.
        _ => Style::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_language_by_extension() {
        let hl = Highlighter::new();
        assert!(hl.lang_for("src/main.rs").is_some());
        assert!(hl.lang_for("a/b/Component.tsx").is_some());
        assert!(hl.lang_for("x.ts").is_some());
        assert!(hl.lang_for("app/models/user.rb").is_some());
        assert!(hl.lang_for("data.json").is_some());
        assert!(hl.lang_for("README.unknownext").is_none());
    }

    #[test]
    fn line_count_matches_split() {
        let hl = Highlighter::new();
        let src = "fn main() {\n    let x = 1;\n}\n";
        let lines = hl.highlight("x.rs", src);
        // Trailing newline yields a final empty line, matching split('\n').
        assert_eq!(lines.len(), src.split('\n').count());
        // Each line's segments concatenate back to the original line text.
        for (segs, original) in lines.iter().zip(src.split('\n')) {
            let joined: String = segs.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(joined, original);
        }
    }

    #[test]
    fn unknown_language_is_plain_but_intact() {
        let hl = Highlighter::new();
        let src = "alpha\nbeta";
        let lines = hl.highlight("x.unknownext", src);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][0].text, "alpha");
    }
}
