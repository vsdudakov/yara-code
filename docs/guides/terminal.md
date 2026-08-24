---
description: The built-in shell where your coding agent runs, in both the terminal UI and the window.
---

# Terminal

Both frontends carry a real login shell on a pseudo-terminal, sharing one
`core::pty` implementation. Tab completion, Ctrl-C and full-screen programs
behave as they do outside Yara Code, with 5000 lines of scrollback (mouse wheel).
The frontends differ only in how they paint the grid.

`Cmd+J` / `Ctrl+J` toggles the panel. `Cmd+Alt+T` / `Ctrl+Alt+T` opens another
session; `Cmd+Alt+W` / `Ctrl+Alt+W` closes one.

## Sessions

Sessions are tabs in the panel header, numbered until you name them:

- **Right-click a tab to rename it** (the window edits the name in place; the
  terminal frontend asks in a prompt). An empty name puts it back to its number.
- **Drag a tab** along the strip to reorder; the active session stays active
  wherever it lands.

The shell starts in the project's first folder. Changing the project restarts
it there.

## Keys in the panel

In the terminal frontend, while the panel has focus every key goes to the shell
— only Toggle Terminal, New Terminal, Close Terminal and Quit stay reserved, so
leave the panel with its toggle or by clicking another pane. In the window, the
grid takes the keyboard when it has focus and gives ⌘-shortcuts back to the app.
