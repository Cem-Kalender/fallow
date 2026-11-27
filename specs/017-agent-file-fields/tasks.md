---
description: "Task list for 017 Agent files are complete and say what they need"
---

# Tasks: Agent files are complete and say what they need

**Input**: design documents from `specs/017-agent-file-fields/`:
- spec (US1-US4, FR-001 to FR-024);
- plan;
- research R-1 to R-10;
- data-model;
- contracts/agent-definition.md, contracts/runtime-model-naming.md, contracts/web-api.md;
- quickstart Q1 to Q6.

**Tests**: requested (the project writes tests first). Each story's tests are written and seen failing before its implementation. They use the fake runtime; Claude Code is checked by hand (quickstart Q2).

**Build and test**: `cargo test --workspace --features automation`, one test file per command while iterating. Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --features automation -- -D warnings` before each commit.

## Format: `[ID] [P?] [Story] Description`

---

## Phase 1: Setup

- [X] T001 [P] Reproduce the list's scrolling with 20 agents before changing it (research R-7, "To verify first"): add a `FALLOW_VISUAL_AGENTS=<n>` option to the visual fixture in `tests/visual_fixture.rs` that writes `<n>` valid checker agents (`agent-01` … `agent-<n>`, `runtime: claude`, `model: claude-sonnet-5-5`, `may_run: [gates]`) to the fixture's user `agents/` folder; open the Agents page in Fallow's window at 1440 × 900 and at 390 × 844 and record in `specs/017-agent-file-fields/checks/README.md` whether the list scrolls on its own and, if not over 900 px, why (the element that breaks `position: sticky` or the height).
- [X] T002 [P] Make every test helper that rewrites a shipped agent assert its pattern was found (research R-10, last line), in `tests/common/mod.rs` (`second_lead`, `assistant_on_fake`) and `tests/us7_runtime.rs` (`on_runtime`): a `replace` that matches nothing panics naming the pattern. Add `pub fn on_fake(text: &str, model: &str) -> String` to `tests/common/mod.rs`, replacing `runtime: claude` with `runtime: fake` and the `model:` line's value with `model` (asserting both were found).

---

## Phase 2: Foundational (the runtime contract and the library's report)

**No user story can start before this phase is done.**

- [X] T003 `ModelNaming` in the runtime contract (research R-1, contracts/runtime-model-naming.md), in `src/agent/mod.rs`:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] pub struct ModelNaming { prefix, example, lookup, lookup_url: Option<&'static str> }`;
  - `fn fits(&self, name: &str) -> bool`: "starts with `prefix`; at most 64 characters; the rest non-empty, only `a-z 0-9 . -`";
  - `fn hint(&self) -> String`, worded as in contracts/runtime-model-naming.md "The hint";
  - `registry::model_naming(name) -> Option<ModelNaming>` (no settings);
  - unit tests: `claude-opus-5-5`, `claude-sonnet-5-5`, `claude-fable-5-2` fit `claude`; `opus`, `claude-`, `Claude-Opus-5-5`, `claude opus`, `claude-opus-5-5[1m]`, `anthropic.claude-opus-5-5`, a 65-character name do not; `fake-1` fits `fake` and not `claude`.
