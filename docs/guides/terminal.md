---
description: The shell under the agent in Yara Code — Ctrl+T opens it in the task's folder, with its own scrollback and the keys while it has them.
---

# The terminal

++ctrl+t++ opens a shell under the agent, in the task's main folder; ++ctrl+t++
again closes it. Each task has one of its own.

![A shell under the agent](../assets/shots/terminal.svg)

It is a real pseudo-terminal: completion, full-screen programs and colours all
behave as they do outside the editor. Which shell it runs is `shell` in
[settings](settings.md) — empty means `$SHELL` — and how much of the pane it
takes is `terminal_height`, in percent.

While the keyboard is in it, every key is the shell's but the editor's own
bound `Ctrl` chords and the function keys. ++f6++ walks the keyboard round —
agent, terminal, files, editor or the follow pane — and a click puts it
wherever it lands. The wheel scrolls the shell's own scrollback.

A shell is not an agent, but the work is watched all the same: a `git
checkout` or a `sed -i` run in it shows up on the timeline as the edits it
made, like anything else that touches the folders.
