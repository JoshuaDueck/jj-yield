//! `ratatui` rendering for the merge-editor view. Pure view over
//! [`crate::app::App`] — no mutation.

use crate::app::{side_title, App, FileListLayout, ResultKind, ResultLine, SideView, SidesLayout};
use crate::conflict::{Accept, TermKind};
use crate::diff::{DiffTag, Word};
use crate::highlight::Seg;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

const HINTS: &str = "j/k scroll · h/l/Tab side · ⏎/1-9 accept · a both · b base · u undo · n/N hunk · ]/[ file · m layout · w write · e edit · ? help · q quit";

const ADD_PALETTE: &[Color] = &[Color::Green, Color::Blue, Color::Magenta, Color::Yellow];
const INSERT_BG: Color = Color::Rgb(24, 44, 30);
const INSERT_EMPH_BG: Color = Color::Rgb(40, 84, 50);
const DELETE_FG: Color = Color::Rgb(0xc0, 0x60, 0x68);
const DELETE_BG: Color = Color::Rgb(46, 26, 30);

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let file_list_constraint = match app.file_list_layout {
        FileListLayout::Full => Constraint::Percentage(22),
        FileListLayout::Short => Constraint::Percentage(15),
        FileListLayout::Mini => Constraint::Percentage(5),
        FileListLayout::None => Constraint::Percentage(0),
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([file_list_constraint, Constraint::Min(30)])
        .split(area);

    draw_file_list(f, app, cols[0]);

    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Percentage(55),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(cols[1]);
    draw_header(f, app, main[0]);
    draw_sides(f, app, main[1]);
    draw_result(f, app, main[2]);
    draw_footer(f, app, main[3]);

    if app.show_help {
        draw_help(f, area);
    }
}

