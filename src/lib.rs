//! jj-yield: compare and resolve multi-sided Jujutsu conflicts.
//!
//! The crate is split into a small, pure core (`conflict` + `parser`) that knows
//! how to read Jujutsu's *snapshot* conflict markers, a thin `jj` integration
//! layer that shells out to the `jj` binary, and a `ratatui`-based `app`/`ui`
//! pair that drives the terminal experience.

pub mod app;
pub mod conflict;
pub mod diff;
pub mod highlight;
pub mod jj;
pub mod parser;
pub mod ui;
