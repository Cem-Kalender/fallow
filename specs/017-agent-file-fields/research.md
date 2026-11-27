# Research: Agent files are complete and say what they need (017)

Each decision is taken against the code on `origin/main` (0.3.1) and the spec's Clarifications.

## R-1: Where each runtime declares its model naming

**Decision**: a new `ModelNaming` in the runtime contract (`src/agent/mod.rs`):

```text
ModelNaming { prefix: &'static str, example: &'static str, lookup: &'static str, lookup_url: &'static str }
```

- Each adapter declares a constant: `claude::MODEL_NAMING` (`prefix: "claude-"`, `example: "claude-sonnet-5-5"`, `lookup: "Anthropic's models overview"`, `lookup_url: "https://platform.claude.com/docs/en/models/overview"`) and `fake::MODEL_NAMING` (`prefix: "fake-"`, `example: "fake-1"`, `lookup: "the fake runtime's scenario"`, no address).
- `agent::registry::model_naming(name) -> Option<ModelNaming>` maps a runtime name to its adapter's constant. It needs no settings, so definitions can be checked without a configured profile (the `fake` profile may be absent and the file still checked).
- One shared rule, `ModelNaming::fits(name)`: starts with `prefix`, is at most 64 characters, and the rest is non-empty and only `a-z`, `0-9`, `.` and `-`.
- `ModelNaming::hint()` gives the text used in every message and in the template: "`claude-` followed by lowercase letters, digits, `.` and `-`, for example `claude-sonnet-5-5`; look the name up in Anthropic's models overview (https://platform.claude.com/docs/en/models/overview)".

**Rationale**: constitution IX: what is particular to one runtime lives in its adapter, and what Fallow needs from a runtime is part of the contract. A prefix plus a fixed character rule covers Anthropic's names today and the common forms of other providers' names, without a list. The fixed rule keeps run records to identifiers (constitution V, R-8).

**Alternatives considered**:
- A function pointer per adapter (`fits: fn(&str) -> bool`): more freedom than any runtime needs now; not serializable for `GET /api/runtimes`. The shared rule can grow into this when a runtime needs it.
- A regular expression per runtime in the settings: puts model naming in the engineer's hands and in config; the brief says the runtime declares it.
- Adding it to `Capabilities`: `Capabilities` comes from a constructed runtime (`fake` needs its profile); the naming must be known without one.

## R-2: The checks in `defs::agent::parse`

**Decision**:
- `parse(file, stem, text, source, runtimes)` takes `runtimes: &[(&str, ModelNaming)]` instead of `&[&str]`. `defs::runtimes()` builds it from `defs::RUNTIMES` and `registry::model_naming`; `Library::load`, `put_agent` and the tests call it.
- A field present without a value (`model:`, `model:   # comment`) is missing. `str_of` already returns `Some("")` with its line; each required field now treats `""` as missing, at that line. An absent field is reported at the frontmatter's closing `---` line (where it would be written), not line 1.
- `model`: required; checked against the naming of the file's runtime when the runtime is known. Unknown or missing runtime: the model is not checked against a form, and the model's problem (if missing) gives every runtime's hint.
- `may_run`: required. For `lead` and `assistant`, it must be an empty list. `[]` must parse: today `split_items("")` gives no items and `value("[]")` gives `List([])`, so it does.
- The messages are fixed in [contracts/agent-definition.md](contracts/agent-definition.md) (the "What belongs there" column).

