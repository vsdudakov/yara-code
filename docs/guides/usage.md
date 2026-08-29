---
description: The AGENT USAGE panel in Yara Code, fed by a command per agent, and how the editor checks for and installs its own updates.
---

# Agent usage and updates

## Agent usage

![What each agent has used](../assets/shots/usage.svg)

The agents only show their limits from inside their own session — `/usage`
in Claude Code and Cursor's CLI, `/status` in Codex — so ++f8++
types that command at the agent and puts the keyboard there. Which command,
per program name, is `usage_slash` in settings.

For a panel of your own — the ten-cell bar, red from 80%, the percent, the
detail and the reset, and the `◐ claude 62%` chip in the header — name a
command per agent in `usage_commands` that prints one JSON object; set, it
is what ++f8++ shows instead:

```json
"usage_commands": {
  "claude": "my-claude-usage",
  "codex":  "my-codex-usage"
}
```

```json
{"plan": "Max", "percent": 62, "detail": "1.2M tokens · 340 requests", "reset": "resets in 3h 20m"}
```

The commands run in the background when the panel opens and at start; an
agent whose command fails or prints something else is listed with the error
in its detail. Without any command the panel says so.

## Updates

`Help → Check for Updates…` asks GitHub for the latest release. When a newer
one exists and the folder the binary lives in is writable, it is downloaded,
its checksum checked, and it replaces the binary in place — the status bar
says `↓ vX downloaded — restart to apply` and the version chip turns to
`vX ✓`. Otherwise it names the package manager that owns the binary.
