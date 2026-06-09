//! `ratatui` rendering. Pure view over [`crate::app::App`] — no mutation.

use crate::app::App;
use crate::conflict::TermKind;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

const HINTS: &str = "j/k scroll · h/l/Tab side · n/N hunk · ]/[ file · ⏎ pick · u unpick · w write · e edit · p preview · ? help · q quit";

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(24), Constraint::Min(24)])
        .split(area);

    draw_file_list(f, app, cols[0]);
    draw_main(f, app, cols[1]);

    if app.show_preview {
        draw_preview(f, app, area);
    }
    if app.show_help {
        draw_help(f, area);
    }
}

fn draw_file_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|fs| {
            let resolved = fs.is_fully_resolved();
            let (mark, mark_color) = if resolved {
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
            let path_style = if resolved {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(mark, Style::default().fg(mark_color)),
                Span::styled(fs.entry.path.clone(), path_style),
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

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(f, app, rows[0]);
    draw_columns(f, app, rows[1]);
    draw_footer(f, app, rows[2]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let (title, info) = match app.file() {
        Some(fs) => {
            let total = fs.parsed.region_count();
            let resolved = fs.resolved_count();
            let sides = fs.parsed.region(app.region).map(|r| r.sides()).unwrap_or(0);
            let info = format!(
                "conflict {}/{}   {} sides   {}/{} regions resolved",
                app.region + 1,
                total.max(1),
                sides,
                resolved,
                total
            );
            (format!(" {} ", fs.entry.path), info)
        }
        None => (
            " jj-yield ".to_string(),
            "no conflicts in the working copy".to_string(),
        ),
    };

    let p = Paragraph::new(Line::from(Span::styled(
        info,
        Style::default().fg(Color::Gray),
    )))
    .block(
        Block::default().borders(Borders::ALL).title(Line::from(
            Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
        )),
    );
    f.render_widget(p, area);
}

fn draw_columns(f: &mut Frame, app: &App, area: Rect) {
    let columns = app.columns();
    if columns.is_empty() {
        let msg = if app.files.is_empty() {
            "No conflicts. Press q to quit."
        } else {
            "No conflict region in this file."
        };
        let p = Paragraph::new(msg)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: true });
        f.render_widget(p, area);
        return;
    }

    let n = columns.len();
    let constraints: Vec<Constraint> = (0..n).map(|_| Constraint::Ratio(1, n as u32)).collect();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    let chosen = app.current_choice();

    for (i, col) in columns.iter().enumerate() {
        let focused = i == app.column;
        let is_chosen = chosen == Some(i);
        let is_base = col.kind == TermKind::Remove;

        let border_style = if is_chosen {
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else if focused {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mut title = String::new();
        title.push_str(&format!("{}. ", i + 1));
        if is_chosen {
            title.push_str("✓ ");
        }
        title.push_str(&col.title());

        let content_style = if is_base {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        let text: Text = if col.content.is_empty() {
            Text::from(Line::from(Span::styled(
                "(empty — side has no content here)",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )))
        } else {
            Text::from(
                col.content
                    .iter()
                    .map(|l| Line::from(Span::styled(l.clone(), content_style)))
                    .collect::<Vec<_>>(),
            )
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Line::from(Span::styled(format!(" {title} "), border_style)));

        let p = Paragraph::new(text).block(block).scroll((app.vscroll, 0));
        f.render_widget(p, chunks[i]);
    }
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

fn draw_preview(f: &mut Frame, app: &App, area: Rect) {
    let popup = centered_rect(82, 82, area);
    f.render_widget(Clear, popup);
    let text = app
        .file()
        .map(|fs| fs.parsed.render(&fs.resolutions))
        .unwrap_or_default();
    let p = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta))
                .title(" Resolution preview — j/k scroll, any other key closes "),
        )
        .scroll((app.vscroll, 0));
    f.render_widget(p, popup);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(64, 80, area);
    f.render_widget(Clear, popup);

    let rows: &[(&str, &str)] = &[
        ("Movement", ""),
        ("  j / k, ↑ / ↓", "scroll the conflict content"),
        ("  Ctrl-d / Ctrl-u", "half-page scroll"),
        ("  gg / G", "jump to top / bottom"),
        ("Compare", ""),
        ("  h / l, Tab / S-Tab", "focus previous / next side"),
        ("  n / N", "next / previous conflict region"),
        ("  ] / [  (or J / K)", "next / previous file"),
        ("Resolve", ""),
        ("  ⏎", "pick the focused side for this region"),
        ("  1 - 9", "pick side by number"),
        ("  u", "un-resolve this region"),
        ("  w", "write resolution(s) to disk"),
        ("  p", "preview the resulting file"),
        ("  e", "open the file in $EDITOR"),
        ("  r", "refresh from jj"),
        ("Other", ""),
        ("  ? ", "toggle this help"),
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
                    Span::styled(
                        format!("{k:<22}"),
                        Style::default().fg(Color::Yellow),
                    ),
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

/// A centered rectangle covering `px`% width and `py`% height of `area`.
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
