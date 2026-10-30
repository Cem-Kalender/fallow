# Contract: the runtime interface

Every agent step and every turn of the lead goes through one interface (FR-029, research S-13). Nothing above it knows which provider runs the agent.

```rust
pub trait Runtime: Send + Sync {
    fn name(&self) -> &str;
    fn available(&self) -> Availability;          // installed, version
    fn start(&self, call: &Call, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome;
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
| rights | `{edits: None \| Anywhere \| Only(globs), protect[], commands[], any_command, no_commands, prompt_tool?}`. Reading is always allowed (`src/agent/rights.rs`) |
| partial | Stream the reply as `message_delta` events, for chats |
| mcp | The bridge configuration that gives the step `ask`, `progress` and, in chats, `approve` |
| model | From the definition |
| limits | Time, turns and cost |

**Rights are given as data.** Each runtime turns them into its own enforcement. For Claude Code (research S-7, verified by T001):
- `--allowedTools` rules under `--permission-mode dontAsk`, with each command allowed for both `Bash` and `PowerShell`;
- `--disallowedTools` for protected files (`Edit(<path>)`, `Write(<path>)`), and for `Bash PowerShell` in Read only mode;
- in Ask mode, `--permission-mode default` with `--permission-prompt-tool mcp__fallow__approve`.

Commands Claude Code classifies as read-only run in every mode but Read only. A runtime that cannot enforce a right refuses the call. It never runs with fewer restrictions.

## Events

| Event | Fields | Shown as |
|---|---|---|
| `message` | text | What the agent says (narration) |
| `message_delta` | text | Partial text, for the chat's live reply only |
| `tool_call` | id, kind (`read`, `edit`, `write`, `run`, `search`, `mcp`, `other`), summary, detail | A feed line. For edits, `detail` is the diff (old and new text); for runs, the command |
| `tool_result` | id, ok, refused, output | The command's output and result: ok, failed, or refused by the rights. The page keeps up to 64 KB |

The step's claim, its cost and how it ended come in the `Outcome`, not as events.

Thinking is never passed up (spec, out of scope). The Claude runtime drops `thinking` blocks when it reads stream-json.

## Outcome

```text
Outcome { result: Option<Value>, failure: Option<String>, cost_usd: Option<f64>,
          session: Option<String>, denials: Vec<String>, duration: Duration }
```

- `result` is the structured result, when it parses; `failure` says why there is none (a time-out, a crash, an error).
- `session` is the session the call ran in: for a fork, the new id from the stream.
- `denials` are the commands the rights refused.
- A lost session is recognised by the loop (006): a resumed call that failed with "no conversation found" and no activity.

## The runtimes in this version

| Name | What |
|---|---|
| `claude` | `claude -p`, with stream-json output, `--json-schema`, `--resume`, `--fork-session`, `--include-partial-messages`, `--allowedTools`, `--disallowedTools`, `--permission-mode dontAsk` or `default`, `--permission-prompt-tool`, `--mcp-config`, `--max-budget-usd` and `--model` |
| `fake` | `fallow-fake-agent`, which plays TOML scenarios. Used by the tests (006 R-15), and by US7's independent test of the interface |