**Rationale**: the parser is the single check every path goes through (load, save, the shipped defaults' test), so the spec's FR-001 to FR-006 hold everywhere at once.

**Alternatives considered**: a separate "lint" pass on save only: files on disk would then be used while the save path refuses them, which FR-006 forbids.

## R-3: Files Fallow could not use, as data

**Decision**: `Library` gains `unusable: Vec<Unusable>` with `Unusable { name, file, source, text, problems: Vec<Problem>, in_force: Option<Source> }`. It is filled in `Library::load` when an agent file fails (`in_force` is the source of the definition of that name still in force after all places are read, or `None`). `lib.problems` keeps its strings for everything (pipelines too), as today.

`Library::why_not(name) -> String`: "there is no agent named X" when no file of that name exists; otherwise "X cannot be used: <file>:<line>: <problem>" (the first problem, and "and N more" when there are more). It is used by:
- `defs::pipeline::parse`: its `kind_of` becomes `&dyn Fn(&str) -> Result<Kind, String>`, the error being `why_not`;
- `run::feature` (the two "there is no agent named" errors: a step's check and `start`);
- `studio::lead_problem` ("the lead X is gone").

**Rationale**: FR-013 needs each file's text and problems by line, and FR-019 needs the reason wherever an agent is looked up; both come from the one load.

**Alternatives considered**: parsing `lib.problems` strings in the page: brittle, and the strings do not carry the text.

## R-4: The Agents page

**Decision**: `src/service/web/screens/agents.js` keeps the list, the project control, "Save for", "Use for this project", Reset/Delete and the `.md.new` notice. It drops `fields()`, `s.form`/`s.orig`, the instructions-only textarea, "Edit as text"/"Back to the fields", `RETURNS` and the runtime/model logic of `act("field")`. The editor is always `#ag-text` with the whole file; `dirty()` compares it with the file's text. Saving sends `{text}` only.

- "+ New agent" asks for the name (as today), then `GET /api/agents/template?name=<n>` and shows that text.
- The list's "Files Fallow could not use" card lists `unusable` entries as buttons (name, place, "the default is in force" / "yours is in force" / "not in force"); selecting one opens its text in the editor with its problems at their lines, "Save for" set to its own place. The remaining strings of `problems` (pipelines) stay as text.
- A problem's `https://` address is shown as a link; a click opens it in the engineer's browser through the page's link handling (016 FR-034 / 014), never in the window.
- The server: `PUT /api/agents/<name>` with `{fields}` is removed (answer.rs route, `Req::PutAgentFields`, `Studio::put_agent_fields`, `agent::set_fields`, `set_body`, `FieldValue`, `FIELD_KEYS`, `write_value` and their tests). A body with `fields` now gets 409 "an agent is saved as its whole text".

**Rationale**: the engineer: "no dropdowns, just the md file". One editor removes the form's round-trip writer (008's hardest part) and its tests.

**Alternatives considered**: keeping the form read-only as a summary: still shows `kind`/`model` outside the file and invites the dropdown back.

## R-5: The template

**Decision**: built in Rust, `defs::agent::template(name, runtimes) -> String`, served by `GET /api/agents/template?name=<n>` (`Req::AgentTemplate`), name checked by `check_name`. The text:

```markdown
---
name: <n>                # the file's name, in a-z, 0-9 and -
description:             # one line: what this agent does
runtime:                 # which program runs it: claude or fake
model:                   # the model's full name. claude: claude-… (for example claude-sonnet-5-5), see Anthropic's models overview: https://platform.claude.com/docs/en/models/overview. fake: fake-… (for example fake-1)
kind:                    # maker (changes files), checker (only runs and verifies), lead or assistant
may_run:                 # the commands it may run, as a list: [gates, plan.commands, "npm test"]; [] for none; always [] for a lead or an assistant
returns:                 # work for a maker, verdict for a checker, reply for a lead, assistant_reply for an assistant
# limits: {minutes: 30, turns: 60, usd: 2}   (optional: per step)
# Below the closing --- line: the agent's instructions, in plain text.
---
```

The runtimes' part of the comments comes from `defs::runtimes()` (R-1), so a new runtime appears without touching the template; the route passes only those usable on this machine (`defs::usable_runtimes(settings)`: a profile in the settings, `claude` when none; spec Clarifications, decided during the build). `name:` is the only value filled in.

**Check of the subset**: a value followed by ` #` is a comment (`strip_comment`), so `description:             # one line…` reads as an empty `description` and is reported missing at its line. Full-line `#` comments are allowed. The template, untouched, fails with exactly 6 missing fields and the empty instructions (a test).

**Rationale**: one source of the rules (Rust), testable in `cargo test`, and the page cannot drift from it.

**Alternatives considered**: built in JavaScript from `GET /api/runtimes`: a second copy of the field rules in the page.

## R-6: The model at the call

**Decision**:
- `claude.rs` `args`: no fallback. A `Call` with `model: None` or empty is refused in `Claude::start` before the process starts, with the outcome's failure "the agent has no model: its definition must name one" (FR-007). `registry::models` is removed.
- `GET /api/runtimes` (`run/engine.rs` `runtimes`, `registry::list`): `models` is replaced by `model_naming: {prefix, example, lookup, lookup_url, hint}`.
- A wrong name (FR-009): Claude Code ends with an error result naming the model; Fallow's adapter already turns an error result into the failure "the agent ended with an error: …", shown in the step and in the chat. No new code; a test with the fake agent playing that error checks it is shown with the agent's name.
- `pipelines.js` uses `runtimes` only for `installed`; it is unaffected beyond the removed `models` key.

## R-7: The agent list's scrolling

**Decision**: `app.css`: over 900 px, keep the sticky list with its own scroll (it is there since 2026-11-01) and verify it with 20 agents; below 900 px, `.ag-side` gets `max-height: 40vh; overflow-y: auto; overscroll-behavior: contain` (the editor starts within the first screen). `render()` already restores the list's `scrollTop`; focused items are scrolled into view by the browser within the list. The check is a new script, `specs/017-agent-file-fields/checks/agents.js` (Node 22 + Chrome, no dependency, as 013's `shots.js`), against the visual fixture with 20 agents, at 1440 × 900 and 390 × 844: scrolling the list leaves `window.scrollY` unchanged; the last agent is selectable without page scroll; the only `select` elements are the project and "Save for" controls.

