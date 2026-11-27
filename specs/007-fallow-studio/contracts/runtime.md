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
| session_total_usd | The running total the runtime last reported for the session resumed or forked (`Outcome.session_total_usd`, as Fallow recorded it); none for a new session |

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
| `launched` | (none) | Nothing. Sent once, as soon as the agent's program runs (2026-11-02). Every runtime sends it: it is how Fallow knows that an agent stopped before it ran never saw its instruction, whatever the runtime reports afterwards |

The step's claim, its cost and how it ended come in the `Outcome`, not as events.

Thinking is never passed up (spec, out of scope). The Claude runtime drops `thinking` blocks when it reads stream-json.

## Outcome

```text
Outcome { result: Option<Value>, failure: Option<String>, cost_usd: Option<f64>,
          session_total_usd: Option<f64>, session: Option<String>, denials: Vec<String>,
          duration: Duration }
```

- `cost_usd` is what this call alone cost; Fallow adds these up (a chat's total, a loop's spend and its limits). A runtime whose reported cost is a running total for the whole session says so in its adapter, returns that total as `session_total_usd`, and gives the call's own cost as the difference from the call's `session_total_usd` (`agent::turn_cost`). A total lower than that is a new count and is the call's own cost. Claude Code is such a runtime: its `total_cost_usd` goes on across `--resume` and `--fork-session` (2026-11-25). The adapter passes `--max-budget-usd` as the call's budget, unchanged: whether Claude Code checks it against the carried total is unverified, and raising it could let a resumed call spend more than its budget. A runtime that reports each call's own cost leaves `session_total_usd` empty.

- `result` is the structured result, when it parses; `failure` says why there is none (a time-out, a crash, an error).
- `session` is the session the call ran in: for a fork, the new id from the stream.
- `denials` are the commands the rights refused.
- A lost session is recognised by the loop (006): a resumed call that failed with "no conversation found" and no activity.

## The runtimes in this version

| Name | What |
|---|---|
| `claude` | `claude -p`, with stream-json output, `--json-schema`, `--resume`, `--fork-session`, `--include-partial-messages`, `--allowedTools`, `--disallowedTools`, `--permission-mode dontAsk` or `default`, `--permission-prompt-tool`, `--mcp-config`, `--max-budget-usd` and `--model` |
| `fake` | `fallow-fake-agent`, which plays TOML scenarios. Used by the tests (006 R-15), and by US7's independent test of the interface |

## The registry

`agent::registry::runtime(name, settings)` gives the runtime a definition names. Its program comes from the settings' `[agents.<name>]` profile; `claude` falls back to the `claude` program on the PATH. A runtime Fallow does not know, one without a profile, or one whose program is not found is refused with a message that names it; the step does not start and the loop asks the engineer. `GET /api/runtimes` lists every runtime with `installed`, `version` (the first line of its `--version`) and the models a definition may name.

> Replaced by 017 (specs/017-agent-file-fields/contracts/runtime-model-naming.md): `GET /api/runtimes` no longer lists the models a definition may name; it gives each runtime's `model_naming`. A call without a model is refused; there is no fallback.

## 012 amendments

Feature 012 (solo sessions) amends this contract; [012's runtime contract](../../012-solo-session/contracts/runtime.md) has the details:
- every runtime declares `Capabilities { images, web_off, control_files }`;
- `Call` gains `attachments`, `web` and `user_settings`;
- `tool_call.kind` gains `web`;
- permission prompts reach Fallow in the contract's terms (`PermissionRequest`), mapped by each adapter;
- which settings apply: none from a working copy; none of the runtime's own for unattended kinds; the engineer's user settings for the assistant, with every command and edit still sent to Fallow;
- every Claude call passes `--tools` with a fixed list, and `--model` always;
- the assistant's result schema, `assistant_reply`.

`GET /api/runtimes` gains `capabilities`.
