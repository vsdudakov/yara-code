# Handoff: Yara Code v1.0.0 — full TUI rewrite

## Overview
A ground-up rewrite of Yara Code (https://github.com/vsdudakov/yara-code) toward **v1.0.0**, TUI-only (the GPU/egui GUI frontend is dropped in v1.0.0), built around a new agent-first TUI: the agent terminal is the permanent left pane, and a FOLLOW pane on the right auto-opens the diff of whatever the agent last edited. The rewrite lands in **new folders**; the existing code is isolated (kept compiling, untouched) and deleted only after the new frontend reaches parity.

## About the Design Files
`Yara TUI.dc.html` is a **design reference created in HTML** — an interactive prototype showing intended look and behavior, not production code. The task is to **recreate this design in Rust with ratatui + crossterm**, using the repo's established patterns (`src/core` primitives, settings/keys/theme systems). Every visual element in the prototype was deliberately constrained to what ratatui can draw: block borders with titles in the border line, per-cell fg/bg colors, glyph ticks, bracketed pills — no gradients, images, or proportional type.

## Fidelity
**High-fidelity for structure and behavior; palette is a theme, not a constant.** Recreate the layout, panes, overlays, key model and states exactly. Colors below define a new built-in theme ("Organic Dark" / "Organic Light") added alongside the existing Dark Modern / Light+ / Monokai; everything must keep reading colors from the theme system.

## Rewrite & isolation strategy
1. Convert the repo to a Cargo **workspace**. New code lives in new folders:
   - `crates/yara-core/` — rewritten core (buffers, git, diff, pty, settings, themes, follow).
   - `crates/yara-tui/` — the new ratatui frontend (this design).
   - `crates/ycode/` — the `ycode` binary, wired to the new crates.
   - **No GUI crate**: `src/gui` and `ycode-gui` are not ported; they are deleted with the rest of the legacy code.
2. Old code is **isolated, not modified**: move nothing; keep `src/` building as `yara-legacy` behind a `legacy` feature (or a `[[bin]] ycode-legacy`). No new code may depend on `src/`; enforce with a CI check.
3. Port module by module, carrying over `src/core`'s test suite (369 tests; `tests/tui_e2e.rs` pattern — ratatui TestBackend, read the frame back). Gate each ported module at the existing 90% coverage bar.
4. When `crates/yara-tui` passes the e2e parity checklist below, delete `src/` and the legacy feature in one commit. Tag v1.0.0.

## Screens / Views

### 1. Start page
- Centered column: block-glyph "yara" logotype (accent color), tagline "the terminal editor for the agent loop" (muted), RECENT box, key hints line.
- RECENT: bordered block, title `RECENT` in the top border. First row = last project, selected (accent-200 bg, accent-900 text, `▸` prefix, `⏎` right-aligned). Other rows muted. Enter / click opens the project.
- Hints: `⏎ open project · Ctrl+P go to file · F1 keys`.

### 2. Main loop (the product)
Grid, one gap column between panes:
- **Header row**: `YARA` (accent, bold) · `File` `Help` menu items · `│` · project path (muted) · `│` · `⌥ worktree: <name>` (sage) · right: `◐ claude 62%` usage chip (click → AGENT USAGE) · `● claude — running` (sage).
- **FILES sidebar** (toggle Ctrl+B, hidden by default): bordered, title `FILES`. Tree rows: `▾` folders (muted), `▫` files; files changed by the agent tinted accent-800 with a right-aligned `●`; opened file row bg accent-100. Footer inside pane: `^B hide · ⏎ open`. Width 30ch.
- **AGENT pane** (42% width): bordered, title `AGENT · claude`. A scrollback of the agent session, bottom-aligned, plus a blinking 1-cell block cursor. Line roles: command (text color, semibold), agent prose (bright neutral), tool calls `● Read …` / `✳ Edit … (+5 −1)` (accent-800), success lines (sage-700, medium).
- **FOLLOW pane** (rest): border and title reflect state — following: accent border, title `FOLLOW · LIVE`; scrubbed back: neutral border, `FOLLOW · PAUSED` + right border-title button `[ f → live ]`.
  - File row: path (bold) · `+n` (sage) `−n` (accent) · `[✓ reviewed]` when marked · right `[ v: file ]` / `[ v: diff ]` toggle. Truncate with ellipsis, never wrap.
  - Timeline row: `edits` label · tick strip: `◉` current (accent), `●` unreviewed (accent-800), `○` reviewed (muted); >12 edits → windowed around current with `‥` overflow glyphs (tooltip "N more") · `[k/n]` counter · right hints `← → scrub · f live · ⏎ mark reviewed` (first to truncate).
  - Body: unified diff. Line = gutter bar cell, line number (5ch right-aligned, muted), sign column (`+` sage-800 / `−` accent-800), code. Added rows bg sage-100, removed rows bg accent-100. FILE view: no signs/bg, added lines carry a `▎` accent gutter bar.
  - EDIT mode (file opened from FILES): title `EDIT`, body is an editable buffer; file row shows `path ●` while dirty; Ctrl+S → status-bar note `✓ saved <path>`; Esc closes back to follow.
- **Status bar**: `⎇ feature/follow-mode` · worktree path (muted, truncates) · `+41` (sage) `−1` (accent) · `◆ N unreviewed` (accent, click = jump to next unreviewed edit) or `✓ all reviewed` (sage) · right: `^⇧G changes` `^B files` `^⇧P palette` `^⇧F search` `F1 keys` · version chip `v0.9.4 ↑` (sage-700; after update `v1.0.0 ✓`).

### 3. Overlays (all: dimmed backdrop, bordered box, title in border, Esc closes; backdrop click closes only on the backdrop itself)
- **CHANGES (Ctrl+Shift+G)** — git status of the worktree vs main: rows `A/M/✓` state (sage for A/✓, accent for M) · path · `+n −n`; current file row bg accent-100; click opens its diff. Footer: `git status vs main · N files · +a −d · ⏎ open diff · Esc close`. Distinct from the edit timeline: timeline = agent events in order, CHANGES = resulting git state.
- **COMMAND PALETTE (Ctrl+Shift+P)** — prompt row `> type a command…`, command rows with right-aligned chords.
- **SEARCH PROJECT (Ctrl+Shift+F)** — query row with block cursor, result rows `path:line  text`, footer `7 matches in 4 files · exclude: target/**`.
- **KEY BINDINGS (F1)** — dotted-leader rows name ···· chord; scrolls inside, border never scrolls.
- **AGENT USAGE (Ctrl+Shift+U or the header chip; not in the menu)** — one row per CLI agent (claude/cursor/codex): name · plan · 10-cell bar `▰▰▰▱▱` (sage, accent when ≥80%) · percent · tokens/requests detail · reset line. Footer names the data source: polled from each agent CLI. 
- **OPEN RECENT (Ctrl+R)** — recent projects; current one first with its worktree.
- **File / Help menus** (click or F10) — anchored dropdowns under the header. File: New File ^N, Open Folder… ^⇧O, Add Folder to Project… ^⇧A, Open Recent… ^R, Save ^S, Settings ^, , Quit ^Q. Help: Documentation ↗ (vsdudakov.github.io/yara-code), Key Bindings F1, Check for Updates… (→ `↓ vX downloaded — restart to apply`), version + license line.

## Interactions & Behavior
- **Follow**: every agent edit appends to the timeline; while LIVE the FOLLOW pane snaps to the newest edit's diff. ←/→ scrubs (drops to PAUSED). `f` snaps back to LIVE. Tick click jumps.
- **Review**: Enter marks the current edit reviewed → jumps to the oldest unreviewed; when none remain, snap to LIVE. Status-bar counter click = jump to next unreviewed.
- **Keys** (all rebindable via existing settings/keys system): f, ←/→, ⏎, v (diff/file), Ctrl+B, Ctrl+R, Ctrl+Shift+G/P/F/U, F1, F10, Esc, Ctrl+S. While the editor buffer has focus, only Ctrl+S and Esc are captured (mirror the repo's terminal-panel focus rules).
- Blink ~1.1s step cursor; no animation elsewhere.

## State Management
`FollowState { live: bool, cursor: usize }` + `edits: Vec<EditEvent { path, hunks }>` + `reviewed: BitSet` in core; UI state (sidebar, overlay, view mode, opened buffer, dirty) in the frontend. Usage data polled asynchronously from agent CLIs, cached with a "refreshed Ns ago" stamp.

## Design Tokens (theme "Organic Dark" — light variant in the prototype's helmet)
- bg `oklch(0.21 0.015 60)` ≈ #322b24 · text `oklch(0.93 0.02 75)`
- neutral ramp 100–900: L 0.25/0.30/0.34/0.44/0.54/0.63/0.72/0.82/0.90, C 0.015–0.02, H 60–75
- accent (terracotta) base #c67139; dark-theme steps: 100 `oklch(0.29 0.05 55)`, 200 `0.35 0.07 55`, 300 `0.42 0.09 55`, 700 `0.74 0.13 55`, 800 `0.80 0.12 58`, 900 `0.87 0.09 65`
- accent-2 (sage) base #7a8a5e; 100 `oklch(0.29 0.04 130)`, 700 `0.74 0.09 130`, 800 `0.81 0.08 130`
- Roles: accent = deletions, unreviewed, chrome emphasis, active border; sage = additions, success, agent-running, worktree.
- Mono type only; glyphs used: ▸▾▫●○◉‥▰▱▎◆⎇⌥◐│✓✳

## Parity checklist before deleting src/
Start page · project open · file tree + edit + save · follow loop (live/scrub/review) · CHANGES vs timeline distinction · palette · project search · F1 overlay · menus · recent · updater · usage panel · themes (4 built-in + VS Code JSON) · kitty keyboard protocol · mouse support · e2e frame tests green on TestBackend.

## Assets
None — everything is characters and theme colors.

## Files
- `Yara TUI.dc.html` — the interactive prototype (open in a browser; Tweaks: theme dark/light, demo agent speed/pause/loop).