**To verify first** (T001): whether the list scrolls today over 900 px with 20 agents in Fallow's window (the engineer saw it fail). If not, the cause (for example a parent with `overflow: hidden` breaking `position: sticky`, or the window's height) is fixed in the same place.

## R-8: Run records

**Decision**: `measure::record::model_id(runtime, s)` keeps a model name when it fits its runtime's naming (R-1), or one of the forms it accepted before (so records written before 017 still validate). The record's runtime is the definition's runtime (already recorded). `tests/record_marker.rs` moves its marker out of the model: a model with the marker in it is now refused at load (the builder falls back to the default, which the test checks), and the record holds `claude-sonnet-5-5`.

**Rationale**: constitution V allows identifiers; a name of the declared form (lowercase letters, digits, `.`, `-`, ≤ 64) is one. Fallow no longer needs a list of vendors' forms for new records.

## R-9: Existing files and open sessions

**Decision**: no migration and no rewrite (FR-017).
- Unedited copies: replaced by `install_defaults` as today (their hash matches the record).
- Edited copies: kept, `<name>.md.new` written (today's code); the copy is `unusable`, `in_force: Default`.
- The engineer's own and project files: `unusable`, `in_force` as resolved.
- Spec sessions: `lead_changed` compares the recorded `lead_model` (`"opus"`) with the definition's (`"claude-opus-5-5"`) and reports a change (009 FR-019a): already the behaviour; a test pins it.
- Solo sessions: `solo.rs` reads the definition at each turn; the next turn passes the new model to `--resume`, which Claude Code accepts.
- Loops: `run::feature::start` refuses a step whose agent is missing with `why_not` (R-3), and the loop asks the engineer as for a missing runtime.

## R-10: Tests that write agent files

Files that write or edit agent frontmatter, and what changes:

| File | Today | 017 |
|---|---|---|
| `src/defs/agent.rs` tests | `the_contracts_example_parses` (model opus, no may_run in `base`), `the_rules_are_checked`, `fields_change_only_their_lines`, `an_absent_field_is_added_and_others_are_refused` | full model names, `may_run` everywhere; the two `set_fields` tests removed with it; new tests R-2 |
| `src/defs/mod.rs` tests | reviewer override `model: opus`, `ui-checker` without model and may_run | full names; a test of `unusable` and `in_force` |
| `src/agent/claude.rs` tests (~1316-1341) | `model: None` → `--model opus` | `None` refused; `Some` passed as is |
| `tests/us6_defs.rs` | CHECKER `model: haiku`, `replace("model: sonnet", "model: opus")` | full names, `may_run` |
| `tests/us8_agent_fields.rs` | the `{fields}` save | rewritten: a `{text}` save changes one line only; `{fields}` refused |
| `tests/us7_runtime.rs` | `on_runtime(..., "fake", "fake-1")` replaces `model: sonnet` | replaces the shipped full name; `fake-1` fits |
| `tests/common/mod.rs` `second_lead`, `assistant_on_fake` (~590, ~790) | `replacen("model: opus", ...)`, runtime → fake keeps claude's model | helper `on_fake(text, model)` replacing runtime and model; callers pass `fake-…` |
| `tests/us9_leads.rs` | models `careful-1`, `careful-2`, `x`, scenario `model = "opus"`/`"sonnet"` | `fake-careful-1`/`claude-…` per runtime; scenario filters on full names |
| `tests/visual_009.rs` | `second_lead(..., "claude", "opus", ...)` | `claude-opus-5-5` |
| `tests/us15_claude.rs` | `replace("model: opus", MODEL)` | replace the shipped full name |
| `tests/record_marker.rs` | `model: sonnet-<marker>` | R-8 |
| `tests/us8_verification.rs`, `us8_parallel.rs`, `us2_resume_not_begun.rs`, `us2_repairs.rs` | `replace("runtime: claude", "runtime: fake")` | `on_fake` (the model must change with the runtime) |
| `tests/run_events.rs` | `model: "opus"` in an event | unchanged (an event's field, not a definition) |

A `replace` that no longer matches leaves the file silently unchanged: each helper asserts that its pattern was found.
