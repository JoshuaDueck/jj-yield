//! Application state and input handling (no rendering — see [`crate::ui`]).
//!
//! The merge-editor view is derived state: whenever the file, region, or a
//! resolution changes, [`App::rebuild_view`] recomputes the per-side diffs +
//! syntax highlighting and the assembled Result pane, so rendering stays cheap.

use crate::conflict::{Accept, ParsedFile, SideLabel, TermKind};
use crate::diff::{self, DiffLine};
use crate::highlight::{Highlighter, Seg};
use crate::jj::{ConflictEntry, Jj};
use crate::parser;
use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What the main loop should do after handling a key.
pub enum Action {
    None,
    OpenEditor,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidesLayout {
    SideBySide,
    Tabbed,
}

/// State for one conflicted file.
pub struct FileState {
    pub entry: ConflictEntry,
    pub parsed: ParsedFile,
    /// Per-region resolution (`None` = unresolved).
    pub resolutions: Vec<Option<Accept>>,
}

impl FileState {
    pub fn is_fully_resolved(&self) -> bool {
        self.parsed.fully_resolved(&self.resolutions)
    }
    pub fn has_staged(&self) -> bool {
        self.resolutions.iter().any(|r| r.is_some())
    }
    pub fn resolved_count(&self) -> usize {
        self.resolutions.iter().filter(|r| r.is_some()).count()
    }
}

/// One conflict side as shown in the top panes: its diff vs base + syntax.
pub struct SideView {
    pub label: SideLabel,
    pub kind: TermKind,
    /// Index into the region's `columns()` (what `Accept::Side` refers to).
    pub col_index: usize,
    pub diff: Vec<DiffLine>,
    /// Syntax-highlighted side content, indexed by `DiffLine::side_line`.
    pub hl: Vec<Vec<Seg>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    Context,
    Resolved,
    Marker,
}

pub struct ResultLine {
    pub kind: ResultKind,
    pub region: Option<usize>,
    pub segs: Vec<Seg>,
}

#[derive(Default)]
pub struct ResultView {
    pub lines: Vec<ResultLine>,
    /// Result-line index where each conflict region's block begins.
    pub region_start: Vec<usize>,
}

pub struct App {
    pub jj: Jj,
    pub highlighter: Highlighter,
    pub files: Vec<FileState>,
    pub selected_file: usize,
    pub region: usize,
    pub focused_side: usize,
    pub vscroll: u16,
    pub sides_layout: SidesLayout,
    pub status: String,
    pub show_help: bool,
    pub pending_g: bool,

    // Derived view, rebuilt on change.
    pub sides: Vec<SideView>,
    pub result: ResultView,
    pub result_scroll: u16,
}

impl App {
    pub fn new(jj: Jj) -> Result<Self> {
        let mut app = App {
            jj,
            highlighter: Highlighter::new(),
            files: Vec::new(),
            selected_file: 0,
            region: 0,
            focused_side: 0,
            vscroll: 0,
            sides_layout: SidesLayout::SideBySide,
            status: String::new(),
            show_help: false,
            pending_g: false,
            sides: Vec::new(),
            result: ResultView::default(),
            result_scroll: 0,
        };
        app.load()?;
        Ok(app)
    }

    /// (Re)load the conflict list and materialize each file, preserving the
    /// current selection by path where possible.
    pub fn load(&mut self) -> Result<()> {
        let prev_path = self.current_path();
        let entries = self.jj.list_conflicts()?;
        let mut files = Vec::with_capacity(entries.len());
        for entry in entries {
            let text = self.jj.materialize(&entry.path)?;
            let parsed = parser::parse(&text);
            let resolutions = vec![None; parsed.region_count()];
            files.push(FileState { entry, parsed, resolutions });
        }
        self.files = files;
        if let Some(p) = prev_path {
            self.selected_file = self.files.iter().position(|f| f.entry.path == p).unwrap_or(0);
        }
        self.clamp_selection();
        self.rebuild_view();
        Ok(())
    }

