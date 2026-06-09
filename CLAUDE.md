# jj-yield — notes for Claude

A `ratatui` TUI for comparing/resolving multi-sided Jujutsu conflicts in the
**snapshot** marker style.

## Commands

```sh
cargo build
cargo test            # parser unit tests + fixture integration tests
cargo run             # launch against the current jj repo's conflicts
cargo run -- --help
```

There is no Rust toolchain assumption baked in beyond `ratatui = "0.29"` (which
re-exports `crossterm` as `ratatui::crossterm` — do not add a separate crossterm
dependency, it would risk a version mismatch).

## Architecture

- `src/conflict.rs` — pure data model (`ParsedFile`, `Segment`, `ConflictRegion`,
  `Term`, `Column`, `SideLabel`). `ConflictRegion::columns()` collapses identical
  bases; `ParsedFile::render(resolutions)` reassembles the file (lossless when all
  resolutions are `None`).
- `src/parser.rs` — line-oriented snapshot-marker parser. Pure; no I/O.
- `src/jj.rs` — shells out to `jj`. All commands run with `current_dir = jj root`
  so paths are repo-root-relative.
- `src/app.rs` — `App` state + key handling. No rendering.
- `src/ui.rs` — `ratatui` rendering. No mutation.
- `src/main.rs` — terminal lifecycle, event loop, `$EDITOR` handoff.

## jj integration facts (verified against jj 0.42)

- `jj root` → absolute repo root, one line.
- `jj resolve --list` → rows like `f.txt    3-sided conflict` (path and
  description separated by a run of >= 2 spaces). Exits **non-zero** with
  `Error: No conflicts found...` when there are none — treated as an empty list.
- `jj --config ui.conflict-marker-style=snapshot file show -r @ -- <path>`
  materializes snapshot markers regardless of the user's config. `file show`
  paths are **cwd-relative** (hence we pin `current_dir`).
- Writing resolved content to the working-copy file and then re-running
  `jj resolve --list` is a complete refresh — jj snapshots on read.

## Snapshot marker grammar

```text
<context lines>
<<<<<<< conflict <n> of <m>
+++++++ <change_id> <commit_id> "<description>"   ← Add: a present side
<content lines...>
------- <change_id> <commit_id> "<description>"   ← Remove: a merge base
<content lines...>
+++++++ ...                                       ← (repeats; k-sided => k Adds)
<content lines...>
>>>>>>> conflict <n> of <m> ends
<context lines>
```

- A k-sided conflict has **k `+++++++` terms and k-1 `-------` terms** (the base,
  usually byte-identical and repeated). The UI collapses identical bases.
- A term's content may be **empty** (e.g. a side that deleted the lines).
- jj **lengthens** the marker run (8+ chars) when file content would otherwise
  collide with a 7-char marker. The parser locks onto the opening `<<<<<<<`
  run length and only treats lines matching that exact length as markers.
- For reference, the other styles (NOT parsed yet): `git` uses diff3
  (`<<<<<<<` / `|||||||` / `=======` / `>>>>>>>`) and falls back to snapshot for
  >2 sides; `diff` uses `%%%%%%%`/`\\\\\\\` diff hunks plus `+++++++` snapshots.

## Fixtures

`tests/fixtures/*.txt` are real `jj file show` snapshot outputs (2-sided,
3-sided, multi-region, deletion). Regenerate with a scratch repo + octopus merge
if the format changes; keep the round-trip test green (it asserts lossless
re-emit).
