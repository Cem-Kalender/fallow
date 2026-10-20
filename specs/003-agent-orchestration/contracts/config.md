# Contract: Configuration (003 additions)

Location and rules as in [002 contracts/config.md](../../002-agent-sessions/contracts/config.md).
003 adds one section to `settings.toml` and one key to `profiles.toml`. It writes no new file in
the config directory. Orchestration state is memory-only (FR-023).

## settings.toml: `[orchestration]`

```toml
[orchestration]
enabled = true                # false: no session is started with the bridge; no channel is ever opened
request_timeout_s = 600       # FR-012; 60..=3600: unanswered delivery, task and start requests expire
delivery_delay_ms = 3000      # FR-014; 500..=30000: pause in the engineer's typing before a delivery
permission_default_min = 30   # FR-015; 1..=480: preselected duration when granting
wait_block_s = 45             # research R-4; 5..=55: longest single event_wait call
rate_per_min = 30             # FR-008; 1..=600 tool calls per session per minute
board_messages = 1000         # FR-008; 100..=1000 per board
activity_entries = 5000       # FR-023; 500..=5000
```

Changing `enabled` takes effect for sessions started afterwards. Other values take effect at once
after "Reload configuration".

## profiles.toml: `orchestration`

```toml
[[profile]]
id = "claude-work"
name = "Claude Code (work)"
command = "claude"
args = ["--model", "opus"]
orchestration = "claude"      # "claude" | "codex" | "gemini" | "none" (default for user profiles)
```

- The value selects how Fallow connects the agent for one run ([research R-5](../research.md)).
- Built-ins: `claude` → `"claude"`, `codex` → `"codex"`, `gemini` → `"gemini"`, `aider` and
  `shell` → `"none"`.
- A user profile that replaces a built-in by `id` and omits the key keeps the built-in's value.
- Help (F1, "Agent orchestration") shows this key as the one-step setup for other agents (FR-004).

## Files Fallow creates outside the config directory

All of them live under `<cache_dir>/fallow/run/` (mode `0700`, Linux and macOS). They exist only
while Fallow runs, and nothing in them is orchestration content.

| File | When | Contents |
|---|---|---|
| `<pid>.sock` | first orchestrated session starts, removed on exit | the channel (Linux, macOS) |
| `gemini-<session>.json` (0600) | a Gemini session starts with orchestration, removed when it exits | the `fallow` server entry including that session's token |

On Windows the channel is a named pipe, not a file, and the Gemini file is created in
`<cache_dir>\fallow\run\` with an owner-only ACL.