    /// Reload, surfacing any error in the status line instead of propagating.
    pub fn reload(&mut self) {
        if let Err(e) = self.load() {
            self.status = format!("Reload error: {e}");
        } else if self.files.is_empty() {
            self.status = "All conflicts resolved 🎉  (press q to quit)".to_string();
        }
    }

    // ---- accessors -------------------------------------------------------

    pub fn file(&self) -> Option<&FileState> {
        self.files.get(self.selected_file)
    }

    pub fn current_path(&self) -> Option<String> {
        self.file().map(|f| f.entry.path.clone())
    }

    pub fn region_count(&self) -> usize {
        self.file().map(|f| f.parsed.region_count()).unwrap_or(0)
    }

    /// The resolution chosen for the current region, if any.
    pub fn region_accept(&self) -> Option<&Accept> {
        self.file()
            .and_then(|f| f.resolutions.get(self.region))
            .and_then(|o| o.as_ref())
    }

    fn max_side_scroll(&self) -> u16 {
        let h = self.sides.iter().map(|s| s.diff.len()).max().unwrap_or(0);
        if h == 0 { 0 } else { (h.saturating_sub(1)) as u16 }
    }

    // ---- view derivation -------------------------------------------------

    /// Recompute the cached side panes + Result pane for the current selection.
    fn rebuild_view(&mut self) {
        if self.files.is_empty() {
            self.sides.clear();
            self.result = ResultView::default();
            self.result_scroll = 0;
            self.focused_side = 0;
            return;
        }
        let sel = self.selected_file.min(self.files.len() - 1);
        let (sides, result) = {
            let file = &self.files[sel];
            let path = file.entry.path.as_str();

            // Result pane: assemble lines + per-line metadata, then highlight.
            let (texts, meta, region_start) = build_result(&file.parsed, &file.resolutions);
            let joined = texts.join("\n");
            let hl = self.highlighter.highlight(path, &joined);
            let lines: Vec<ResultLine> = meta
                .into_iter()
                .enumerate()
                .map(|(i, (kind, region))| ResultLine {
                    kind,
                    region,
                    segs: hl.get(i).cloned().unwrap_or_default(),
                })
                .collect();

            // Side panes: diff each column vs base, highlight its content.
            let mut sides = Vec::new();
            if let Some(region) = file.parsed.region(self.region) {
                let cols = region.columns();
                let base = cols
                    .iter()
                    .find(|c| c.kind == TermKind::Remove)
                    .map(|c| c.content.clone())
                    .unwrap_or_default();
                for (ci, col) in cols.iter().enumerate() {
                    let d = diff::diff_side(&base, &col.content);
                    let shl = self.highlighter.highlight(path, &col.content.join("\n"));
                    sides.push(SideView {
                        label: col.label.clone(),
                        kind: col.kind,
                        col_index: ci,
                        diff: d,
                        hl: shl,
                    });
                }
            }
            (sides, ResultView { lines, region_start })
        };

        let region_line = result.region_start.get(self.region).copied().unwrap_or(0);
        self.sides = sides;
        self.result = result;
        self.result_scroll = (region_line.saturating_sub(2)) as u16;
        self.focused_side = if self.sides.is_empty() {
            0
        } else {
            self.focused_side.min(self.sides.len() - 1)
        };
        self.vscroll = self.vscroll.min(self.max_side_scroll());
    }

    fn clamp_selection(&mut self) {
        if self.files.is_empty() {
            self.selected_file = 0;
            self.region = 0;
            return;
        }
        if self.selected_file >= self.files.len() {
            self.selected_file = self.files.len() - 1;
        }
        let regions = self.region_count();
        self.region = if regions == 0 { 0 } else { self.region.min(regions - 1) };
    }

    // ---- navigation ------------------------------------------------------

    fn next_file(&mut self) {
        if self.selected_file + 1 < self.files.len() {
            self.selected_file += 1;
            self.region = 0;
            self.focused_side = 0;
            self.vscroll = 0;
            self.rebuild_view();
        }
    }

    fn prev_file(&mut self) {
        if self.selected_file > 0 {
            self.selected_file -= 1;
            self.region = 0;
            self.focused_side = 0;
            self.vscroll = 0;
            self.rebuild_view();
        }
    }

