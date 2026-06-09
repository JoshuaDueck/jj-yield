//! Thin integration layer over the `jj` CLI.
//!
//! Every command runs with its working directory set to the repo root (from
//! `jj root`), so all paths are repo-root-relative and unambiguous. Conflicts
//! are always materialized with `--config ui.conflict-marker-style=snapshot` so
//! jj-yield sees snapshot markers regardless of the user's own configuration.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Command;

/// A handle to the Jujutsu repository jj-yield is operating on.
pub struct Jj {
    pub root: PathBuf,
}

/// One row of `jj resolve --list`: a conflicted path and its description.
#[derive(Debug, Clone)]
pub struct ConflictEntry {
    pub path: String,
    pub description: String,
    /// Side count parsed from a description like `3-sided conflict`.
    pub sides: Option<usize>,
}

impl Jj {
    /// Locate the repository by running `jj root`.
    pub fn discover() -> Result<Self> {
        let out = Command::new("jj")
            .arg("root")
            .output()
            .context("could not run `jj` — is it installed and on your PATH?")?;
        if !out.status.success() {
            bail!(
                "`jj root` failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if root.is_empty() {
            bail!("`jj root` returned an empty path");
        }
        Ok(Jj { root: PathBuf::from(root) })
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new("jj");
        c.current_dir(&self.root);
        c
    }

    /// Absolute path of a repo-root-relative path.
    pub fn abs_path(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }

    /// List conflicted paths in the working copy (`@`).
    pub fn list_conflicts(&self) -> Result<Vec<ConflictEntry>> {
        let out = self
            .cmd()
            .args(["resolve", "--list"])
            .output()
            .context("failed to run `jj resolve --list`")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // jj exits non-zero with this message when there is nothing to resolve.
            if stderr.contains("No conflicts") {
                return Ok(Vec::new());
            }
            bail!("`jj resolve --list` failed: {}", stderr.trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(parse_list_line)
            .collect())
    }

    /// Materialize `path` at `@` using snapshot conflict markers.
    pub fn materialize(&self, path: &str) -> Result<String> {
        let out = self
            .cmd()
            .args([
                "--config",
                "ui.conflict-marker-style=snapshot",
                "file",
                "show",
                "-r",
                "@",
                "--",
                path,
            ])
            .output()
            .with_context(|| format!("failed to run `jj file show` for {path}"))?;
        if !out.status.success() {
            bail!(
                "`jj file show {}` failed: {}",
                path,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Write resolved/edited content back to the working-copy file. The next
    /// `list_conflicts` call triggers jj to re-snapshot and recompute state.
    pub fn write_resolution(&self, path: &str, content: &str) -> Result<()> {
        let abs = self.abs_path(path);
        std::fs::write(&abs, content)
            .with_context(|| format!("failed to write {}", abs.display()))?;
        Ok(())
    }
}

/// Parse one `jj resolve --list` row. Path and description are separated by a
/// run of >= 2 spaces (column padding); real paths rarely contain that.
fn parse_list_line(line: &str) -> Option<ConflictEntry> {
    let line = line.trim_end();
    if line.is_empty() {
        return None;
    }
    let (path, description) = match line.find("  ") {
        Some(idx) => (line[..idx].to_string(), line[idx..].trim().to_string()),
        None => (line.to_string(), String::new()),
    };
    let sides = parse_sides(&description);
    Some(ConflictEntry { path, description, sides })
}

/// Extract the leading integer from a description like `3-sided conflict`.
fn parse_sides(description: &str) -> Option<usize> {
    description.split('-').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resolve_list_rows() {
        let e = parse_list_line("f.txt    3-sided conflict").unwrap();
        assert_eq!(e.path, "f.txt");
        assert_eq!(e.description, "3-sided conflict");
        assert_eq!(e.sides, Some(3));

        let e = parse_list_line("src/a b.rs    2-sided conflict").unwrap();
        assert_eq!(e.path, "src/a b.rs"); // single spaces in path are preserved
        assert_eq!(e.sides, Some(2));

        assert!(parse_list_line("").is_none());
    }
}
