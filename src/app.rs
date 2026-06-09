//! Application state and input handling (no rendering — see [`crate::ui`]).

use crate::conflict::{Column, ParsedFile};
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

/// State for one conflicted file.
pub struct FileState {
    pub entry: ConflictEntry,
    pub parsed: ParsedFile,
    /// Per-region chosen column index (`None` = unresolved).
    pub resolutions: Vec<Option<usize>>,
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

pub struct App {
    pub jj: Jj,
    pub files: Vec<FileState>,
    pub selected_file: usize,
    /// Current conflict region within the selected file.
    pub region: usize,
    /// Focused comparison column within the current region.
    pub column: usize,
    /// Synced vertical scroll across columns.
    pub vscroll: u16,
    pub status: String,
    pub show_help: bool,
    pub show_preview: bool,
    /// True after a lone `g`, awaiting a second `g` (vim `gg`).
    pub pending_g: bool,
}

impl App {
    pub fn new(jj: Jj) -> Result<Self> {
        let mut app = App {
            jj,
            files: Vec::new(),
            selected_file: 0,
            region: 0,
            column: 0,
            vscroll: 0,
            status: String::new(),
            show_help: false,
            show_preview: false,
            pending_g: false,
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
            self.selected_file = self
                .files
                .iter()
                .position(|f| f.entry.path == p)
                .unwrap_or(0);
        }
        self.clamp();
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

    /// Columns for the current region (cloned; cheap enough for a TUI).
    pub fn columns(&self) -> Vec<Column> {
        self.file()
            .and_then(|f| f.parsed.region(self.region))
            .map(|r| r.columns())
            .unwrap_or_default()
    }

    /// Chosen column index for the current region, if resolved.
    pub fn current_choice(&self) -> Option<usize> {
        self.file()
            .and_then(|f| f.resolutions.get(self.region).copied().flatten())
    }

    fn content_height(&self) -> usize {
        self.columns()
            .iter()
            .map(|c| c.content.len())
            .max()
            .unwrap_or(0)
    }

    fn max_scroll(&self) -> u16 {
        let h = self.content_height();
        if h == 0 { 0 } else { (h.saturating_sub(1)) as u16 }
    }

    /// Keep all indices within bounds after any state change.
    fn clamp(&mut self) {
        if self.files.is_empty() {
            self.selected_file = 0;
            self.region = 0;
            self.column = 0;
            self.vscroll = 0;
            return;
        }
        if self.selected_file >= self.files.len() {
            self.selected_file = self.files.len() - 1;
        }
        let regions = self.file().map(|f| f.parsed.region_count()).unwrap_or(0);
        self.region = if regions == 0 { 0 } else { self.region.min(regions - 1) };
        let cols = self.columns().len();
        self.column = if cols == 0 { 0 } else { self.column.min(cols - 1) };
        self.vscroll = self.vscroll.min(self.max_scroll());
    }

    // ---- navigation ------------------------------------------------------

    fn next_file(&mut self) {
        if self.selected_file + 1 < self.files.len() {
            self.selected_file += 1;
            self.region = 0;
            self.column = 0;
            self.vscroll = 0;
        }
        self.clamp();
    }

    fn prev_file(&mut self) {
        if self.selected_file > 0 {
            self.selected_file -= 1;
            self.region = 0;
            self.column = 0;
            self.vscroll = 0;
        }
        self.clamp();
    }

    fn next_region(&mut self) {
        let total = self.file().map(|f| f.parsed.region_count()).unwrap_or(0);
        if self.region + 1 < total {
            self.region += 1;
            self.vscroll = 0;
        }
        self.clamp();
    }

    fn prev_region(&mut self) {
        if self.region > 0 {
            self.region -= 1;
            self.vscroll = 0;
        }
        self.clamp();
    }

    fn next_column(&mut self) {
        let n = self.columns().len();
        if n > 0 {
            self.column = (self.column + 1) % n;
        }
    }

    fn prev_column(&mut self) {
        let n = self.columns().len();
        if n > 0 {
            self.column = (self.column + n - 1) % n;
        }
    }

    fn scroll_down(&mut self, n: u16) {
        self.vscroll = self.vscroll.saturating_add(n).min(self.max_scroll());
    }

    fn scroll_up(&mut self, n: u16) {
        self.vscroll = self.vscroll.saturating_sub(n);
    }

    fn top(&mut self) {
        self.vscroll = 0;
    }

    fn bottom(&mut self) {
        self.vscroll = self.max_scroll();
    }

    // ---- resolution ------------------------------------------------------

    fn pick(&mut self) {
        let n = self.columns().len();
        if n == 0 {
            return;
        }
        let col = self.column.min(n - 1);
        let region = self.region;
        let title = self.columns().get(col).map(|c| c.title()).unwrap_or_default();
        if let Some(f) = self.files.get_mut(self.selected_file) {
            if region < f.resolutions.len() {
                f.resolutions[region] = Some(col);
            }
        }
        self.status = format!("Region {} → {}", region + 1, title);
        self.goto_next_unresolved();
    }

    fn pick_n(&mut self, i: usize) {
        if i < self.columns().len() {
            self.column = i;
            self.pick();
        }
    }

    fn unpick(&mut self) {
        let region = self.region;
        if let Some(f) = self.files.get_mut(self.selected_file) {
            if region < f.resolutions.len() {
                f.resolutions[region] = None;
            }
        }
        self.status = format!("Region {} unresolved", region + 1);
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
                    .find(|&idx| f.resolutions.get(idx).copied().flatten().is_none())
            }
        };
        if let Some(idx) = target {
            self.region = idx;
            self.vscroll = 0;
        }
        self.clamp();
    }

    fn write(&mut self) {
        let Some(f) = self.file() else { return };
        let path = f.entry.path.clone();
        if !f.has_staged() {
            self.status = "Nothing staged — pick a side first (⏎ or 1-9)".to_string();
            return;
        }
        let content = f.parsed.render(&f.resolutions);
        let fully = f.is_fully_resolved();
        if let Err(e) = self.jj.write_resolution(&path, &content) {
            self.status = format!("Write failed: {e}");
            return;
        }
        self.reload();
        // `reload` may overwrite status with the "all resolved" banner.
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

        // Overlays swallow input.
        if self.show_help {
            self.show_help = false;
            return Action::None;
        }
        if self.show_preview {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.scroll_down(1),
                KeyCode::Char('k') | KeyCode::Up => self.scroll_up(1),
                _ => self.show_preview = false,
            }
            return Action::None;
        }

        // `gg` chord.
        if self.pending_g {
            self.pending_g = false;
            if let KeyCode::Char('g') = key.code {
                self.top();
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
            KeyCode::Char('G') => self.bottom(),

            KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => self.prev_column(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => self.next_column(),

            KeyCode::Char('n') => self.next_region(),
            KeyCode::Char('N') => self.prev_region(),
            KeyCode::Char(']') | KeyCode::Char('J') => self.next_file(),
            KeyCode::Char('[') | KeyCode::Char('K') => self.prev_file(),

            KeyCode::Enter => self.pick(),
            KeyCode::Char(c @ '1'..='9') => self.pick_n(c as usize - '1' as usize),
            KeyCode::Char('u') => self.unpick(),
            KeyCode::Char('w') => self.write(),
            KeyCode::Char('e') => return Action::OpenEditor,

            KeyCode::Char('p') => {
                self.show_preview = true;
                self.vscroll = 0;
            }
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
