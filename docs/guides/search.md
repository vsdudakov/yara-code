---
description: Project search and the command palette in Yara Code — F3 finds every line, F5 runs any command by a few letters.
---

# Search and the palette

## Search the project

![Project search](../assets/shots/search.svg)

++f3++ opens SEARCH PROJECT. Type; every line containing the
text, in any file under the project, is listed as `path:line  text`. The
footer counts matches and files and names what was skipped: the workspace's
`ignore_folders` — hidden folders, `node_modules`, `target` and the like —
plus whatever `search_exclude` adds, in VS Code's glob spelling. Both are in
[settings](settings.md). ++enter++ opens the hit
with the caret on its line. Case does not matter; the list stops at five
hundred hits.

## The palette

![The command palette](../assets/shots/palette.svg)

++f5++ opens the COMMAND PALETTE: every command with its chord on
the right, found by a few letters — `tf` for Toggle Files. ++enter++ runs
it. The menus (++f10++, then ++right++ for Help) hold the same
commands in the order a menu bar would.

![The File menu](../assets/shots/menu.svg)
