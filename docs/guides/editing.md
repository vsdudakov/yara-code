---
description: Files and editing in Yara Code — the FILES tree, opening a file where the follow pane was, typing, undo, saving, and go to file.
---

# Files and editing

Yara Code is not where you write the code; the agent does that. It is where
you fix a line. So the editor is small, and it opens where the follow pane
was.

## The tree

++ctrl+b++ shows FILES and puts the keyboard on it. ++up++ ++down++ move,
++enter++ opens a folder or a file, ++ctrl+b++ hides it again. Files the
branch changed are tinted with a `●`; the open file has its row lit. The
sidebar is `sidebar_width` columns wide and `show_sidebar` says whether it
starts open.

## The editor

![A file open for editing](../assets/shots/edit.svg)

A file opens in the follow pane's place with the title **EDIT**, its path,
a `●` while it has unsaved changes, and the name of its grammar on the
right. Every key is typing except these:

| | Key |
| --- | --- |
| Save | ++ctrl+s++ — `✓ saved <path>` in the status bar |
| Close, back to the follow pane | ++esc++ |
| Undo / redo | ++ctrl+z++ / ++ctrl+shift+z++ — a run of typing is one step |
| Move | arrows, ++home++, ++end++; ++tab++ inserts four spaces |

A save is a change to the working tree like any other, so it lands on the
timeline too.

## Go to file

++ctrl+p++ finds a file by a few letters — `gd` finds `docs/guide.md` —
and opens it. ++ctrl+n++ makes a new file, in the folder under the FILES
cursor when the tree is open and at the root otherwise, and opens it.
`File → Settings` opens `settings.json` the same way.
