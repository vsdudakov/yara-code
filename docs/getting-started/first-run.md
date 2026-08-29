---
description: The first run of Yara Code — the start page, opening a project, the agent starting, the first edit landing on the timeline.
---

# First run

```bash
ycode
```

With no folder named, the start page lists the projects opened before.
++enter++ opens the one under the cursor; `File → Open Folder…` asks for a
path.

![The start page](../assets/shots/start.svg)

```bash
ycode ~/code/project
```

With a folder, the two panes come up and the agent starts in it — `claude`
unless [settings](../guides/settings.md) say otherwise. The keyboard is on
the agent: talk to it.

When it edits a file, the FOLLOW pane shows the diff of that edit and the
status bar counts one unreviewed. ++f6++ moves the keyboard to the follow
pane; ++enter++ marks the edit reviewed and moves to the next one;
++ctrl+shift+g++ shows what the whole branch differs from `main` by.

That is the loop. The rest is in the guides:

- [The follow loop](../guides/follow.md) — live, paused, reviewed; diff and file.
- [Workspaces](../guides/workspaces.md) — an agent per task, in its own worktree.
- [Files and editing](../guides/editing.md) — the tree, the editor, saving.
- [Key bindings](../guides/keys.md) — which keys are the agent's and which the editor's.
