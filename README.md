# jj-yield

Modern conflict resolution for [Jujutsu](https://docs.jj-vcs.dev/).

A low-friction terminal merge editor for **multi-sided conflicts**, modeled on
VSCode's 3-way merge editor: the conflicting sides on top (each diffed against
the base, with syntax + add/modify/delete highlighting), and a live **Result**
pane below that you build by accepting changes. Each side is labelled by its
source (change id, commit, description). When you'd rather hand-edit, jj-yield
opens the file in your `$EDITOR`.

```
┌ Conflicts ───┐┌ Component.tsx        conflict 1/1   2 sides   0/1 resolved ┐
│› Component.. ││┌ 1 Alice (lwoqkvks) ──────┐┌ 2 Bob (mvmzmyyn) ────────────┐│
│  README.md   │││ const c = useCompliance… ││ const c = useClient(id)      ││
│              │││   (green = changed vs base)│ if (loading) return <Spinner/>││  ← blue side
│              ││└──────────────────────────┘└──────────────────────────────┘│
│              ││ Result ──────────────────────────────────────────────────  │
│              ││▌ ◇ unresolved 2-sided conflict (accept a side above)        │
└──────────────┘└────────────────────────────────────────────────────────────┘
 j/k scroll · h/l/Tab side · ⏎/1-9 accept · a both · b base · n/N hunk · w write
```

## Status

Early. Targets Jujutsu's **snapshot** conflict-marker style
([docs](https://docs.jj-vcs.dev/latest/conflicts/#alternative-conflict-marker-styles)),
the only style that represents conflicts of any arity as a full snapshot per side.

Syntax highlighting (tree-sitter) currently covers: **Rust, TypeScript/TSX,
JavaScript/JSX, Ruby, Python, JSON**. Other languages render as plain text.
Adding one is a one-line registry entry + a grammar dependency — see `CLAUDE.md`.

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
file with `jj file show` (forcing snapshot markers via
`--config ui.conflict-marker-style=snapshot`, so it works regardless of your
config). Accepting a side and writing replaces that conflict region's markers
with the chosen content; jj re-evaluates the conflict on its next snapshot.

The **Result** pane is a live preview of the file you're assembling: accepted
regions show resolved content, unresolved regions show their conflict markers
(in red), and the current region is marked in the gutter.

### Keybindings

| Key | Action |
| --- | --- |
| `j` / `k`, `↑` / `↓` | scroll the side panes |
| `Ctrl-d` / `Ctrl-u` | half-page scroll |
| `gg` / `G` | top / bottom |
| `h` / `l`, `Tab` / `Shift-Tab` | focus previous / next side |
| `m` | toggle side-by-side ↔ tabbed |
| `n` / `N` | next / previous conflict region |
| `]` / `[` (or `J` / `K`) | next / previous file |
| `⏎` | accept the focused side |
| `1`–`9` | accept a side by number |
| `a` | accept both sides (in order) |
| `b` | accept the base |
| `u` | un-resolve the current region |
| `w` | write resolution(s) to disk |
| `e` | open the file in `$EDITOR` |
| `r` | refresh from jj |
| `?` | toggle help |
| `q` / `Esc` / `Ctrl-c` | quit |

## Development

```sh
cargo test     # parser + diff + highlighter + render tests
cargo run      # launch against the current repo's conflicts
```

Parser/diff fixtures live in `tests/fixtures/` (captured from real `jj` output).
See `CLAUDE.md` for architecture, the snapshot grammar, and how to add a
language.
