# Contract: UI Commands, Keys and Layout (002 additions)

Extends [001 contracts/ui-commands.md](../../001-llm-aware-tui-editor/contracts/ui-commands.md).
Everything there still holds in the editor view. Every new command is in the palette (FR-022), and
every new key is in the Known terminal conflicts table below (001 FR-064).

## Views

Fallow has two views. One key switches between them; each view keeps its own focus and
selection (FR-020).

```text
Agent workspace (default, FR-019)
┌ Projects ────────┬ Sessions: api ─────────────────────────────┬ LLM Processes ─────────────┐
│ ▸ api      2 ●1  │ #1 Claude Code  needs attention  0:42      │ Name▾ Mem CPU Status       │
│   web      1     │ #2 Shell        working          0:03      │ grouped list               │
│   docs           ├ #1 Claude Code · api ──────────────────────┤ (#1 api, #2 api, #3 web)   │
│                  │ (the agent's own screen, full size)        ├ System ────────────────────┤
│ + Add project    │                                            │ total / LLM / Fallow / …   │
├──────────────────┴────────────────────────────────────────────┴────────────────────────────┤
│ ? Help  Ctrl-P Commands  n New session  Enter Open  Ctrl-E Editor  … (context hints)        │
└─────────────────────────────────────────────────────────────────────────────────────────────┘

Editor view (Ctrl-E): the 001 layout for the selected project
(Files | Editor | LLM Processes), with the project name in the Files title.
```

- With a session focused, the session pane takes the whole middle column; the session list shrinks
  to one line naming the session.
- In split layout, two session panes share the middle column side by side (FR-011).
- Below 100×24 the columns become tabs (001 rules). A focused session always gets the full width.
- `●n` on a project row is the number of its sessions that need attention.

## Global (both views)

| Command | Default key |
|---|---|
| Switch agent workspace ⇄ editor view | `Ctrl-E` |
| Session overview (all projects) | `Ctrl-T` |
| Next / previous session | `Ctrl-N` / `Ctrl-B`; palette "Next session" / "Previous session" |
| Help, palette, quit | as 001 (`F1`/`?`, `Ctrl-P`, `Ctrl-Q`) |

`Ctrl-Q` with running sessions shows one confirmation listing them; confirming stops them
gracefully and quits (spec Edge Cases).

## Projects list

| Command | Default key |
|---|---|
| Move | `↑` / `↓`, `Home` / `End` |
| Show sessions of the project | `Enter`, click |
| New session in this project | `n` (profile menu) |
| Add project | `a` (path prompt; also `Ctrl-O` in both views) |
| Remove from list | `Delete` / `d` (confirmation if it has sessions; the folder is never touched) |
| Rename | `F2` / `r` |
| Move up / down | `Shift-↑` / `Shift-↓` |
| Filter processes to this project | `f` (toggle; the panel title shows the filter) |
| Open in editor view | `e` |

## Sessions list (of the selected project) and overview

| Command | Default key |
|---|---|
| Move | `↑` / `↓` |
| Focus session | `Enter`, double-click |
| New session | `n` |
| Split with the focused session | `s` |
| Restart (exited session) | `R` |
| Stop (confirmation, 001 FR-032) | `Shift-K` |
| Close (exited session only) | `w` |
| Jump to the agent's process in the LLM panel | `p` |

## Focused session

| Command | Default key |
|---|---|
| Leave the session (keys go to Fallow again) | `Ctrl-]` (configurable: `sessions.leave_key`; research R-3) |
| Send the leave key itself to the agent | `Ctrl-]` twice |
| Scroll back / forward | `Shift-PgUp` / `Shift-PgDn` after leaving, or the mouse wheel |
| Search scrollback | `Ctrl-F` after leaving |
| Select text | mouse drag (unless the agent enabled mouse reporting; then `Shift`-drag) |
| Copy selection | `Ctrl-C` after leaving, or the palette |

**Every other key goes to the agent unchanged, including `Esc`, `Tab`, `Ctrl-C`, `Ctrl-Q`,
`Ctrl-P` and `F1` (FR-003).** The hint bar shows only the leave key while a session has focus:
`<leave key> Leave session`.

## LLM process panel (addition)

| Command | Default key |
|---|---|
| Jump to the session of the selected process | `j` (only for processes started as sessions) |

Process rows started as sessions show `#n project` after the name (FR-013).

## Known terminal conflicts (002 keys)

Checked like 001 (FR-064), against the terminals' defaults and the built-in agents' default keys
(research R-3). `Ctrl-E`, `Ctrl-T`, `Ctrl-N` and `Ctrl-B` are Fallow's only while no session has
focus; inside a session they go to the agent, where several of them are used (Ctrl-T, Ctrl-B).

| Key | Windows Terminal | Terminal.app | iTerm2 | Built-in agents |
|---|---|---|---|---|
| `Ctrl-]` | free (**verify**) | free (**verify**) | free (**verify**) | not bound by Claude Code, Codex, Gemini CLI; Aider (prompt_toolkit) uses it for character search, reachable by pressing it twice |
| `Ctrl-E` | free | free | free | n/a (outside sessions) |
| `Ctrl-T` | free (WT new tab is `Ctrl-Shift-T`) | free | free | n/a (outside sessions) |
| `Ctrl-N` | free (WT new window is `Ctrl-Shift-N`) | free | free | n/a (outside sessions) |
| `Ctrl-B` | free | free | free | n/a (outside sessions) |

Layouts where `]` needs AltGr (German, Nordic): many terminals also send `Ctrl-]` for `Ctrl-5`, and
the key is configurable (`sessions.leave_key`). Fallow accepts both encodings of the key (legacy
`0x1D`, which some decoders report as `Ctrl-5`, and the kitty-protocol form).
