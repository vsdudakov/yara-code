---
description: Workspaces in Yara Code — one agent per task in its own git worktree, each with a tab, a timeline and a name of your choosing.
---

# Workspaces

A workspace is one task: an agent, and the folders the task is being done in
— a git worktree per repository, most often — with one timeline and one
CHANGES over them all. The tabs in the header are the workspaces.

![Naming a new workspace](../assets/shots/new-tab.svg)

++f7++ — or `[+]` in the header, or `File → New Workspace…` —
asks for a name. The name becomes the tab, and, with spaces and slashes made
dashes, the branch and the folder: `git worktree add -b logout-flow
<worktrees_dir>/logout-flow`. An agent starts there, and the keyboard goes to
it.

![Two workspaces](../assets/shots/tabs.svg)

| | Key |
| --- | --- |
| New workspace | ++f7++ |
| Next / previous | ++ctrl+l++ / ++ctrl+k++, or click the tab |
| Rename | ++f2++, or a right click on the tab |
| Delete the worktree | a right click on the tab — the folder goes, the branch stays |
| Reorder | drag a tab along the strip |
| Close | ++ctrl+w++ — the agent stops; the worktree stays for git |

The first workspace, the one without a name of its own, is called by its
pull request when `gh` is installed and the branch has one (`#42 Fix the
login redirect`), else by its branch, else by its folder.

Where the worktrees go is `worktrees_dir` in [settings](settings.md):
empty means a `<repo>-worktrees` folder beside the repository.

## Several repositories, one task

A feature that touches a backend and a frontend is one workspace with two
folders. `File → Add Folder to Workspace…` takes the path of the other
repository's worktree; from then on the workspace treats them as one:

- the timeline carries the edits of both, each named by its folder —
  `frontend/src/app.js`;
- CHANGES heads each folder with its branch and its counts;
- the FILES tree heads each folder, and ++ctrl+p++ and project search reach
  across both;
- the status bar shows the main folder's branch and `+1 folders`.

The first folder is the main one: the agent runs there, and it names the tab.
