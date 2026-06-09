//! Data model for a materialized Jujutsu conflict in *snapshot* marker style.
//!
//! Snapshot markers look like this (a 3-sided conflict):
//!
//! ```text
//! line before
//! <<<<<<< conflict 1 of 1
//! +++++++ rpulyxsl 46d0a2ab "sideA"
//! AAA
//! ------- yooymqps b6f03a57 "base"
//! shared
//! +++++++ mvmzmyyn 991eceae "sideB"
//! BBB
//! ------- yooymqps b6f03a57 "base"
//! shared
//! +++++++ lwuprqku bb9b960b "sideC"
//! CCC
//! >>>>>>> conflict 1 of 1 ends
//! line after
//! ```
//!
//! Each `+++++++` term is a present side ([`TermKind::Add`]); each `-------`
//! term is a merge base ([`TermKind::Remove`]). A k-sided conflict has k `Add`
//! terms and k-1 `Remove` terms (the base, usually repeated).

/// The source label parsed from a marker header line, e.g.
/// `rpulyxsl 46d0a2ab "sideA"` (change id, short commit id, description).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SideLabel {
    pub change_id: String,
    pub commit_id: String,
    pub description: String,
    /// The full text after the marker run, verbatim (display fallback).
    pub raw: String,
}

impl SideLabel {
    /// Parse the text that follows a `+++++++`/`-------` marker run.
    pub fn parse(rest: &str) -> Self {
        let raw = rest.to_string();
        let mut parts = rest.splitn(3, ' ');
        let change_id = parts.next().unwrap_or("").to_string();
        let commit_id = parts.next().unwrap_or("").to_string();
        let description = match parts.next() {
            Some(d) => strip_quotes(d.trim()).to_string(),
            None => String::new(),
        };
        SideLabel { change_id, commit_id, description, raw }
    }

    /// A short, human label, e.g. `sideA (rpulyxsl)`.
    pub fn short(&self) -> String {
        match (self.description.is_empty(), self.change_id.is_empty()) {
            (false, false) => format!("{} ({})", self.description, self.change_id),
            (true, false) => self.change_id.clone(),
            (false, true) => self.description.clone(),
            (true, true) => self.raw.clone(),
        }
    }
}

fn strip_quotes(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(s)
}

/// Whether a term is a present side or a (removed) merge base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermKind {
    /// A present side of the conflict (`+++++++`).
    Add,
    /// A removed side, i.e. a merge base (`-------`).
    Remove,
}

/// One term of a conflict: a marker header plus its content lines.
#[derive(Debug, Clone)]
pub struct Term {
    pub kind: TermKind,
    pub label: SideLabel,
    pub content: Vec<String>,
}

/// A distinct, selectable side shown as a single comparison column. Identical
/// bases (the common k-sided case repeats the base) are collapsed into one.
#[derive(Debug, Clone)]
pub struct Column {
    pub kind: TermKind,
    pub label: SideLabel,
    pub content: Vec<String>,
}

impl Column {
    /// Title shown above the column.
    pub fn title(&self) -> String {
        match self.kind {
            TermKind::Add => self.label.short(),
            TermKind::Remove => {
                if self.label.change_id.is_empty() {
                    "base".to_string()
                } else {
                    format!("base ({})", self.label.change_id)
                }
            }
        }
    }
}

/// A single conflict region (one `<<<<<<< … >>>>>>>` block).
#[derive(Debug, Clone)]
pub struct ConflictRegion {
    /// `(n, m)` parsed from a `conflict n of m` header, if present.
    pub header: Option<(usize, usize)>,
    /// Length of the marker char run (>= 7) used by this region.
    pub marker_len: usize,
    pub terms: Vec<Term>,
    /// The exact original lines (including markers) for lossless re-emit.
    pub raw: Vec<String>,
}

impl ConflictRegion {
    /// Distinct selectable columns: every `Add` term plus each distinct base.
    pub fn columns(&self) -> Vec<Column> {
        let mut cols: Vec<Column> = Vec::new();
        for term in &self.terms {
            if term.kind == TermKind::Remove
                && cols
                    .iter()
                    .any(|c| c.kind == TermKind::Remove && c.content == term.content)
            {
                continue; // collapse identical bases
            }
            cols.push(Column {
                kind: term.kind,
                label: term.label.clone(),
                content: term.content.clone(),
            });
        }
        cols
    }

    /// Number of present sides (`+++++++` terms).
    pub fn sides(&self) -> usize {
        self.terms.iter().filter(|t| t.kind == TermKind::Add).count()
    }
}

/// A span of a parsed file: either plain context or a conflict region.
#[derive(Debug, Clone)]
pub enum Segment {
    Context(Vec<String>),
    Conflict(ConflictRegion),
}

/// How a conflict region has been resolved. Column indices refer to
/// [`ConflictRegion::columns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accept {
    /// Take a single side/base.
    Side(usize),
    /// Take several columns concatenated in the given order ("accept both").
    Both(Vec<usize>),
}

/// A fully parsed materialized file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub segments: Vec<Segment>,
    /// Whether the original text ended with a trailing newline.
    pub trailing_newline: bool,
}

impl ParsedFile {
    pub fn region_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|s| matches!(s, Segment::Conflict(_)))
            .count()
    }

    pub fn regions(&self) -> impl Iterator<Item = &ConflictRegion> {
        self.segments.iter().filter_map(|s| match s {
            Segment::Conflict(r) => Some(r),
            Segment::Context(_) => None,
        })
    }

    /// The `idx`-th conflict region (0-based), if any.
    pub fn region(&self, idx: usize) -> Option<&ConflictRegion> {
        self.regions().nth(idx)
    }

    /// Reassemble the file text. `resolutions[i]` is the [`Accept`] choice for
    /// region `i`; `None` re-emits the original conflict markers verbatim.
    pub fn render(&self, resolutions: &[Option<Accept>]) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut region_idx = 0;
        for seg in &self.segments {
            match seg {
                Segment::Context(lines) => out.extend(lines.iter().cloned()),
                Segment::Conflict(region) => {
                    match resolutions.get(region_idx).and_then(|o| o.as_ref()) {
                        Some(accept) => emit_accept(region, accept, &mut out),
                        None => out.extend(region.raw.iter().cloned()),
                    }
                    region_idx += 1;
                }
            }
        }
        let mut text = out.join("\n");
        if self.trailing_newline {
            text.push('\n');
        }
        text
    }

    /// True when every conflict region has a chosen resolution.
    pub fn fully_resolved(&self, resolutions: &[Option<Accept>]) -> bool {
        let total = self.region_count();
        total > 0 && (0..total).all(|i| resolutions.get(i).map(Option::is_some).unwrap_or(false))
    }
}

/// Append the content selected by `accept` for `region` to `out`. Falls back to
/// the raw markers if the choice references no valid columns.
fn emit_accept(region: &ConflictRegion, accept: &Accept, out: &mut Vec<String>) {
    let cols = region.columns();
    let chosen: Vec<usize> = match accept {
        Accept::Side(i) => vec![*i],
        Accept::Both(idxs) => idxs.clone(),
    };
    let mut emitted = false;
    for i in chosen {
        if let Some(col) = cols.get(i) {
            out.extend(col.content.iter().cloned());
            emitted = true;
        }
    }
    if !emitted {
        out.extend(region.raw.iter().cloned());
    }
}