fn draw_file_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|fs| {
            let (mark, color) = if fs.is_fully_resolved() {
                ("✔ ", Color::Green)
            } else if fs.has_staged() {
                ("• ", Color::Yellow)
            } else {
                ("  ", Color::DarkGray)
            };
            let sides = fs
                .entry
                .sides
                .map(|n| format!("  [{n}]"))
                .unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(mark, Style::default().fg(color)),
                Span::raw(fs.entry.path.clone()),
                Span::styled(sides, Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Conflicts "))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut state = ListState::default();
    if !app.files.is_empty() {
        state.select(Some(app.selected_file));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let (title, info) = match app.file() {
        Some(fs) => {
            let total = fs.parsed.region_count();
            let info = format!(
                "conflict {}/{}   {} sides   {}/{} resolved",
                app.region + 1,
                total.max(1),
                app.sides.iter().filter(|s| s.kind == TermKind::Add).count(),
                fs.resolved_count(),
                total
            );
            (format!(" {} ", fs.entry.path), info)
        }
        None => (" jj-yield ".to_string(), "no conflicts".to_string()),
    };
    let p = Paragraph::new(Line::from(Span::styled(
        info,
        Style::default().fg(Color::Gray),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Line::from(Span::styled(
                title,
                Style::default().add_modifier(Modifier::BOLD),
            ))),
    );
    f.render_widget(p, area);
}

fn accents(app: &App) -> Vec<Color> {
    let mut out = Vec::with_capacity(app.sides.len());
    let mut add_rank = 0;
    for s in &app.sides {
        if s.kind == TermKind::Remove {
            out.push(Color::Cyan);
        } else {
            out.push(ADD_PALETTE[add_rank % ADD_PALETTE.len()]);
            add_rank += 1;
        }
    }
    out
}

fn is_chosen(app: &App, col_index: usize) -> bool {
    match app.region_accept() {
        Some(Accept::Side(i)) => *i == col_index,
        Some(Accept::Both(v)) => v.contains(&col_index),
        None => false,
    }
}

fn draw_sides(f: &mut Frame, app: &App, area: Rect) {
    if app.sides.is_empty() {
        let msg = if app.files.is_empty() {
            "No conflicts. Press q to quit."
        } else {
            "No conflict region in this file."
        };
        f.render_widget(
            Paragraph::new(msg)
                .block(Block::default().borders(Borders::ALL))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let accent = accents(app);
    match app.sides_layout {
        SidesLayout::SideBySide => {
            let n = app.sides.len();
            let constraints: Vec<Constraint> =
                (0..n).map(|_| Constraint::Ratio(1, n as u32)).collect();
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(area);
            for (i, side) in app.sides.iter().enumerate() {
                draw_side_pane(f, app, side, i, accent[i], chunks[i]);
            }
        }
        SidesLayout::Tabbed => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(area);
            // Tab bar.
            let mut spans: Vec<Span> = Vec::new();
            for (i, side) in app.sides.iter().enumerate() {
                let mut st = Style::default().fg(accent[i]);
                if i == app.focused_side {
                    st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                }
                let chosen = if is_chosen(app, side.col_index) {
                    "✓"
                } else {
                    ""
                };
                spans.push(Span::styled(
                    format!(" {}{} {} ", i + 1, chosen, side_title(side)),
                    st,
                ));
                spans.push(Span::raw(" "));
            }
            f.render_widget(Paragraph::new(Line::from(spans)), rows[0]);
            let side = &app.sides[app.focused_side];
            draw_side_pane(
                f,
                app,
                side,
                app.focused_side,
                accent[app.focused_side],
                rows[1],
            );
        }
    }
}

fn draw_side_pane(
    f: &mut Frame,
    app: &App,
    side: &SideView,
    idx: usize,
    accent: Color,
    area: Rect,
) {
    let focused = idx == app.focused_side;
    let chosen = is_chosen(app, side.col_index);
    let border_style = if focused {
        Style::default().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut title = format!(" {}", idx + 1);
    if chosen {
        title.push_str(" ✓");
    }
    title.push(' ');
    title.push_str(&side_title(side));
    title.push(' ');

    let lines = side_body_lines(side);
    let p = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(Line::from(Span::styled(title, border_style.fg(accent)))),
        )
        .scroll((app.vscroll, 0));
    f.render_widget(p, area);
}

fn side_body_lines(side: &SideView) -> Vec<Line<'static>> {
    let mut out = Vec::with_capacity(side.diff.len());
    for dl in &side.diff {
        match dl.tag {
            DiffTag::Delete => {
                let style = Style::default().fg(DELETE_FG).bg(DELETE_BG);
                out.push(Line::from(vec![
                    Span::styled("-", style.add_modifier(Modifier::DIM)),
                    Span::styled(format!(" {}", dl.text), style),
                ]));
            }
            DiffTag::Insert | DiffTag::Equal => {
                let segs = dl
                    .side_line
                    .and_then(|i| side.hl.get(i))
                    .map(|v| v.as_slice())
                    .unwrap_or(&[]);
                let line_bg = if dl.tag == DiffTag::Insert {
                    Some(INSERT_BG)
                } else {
                    None
                };
                out.push(styled_diff_line(
                    &dl.text,
                    segs,
                    &dl.words,
                    line_bg,
                    Some(INSERT_EMPH_BG),
                ));
            }
        }
    }
    out
}

/// Overlay syntax foreground (`segs`) with diff background (`line_bg`) and
/// intra-line emphasis (`words` + `emph_bg`) by building a per-character style
/// array and coalescing equal-adjacent runs into spans.
fn styled_diff_line(
    text: &str,
    segs: &[Seg],
    words: &[Word],
    line_bg: Option<Color>,
    emph_bg: Option<Color>,
) -> Line<'static> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return match line_bg {
            Some(bg) => Line::from(Span::styled(" ", Style::default().bg(bg))),
            None => Line::from(String::new()),
        };
    }

    let mut fg = vec![Style::default(); chars.len()];
    let mut i = 0;
    for seg in segs {
        for _ in seg.text.chars() {
            if i < fg.len() {
                fg[i] = seg.style;
            }
            i += 1;
        }
    }
    let mut emph = vec![false; chars.len()];
    i = 0;
    for w in words {
        for _ in w.text.chars() {
            if i < emph.len() {
                emph[i] = w.emphasized;
            }
            i += 1;
        }
    }

    let mut spans: Vec<Span> = Vec::new();
    let mut cur = String::new();
    let mut cur_style: Option<Style> = None;
    for (idx, ch) in chars.iter().enumerate() {
        let mut st = fg[idx];
        if emph[idx] {
            if let Some(bg) = emph_bg {
                st = st.bg(bg);
            }
            st = st.add_modifier(Modifier::BOLD);
        } else if let Some(bg) = line_bg {
            st = st.bg(bg);
        }
        if cur_style != Some(st) {
            if let Some(ps) = cur_style {
                if !cur.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut cur), ps));
                }
            }
            cur_style = Some(st);
        }
        cur.push(*ch);
    }
    if let (Some(ps), false) = (cur_style, cur.is_empty()) {
        spans.push(Span::styled(cur, ps));
    }
    Line::from(spans)
}