    fn next_region(&mut self) {
        if self.region + 1 < self.region_count() {
            self.region += 1;
            self.vscroll = 0;
            self.rebuild_view();
        }
    }

    fn prev_region(&mut self) {
        if self.region > 0 {
            self.region -= 1;
            self.vscroll = 0;
            self.rebuild_view();
        }
    }

    fn next_side(&mut self) {
        if !self.sides.is_empty() {
            self.focused_side = (self.focused_side + 1) % self.sides.len();
        }
    }

    fn prev_side(&mut self) {
        if !self.sides.is_empty() {
            let n = self.sides.len();
            self.focused_side = (self.focused_side + n - 1) % n;
        }
    }

    fn scroll_down(&mut self, n: u16) {
        self.vscroll = self.vscroll.saturating_add(n).min(self.max_side_scroll());
    }
    fn scroll_up(&mut self, n: u16) {
        self.vscroll = self.vscroll.saturating_sub(n);
    }

    fn toggle_layout(&mut self) {
        self.sides_layout = match self.sides_layout {
            SidesLayout::SideBySide => SidesLayout::Tabbed,
            SidesLayout::Tabbed => SidesLayout::SideBySide,
        };
    }

    // ---- resolution ------------------------------------------------------

    fn set_accept(&mut self, accept: Option<Accept>) {
        let region = self.region;
        if let Some(f) = self.files.get_mut(self.selected_file) {
            if region < f.resolutions.len() {
                f.resolutions[region] = accept;
            }
        }
    }

    /// Accept the column at side-pane index `i` (focused side, or numbered key).
    fn accept_side_at(&mut self, i: usize) {
        let Some(side) = self.sides.get(i) else { return };
        let col = side.col_index;
        let title = side_title(side);
        self.focused_side = i;
        self.set_accept(Some(Accept::Side(col)));
        self.status = format!("Region {} → {}", self.region + 1, title);
        self.after_accept();
    }

    fn accept_both(&mut self) {
        let adds: Vec<usize> = self
            .sides
            .iter()
            .filter(|s| s.kind == TermKind::Add)
            .map(|s| s.col_index)
            .collect();
        if adds.len() < 2 {
            self.status = "Accept-both needs ≥2 sides".to_string();
            return;
        }
        self.set_accept(Some(Accept::Both(adds)));
        self.status = format!("Region {} → both sides", self.region + 1);
        self.after_accept();
    }

    fn accept_base(&mut self) {
        let base = self.sides.iter().find(|s| s.kind == TermKind::Remove).map(|s| s.col_index);
        match base {
            Some(col) => {
                self.set_accept(Some(Accept::Side(col)));
                self.status = format!("Region {} → base", self.region + 1);
                self.after_accept();
            }
            None => self.status = "No base in this conflict".to_string(),
        }
    }

    fn unresolve(&mut self) {
        self.set_accept(None);
        self.status = format!("Region {} unresolved", self.region + 1);
        self.rebuild_view();
    }

    fn after_accept(&mut self) {
        self.goto_next_unresolved();
        self.rebuild_view();
    }

    fn goto_next_unresolved(&mut self) {
        let target = {
            let Some(f) = self.file() else { return };
            let total = f.parsed.region_count();
            if total == 0 {
                None
            } else {
                (1..=total)
                    .map(|off| (self.region + off) % total)
                    .find(|&idx| f.resolutions.get(idx).map(Option::is_none).unwrap_or(true))
            }
        };
        if let Some(idx) = target {
            self.region = idx;
            self.vscroll = 0;
        }
    }

    fn write(&mut self) {
        let Some(f) = self.file() else { return };
        if !f.has_staged() {
            self.status = "Nothing accepted yet (⏎ / 1-9 / a / b)".to_string();
            return;
        }
        let path = f.entry.path.clone();
        let content = f.parsed.render(&f.resolutions);
        let fully = f.is_fully_resolved();
        if let Err(e) = self.jj.write_resolution(&path, &content) {
            self.status = format!("Write failed: {e}");
            return;
        }
        self.reload();
        if !self.files.is_empty() {
            self.status = if fully {
                format!("Wrote {path} (fully resolved)")
            } else {
                format!("Wrote {path} (partial — markers remain)")
            };
        }
    }

