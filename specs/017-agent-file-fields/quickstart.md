# Quickstart: checking 017 by hand and by test

## Prerequisites

- `cargo build --features automation` and `cargo test --workspace --features automation` pass.
- Node 22 or later and Chrome, for the list check (Q5).
- A scratch `FALLOW_HOME`/config folder; never the engineer's own `~/.fallow`.

## Q1: the checks (US1)

`cargo test --lib defs::agent` and `cargo test --test us6_defs --features automation`.

Expected: for each of the 7 required fields, absent and empty are both refused, at the right line, with the "what belongs there" text of [contracts/agent-definition.md](contracts/agent-definition.md); `model: opus` and `model: claude-sonnet-5-5` with `runtime: fake` are refused with the hint; a lead with `may_run: [gates]` is refused; every shipped agent parses.

## Q2: the model at the call (US1 AS5, AS6)

`cargo test --test us7_runtime --features automation` and the `claude.rs` unit tests.

Expected: the fake agent receives `--model fake-1` exactly; a call without a model is refused before the program starts; a scenario step ending with the provider's "model not found" error shows in the step's failure with the agent's name.

## Q3: the page is the file (US2)

1. Start Fallow in a scratch home; open Agents.
2. Pick `builder`: one text editor with the whole file; no select or input for agent fields.
3. Change `description`, save; diff the file: one line changed.
4. "+ New agent", name `ui-checker`: the template of [contracts/agent-definition.md](contracts/agent-definition.md#the-template-for-a-new-agent). Save untouched: 6 missing fields and the empty instructions, at their lines; no file written.
5. Fill it in from the comments only (`runtime: claude`, `model: claude-sonnet-5-5`, `kind: checker`, `may_run: [gates]`, `returns: verdict`, a description and a line of instructions). Save: it is listed under Checkers.

Automated: `cargo test --test us8_agent_fields --features automation` (text save, `{fields}` refused, template refused untouched and accepted filled in).

## Q4: existing files (US3)

`cargo test --test us17_existing --features automation` builds a config folder as 0.3.1 left it (unedited `builder`, edited `reviewer` with `model: sonnet`, own `ui-checker` with `model: haiku` and no `may_run` used by a project pipeline, a project lead override with `model: opus`, a spec session started on `lead` with `opus`), then starts Fallow.

Expected: `builder` replaced; `reviewer` kept byte for byte, `reviewer.md.new` written, listed in `unusable` with `in_force: default`, steps run with the default; `ui-checker` unusable with `in_force: null`, the pipeline's problem names `ui-checker.md` and its line; the project lead's problem names its file; the spec session reports its lead changed and the next message starts a new agent session; no agent file's bytes changed.

By hand: open Agents, click `reviewer` under "Files Fallow could not use": its text with problems at their lines; fix the model; save; it leaves the list.

## Q5: the list scrolls (US4)

```sh
FALLOW_VISUAL_ROOT=<empty folder> FALLOW_VISUAL_AGENTS=20 cargo test --test visual_017 --features automation -- --ignored --nocapture &
node specs/017-agent-file-fields/checks/agents.js <that folder>
```

Expected, at 1440 × 900 and at 390 × 844: scrolling the list to its end leaves the page's scroll position at 0; the last agent is selected without scrolling the page and the editor shows it; the list keeps its position after the selection; below 900 px the list is at most 40 % of the window's height; the only `select` elements are the project and "Save for" controls; Tab through the list keeps the focused agent visible. The script prints each check and exits non-zero on a failure.

## Q6: nothing else moved

`cargo test --workspace --features automation`, `cargo clippy --all-targets --features automation -- -D warnings`, and the CI budget benchmarks (constitution VIII).
