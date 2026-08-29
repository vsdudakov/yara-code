---
description: Every key binding in Yara Code, which keys the agent keeps, and how to rebind any of them in settings.json.
---

# Key bindings

++f1++ shows every command and its chord, always current, because the
overlay is drawn from the same table the editor dispatches on.

![Every key binding](../assets/shots/keys.svg)

## Whose key is it

With the keyboard on the **agent**, plain keys, ++enter++, ++esc++,
++tab++ and the arrows are the agent's, whatever the editor binds them to —
they are how it is talked to. A bound ++ctrl++ or ++alt++ chord, or a
function key, is the editor's, except the ones `agent_keys` lists
(++ctrl+r++, ++ctrl+n++, ++ctrl+z++ by default), which the agents use
themselves. Chords nobody bound reach the agent.

With the keyboard in the **editor**, everything is typing except ++ctrl+s++,
++esc++, ++ctrl+z++, ++ctrl+shift+z++ and the function keys.

++f6++ moves the keyboard round: agent → files (when shown) → editor or
follow pane → agent. A click moves it too.

## The defaults

| Command | Key |
| --- | --- |
| New File | ++ctrl+n++ |
| Open Folder… | ++ctrl+shift+o++ |
| Open Recent… | ++ctrl+r++ |
| Save | ++ctrl+s++ |
| Settings | ++ctrl+comma++ |
| Quit | ++ctrl+q++ |
| Documentation | ++ctrl+shift+h++ |
| Key Bindings | ++f1++ |
| Toggle Files | ++ctrl+b++ |
| Changes | ++ctrl+shift+g++ |
| Command Palette | ++ctrl+shift+p++ |
| Search Project | ++ctrl+shift+f++ |
| Agent Usage | ++ctrl+shift+u++ |
| Theme… | ++ctrl+shift+t++ |
| Go to File | ++ctrl+p++ |
| File Menu / Help Menu | ++f10++ / ++shift+f1++ |
| Switch Pane | ++f6++ |
| Close | ++esc++ |
| New Workspace… | ++ctrl+shift+n++ |
| Close Workspace | ++ctrl+shift+w++ |
| Next / Previous Workspace | ++ctrl+pgdn++ / ++ctrl+pgup++ |
| Rename Workspace… | ++f2++ |
| Undo / Redo | ++ctrl+z++ / ++ctrl+shift+z++ |
| Follow: Go Live | ++f++ |
| Follow: Previous / Next Edit | ++left++ / ++right++ |
| Follow: Mark Reviewed | ++enter++ |
| Follow: Diff / File | ++v++ |

## Rebinding

Only the bindings that differ need listing:

```json
"keys": { "follow_live": "L", "changes": "Ctrl+G" }
```

Chords are `Ctrl`/`Alt`/`Shift` and a key — `Ctrl+Shift+F`, `Ctrl+-`,
`Alt+Left`, `F12` — or a bare key like `F` for the follow pane. A chord that
cannot be read keeps its default and is named in the status bar; one given
to two commands is reported too. Telling ++ctrl+shift+s++ from ++ctrl+s++
needs the kitty keyboard protocol; in a terminal without it, move those
chords elsewhere.
