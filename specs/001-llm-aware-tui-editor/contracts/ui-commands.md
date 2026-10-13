# Contract: UI Commands, Keys and Displayed Semantics

This is the user-facing interface. Every command is reachable from the command palette (FR-062).
Defaults can be rebound later. Rebinding is not part of v1.

**Notation**: `Ctrl-X` means Control+X. "Text input" means the editor, the find box, the palette
filter, and any rename or new-name prompt.

## Layout

```text
┌ Files ───────┬ Editor ─────────────────────────────┬ LLM Processes ─────────────────┐
│ tree         │ buffer with highlighting            │ Name▾ Mem CPU Up Status Conf   │
│              │ [find: ____ (this file ⇄ project)]  │ grouped list                   │
│              │                                     ├ System ────────────────────────┤
│              │                                     │ total / LLM / local / Fallow / │
│              │                                     │ remaining  (metric: PSS)       │
├──────────────┴─────────────────────────────────────┴────────────────────────────────┤
│ ? Help  Ctrl-P Commands  Tab Next pane  … context-sensitive hints (FR-060)           │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

- Pane borders can be dragged with the mouse (FR-063).
- At small sizes, panes become tabs that are switched with `Tab`.
- The process panel always shows its coverage note (FR-018): "Not covered: containers, remote
  machines", plus "WSL2" on Windows.

## Keyboard input mode (FR-067)

At startup, Fallow asks the terminal to report modifier keys unambiguously:
- **Linux and macOS**: Fallow pushes the kitty keyboard protocol flags (disambiguate escape codes)
  without first querying support, because a query round-trip would cost startup time
  (FR-041). Terminals that don't support the protocol ignore the request, and Fallow falls back
  to legacy key decoding. On exit, and on panic, Fallow pops the flags.
- **Windows**: the console input API already reports modifiers reliably, so no protocol is
  needed.

Every default binding below works in legacy mode, except where the
[Known terminal conflicts](#known-terminal-conflicts) table says otherwise.

## Panes and focus (FR-065)

| Command | Default key | Notes |
|---|---|---|
| Next / previous pane | `Tab` / `Shift-Tab` | **Primary** way to switch panes. In the editor, see the rule below. |
| Focus Files / Editor / Processes | `Alt-1` / `Alt-2` / `Alt-3` | Optional shortcut. On macOS it needs "Use Option as Meta key" (Terminal.app) or "Option key: Esc+" (iTerm2). The help overlay says so. |
| Release editor focus | `Esc` (in the editor, no find bar or selection active) | The next `Tab` / `Shift-Tab` then switches panes. |

**Tab in the editor**: In the editor, `Tab` / `Shift-Tab` indent and outdent, because a code
editor needs them. To leave the editor with the keyboard, press `Esc` and then `Tab`. While the
editor has focus, the hint bar shows `Esc Tab: next pane`. In every other pane, `Tab` switches
panes immediately.

## Global commands

| Command | Default key |
|---|---|
| Help overlay (FR-061, FR-066) | `F1`; `?` when focus isn't in a text input; palette "Help" |
| Command palette (FR-062) | `Ctrl-P` |
| Open project | `Ctrl-O` |
| Reload configuration | palette only |
| Quit (FR-007 warns if unsaved) | `Ctrl-Q` |

**Hint bar and help (FR-066)**: The hint bar always shows a way to open help that isn't `F1`,
because on Mac keyboards F-keys are media keys unless `Fn` is held:
- `? Help` when focus isn't in a text input;
- `Ctrl-P › Help` while typing.

## Editor

| Command | Default key |
|---|---|
| Save | `Ctrl-S` |
| Undo / Redo | `Ctrl-Z` / `Ctrl-Y` |
| Find (FR-005) | `Ctrl-F`: opens the find bar (see below) |
| Go to line (FR-009) | `Ctrl-G`: prompts for a line number, or `line:column` |
| Close buffer | `Ctrl-W` |
| Next / previous buffer | `Ctrl-PgDn` / `Ctrl-PgUp`; palette "Next buffer" / "Previous buffer" |
| Indent / outdent | `Tab` / `Shift-Tab` |

### Selection and clipboard (FR-008)

| Command | Default key / gesture |
|---|---|
| Extend selection | `Shift-←/→/↑/↓`, `Shift-Home` / `Shift-End`; mouse drag |
| Select all | `Ctrl-A` |
| Copy | `Ctrl-C`: copies the selection. **With no selection it does nothing**, and the hint bar shows `Quit: Ctrl-Q` for 3 seconds. |
| Cut | `Ctrl-X` |
| Paste | `Ctrl-V`, or the terminal's own paste (see below) |

- **Copy and cut** write the selection to the system clipboard in two ways:
  1. An **OSC 52** sequence, always emitted. It works over SSH in terminals that support it, and
     terminals that don't ignore it.
  2. The **platform clipboard** (Windows clipboard, macOS pasteboard, X11 or Wayland on Linux),
     when a local clipboard is reachable.
  Failure of either is silent. If both fail, the hint bar says `Copy failed: clipboard
  unavailable`.
- **Paste**: Fallow enables **bracketed paste**. Text pasted through the terminal (`Cmd-V`,
  `Ctrl-Shift-V`, right-click, or `Ctrl-V` in Windows Terminal) arrives as a single paste event
  and is inserted literally, with no auto-indent and no key interpretation. `Ctrl-V` that
  reaches Fallow as a key reads the platform clipboard. If that clipboard isn't available, for
  example over SSH, the hint bar suggests the terminal's own paste shortcut.
- Selection, copy, cut and paste also work in the find box and in name prompts.

### Find bar (FR-005)

| Command | Default key |
|---|---|
| Open find bar (scope: this file) | `Ctrl-F` |
| Toggle scope "this file" ⇄ "whole project" | `Tab` inside the find box |
| Next / previous match (this file) | `Enter` or `↓` / `↑` |
| Run project search; results list opens | `Enter` (scope: whole project) |
| Open a result | `Enter` or double-click on the result |
| Cancel a running search or close the find bar | `Esc` |

The current scope is always shown in the find bar. Project search respects `.gitignore` and runs
only when `Enter` is pressed, never while typing.

## Files pane (FR-002, FR-068)

| Command | Default key / gesture |
|---|---|
| Move selection | `↑` / `↓`, `PgUp` / `PgDn`, `Home` / `End` |
| Open file, or expand/collapse folder | `Enter`, double-click |
| Collapse folder or go to parent | `←` |
| Expand folder | `→` |
| New file | `a`; palette "New file" |
| New folder | `Shift-A`; palette "New folder" |
| Rename | `F2` or `r`; palette "Rename" |
| Delete | `Delete` or `d`; palette "Delete". **Always confirmed** |

- New items are created inside the selected folder, or next to the selected file.
- **Delete confirmation**: the dialog shows the full path, and for a folder the number of files
  and folders inside it. Default focus is **Cancel**. Confirm with `y`, or `Enter` on the Confirm
  button. Deletion is permanent. v1 has no trash integration, and the dialog says so.
- If an open buffer's file is renamed or deleted from this pane, the buffer follows the rename,
  or becomes unsaved after a delete (FR-006).

## Process panel

| Command | Default key / gesture |
|---|---|
| Sort by memory / CPU / uptime / name (FR-014) | `m` / `c` / `u` / `n` (repeat to reverse) |
| Sort by any column, including status and confidence | **click the column header** (click again to reverse); palette "Sort by …" |
| Move selection | `↑` / `↓`, `PgUp` / `PgDn`, `Home` / `End` |
| Expand / collapse group | `→` / `←`, `Space` |
| Details (signals, FR-025) | `Enter` |
| Go to working directory + recent changes (FR-017) | `g` |
| Pause / Resume (confirmation) | `p` / `r` |
| Kill (confirmation) | `Shift-K` (capital letter, to avoid accidental presses) |
| Toggle browser category | palette only |

Confidence has no single-key sort, because there is no free mnemonic letter. It is sorted by
header click or the palette. The current sort column shows `▾` or `▴` in the header.

## Confirmation dialogs (FR-032, FR-068)

- **Process actions** show the action, process name, PID, owner (in all-users mode), group size,
  and whether children are included. On Windows, pause and resume also show the R-7 difference
  text.
- **File delete** shows the path and, for folders, the item counts.
- In all dialogs, default focus is **Cancel**. `y`, or `Enter` on Confirm, confirms. `Esc`,
  `n` and `q` cancel. There is no "don't ask again".

## Help overlay content (FR-061)

The overlay lists every binding, grouped by pane, and the table under
[Known terminal conflicts](#known-terminal-conflicts). It also includes these notes:
- **Selecting text with the terminal**: "Fallow captures the mouse. To use your terminal's own
  text selection, hold a modifier while dragging. This is usually `Shift`; in iTerm2 it is
  `Option`, and in Terminal.app `Fn`."
- **Alt shortcuts on macOS**: "`Alt-1/2/3` need 'Use Option as Meta key' (Terminal.app ›
  Settings › Profiles › Keyboard) or 'Left Option key: Esc+' (iTerm2 › Settings › Profiles ›
  Keys). `Tab` / `Shift-Tab` always work."
- **F-keys on Mac keyboards**: "Hold `Fn` for `F1` / `F2`, or use `?` and the palette."
- **Leaving the editor**: "`Esc`, then `Tab`."

## Displayed semantics

| Element | Text shown in legend/help |
|---|---|
| Status `serving` | Local model server with at least one connected client. CPU may be low while the GPU works. |
| Status `LLM connected` | Has an open connection to an LLM provider or local model server. This does not mean a request is in progress: Fallow can't see traffic without reading payloads or extra privileges. |
| Status `busy` | Averaged above 2% of one CPU core over the last 10 s. |
| Status `idle` | None of the above. |
| Confidence `certain` | Matched a known signature. |
| Confidence `likely` | Seen only through its connections. It may be a false positive, for example when providers share CDN addresses. |
| Memory metric label | `footprint` (macOS), `private WS` (Windows), `PSS` / `RSS` (Linux, per process). The system summary lists the metric. "GPU memory (VRAM) not included" is shown where it applies. |
| Recent changes | "Changed in this directory in the last 30 min. Not proven to come from this process." |

## Mouse (FR-063)

- Click selects a row, file or editor position.
- Drag in the editor selects text.
- Double-click opens a file or search result, or expands a folder.
- Clicking a column header sorts by that column.
- The wheel scrolls the pane under the pointer.
- Dragging a border resizes the panes.

Every mouse action has a keyboard equivalent listed above. Mouse capture can be turned off with
`ui.mouse = false`, which hands selection back to the terminal.

## Known terminal conflicts

The table checks every default binding against each terminal's **default** settings:
- **Windows Terminal**: its shipped `defaults.json` plus the `settings.json` it generates on
  first run.
- **macOS Terminal.app** and **iTerm2**: their default profiles, plus macOS system shortcuts.

**Status of this check**:
- **Windows Terminal**: verified against its published `defaults.json`. The only default
  binding that intercepted a Fallow key was `Ctrl-Shift-F`, which has been removed.
- **Terminal.app and iTerm2**: checked against documented defaults. Rows marked **verify** must
  be confirmed in the macOS hardware run-through (plan.md, Release blockers, item 4). That
  run-through decides whether "none intercepted" holds on macOS.

| Key | Windows Terminal | Terminal.app | iTerm2 |
|---|---|---|---|
| `Tab`, `Shift-Tab` | free | free | free |
| `Esc` | free | free | free |
| `Alt-1/2/3` | free (WT uses `Ctrl-Alt-1…9` for tabs) | needs Option as Meta, off by default | needs Option = Esc+, off by default |
| `F1` | free | free; needs `Fn` on Mac keyboards (OS setting) | free; needs `Fn` on Mac keyboards |
| `F2` | free | free; needs `Fn` | free; needs `Fn` |
| `?` | free | free | free |
| `Ctrl-P` | free (WT palette is `Ctrl-Shift-P`) | free | free |
| `Ctrl-O` | free | free (raw mode disables the tty "discard" char) | free (same) |
| `Ctrl-Q` | free | free (raw mode disables XON/XOFF) | free (same) |
| `Ctrl-S` | free | free (raw mode disables XON/XOFF) | free (same) |
| `Ctrl-Z` | free | free (raw mode disables job-control signals) | free (same) |
| `Ctrl-Y` | free | free (raw mode disables the tty "delayed suspend" char) | free (same) |
| `Ctrl-F` | free (WT find is `Ctrl-Shift-F`) | free | free |
| `Ctrl-G` | free | free | free |
| `Ctrl-W` | free (WT close pane is `Ctrl-Shift-W`) | free | free |
| `Ctrl-A` | free (WT select-all is `Ctrl-Shift-A`) | free | free |
| `Ctrl-C` | **conditional**: WT's generated settings bind `Ctrl-C` to copy *only while WT has its own selection*; otherwise it passes through. Fallow's mouse capture means WT rarely has a selection. | free (raw mode disables SIGINT) | free (same) |
| `Ctrl-X` | free | free | free |
| `Ctrl-V` | **intercepted, same result**: WT's generated settings bind `Ctrl-V` to paste. The text arrives as a bracketed paste, which Fallow handles as a paste. | free (raw mode disables the tty "literal next" char) | free (same) |
| `Ctrl-PgUp/PgDn` | free (WT scroll is `Ctrl-Shift-PgUp/PgDn`) | **verify**: Terminal.app's PgUp/PgDn handling may consume the combination | **verify** |
| `Shift-←/→` | free | free | free |
| `Shift-↑/↓` | free | **verify** | free |
| `Shift-Home/End` | free | **verify**: Terminal.app maps Home/End to scrollback by default and may send unmodified Home/End when Shift is held | **verify** |
| `Home`/`End`/`PgUp`/`PgDn` | free | **intercepted by default**: they scroll the scrollback. Fallow uses the alternate screen, where Terminal.app passes them to the app. **verify** | free in alternate screen; **verify** |
| `Delete` | free | free (`Fn-Delete` on Mac keyboards) | free (`Fn-Delete`) |
| `Enter` | free (WT's `Enter` = copy applies only while WT has a selection) | free | free |
| `↑↓←→` | free (WT uses `Alt-arrows` for its panes) | free (macOS uses `Ctrl-arrows` for Spaces; Fallow doesn't use them) | free |
| `a`, `Shift-A`, `d`, `r`, `g`, `p`, `m`, `c`, `u`, `n`, `Shift-K`, `Space` | free | free | free |
| Mouse drag, click, wheel | captured by Fallow; the terminal's own selection needs `Shift` | captured; own selection needs `Fn` (**verify**) | captured; own selection needs `Option` |

**Fallbacks where a row is intercepted on macOS**:
- Buffer switching, selection extension and paging all have palette commands.
- `Shift-Home/End` selection can also be done with `Shift-←/→` or a mouse drag.

Any macOS row that the run-through finds intercepted gets a documented alternative key before the
"beta" label is removed.

**Multiplexers (not terminals, noted for completeness)**:
- GNU screen's default prefix `Ctrl-A` intercepts "select all". Use palette "Select all".
- tmux (prefix `Ctrl-B`) doesn't collide with any default. It needs
  `set -g extended-keys on` for kitty-protocol modifiers.
