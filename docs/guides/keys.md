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
(++ctrl+r++, ++ctrl+n++, ++ctrl+z++, , ++ctrl+w++, ++ctrl+y++ by
default), which the agents use themselves. Chords nobody bound reach the
agent.

With the keyboard in the **editor**, everything is typing except ++ctrl+s++,
++esc++, ++ctrl+z++, ++ctrl+y++ and the function keys.

++f6++ moves the keyboard round: agent → files (when shown) → editor or
follow pane → agent. A click moves it too.

## The defaults

| Command | Key |
| --- | --- |
| New File | ++ctrl+n++ |
| Open Folder… |  |
| Open Recent… | ++ctrl+r++ |
| Save | ++ctrl+s++ |
| Settings | ++f12++ |
| Quit | ++ctrl+q++ |
| Key Bindings | ++f1++ |
| Toggle Files | ++ctrl+b++ |
| Changes | ++f4++ |
| Command Palette | ++f5++ |
| Search Project | ++f3++ |
| Agent Usage | ++f8++ |
| Theme… | ++f9++ |
| Go to File | ++ctrl+p++ |
| File Menu | ++f10++ — ++right++ for Help |
| Switch Pane | ++f6++ |
| Copy / Paste | ++ctrl+c++ / ++ctrl+v++ |
| Close | ++esc++ |
| New Task… | ++f7++ |
| Close Task | ++ctrl+w++ |
| Next / Previous Task | ++ctrl+l++ / ++ctrl+k++ |
| Rename Task… | ++f2++ |
| Undo / Redo | ++ctrl+z++ / ++ctrl+y++ |
| Follow: Go Live | ++f++ |
| Follow: Previous / Next Edit | ++left++ / ++right++ |
| Follow: Mark Reviewed | ++enter++ |
| Follow: Diff / File | ++v++ |

## Rebinding

Only the bindings that differ need listing:

```json
"keys": { "follow_live": "L", "changes": "Ctrl+G" }
```

Chords are `Ctrl`/`Alt`/`Shift` and a key — `F3`, `Ctrl+-`,
`Alt+Left`, `F12` — or a bare key like `F` for the follow pane. A chord that
cannot be read keeps its default and is named in the status bar; one given
to two commands is reported too. The defaults use only what every terminal
sends; a `Ctrl+Shift` rebinding works where the kitty keyboard protocol does
— Ghostty, kitty, WezTerm, foot, iTerm2 3.5 and later.