- [X] T004 [P] The adapters' constants: `pub const MODEL_NAMING` in `src/agent/claude.rs` (`prefix: "claude-"`, `example: "claude-sonnet-5-5"`, `lookup: "Anthropic's models overview"`, `lookup_url: Some("https://platform.claude.com/docs/en/models/overview")`) and in `src/agent/fake.rs` (`prefix: "fake-"`, `example: "fake-1"`, `lookup: "the fake runtime's scenario"`, `lookup_url: None`), wired into `registry::model_naming` in `src/agent/mod.rs`.
- [X] T005 `defs::runtimes() -> Vec<(&'static str, ModelNaming)>` in `src/defs/mod.rs`, built from `RUNTIMES` and `registry::model_naming`; change `defs::agent::parse`'s last parameter to `runtimes: &[(&str, ModelNaming)]` in `src/defs/agent.rs` (no new rule yet) and update every caller: `Library::load` in `src/defs/mod.rs`, `put_agent` in `src/studio/defs.rs`, the tests' `RT` in `src/defs/agent.rs`. `cargo test` passes unchanged.
- [X] T006 `Unusable` and `why_not` (research R-3, data-model "Library"), in `src/defs/mod.rs`:
  - `pub struct Unusable { name, file, source: Source, text, problems: Vec<Problem>, in_force: Option<Source> }`, `Serialize`;
  - `Library.unusable: Vec<Unusable>`, filled in `load` for every agent file that fails, `in_force` set after all places are read (the source of `lib.agents[name]`, or `None`); `lib.problems` keeps its strings;
  - `Library::why_not(&self, name) -> String`: "there is no agent named \"X\"" when no file of that name exists, else "X cannot be used: <file>:<line>: <message>" plus " (and N more)" when there are more;
  - unit tests in `src/defs/mod.rs`: a broken user copy of `reviewer` gives `in_force: Some(Default)`; a broken own `ui-checker` gives `None`; a broken project copy with a valid user copy gives `Some(User)`; `why_not` for each.
- [X] T007 The reason at every lookup (FR-019, research R-3):
  - `src/defs/pipeline.rs`: `kind_of: &dyn Fn(&str) -> Result<Kind, String>`; the `None` arm becomes the error text, at the step; update `Library::load` in `src/defs/mod.rs`, `studio/defs.rs` (`would_break`, `check_pipeline`, `put_pipeline`, `delete_agent`) and `run/feature.rs:57`, passing `lib.why_not`; update the tests in `src/defs/pipeline.rs` (`no agent named` still for a name with no file);
  - `src/run/feature.rs` (~646, ~2396): `ok_or_else(|| lib.why_not(...))`;
  - `src/studio/mod.rs` `lead_problem` (~725): `lib.why_not(name)` instead of "the lead X is gone" when the name has an unusable file.

**Checkpoint**: `cargo test --workspace --features automation` passes; behaviour unchanged except the clearer reasons.

---

## Phase 3: User Story 1 - The file says exactly which model runs (Priority: P1) 🎯 MVP

**Goal**: every required field checked with a message saying what belongs there; `model` in its runtime's form; no model list; no fallback.

**Independent Test**: quickstart Q1 and Q2.

### Tests for User Story 1 (write first, see them fail)

