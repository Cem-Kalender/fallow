# Implementation Plan: Agent files are complete and say what they need

**Branch**: `017-agent-file-fields` | **Date**: 2026-11-27 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/017-agent-file-fields/spec.md`

## Summary

- **Every field is required** except `limits`: `defs::agent::parse` treats an absent or empty `model`, `may_run` and the others as missing, at their lines, with a message saying what belongs there; a lead's and an assistant's `may_run` must be `[]` (research R-2).
- **Each runtime declares its model naming** (`ModelNaming`: prefix, example, where to look names up) in its adapter; `model` must fit it. Fallow's model list (`registry::models`) and the Claude adapter's fallback to `opus` are removed (R-1, R-6).
- **The Agents page is the file**: one editor with the whole text, no form and no dropdowns; `PUT /api/agents/<name>` takes `{text}` only, and 008's field writer goes. A new agent starts from a template built in Rust with a comment on every line (R-4, R-5).
- **Existing files** are never rewritten. The library records each unusable file with its text, problems and which definition stays in force; the page opens it to be fixed, and every place that needs the agent names the file's problem (R-3, R-9).
- **Shipped agents** name `claude-opus-5-5` / `claude-sonnet-5-5`, and the lead and the assistant get `may_run: []`.
- **The agent list** scrolls on its own at every width, checked with 20 agents (R-7).

## Technical Context

**Language/Version**: Rust, edition 2024 (as `Cargo.toml`). The page stays plain JavaScript and CSS, compiled in.

**Primary Dependencies**: none added.

**Storage**: none new. Agent files as today; `~/.fallow/.shipped.json` as today.

**Testing**: `cargo test --workspace --features automation`; unit tests in `src/defs/agent.rs`, `src/defs/mod.rs`, `src/agent/claude.rs`, `src/measure/record.rs`; integration tests in `tests/` on the fake agent; a Node + Chrome check script for the list (R-7), as 013's.

**Target Platform**: Windows 10/11 x64 and macOS on Apple silicon (releases); Linux for development.

**Project Type**: a desktop app (014, 016): a Rust service with its own window showing a local web UI.

**Performance Goals**: none new. Checking a file stays a single pass over its lines.

**Constraints**: constitution VIII budgets unchanged; no network request of Fallow's own (the lookup address is text and a link the engineer clicks); no list of models anywhere.

**Scale/Scope**:
- `src/defs/agent.rs`: the checks, the template; minus `set_fields`, `set_body`, `write_value`, `FieldValue`, `FIELD_KEYS` (about −200, +150 lines);
- `src/defs/mod.rs`: `Unusable`, `why_not`, `runtimes()`; `src/defs/pipeline.rs`: `kind_of` returns the reason;
- `src/agent/mod.rs`, `claude.rs`, `fake.rs`: `ModelNaming`, `model_naming`, no `models`, no fallback;
- `src/studio/defs.rs`, `src/studio/mod.rs`, `src/service/answer.rs`, `src/run/engine.rs`: the routes (`unusable`, template, no `{fields}`, `model_naming`), `lead_problem`;
- `src/run/feature.rs`: `why_not` at the two lookups; `src/measure/record.rs`: `model_id(runtime, …)`;
- `assets/agents/*.md`: 7 files;
- `src/service/web/screens/agents.js` (about −120 lines), `app.css` (a few lines);
- tests: research R-10's table; new `tests/us17_existing.rs`, `tests/visual_017.rs`, `specs/017-agent-file-fields/checks/agents.js`;
- documents: the replaced parts of 007 and 008 marked (FR-024).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check |
|---|---|
| I. Verified, Not Trusted | Not affected: loops and their verification are unchanged. A file Fallow cannot check is not used. |
| II. The Engineer Decides | Pass: Fallow rewrites no agent file on its own (FR-017); the engineer fixes each file. An open spec session asks nothing new: 009 FR-019a tells the engineer the lead changed. |
| III. Coordination in Code | Pass: definitions stay files; the reason an agent is unusable comes from the one load (R-3). |
| IV. Simplest Setup First | Pass: one editor instead of a form plus a text mode; 008's line writer is removed. |
| V. Local and Private | Pass: no model list fetched, no API called (FR-023); the lookup address opens in the engineer's browser only on their click. Run records keep a model name only in its runtime's declared form, lowercase letters, digits, `.` and `-` (R-8): an identifier, never free text. |
| VI. Least Privilege | Pass: rights still come from the kind (007 FR-026). `may_run: []` for a lead and an assistant states what is true; their commands follow the chat mode. |
| VII. Runs Survive | Pass: a loop whose next step's agent became unusable asks the engineer, naming the file; nothing is lost (FR-020). |
| VIII. Small and Measured | Pass: no dependency; less page code; no budget affected. |
| IX. Runtime-Agnostic | Pass: what model names look like is declared by each adapter through the contract (R-1); Fallow holds no runtime's or vendor's model list. The fake runtime declares its own naming, so the check is tested through the same contract. |
| CI gates (Workflow) | Unchanged. New tests join the suite; the list check runs by hand with the visual fixture, as 013's. |

**Re-check after Phase 1 design**: unchanged. The contracts add one read-only route (the template), remove one write route (`{fields}`), and add no write place.

## Project Structure

### Documentation (this feature)

```text
specs/017-agent-file-fields/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── agent-definition.md       # replaces 007's Format and defaults tables
│   ├── runtime-model-naming.md   # amends 007/012 runtime contract
│   └── web-api.md                # replaces 008 agents-api.md; amends 007 web-api.md
├── checks/
│   └── agents.js                 # the list check (R-7)
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
assets/agents/*.md                    # full model names, may_run
src/agent/mod.rs                      # ModelNaming, registry::model_naming, list without models
src/agent/claude.rs                   # MODEL_NAMING, no fallback, refuse a call without a model
src/agent/fake.rs                     # MODEL_NAMING
src/defs/agent.rs                     # required fields, messages, template; field writer removed
src/defs/mod.rs                       # runtimes(), Unusable, why_not
src/defs/pipeline.rs                  # kind_of gives the reason
src/run/feature.rs                    # why_not at step lookups
src/run/engine.rs                     # GET /api/runtimes model_naming; Req::AgentTemplate; no PutAgentFields
src/studio/mod.rs                     # Req variants; lead_problem with why_not
src/studio/defs.rs                    # agents_view unusable; template; put_agent_fields removed
src/service/answer.rs                 # routes
src/measure/record.rs                 # model_id(runtime, name)
src/service/web/screens/agents.js     # one editor, template, unusable files
src/service/web/app.css               # list scroll below 900 px
tests/…                               # research R-10; new us17_existing.rs, visual_017.rs
specs/007-fallow-studio/…, specs/008-studio-redesign/…   # marked replaced (FR-024)
```

**Structure Decision**: the existing single Rust crate with its compiled-in page; no new module.

## Complexity Tracking

No violations.
