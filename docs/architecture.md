# Architecture

Yara is one crate with three modules and a rule: **anything a user could do in
both frontends lives in `core`**. Each frontend only paints it and translates
its own input.

```
src/
  core/    frontend-independent logic
  gui/     egui on wgpu — the window
  tui/     ratatui on crossterm — the terminal
  bin/     ycode-gui.rs, yara.rs
```

Cargo features draw the line: `--no-default-features --features tui` builds the
terminal frontend with no graphics stack at all, which is what makes it usable
over SSH on a headless machine. CI builds that configuration on every push, so
it cannot rot.

## The core

| Module | What it owns |
| --- | --- |
| `buffer.rs` | Open files and the set of them; tab order. |
| `history.rs` | Undo/redo as whole-text snapshots, with typing runs folded into one step. |
| `command.rs` | Every action as a `Command`, the menus, and the key-chord type. |
| `settings.rs` | `settings.json`, the two default keymaps, chord lookup both ways. |
| `project.rs` | The folder list behind "Add Folder to Project". |
| `search.rs` | Project search and replace-across-files. |
| `find.rs` | Find/replace inside the open file. |
| `diff.rs` | Side-by-side rows built from a unified diff. |
| `git.rs` | Status, worktrees, diff, changed lines and blame — through the `git` CLI. |
| `fold.rs`, `indent.rs` | Folding regions; smart indentation. |
| `syntax.rs`, `theme.rs` | syntect grammars; VS Code theme JSON. |
| `pty.rs` | The shell behind both terminals. |
| `fs_ops.rs`, `glob.rs` | File operations; the exclude-glob matcher. |

## Two frontends, one behaviour

A feature lands in `core` first, then twice in the drawing layer. The window
uses egui's widgets; the terminal draws every row itself. What they agree on is
not a coincidence — it is that they read the same state:

- the same `Command` set, so a menu entry, a key chord and a mouse click all end
  up in one `execute`;
- the same settings file, so a rebinding moves the action in both;
- the same theme, down to the terminal palette used for git tints.

## Adding an action

1. A `Command` variant with an id and a label.
2. A default chord in **both** keymaps in `core/settings.rs`.
3. An arm in each frontend's `execute`.

The settings tests then enforce the rest: every command must be bound out of the
box (bar the handful the text widget or the mouse owns), and no two commands may
share a chord.

## What Yara deliberately does not have

- No language server. Go-to-definition is a keyword heuristic that falls back to
  listing references.
- No plugin system, no scripting host.
- No libgit2 — the `git` binary is already on the machine.
- No editor framework. The text widget, the tree, the tabs and the terminal grid
  are all drawn here.
