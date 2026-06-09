# jj-yield

Modern conflict resolution for [Jujutsu](https://docs.jj-vcs.dev/).

A low-friction terminal UI for **comparing the sides of multi-sided conflicts**.
Each side is materialized as a full *snapshot* and labelled by its source (change
id, commit, description), so you can see exactly what every version says. When you
want to make edits, jj-yield hands off to your `$EDITOR`; when a side is right as-is,
pick it and write the resolution back.

```
┌ Conflicts ──────┐┌ src/lib.rs ─────────────────────────────────────────┐
│› src/lib.rs  [3]││ conflict 1/1   3 sides   0/1 regions resolved        │
│  README.md   [2]│├ 1. Alice (lwoqkvks) ─┬ 2. base ─┬ 3. Bob ─┬ 4. Carol ┤
│                 ││ AAA                  │ shared    │ BBB     │ CCC      │
│                 ││                      │           │         │          │
│                 │└──────────────────────┴───────────┴─────────┴──────────┘
└─────────────────┘ j/k scroll · h/l/Tab side · n/N hunk · ⏎ pick · w write
```

## Status

Early. Targets Jujutsu's **snapshot** conflict-marker style
([docs](https://docs.jj-vcs.dev/latest/conflicts/#alternative-conflict-marker-styles)),
which is the only style that represents conflicts of any arity as a full snapshot
per side. Other styles (`diff`, `git`/diff3) may come later.

## Install

Requires a Rust toolchain (`cargo`) and `jj` on your `PATH`.

```sh
cargo install --path .
# or, while developing:
cargo run
```

## Usage

Run it from inside a Jujutsu repo that has unresolved conflicts:

```sh
jj-yield
```

jj-yield reads the conflict list with `jj resolve --list` and materializes each
conflicted file with `jj file show` (forcing snapshot markers via
`--config ui.conflict-marker-style=snapshot`, so it works regardless of your own
config). Picking a side and writing replaces that conflict region's markers with
the chosen content; jj re-evaluates the conflict on its next snapshot.

### Keybindings

| Key | Action |
| --- | --- |
| `j` / `k`, `↑` / `↓` | scroll content |
| `Ctrl-d` / `Ctrl-u` | half-page scroll |
| `gg` / `G` | top / bottom |
| `h` / `l`, `Tab` / `Shift-Tab` | focus previous / next side |
| `n` / `N` | next / previous conflict region |
| `]` / `[` (or `J` / `K`) | next / previous file |
| `⏎` | pick the focused side for this region |
| `1`–`9` | pick a side by number |
| `u` | un-resolve the current region |
| `w` | write resolution(s) to disk |
| `p` | preview the resulting file |
| `e` | open the file in `$EDITOR` |
| `r` | refresh from jj |
| `?` | toggle help |
| `q` / `Esc` / `Ctrl-c` | quit |

## Development

```sh
cargo test     # unit + parser fixture tests
cargo run      # launch against the current repo's conflicts
```

The parser is tested against fixtures in `tests/fixtures/` captured from real `jj`
output. See `CLAUDE.md` for architecture notes and the snapshot grammar.