- [X] T008 [P] [US1] Unit tests of the checks in `src/defs/agent.rs` (contracts/agent-definition.md): for each of `name`, `description`, `runtime`, `model`, `kind`, `may_run`, `returns`, the file without the line is refused at the closing `---` line and with the line but no value (`key:` and `key:   # comment`) at its own line, each message containing the "What belongs there" text (14 cases, SC-003); `model: opus` refused with `is not a full model name for claude` and the overview's address; `model: claude-sonnet-5-5` with `runtime: fake` refused with the `fake-` hint; `runtime: gpt` with `model: x` reports the runtime only; `kind: lead` / `kind: assistant` with `may_run: [gates]` refused with "follow the chat mode … write []"; `may_run: []` accepted for every kind; the empty body's new message.
- [X] T009 [P] [US1] Update the existing unit tests to complete files in `src/defs/agent.rs` (`the_contracts_example_parses` with `model: claude-opus-5-5`; `the_rules_are_checked`'s `base` with `model: claude-sonnet-5-5` and `may_run: []`) and `src/defs/mod.rs` (`a_project_overrides_by_name_and_a_broken_file_is_reported`: full names, `may_run`).
- [X] T010 [P] [US1] The shipped agents (FR-016, SC-001) in `src/defs/agent.rs` `the_shipped_defaults_parse`: each shipped file parses with `defs::runtimes()`; `lead`, `assistant`, `security-reviewer` name `claude-opus-5-5`; the others `claude-sonnet-5-5`; `lead` and `assistant` have `may_run` `[]`.
- [X] T011 [P] [US1] The model at the call in `src/agent/claude.rs` tests (~1316-1341): `c.model = None` and `Some("")` are refused by `Claude::start` before the program starts, with "the agent has no model: its definition must name one"; `Some("claude-sonnet-5-5")` gives `--model claude-sonnet-5-5`. In `tests/agent_claude.rs` the `call()` helper sets `model: Some("fake-1")`.
- [X] T012 [P] [US1] `GET /api/runtimes` in `tests/us7_runtime.rs`: each runtime has `model_naming` with `prefix`, `example`, `lookup`, `hint`, and no `models` key (FR-008); update `on_runtime` to replace the shipped full name (T002's assert); the fake agent is handed `--model fake-1` exactly (US1 AS5).
- [X] T013 [P] [US1] A model the provider refuses (US1 AS6, FR-009) in `tests/us7_runtime.rs`: a scenario step for the reviewer on `runtime: fake` with `model: fake-nonexistent-1` ends with `is_error = true` and the result text "model not found: fake-nonexistent-1"; the step's failure, as the loop and the activity show it, contains that text and the agent's name; Fallow did not refuse the file before.
- [X] T014 [P] [US1] Saving through the API in `tests/us6_defs.rs`: `CHECKER`'s `model: haiku` becomes `model: claude-sonnet-5-5` (its `may_run: [gates]` stays); the existing `replace("model: sonnet", "model: opus")` becomes `claude-sonnet-5-5` → `claude-opus-5-5`; add: `PUT {text}` with `model: opus` is 409 with the line and the hint, and nothing is written.

### Implementation for User Story 1

- [X] T015 [US1] The required fields and messages in `src/defs/agent.rs` `parse` (research R-2, contracts/agent-definition.md), quoting the contract's "What belongs there" texts verbatim: an empty value is missing, at its own line; an absent key at the closing `---` line; `model` required and checked with the runtime's `ModelNaming::fits`, refused as `model "<m>" is not a full model name for <runtime>: <hint>`; with an unknown or missing runtime the model is not checked against a form and a missing model's message gives every runtime's hint; `may_run` required, and `[]` for `lead` and `assistant` ("may_run: a lead's commands follow the chat mode (Manual or Auto), not this list: write []", "an assistant's" for the assistant); the empty-body message. T008-T010 pass.
- [X] T016 [P] [US1] The shipped agents (FR-016, data-model "Shipped agents"): `model: claude-opus-5-5` in `assets/agents/lead.md`, `assets/agents/assistant.md`, `assets/agents/security-reviewer.md`; `model: claude-sonnet-5-5` in `assets/agents/builder.md`, `assets/agents/reviewer.md`, `assets/agents/test-writer.md`, `assets/agents/docs-writer.md`; add `may_run: []` to `assets/agents/lead.md` and `assets/agents/assistant.md`, after `kind`.
- [X] T017 [US1] No model list, no fallback (FR-003, FR-007, research R-6), in `src/agent/mod.rs`, `src/agent/claude.rs` and `src/agent/fake.rs`: remove `registry::models`; `Claude::start` and `Fake::start` refuse a call without a model (T011); `args` pushes `--model` with the call's model only; `registry::list` returns `(name, Availability)` without models.
- [X] T018 [US1] `GET /api/runtimes` in `src/run/engine.rs` `runtimes` (~4480): `models` replaced by `model_naming` (the `ModelNaming` fields plus `hint`), as contracts/runtime-model-naming.md. T012 passes.
- [X] T019 [US1] Run records (FR-022, research R-8), in `src/measure/record.rs`: `model_id(runtime: &str, s: &str)` keeps `s` when `registry::model_naming(runtime)` fits it or the pre-017 pattern matches (kept for old records); update the two callers (~166 with the step's definition runtime, ~294 with `a.runtime`) and the unit tests (~385, ~408: `"my words"` still refused, `claude-fable-5-2` now kept).
- [X] T020 [US1] Tests that put shipped agents on the fake runtime or other models (research R-10), using `on_fake` and full names: `tests/common/mod.rs` (`second_lead`, `assistant_on_fake`), `tests/us9_leads.rs` (models `fake-careful-1`, `fake-careful-2`, `fake-x` on `fake`; scenario filters `model = "claude-opus-5-5"` / `"claude-sonnet-5-5"` for shipped agents on `claude`), `tests/visual_009.rs` (`claude-opus-5-5`), `tests/us15_claude.rs` (replace `model: claude-opus-5-5`), `tests/us8_verification.rs`, `tests/us8_parallel.rs`, `tests/us2_resume_not_begun.rs`, `tests/us2_repairs.rs` (`on_fake(shipped, "fake-1")`).
- [X] T021 [US1] `tests/record_marker.rs` (research R-8): the builder override with `model: sonnet-<marker>` is now unusable; assert it is listed as unusable and the loop runs with the default builder; the record holds `claude-sonnet-5-5` and no marker anywhere (constitution V).

**Checkpoint**: quickstart Q1 and Q2 pass; `cargo test --workspace --features automation` green.

---

## Phase 4: User Story 2 - The Agents page is the file (Priority: P1)

**Goal**: one editor with the whole file, no form or dropdown, a template with a comment on every line, `{text}` saves only.

**Independent Test**: quickstart Q3.

### Tests for User Story 2 (write first, see them fail)

- [ ] T022 [P] [US2] The template (FR-012, research R-5) in `src/defs/agent.rs` unit tests: `template("ui-checker", &runtimes())` holds `name: ui-checker` and lines for `description`, `runtime`, `model`, `kind`, `may_run`, `returns` without values, each with a `#` comment; the model's comment holds each runtime's `hint()`; a `# limits:` comment line; a comment line about the instructions below the closing `---`; `parse` of it untouched gives exactly 6 missing-field problems at the fields' own lines plus the empty-instructions problem; filling the six values and one line of instructions parses.
- [ ] T023 [P] [US2] Rewrite `tests/us8_agent_fields.rs` for the text save (contracts/web-api.md): a `PUT {text}` changing one line of a copy leaves every other line byte for byte (008 SC-003 replaced: "differs in that line only"); a body with `fields` is 409 "an agent is saved as its whole text"; `GET /api/agents/template?name=ui-checker` returns the template; `name=Bad Name` is refused; saving the template untouched is 409 listing 7 problems and writes no file.
- [ ] T024 [P] [US2] Page source test (FR-010, SC-002) in the new `tests/web_agents.rs` (plain file reads, like `tests/web_markdown.rs`; no `[[test]]` entry needed): `src/service/web/screens/agents.js` contains no `data-k=` field inputs, no `<select` other than the project control (`id="ag-project"`), and exactly one `id="ag-text"` ("Save for" comes from `app.js`'s `saveFor`); no file under `src/service/web/` and neither `src/agent/mod.rs` nor `src/agent/claude.rs` holds a list of model names (the words `"opus"`, `"sonnet"`, `"haiku"` as string literals do not occur).

### Implementation for User Story 2

- [ ] T025 [US2] `defs::agent::template(name, runtimes) -> String` in `src/defs/agent.rs`, exactly as research R-5's text, the runtimes' part from `runtimes`. T022 passes.
- [ ] T026 [US2] Remove 008's field save (research R-4): in `src/defs/agent.rs` delete `FIELD_KEYS`, `FieldValue`, `write_value`, `set_fields`, `set_body` and their tests (`fields_change_only_their_lines`, `an_absent_field_is_added_and_others_are_refused`); in `src/studio/defs.rs` delete `put_agent_fields`; in `src/studio/mod.rs` delete `Req::PutAgentFields`; in `src/run/engine.rs` delete its arms (~4041, ~4433); in `src/service/answer.rs` (~215) a `PUT /api/agents/<name>` body with `fields` answers `Err("an agent is saved as its whole text")`.
- [ ] T027 [US2] The template route (contracts/web-api.md): `Req::AgentTemplate { name }` in `src/studio/mod.rs`, `("GET", ["api", "agents", "template"])` with `query_arg(url, "name")` in `src/service/answer.rs` (before any `["api", "agents", name]` arm), handled in `src/run/engine.rs` by `Studio::agent_template` in `src/studio/defs.rs` (`check_name`, then `{ "text": template(name, &defs::runtimes()) }`). T023 passes.
- [ ] T028 [US2] The page (FR-010 to FR-012, FR-015, research R-4) in `src/service/web/screens/agents.js`: remove `fields()`, `opt` for agent fields, `RETURNS`, `s.form`, `s.orig`, `s.body`, `split`'s use for the body, `as-text`/`as-form` and the `field` action; the editor is always `<textarea id="ag-text">` with the whole file and a label `<name>.md`; `dirty()` compares `#ag-text` with the file's text; `doSave` always sends `{text}`; "+ New agent" asks the name, then `GET /api/agents/template?name=<n>` and shows its text; the list's per-agent line keeps `runtime · model`; a refused save lists every problem at its line in the `role="alert"` list; `load()` no longer reads `models`. T024 passes.
- [ ] T029 [US2] `src/service/web/app.css`: remove the `.ag-fields` rules (~617-623); give `#ag-text` the editor's full height (rows 28 on wide screens) without changing other screens.

**Checkpoint**: quickstart Q3 passes.

---

## Phase 5: User Story 3 - Existing agent files after the update (Priority: P1)

**Goal**: files that stopped being valid are listed, openable and fixable; shipped defaults stay in force; nothing is rewritten; open sessions and loops behave as specified.

**Independent Test**: quickstart Q4.

### Tests for User Story 3 (write first, see them fail)

- [ ] T030 [US3] `tests/us17_existing.rs` (new; `[[test]]` with `required-features = ["automation"]` in `Cargo.toml` if the suite needs it): a config folder as 0.3.1 left it, built from the 0.3.1 texts kept as fixtures in the test (the shipped `builder.md` with `model: sonnet`, `reviewer.md` edited with `model: sonnet`, the record `.shipped.json` with 0.3.1's hashes, computed with FNV-1a 64 as `src/defs/mod.rs` `fnv` does: make `fnv` `pub` (as `defs::fingerprint`) so the test uses the same function), an own `ui-checker.md` (`model: haiku`, no `may_run`) used by a project pipeline, and a project `.fallow/agents/careful-lead.md` (kind lead, `model: opus`) chosen as the project's lead. Start Fallow and assert:
  - US3 AS1: `builder.md` equals the new shipped text;
  - US3 AS2: `reviewer.md` bytes unchanged, `reviewer.md.new` equals the new shipped text, `GET /api/agents` lists `reviewer` in `unusable` with `in_force: "default"` and the model problem at its line, and `agents[reviewer].source == "default"`;
  - US3 AS3: `ui-checker` in `unusable` with `in_force: null`; the project pipeline's problem contains `ui-checker.md:` and `is not a full model name`; the project's lead problem names `careful-lead.md` and its problem;
  - US3 AS7 / FR-017: every agent file's bytes are as before, after the start, a `GET /api/agents`, a loop start and a page save of another agent.
- [ ] T031 [P] [US3] Open sessions (US3 AS5, FR-021) in `tests/us9_leads.rs`, with the `claude` profile pointed at the fake agent (as the file's other tests): write a spec session's conversation log as 0.3.1 wrote it, its lead `lead` recorded with runtime `claude` and `lead_model` `"opus"`; start Fallow with the new shipped lead; the conversation view has `lead.changed == true`, and the next message starts a new agent session (the fake agent is not given `--resume`), the conversation and documents kept.
- [ ] T032 [US3] A loop across the update (US3 AS6, FR-020) in `tests/us17_existing.rs`: a locked loop whose pipeline names `ui-checker`, written valid at lock; replace the file with an alias before its step is due; the step does not start and the loop asks the engineer with a question naming `ui-checker.md` and its problem.
- [ ] T033 [US3] Fixing from the page (US3 AS4, FR-013) in `tests/us17_existing.rs`: `PUT /api/agents/reviewer?scope=user {text}` with the file's text and `model: claude-sonnet-5-5` saves; afterwards `reviewer` is not in `unusable` and `agents[reviewer].source == "user"`.

### Implementation for User Story 3

- [ ] T034 [US3] `unusable` in `GET /api/agents` (contracts/web-api.md) in `src/studio/defs.rs` `agents_view`: `{ name, file, source, text, problems: [{line, message}], in_force }` from `lib.unusable`. T030's API asserts pass.
- [ ] T035 [US3] The page's "Files Fallow could not use" (FR-013, FR-018) in `src/service/web/screens/agents.js`: each `unusable` entry a button (name, its place, "the default is in force" / "yours is in force" / "this project's is in force" / "not in force"); selecting it opens its `text` in `#ag-text` with its problems at their lines, "Save for" set to its own place (`user` or `project`), and saving `{text}` there; the remaining `problems` strings (pipelines) stay as text; a problem's `https://` address rendered as a link that the window opens in the engineer's browser (FR-023), using the page's existing external-link handling.
- [ ] T036 [US3] Confirm the loop's question for an unusable step agent (FR-020) in `src/run/feature.rs` `start`: the error from `why_not` (T007) reaches the engineer as the question for a missing runtime does; adjust the wording only if T032 shows the file or problem missing.
- [ ] T037 [US3] Confirm open sessions (FR-021) in `src/studio/mod.rs` `lead_changed` and `src/studio/solo.rs` (~649, ~689): no code change expected; T031 pins it. If T031 fails, fix `lead_changed` so an alias recorded before 017 compares as changed.

**Checkpoint**: quickstart Q4 passes.

---

## Phase 6: User Story 4 - The agent list scrolls on its own (Priority: P2)

**Goal**: with 20 agents the list scrolls on its own at every width; the editor and Save stay in view.

**Independent Test**: quickstart Q5.

### Tests for User Story 4 (write first, see them fail)

- [ ] T038 [US4] `tests/visual_017.rs` (new, `#[ignore]`, `[[test]]` with `required-features = ["automation"]` in `Cargo.toml`, as `visual_013`): serves the visual fixture with `FALLOW_VISUAL_AGENTS=20` (T001) and writes `url.txt`.
- [ ] T039 [US4] `specs/017-agent-file-fields/checks/agents.js` (Node 22 + Chrome, no dependency, modelled on `specs/013-dark-mode/checks/shots.js`): at 1440 × 900 and 390 × 844, open `#/agents`; scroll `.ag-side` to its end and assert `window.scrollY` is unchanged and `.ag-editor`'s Save button is in the viewport; click the last agent and assert the editor shows `agent-20.md`, `.ag-side.scrollTop` kept, `window.scrollY` unchanged; below 900 px assert `.ag-side`'s height ≤ 40 % of the viewport and the top of `#ag-text` within the first viewport height (US4 AS2); Tab to the last list item and assert it is visible within `.ag-side`; assert the page's only `select` elements are `#ag-project` and `#ag-scope` (SC-002); print each check and exit non-zero on a failure.

### Implementation for User Story 4

- [ ] T040 [US4] `src/service/web/app.css` (~745): keep the sticky list over 900 px and fix what T001 found; add `@media (max-width: 900px) { .ag-side { max-height: 40vh; overflow-y: auto; overscroll-behavior: contain; } }`; in `src/service/web/screens/agents.js` keep `.ag-side`'s `scrollTop` across `render()` (it does today) and, on focus of an `.ag-item`, `scrollIntoView({block: "nearest"})` within the list. T039 passes at both sizes.

**Checkpoint**: quickstart Q5 passes.

---

## Phase 7: Polish & Cross-Cutting Concerns

- [ ] T041 [P] Mark what 017 replaces (FR-024) in the earlier documents, each with a line "Replaced by 017 (specs/017-agent-file-fields/…)": `specs/007-fallow-studio/contracts/agent-definition.md` (the Format table's `model`, `may_run` and `kind` rows, "empty means the runtime's default", the defaults table), `specs/007-fallow-studio/contracts/runtime.md` and `specs/007-fallow-studio/contracts/web-api.md` (`GET /api/runtimes` models), `specs/008-studio-redesign/spec.md` (FR-007's list scrolling, FR-008, US3 AS2, the form-field edge case, SC-003), `specs/008-studio-redesign/contracts/agents-api.md` (the `{fields, body?}` body).
- [ ] T042 [P] `README.md`, "Agents and pipelines" (~53-58): replace "edits them as form fields and instructions" with the one editor; say every agent file names `runtime`, the model's full name (for Claude, as in Anthropic's models overview), `kind`, `may_run` and `returns`, and that a new agent starts from a template saying what to fill in.
- [ ] T043 [P] `README.md`, a short "Upgrading from 0.3" note under the install section: agent files with a short model name (`opus`) or without `model` or `may_run` are listed under "Files Fallow could not use" with what to change, and are never changed by Fallow; for a shipped agent's name the shipped default runs meanwhile, with the new default beside your copy as `<name>.md.new`; an open spec session starts a new agent session at its next message. (The GitHub release notes are generated by `.github/workflows/release.yml` and hold no per-version text.)
- [ ] T044 Run quickstart Q1-Q6; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --features automation -- -D warnings`; the CI budget benchmarks (SC-007).

---

## Dependencies & Execution Order

- **Setup (T001-T002)**: no dependencies; T001 informs T040, T002 is used by T012 and T020.
- **Foundational (T003-T007)**: T003 → T004 → T005 → T006 → T007. Blocks every story.
- **US1 (T008-T021)**: after Phase 2. Tests T008-T014 first; T015 → T016 (the shipped files must pass the new checks) → T017 → T018; T019-T021 after T015.
- **US2 (T022-T029)**: after Phase 2; T025 needs T005's `runtimes()`; T026-T028 independent of US1's implementation but its tests' files need US1's rules (T015) to fail an untouched template.
- **US3 (T030-T037)**: after US1 (the checks make 0.3.1's files unusable) and T006-T007; T035 after T028 (the page's editor).
- **US4 (T038-T040)**: after T001; independent of US1-US3 except that T001's 20 agents must be valid under US1's rules.
- **Polish (T041-T044)**: after the stories.

### Parallel opportunities

- T001 ∥ T002.
- US1 tests T008-T014 in parallel (different files); T016 ∥ T017.
- US2 tests T022-T024 in parallel.
- US3: T031 (tests/us9_leads.rs) in parallel with T030; T032 and T033 extend T030's file after it.
- US4 can run beside US2/US3 once T001 is done.
- T041-T043 in parallel.

### Example: US1's tests together

```text
T008 src/defs/agent.rs checks      T011 src/agent/claude.rs, tests/agent_claude.rs
T012 tests/us7_runtime.rs          T014 tests/us6_defs.rs
```

## Implementation Strategy

- **MVP**: Phases 1-3 (US1): files name full models, every field is checked, no list and no fallback. With US1 alone the old form still exists but its Model dropdown is gone with `models`; ship US1 and US2 together.
- **Then** US3 (needed before any release: 0.3.1 users' files), then US4.
- Each checkpoint: `cargo test --workspace --features automation` green.
