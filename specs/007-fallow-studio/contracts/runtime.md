# Contract: the runtime interface

Every agent step and every turn of the lead goes through one interface (FR-029, research S-13). Nothing above it knows which provider runs the agent.

```rust
pub trait Runtime: Send + Sync {
    fn name(&self) -> &str;
    fn available(&self) -> Availability;          // installed, version, models
    fn start(&self, call: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> End;
    // A call with `session: Session::Resume(id)` or `Session::Fork(id)` resumes or forks.
}
```

## Call

| Field | Meaning |
|---|---|
| instruction | The step's instruction (the user turn) |
| system | Fallow's rules for the kind, plus the definition's body |
| schema | The JSON Schema of the result (contracts/step-results.md) |
| session | `New(id)`, `Resume(id)` or `Fork(id)` |
| cwd | The step's worktree, or the folder for a plain project's chat |
| rights | `{read: true, edit_paths[], commands[], any_command: bool, prompt_tool?}` |
| mcp | The bridge configuration that gives the step `ask`, `progress` and, in chats, `approve` |
| model | From the definition |
| limits | Time, turns and cost |

**Rights are given as data.** Each runtime turns them into its own enforcement. For Claude Code, that is `--allowedTools` rules under `dontAsk`, and in Ask mode `--permission-prompt-tool` (research S-7). A runtime that cannot enforce a right refuses the call. It never runs with fewer restrictions.

## Events

| Event | Fields | Shown as |
|---|---|---|
| `message` | text | What the agent says (narration) |
| `message_delta` | text | Partial text, for the chat's live reply only |
| `tool_call` | id, kind (`read`, `edit`, `write`, `run`, `search`, `mcp`, `other`), summary, detail | A feed line. For edits, `detail` is the diff (old and new text); for runs, the command |
| `tool_result` | id, ok, output (trimmed to 64 KB) | The command's output and result |
| `refused` | id, what | A command the rights refused |
| `result` | value, cost_usd, session, turns | The step's claim |
| `failed` | why, session? | No usable result: a time-out, a crash, a lost session (006) |

Thinking is never passed up (spec, out of scope). The Claude runtime drops `thinking` blocks when it reads stream-json.

## End

```text
End { result: Option<Value>, failure: Option<String>, cost_usd: Option<f64>,
      session: Option<String>, refused: Vec<String>, duration_ms: u64, lost_session: bool }
```

## The runtimes in this version

| Name | What |
|---|---|
| `claude` | `claude -p`, with stream-json output, `--json-schema`, `--resume`, `--fork-session`, `--include-partial-messages`, `--allowedTools`, `--permission-mode dontAsk`, `--permission-prompt-tool`, `--mcp-config`, `--max-budget-usd` and `--model` |
| `fake` | `fallow-fake-agent`, which plays TOML scenarios. Used by the tests (006 R-15), and by US7's independent test of the interface |
