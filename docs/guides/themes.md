---
description: Themes and syntax colours in Yara Code — Dark Modern, Monokai, Dracula, One Dark Pro, Tokyo Night and Nord built in, any VS Code theme JSON dropped beside the settings, syntect grammars.
---

# Themes and syntax

Six themes ship: VS Code's **Dark Modern**, so the editor looks like the one
beside it, and the popular ones — **Monokai**, **Dracula**, **One Dark Pro**,
**Tokyo Night** and **Nord** — each as the file its VS Code extension
carries. Any other is a VS Code colour theme JSON — the file a theme
extension carries under `themes/`, comments and all — dropped into
`~/.config/ycode/themes/`. Its `colors` drive the chrome and the agent pane's
palette, its `tokenColors` the syntax; a translucent colour is laid over the
editor's ground, and whatever the file leaves out keeps Dark Modern's value.

++f9++ lists what is there and switches; the choice is saved as `theme`.

The colours are used by role rather than by name: the accent is deletions,
unreviewed edits and the live border; the success colour is additions, the
agent at work and the branch. A theme that names `list.hoverBackground` gets
its own hover; one that does not keeps Dark Modern's.

Syntax colours come from syntect's grammars, which cover the usual languages.
A `.sublime-syntax` in `~/.config/ycode/syntaxes/` adds one, and a few
extensions without a grammar of their own — `ts`, `toml`, `kt`, `swift`, `zig`
— borrow the nearest.
