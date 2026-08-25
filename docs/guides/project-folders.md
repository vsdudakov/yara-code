---
description: Hold several folders in one window; search, go-to-definition and the navigator treat them as one project.
---

# Project folders

A window opens on the folders you give it. Launched with a path, that path is
the project; launched with none, it opens empty and the start page says how to
put a folder in it.

## Adding a folder

**File → Add Folder to Project…** (`Cmd+Shift+A` / `Ctrl+Shift+A`) puts more
folders beside the first. Each one heads its own subtree in the navigator, and
everything that walks the project — search, replace-across-files,
go-to-definition — covers all of them.

Paths are written with the folder's name in front once more than one is open, so
two files called `main.rs` never read alike:

```
core/src/main.rs
tools/src/main.rs
```

Overlapping folders are refused: one inside another would list and search the
same files twice, so Yara Code says `already inside core` rather than doing it.

## Removing one

Right-click the folder's row in the navigator → **Remove Folder from Project**,
which stands where Add Folder to Project stands on any other folder. It
leaves the folder on disk and only drops it from the window. Removing the last
one puts the window back to its empty state — that is a valid place to be, not
an error.

A project folder's row does not offer Rename, Move To… or Delete: renaming the
folder you are working in from inside the editor is a foot-gun, and the
navigator says so by not offering it.

## What stays with the first folder

Git and the terminal's working directory are anchored to the **first** folder —
the project root. Adding a second folder does not open a second repository or a
second shell.
