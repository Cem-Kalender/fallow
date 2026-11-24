# Contract: Configuration and State Files (002 additions)

Location and rules as in [001 contracts/config.md](../../001-llm-aware-tui-editor/contracts/config.md):
`<config_dir>/fallow/`, `--config-dir` overrides it, invalid entries are reported as
`file:line: message` and the rest still applies, and user files are reloaded only by "Reload
configuration".

002 adds one user-edited file (`profiles.toml`), one section in `settings.toml`, and one file that
Fallow writes itself (`projects.toml`). `projects.toml` is the only file Fallow writes in its
config directory (Principle V).

## settings.toml: `[sessions]`

```toml
[sessions]
scrollback_lines = 10000      # FR-004; 1000..=100000
attention_delay_ms = 5000     # FR-009; 1000..=600000
max_sessions = 8              # FR-007; 1..=16
leave_key = "ctrl-]"          # FR-003; one key: "ctrl-]", "ctrl-\\", "ctrl-5", "f12" …
worktrees = "auto"            # FR-031; "auto": a new worktree by default for git projects; or "never"
worktree_root = ""            # FR-034; empty: <parent of the project>/<project>.worktrees
```

## profiles.toml

```toml
[[profile]]
id = "claude"                 # [a-z0-9_-]{1,32}; same id as a built-in replaces it completely
name = "Claude Code"
command = "claude"            # PATH lookup (with PATHEXT on Windows) or an absolute path
args = []                     # passed as given; no shell expansion
enabled = true                # false hides a built-in

[profile.env]                 # optional additions to the inherited environment
CLAUDE_CODE_SOMETHING = "1"   # names are shown in Fallow; values are never shown or logged
```

Built-in profiles (overridable by `id`). Each is offered only if its command is found (FR-023):

| id | name | command |
|---|---|---|
| `claude` | Claude Code | `claude` |
| `codex` | Codex CLI | `codex` |
| `gemini` | Gemini CLI | `gemini` |
| `aider` | Aider | `aider` |
| `shell` | Shell | `$SHELL`, else `/bin/sh` (Linux, macOS); `pwsh.exe`, else `powershell.exe` (Windows) |

Validation: `id` pattern and uniqueness (a repeat uses the later entry and warns), `name` 1..=64
chars, `command` non-empty, `args` strings, `env` names matching `[A-Za-z_][A-Za-z0-9_]*`.

## projects.toml (written by Fallow)

```toml
# Written by Fallow. Edits are read at the next start.
selected = 1                   # index into [[project]], optional

[[project]]
path = "/home/me/src/api"      # absolute
name = "api"                   # optional; folder name when absent

[[project]]
path = "/home/me/src/web"
```

- Written atomically (temporary file in the same directory, then rename) after every change to the
  list (FR-015). A failed write is shown as a notice; the in-memory list stays as it is.
- Read once at startup, after the first frame (FR-028). An invalid entry is skipped and reported
  as `projects.toml:line: message`; the others load.
- Contains paths and names only. No session data, output or environment is ever written
  (FR-026, FR-027).
- A folder given on the command line is added (if new), selected, and written (FR-017).