    // ---- input -----------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if self.show_help {
            self.show_help = false;
            return Action::None;
        }
        if self.pending_g {
            self.pending_g = false;
            if let KeyCode::Char('g') = key.code {
                self.vscroll = 0;
                return Action::None;
            }
        }

        match key.code {
            KeyCode::Char('c') if ctrl => return Action::Quit,
            KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,

            KeyCode::Char('d') if ctrl => self.scroll_down(10),
            KeyCode::Char('u') if ctrl => self.scroll_up(10),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_down(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_up(1),
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') => self.vscroll = self.max_side_scroll(),

            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => self.next_side(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => self.prev_side(),

            KeyCode::Char('n') => self.next_region(),
            KeyCode::Char('N') => self.prev_region(),
            KeyCode::Char(']') | KeyCode::Char('J') => self.next_file(),
            KeyCode::Char('[') | KeyCode::Char('K') => self.prev_file(),

            KeyCode::Enter => self.accept_side_at(self.focused_side),
            KeyCode::Char(c @ '1'..='9') => self.accept_side_at(c as usize - '1' as usize),
            KeyCode::Char('a') => self.accept_both(),
            KeyCode::Char('b') => self.accept_base(),
            KeyCode::Char('u') => self.unresolve(),

            KeyCode::Char('m') => self.toggle_layout(),
            KeyCode::Char('w') => self.write(),
            KeyCode::Char('e') => return Action::OpenEditor,
            KeyCode::Char('r') => {
                self.reload();
                if !self.files.is_empty() {
                    self.status = "Refreshed".to_string();
                }
            }
            KeyCode::Char('?') => self.show_help = true,

            _ => {}
        }
        Action::None
    }
}

/// Display title for a side pane.
pub fn side_title(side: &SideView) -> String {
    match side.kind {
        TermKind::Add => side.label.short(),
        TermKind::Remove => {
            if side.label.change_id.is_empty() {
                "base".to_string()
            } else {
                format!("base ({})", side.label.change_id)
            }
        }
    }
}

/// Assemble the Result pane: the file lines, a `(kind, region)` per line, and the
/// starting line index of each conflict region's block.
fn build_result(
    parsed: &ParsedFile,
    resolutions: &[Option<Accept>],
) -> (Vec<String>, Vec<(ResultKind, Option<usize>)>, Vec<usize>) {
    use crate::conflict::Segment;
    let mut texts = Vec::new();
    let mut meta = Vec::new();
    let mut region_start = Vec::new();
    let mut ridx = 0;

    for seg in &parsed.segments {
        match seg {
            Segment::Context(lines) => {
                for l in lines {
                    texts.push(l.clone());
                    meta.push((ResultKind::Context, None));
                }
            }
            Segment::Conflict(region) => {
                region_start.push(texts.len());
                match resolutions.get(ridx).and_then(|o| o.as_ref()) {
                    Some(accept) => {
                        let cols = region.columns();
                        let idxs = match accept {
                            Accept::Side(i) => vec![*i],
                            Accept::Both(v) => v.clone(),
                        };
                        let mut any = false;
                        for i in idxs {
                            if let Some(col) = cols.get(i) {
                                for l in &col.content {
                                    texts.push(l.clone());
                                    meta.push((ResultKind::Resolved, Some(ridx)));
                                }
                                any = true;
                            }
                        }
                        if !any {
                            for l in &region.raw {
                                texts.push(l.clone());
                                meta.push((ResultKind::Marker, Some(ridx)));
                            }
                        }
                    }
                    None => {
                        for l in &region.raw {
                            texts.push(l.clone());
                            meta.push((ResultKind::Marker, Some(ridx)));
                        }
                    }
                }
                ridx += 1;
            }
        }
    }
    (texts, meta, region_start)
}
