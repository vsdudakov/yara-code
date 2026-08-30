---
description: Yara Code — the terminal editor for the agent loop. Your coding agent on the left, the diff of what it just did on the right, in one small Rust binary.
---

# Yara Code

**The terminal editor for the agent loop: your coding agent on the left, the
diff of what it just did on the right.**

![The agent's session beside the diff of its last edit](assets/shots/hero.svg)

```bash
brew install vsdudakov/tap/ycode
ycode ~/code/project
```

You write code with an agent now — Claude Code, Codex, Cursor's CLI — and the
agent lives in a terminal. What you do all day is *watch it work*: read the
diff it just made, decide whether it is right, tell it what to do next. Yara
Code is one screen for that.

## The two panes

**AGENT** is the agent itself, running on a real pseudo-terminal in the left
pane. Type at it as you would anywhere; it keeps every key it uses. ++ctrl+t++
puts a shell under it, in the same folder.

**FOLLOW** watches the folders you work in. Every edit becomes a tick on a
timeline and the pane snaps to its diff. Scrub back with ++left++ and
++right++, mark an edit reviewed with ++enter++, return to live with ++f++,
see the file instead of the diff with ++v++. The status bar counts what is
still unreviewed. → [The follow loop](guides/follow.md)

**CHANGES** (++f4++) is the same work from the other end: what the branch
differs from `main` by, file by file, headed by each repository — the result,
beside the timeline of how it got there.

## Workspaces and tasks

A **workspace** is what you work on: a list of folders. A repository, a folder
that holds several of them, or any folder at all. The `File` menu adds them,
takes them out, and remembers them.

A **task** is what an agent is doing: a tab, with its own agent, its own
timeline and its own CHANGES. ++f7++ names one, makes a git worktree of that
name for every repository in the workspace, and starts an agent there — two
agents on two branches at once, without either treading on the other.
→ [Workspaces and tasks](guides/tasks.md)

![Two tasks](assets/shots/tasks.svg)

## And the small editor around it

A file tree (++ctrl+b++), a shell (++ctrl+t++), an editor with undo and syntax
colours for fixing a line (++ctrl+s++, ++esc++), go to file (++ctrl+p++),
project search (++f3++), a command palette (++f5++), ++f1++ for every key, and
a commented `settings.json` that holds everything you might want different —
the agent command, which side it sits on, its width, the folders nobody works
in, the keys.

Start with [Installation](getting-started/installation.md), then
[First run](getting-started/first-run.md).
