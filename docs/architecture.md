---
description: How Yara Code is built — a core crate that knows everything and a terminal crate that draws it, the workspace, the task, the watcher, the pty, and the tests.
---

# Architecture

Two crates in one workspace, and one rule between them: `yara-core` knows,
`ycode` draws.

## `crates/yara-core`

Everything the editor knows and nothing it shows:

- `follow.rs` — the timeline: `EditEvent`s with their hunks, and `Follow` with
  its live/paused cursor and the reviewed flags.
- `git.rs` — the repository through the `git` CLI: the base branch, what
  differs from it, the `Watcher` that turns movement into edits, the
  repositories inside a folder of them, worktrees, and pull request titles
  through `gh`.
- `pty.rs` and `keyboard.rs` — a program on a pseudo-terminal, its screen
  parsed by `vt100`, and the kitty keyboard protocol it may ask for.
- `command.rs`, `settings.rs`, `theme.rs` — every action and its chord,
  `settings.json`, the theme and VS Code theme files.
- `buffer.rs`, `history.rs`, `syntax.rs`, `tree.rs`, `search.rs`, `glob.rs`,
  `fuzzy.rs`, `clipboard.rs` — the editor, the tree, search and the finders.
- `update.rs`, `usage.rs` — releases through `curl`, usage through the
  commands the settings name.

## `crates/ycode`

- `app.rs` — `App` holds the **workspace** (a `Vec<PathBuf>` of folders) and
  its **tasks**. A `Task` is a tab: an agent pty, a terminal pty, one
  timeline, an editor, and the `Vec<Folder>` it works in — the workspace's
  folders, or a worktree of each where the task has one of its own.
  `Task::resync` points a task at the workspace again, expanding a folder
  that holds repositories into each of them; `App` derefs to the active task,
  so the drawing code reads `app.follow` and means the one in front.
  `handle_key` decides whose key it is, `execute` runs a `Command`, and
  `handle_mouse` matches a click against the `Hits` the last frame recorded.
- `ui.rs` — every frame: header and tabs, panes, status bar, overlays, hover
  and selection.
- `keys.rs`, `theme.rs`, `main.rs` — a key event as a chord, a theme as
  ratatui styles, and the loop that draws and polls.

## How the watching works

Each refresh, every folder is asked whether it moved: a repository through one
`git status` and the modification times of what it lists; a plain folder
through a walk of its files. Only a folder that moved is read further, and then
each changed file is diffed against what was last seen — so an edit is the
step the agent just took. Every file is reported by the folder closest to it,
so a repository inside a folder of repositories is not counted twice.

## The tests

They drive the real `App` on ratatui's `TestBackend` and read the frame back:
keys and mouse in, text out. Tests that need git make a real repository in a
temp folder; tests that need an agent spawn `cat` on a real pty.
`examples/screenshot.rs` is the same idea aimed at SVG — a bench of two
repositories and a stand-in agent — and it draws every image on these pages.