fn draw_result(f: &mut Frame, app: &App, area: Rect) {
    let lines: Vec<Line> = app
        .result
        .lines
        .iter()
        .map(|rl| result_line(rl, rl.region == Some(app.region)))
        .collect();
    let p = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Line::from(Span::styled(
                    " Result ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ))),
        )
        .scroll((app.result_scroll, 0));
    f.render_widget(p, area);
}

fn result_line(rl: &ResultLine, is_current: bool) -> Line<'static> {
    let gutter = if is_current {
        Span::styled("▌ ", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("  ")
    };
    let mut spans = vec![gutter];
    match rl.kind {
        ResultKind::Marker => {
            let text: String = rl.segs.iter().map(|s| s.text.as_str()).collect();
            spans.push(Span::styled(
                text,
                Style::default().fg(DELETE_FG).add_modifier(Modifier::DIM),
            ));
        }
        ResultKind::Context | ResultKind::Resolved => {
            for seg in &rl.segs {
                spans.push(Span::styled(seg.text.clone(), seg.style));
            }
        }
    }
    Line::from(spans)
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let line = if app.status.is_empty() {
        Line::from(Span::styled(HINTS, Style::default().fg(Color::DarkGray)))
    } else {
        Line::from(vec![
            Span::styled(
                app.status.as_str(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("   ·   ", Style::default().fg(Color::DarkGray)),
            Span::styled(HINTS, Style::default().fg(Color::DarkGray)),
        ])
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(66, 84, area);
    f.render_widget(Clear, popup);

    let rows: &[(&str, &str)] = &[
        ("Movement", ""),
        ("  j / k, ↑ / ↓", "scroll the side panes"),
        ("  Ctrl-d / Ctrl-u", "half-page scroll"),
        ("  gg / G", "top / bottom"),
        ("Compare", ""),
        ("  h / l, Tab / S-Tab", "focus previous / next side"),
        ("  m", "toggle side-by-side ↔ tabbed"),
        ("  n / N", "next / previous conflict region"),
        ("  ] / [  (or J / K)", "next / previous file"),
        ("Resolve (updates Result pane)", ""),
        ("  ⏎", "accept the focused side"),
        ("  1 - 9", "accept side by number"),
        ("  a", "accept both sides (in order)"),
        ("  b", "accept the base"),
        ("  u", "un-resolve this region"),
        ("  w", "write resolution(s) to disk"),
        ("  e", "open the file in $EDITOR"),
        ("  r", "refresh from jj"),
        ("Other", ""),
        ("  ?", "toggle this help"),
        ("  q / Esc / Ctrl-c", "quit"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, v)| {
            if v.is_empty() {
                Line::from(Span::styled(
                    *k,
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(vec![
                    Span::styled(format!("{k:<24}"), Style::default().fg(Color::Yellow)),
                    Span::raw(*v),
                ])
            }
        })
        .collect();
    let p = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(" jj-yield — keybindings "),
    );
    f.render_widget(p, popup);
}

fn centered_rect(px: u16, py: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - py) / 2),
            Constraint::Percentage(py),
            Constraint::Percentage((100 - py) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - px) / 2),
            Constraint::Percentage(px),
            Constraint::Percentage((100 - px) / 2),
        ])
        .split(vertical[1])[1]
}
