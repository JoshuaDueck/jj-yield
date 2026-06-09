//! jj-yield binary: terminal lifecycle, the event loop, and `$EDITOR` handoff.

use anyhow::{Context, Result};
use jj_yield::app::{Action, App};
use jj_yield::jj::Jj;
use jj_yield::ui;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;
use std::io::{self, Stdout};
use std::process::Command;
use std::time::Duration;

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("-h" | "--help") => {
            print_usage();
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("jj-yield {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }

    let jj = Jj::discover()
        .context("could not start jj-yield — make sure `jj` is installed and you are inside a repo")?;
    let mut app = App::new(jj)?;

    if app.files.is_empty() {
        println!("No conflicts in the working copy. 🎉");
        return Ok(());
    }

    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}

fn run(terminal: &mut Tui, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        // A short poll keeps the UI responsive without busy-looping.
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match app.handle_key(key) {
                Action::Quit => break,
                Action::OpenEditor => edit_current(terminal, app)?,
                Action::None => {}
            }
        }
    }
    Ok(())
}

/// Suspend the TUI, run `$EDITOR` on the current file, then restore and reload.
fn edit_current(terminal: &mut Tui, app: &mut App) -> Result<()> {
    let Some(path) = app.current_path() else {
        return Ok(());
    };
    let abs = app.jj.abs_path(&path);
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "vi".to_string());

    // Leave the alternate screen so the editor owns the terminal.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    // Run through `sh -c` so editor strings with flags (e.g. "code --wait") work.
    let command = format!("{editor} {}", shell_single_quote(&abs.to_string_lossy()));
    let status = Command::new("sh").arg("-c").arg(&command).status();

    // Restore the TUI.
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;

    match status {
        Ok(s) if s.success() => {
            app.reload();
            if !app.files.is_empty() {
                app.status = format!("Reloaded {path} after editing");
            }
        }
        Ok(s) => {
            app.reload();
            app.status = format!("Editor exited with {s}; reloaded {path}");
        }
        Err(e) => app.status = format!("Failed to launch editor `{editor}`: {e}"),
    }
    Ok(())
}

/// Single-quote a string for safe use in a POSIX shell.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout)).context("failed to initialize terminal")
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn print_usage() {
    println!(
        "jj-yield {ver} — compare and resolve multi-sided Jujutsu conflicts.

USAGE:
    jj-yield            Open the TUI for conflicts in the current jj repo.
    jj-yield --help     Show this message.
    jj-yield --version  Show the version.

Run it from inside a Jujutsu repository that has unresolved conflicts.
See the keybindings with `?` once the TUI is open.",
        ver = env!("CARGO_PKG_VERSION")
    );
}
