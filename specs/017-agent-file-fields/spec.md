# Feature Specification: Agent files are complete and say what they need

**Feature Branch**: `017-agent-file-fields` (the spec lives in `specs/017-agent-file-fields`)

**Created**: 2026-11-27

**Status**: Draft

**Input**: User description:

> The engineer, 2026-11-27, looking at the Agents page:
> - "I still see 'opus' instead of the actual model name. fallow needs to list available models in a vendor-agnostic way"
> - "what if I want to select opus 4.8? No it just says 'opus'"
> - "instead of using a drop down why cant we just fill in the model name in the agent file"
> - "the agent file template must be followed. if there is no model, fallow says so and you cant save the file. the model name has to match the name given to it by anthropic. we dont have to use an API for this. we just prompt the user to look up the correct name."
> - "no, i dont want any drop downs. just the md file"
> - "not only model, runtime becomes required to."
> - "NO DROPDOWNS. eveyrhting that is now in dropdowns need to be filled in the md file. when creating a new agent, the user get's the template. all of this needs to be filled: name, description, runtime, model, kind, returns"
> - "The allowed commands also needs to be a tag, and also needs to be filled. The user should know what to fill based on the template"
> - "the left vertical bar that shows the agents needs to be scrollable. when I add 20 agents, I don't want to scroll all the way down. that is not good UI"
>
> Decisions taken for the engineer by the session acting for them (2026-11-27; the engineer may overrule): listed under Clarifications, "from the brief".

## Context

An agent is a markdown file: a short frontmatter (name, description, runtime, model, kind, the commands it may run, its result shape, optional limits) and its instructions (007 contracts/agent-definition.md). Today:

- **The model is optional and short.** `model` may be left out or empty, and is not checked. The shipped agents name an alias (`opus`, `sonnet`), which the runtime resolves to whichever model the alias points at today. The engineer cannot tell from the file which model runs, nor pick a specific one.
- **Fallow keeps a model list.** For `claude` it holds `opus, sonnet, haiku` (and misses newer families). The Agents page's Model dropdown is built from it, and a call without a model falls back to its first entry. A list kept inside Fallow is out of date the day a provider ships a model, and ties Fallow to one supplier (constitution IX).
- **The Agents page is a form.** Runtime, model, kind and returns are dropdowns; description and the commands are inputs; the instructions are a separate text box; "Edit as text" shows the whole file (008 FR-008). A new agent starts from a fixed text with `model: sonnet` already filled in.
- **`may_run` is optional.** It matters only for makers and checkers in a loop; a lead's and an assistant's commands follow the chat mode (015), so their files leave it out.
- **The agent list** beside the editor scrolls on its own only at widths over 900 px; below that it is part of the page, so with 20 agents the editor is far down.

**What the engineer wants**: the file is the only place an agent is described. Every field it needs is written in it, the model by the full name its provider gives it, and Fallow refuses a file that is not complete, saying what belongs where. No dropdowns. A new agent starts from a template that says, on every line, what to fill in. Fallow keeps no list of models and calls no API to find one: each runtime says how its provider's model names look and where to look them up.

**What this replaces in earlier features** (FR-024 lists them one by one):
- 007 contracts/agent-definition.md: `model` and `may_run` become required; "empty means the runtime's default" goes; the shipped defaults name full model names.
- 007 contracts/runtime.md and web-api.md: `GET /api/runtimes` no longer lists models; it gives each runtime's model naming instead.
- 008 FR-008, US3 acceptance scenario 2, the edge case "an agent's form field cannot show a value written in the file by hand", SC-003, and contracts/agents-api.md's `{fields, body?}` save: the form is replaced by the file.
- 008 FR-007: the agent list scrolls on its own at every width.
- The model fallback in the Claude adapter (a call without a model ran `opus`).

**What does not change**: where definitions live and which one wins (built in, `~/.fallow/agents/`, a project's `.fallow/agents/`); the frontmatter's strict subset of YAML; `limits` (optional); the rights Fallow gives each kind whatever the file says (007 FR-026, constitution VI); how a shipped default is copied and updated (007 FR-025: an unedited copy is replaced, an edited copy is kept and the new default written beside it as `<name>.md.new`); the project and "Save for" controls of the page; "Use for this project", Reset and Delete.

**Out of scope**:
- Checking that a model name exists before a call (that needs the provider's API, which Fallow does not call, constitution V). A wrong name fails at the call, with the runtime's own message (FR-009).
- Showing the model the runtime reports it actually ran (Clarifications).
- New runtimes, and other runtimes' model names.
- The settings' `roles` of 004, which do not reach agent definitions.

## Clarifications

### Session 2026-11-27

- Q: Which fields must every agent file give? → A (from the brief): name, description, runtime, model, kind, may_run and returns. `limits` stays optional. A field written with no value counts as missing. Each "missing" message says what belongs there.
- Q: What is a valid model? → A (from the brief): the provider's full name for the model. Each runtime declares the form its provider's names take and where to look them up; for `claude`, a name starting with `claude-`, looked up in Anthropic's models overview. Fallow keeps no model list and calls no API. An alias (`opus`) is refused with that hint. Whether the name exists is not checked before a call; a wrong name fails at the call with the runtime's message.
- Q: What does a lead or an assistant write for `may_run`, which Fallow does not use for them? → A (from the brief): it is required for them too, and must be `[]`. A non-empty list would suggest the agent may run those commands, which is not so: the chat mode decides (015).
- Q: What does the Agents page offer? → A (from the brief): no form and no dropdowns. The whole file in one editor. A new agent starts from a template in which every required field is present, with a comment on each line saying what to fill in; saving is refused until every field is filled in correctly.
- Q: What happens to the shipped agents? → A (from the brief): they name full model names (`opus` → `claude-opus-5-5`, `sonnet` → `claude-sonnet-5-5`), and the lead and the assistant get `may_run: []`.
- Q: Should Fallow show the model the runtime reports it ran (Claude Code reports it in its first event)? → A (from the brief, "only if it stays small"): out of scope. With full names required, the file names the model exactly, so what the report would add is small, while showing it needs a new event in the runtime contract and a place in every chat and step. It can be its own feature.
- Q: What happens to existing files that stop being valid (an alias, a missing field)? → A (from the brief): each is listed with its problems; for a shipped agent's name, the shipped default stays in force; nothing is rewritten without the engineer.
- Q: Must the agent list scroll? → A (from the brief): it scrolls on its own beside the editor, verified with 20 agents.
- Q: Which form does the test runtime (`fake`) declare? → A (decided in /speckit-specify, not in the brief): names starting with `fake-`, so the form check runs through the same runtime contract on the runtime Fallow's tests use (constitution IX).
- Q: May a maker or a checker write `may_run: []`? → A (decided in /speckit-specify, not in the brief): yes. The field must be written; an empty list is a real answer (no commands beyond the gates and the plan's commands Fallow adds, as today).
- Q: Can the engineer fix a file Fallow could not use from the page? → A (decided in /speckit-specify, not in the brief): yes. Each file under "Files Fallow could not use" opens in the editor with its problems at their lines, and saving it once it is valid puts it in force.
- Q: Below 900 px, where the list sits above the editor, must it scroll too? → A (decided in /speckit-specify, not in the brief): yes. The engineer's complaint is having to scroll past every agent; below 900 px the list keeps to a fixed share of the window's height and scrolls on its own.
- Q (/speckit-clarify): Exactly which `claude` names fit the form? → A (the brief says "starts with `claude-`"; the detail decided in /speckit-clarify): `claude-` followed by lowercase letters, digits, `.` and `-`, as Anthropic writes its model names, at most 64 characters, no spaces; uppercase is refused. Claude Code's own suffixes and cloud providers' names are not Anthropic's names and are refused (Assumptions).
- Q (/speckit-clarify): Where does the engineer learn that a file stopped working, besides the Agents page? → A (decided in /speckit-clarify, not in the brief): at every place that needs the agent (FR-019, FR-020) and in the Agents tab's list; Fallow adds no new notice elsewhere. A file that is not needed anywhere is reported only in the Agents tab.
- Q (during the build, decided by the engineer's stand-in, not in the brief): Which runtimes does the template name? → A: only those usable on this machine: a runtime with a profile in the settings (`claude` always has one: its program on the PATH). The test runtime `fake` is named only when `[agents.fake]` is set up. The `runtime:` comment and the `model:` comment's hints follow that list; when none is usable, the template still names `claude`. A saved file is still checked against every runtime Fallow knows.
- Q (/speckit-clarify): Does the template mention `limits`? → A (decided in /speckit-clarify): yes, as a comment line saying it is optional and how it is written, so the template shows every field the file may hold.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The file says exactly which model runs (Priority: P1)

The engineer opens an agent and reads `model: claude-opus-5-5`: the model, by the name Anthropic gives it. To use another model they write its full name. If they write `opus`, or nothing, Fallow refuses the file and says what belongs there and where to look the name up. Every other required field is checked the same way.

**Why this priority**: it is the request. An alias hides which model runs, a missing field hides what the agent is, and a model list in Fallow is always out of date.

**Independent Test**: save agent files through Fallow's save with each required field missing, empty, or (for the model) an alias, and check each is refused with a message naming the field, its line and what belongs there; save a complete file and check it is in force and a step started with it hands the runtime that exact model name.

**Acceptance Scenarios**:

1. **Given** an agent file with `model: opus` and runtime `claude`, **When** the engineer saves it, **Then** the save is refused at the model's line with a message saying the model must be the provider's full name, that `claude` model names start with `claude-`, and where to look them up (Anthropic's models overview). (must fail)
2. **Given** an agent file without a `model` line, or with `model:` and no value, **When** the engineer saves it, **Then** the save is refused, saying the model is required and what belongs there, as in scenario 1. (must fail)
3. **Given** an agent file missing any one of name, description, runtime, kind, may_run or returns, or giving it with no value, **When** the engineer saves it, **Then** the save is refused naming that field and saying what belongs there (for example "runtime is required: the program that runs this agent, one of claude, fake"; every message is listed in contracts/agent-definition.md). (must fail)
4. **Given** an agent of kind `lead` or `assistant` with `may_run: [gates]`, **When** the engineer saves it, **Then** the save is refused at that line, saying a lead's or an assistant's commands follow the chat mode and its `may_run` must be `[]`. (must fail)
5. **Given** a complete file with `model: claude-sonnet-5-5`, **When** a step or a chat starts with that agent, **Then** the runtime is given exactly `claude-sonnet-5-5`.
6. **Given** a complete file with a well-formed name that does not exist (`model: claude-nonexistent-1`), **When** a step or a chat starts with that agent, **Then** the call fails and Fallow shows the runtime's own message where the call failed, naming the agent; Fallow did not refuse the name beforehand.
7. **Given** the runtimes Fallow knows, **When** the page or the engineer asks Fallow which runtimes there are, **Then** each runtime gives the form of its model names and where to look them up, and no list of models.
8. **Given** a fresh install, **When** the engineer opens any shipped agent, **Then** its model is a full name (`claude-opus-5-5` or `claude-sonnet-5-5`) and every required field is present, `may_run: []` for the lead and the assistant included.

---

### User Story 2 - The Agents page is the file (Priority: P1)

The engineer picks an agent and sees its file, whole, in one editor: frontmatter and instructions. There is no form and no dropdown. They change a line and save; if anything is wrong, the problems appear at their lines and nothing is saved. "+ New agent" asks for a name and opens a template in which every required field is present and a comment on each line says what to fill in; it cannot be saved until it is filled in.

**Why this priority**: the engineer asked for it twice ("NO DROPDOWNS"), and the form is what showed `opus` without saying which model it is.

**Independent Test**: open the Agents page on a project; check that no select element or form field for an agent field exists; edit a line of an agent and save it; create a new agent, save the untouched template and check every missing field is reported at its line; fill it in following only the template's comments and save it.

**Acceptance Scenarios**:

1. **Given** the Agents page, **When** the engineer selects an agent, **Then** its whole file is shown in one text editor, and the page has no dropdown or input for any agent field (runtime, model, kind, returns, may_run, description).
2. **Given** an agent shown in the editor, **When** the engineer changes one line and saves, **Then** the saved file differs from the old one in that line only.
3. **Given** "+ New agent" and a name, **When** the template opens, **Then** it holds the name filled in and every other required field (description, runtime, model, kind, may_run, returns) present without a value, each line with a comment saying what to write there, the model's comment giving each runtime's form and where to look names up; and a comment saying the instructions go below the closing `---`.
4. **Given** the untouched template, **When** the engineer saves it, **Then** the save is refused and every missing field and the empty instructions are listed at their lines; no file is written. (must fail)
5. **Given** the template filled in by following its comments only, **When** the engineer saves it, **Then** it is saved and appears in the list under its kind.
6. **Given** an unsaved change in the editor, **When** the engineer picks another agent or leaves the page, **Then** Fallow asks before dropping the change, as today (009 US4).

---

### User Story 3 - Existing agent files after the update (Priority: P1)

The engineer updates Fallow. Their unedited copies of the shipped agents now name full models. Their edited copies, and their own agents, written with `model: opus` or without `model` or `may_run`, can no longer be used: Fallow says so, file by file, with what to change; for a shipped name the shipped default runs meanwhile. Nothing in their files is rewritten without them. They open each file from the page, fix it, and save.

**Why this priority**: every engineer who used Fallow before has such files; without this they would find agents silently gone or changed.

**Independent Test**: with a Fallow data folder from before 017 holding an unedited copy of `builder`, an edited copy of `reviewer` (`model: sonnet`, instructions changed), the engineer's own `ui-checker` (`model: haiku`, no `may_run`) used by a pipeline, and a project's lead override with `model: opus`, start the updated Fallow and check each case below.

**Acceptance Scenarios**:

1. **Given** an unedited copy of a shipped agent in the engineer's folder, **When** Fallow starts after the update, **Then** the copy is replaced by the new default with its full model name, as 007 FR-025 does for every update.
2. **Given** an edited copy of a shipped agent that names an alias, **When** Fallow starts after the update, **Then** the copy is kept byte for byte, the new default is written beside it as `<name>.md.new`, the copy is listed under "Files Fallow could not use" with its problems at their lines and a note that the shipped default is in force meanwhile, and steps and chats using that name run with the shipped default.
3. **Given** the engineer's own agent that names an alias or lacks a required field, **When** Fallow starts after the update, **Then** it is listed under "Files Fallow could not use" with its problems, the file is unchanged, and every place that needs it says it cannot be used and why: the pipeline that names it is shown as not usable naming the agent's file and its problem, and a project or session whose lead it is says the lead's file cannot be used, with the problem. (must fail)
4. **Given** a file listed under "Files Fallow could not use", **When** the engineer clicks it, **Then** its text opens in the editor with its problems at their lines; saving it once valid puts it in force and removes it from the list.
5. **Given** a spec session whose lead's model changes with the update (from `opus` to `claude-opus-5-5`), **When** the engineer opens the session, **Then** Fallow says the lead's model changed and the next message starts a new agent session with the same lead, as 009 FR-019a; the conversation and the documents stay.
6. **Given** a loop running across the update whose next step's agent can no longer be used, **When** that step is due, **Then** the step does not start, and the loop asks the engineer, naming the agent's file and its problem.
7. **Given** any file that stopped being valid, **When** Fallow starts, lists, runs or saves anything, **Then** Fallow never writes to that file on its own.

---

### User Story 4 - The agent list scrolls on its own (Priority: P2)

With 20 agents, the list beside the editor scrolls on its own; the editor and its Save button stay in view, at every window width.

**Why this priority**: the engineer named it, but agents can still be reached by scrolling the page today.

**Independent Test**: with 20 agents in the engineer's folder, open the Agents page at 1440 × 900 and at 390 × 844; scroll the list to its last agent and check the page itself did not scroll and the editor stays in view; select the last agent and check its file shows without scrolling the page back.

**Acceptance Scenarios**:

1. **Given** 20 agents and a window wider than 900 px, **When** the engineer scrolls the agent list, **Then** only the list scrolls; the editor and its Save button stay where they are.
2. **Given** 20 agents and a window narrower than 900 px, **When** the page opens, **Then** the list takes at most a fixed share of the window's height and scrolls on its own, and the editor starts within the first screen.
3. **Given** the list scrolled to its last agent, **When** the engineer selects it, **Then** the list keeps its scroll position and the editor shows that agent.
4. **Given** keyboard use, **When** the engineer moves focus through the list with Tab, **Then** the focused agent is scrolled into view within the list.

---

### Edge Cases

- **A model with spaces or other characters outside its runtime's form** (`claude opus`, `Claude-Opus-5-5`, `claude-opus-5-5[1m]`): refused with the same hint as an alias. Spaces around a value are not part of it, as for every field (`model: claude-opus-5-5 ` is valid).
- **A model that has the form but belongs to another runtime** (`model: claude-sonnet-5-5` with `runtime: fake`): refused, naming the form the chosen runtime expects.
- **The runtime itself is missing or unknown**: the runtime's problem is reported; a model that is written is not checked against a form until the runtime is known; a missing model's message gives every runtime's form.
- **A name the provider retired**: it has the form, so it is saved; it fails at the call with the runtime's message (US1 AS6). Fallow does not keep track of retired names.
- **Claude Code configured for a cloud provider whose model names do not start with `claude-`** (Amazon Bedrock's `anthropic.claude-…`): refused by the form. 017 supports Anthropic's own names only (Assumptions).
- **The template saved with a comment still on a filled-in line** (`model: claude-sonnet-5-5   # the provider's full name…`): valid; comments are kept as they are.
- **A file written by hand outside Fallow while the page shows it**: the page shows the file as it was when opened; the save writes what is in the editor, as 007 does (no version check for agents, 008).
- **An edited copy whose only change was the alias** (the engineer wrote `model: opus` in an otherwise shipped file): treated as an edited copy (US3 AS2); Fallow does not guess what they meant.
- **A `<name>.md.new` exists from an earlier update**: it is replaced by the new default, as today.
- **A project override of a shipped agent that is invalid**: the engineer's own copy, or else the shipped default, stays in force for that project; the project file is listed with its problems.

## Constitution Check

| Principle | How 017 holds it |
|---|---|
| II. The Engineer Decides | Fallow rewrites no agent file; the engineer fixes each (FR-017). Open spec sessions follow 009 FR-019a (FR-021). |
| V. Local and Private | No model list fetched, no API called; the lookup address opens in the engineer's browser only on their click (FR-023). Run records keep a model name only in its runtime's declared form (FR-022). |
| VI. Least Privilege | Rights still come from the kind, whatever the file says; `may_run: []` for a lead and an assistant says what is true (FR-005). |
| VII. Runs Survive | A loop whose next agent became unusable asks the engineer (FR-020). |
| IX. Runtime-Agnostic | Each runtime declares its model naming through the runtime contract; Fallow keeps no vendor's model list (FR-003, FR-004); the check runs on the fake runtime too. |
| I, III, IV, VIII | Not affected: verification, coordination, the setup and the budgets are unchanged (SC-007). |

## Requirements *(mandatory)*

### Functional Requirements

**What an agent file must give**

- **FR-001**: An agent file MUST give `name`, `description`, `runtime`, `model`, `kind`, `may_run` and `returns`. A field that is absent or written with no value MUST be reported as missing. `limits` stays optional. The instructions after the frontmatter stay required.
- **FR-002**: Every problem MUST name its line and say what belongs there: for a missing field its meaning and allowed values (for `runtime`, the runtimes Fallow knows; for `kind` and `returns`, the values and which `returns` goes with which kind; for `may_run`, the list form and its special words `gates` and `plan.commands`; for `model`, FR-004's hint).
- **FR-003**: `model` MUST be the provider's full name for the model, in the form the agent's runtime declares. Fallow MUST NOT keep a list of models, MUST NOT call a provider or a runtime to find or check one, and MUST NOT accept an alias.
- **FR-004**: Each runtime MUST declare, through the runtime contract (constitution IX), the form of its model names and where to look them up. A model that does not fit its runtime's form MUST be refused with that runtime's hint: what the name looks like and where to look it up. `claude`: Anthropic's model names, `claude-` followed by lowercase letters, digits, `.` and `-`, at most 64 characters, looked up in Anthropic's models overview. `fake`: `fake-` followed by the same characters.
- **FR-005**: For kind `lead` and `assistant`, `may_run` MUST be `[]`; anything else MUST be refused, saying their commands follow the chat mode (015). For `maker` and `checker`, `may_run` MAY be `[]`.
- **FR-006**: A file that fails any check MUST NOT be saved, and a file on disk that fails any check MUST NOT be used; the earlier definition of that name, if any, stays in force (as 007).

**Runtimes and the model at the call**

- **FR-007**: Fallow MUST hand the runtime the model exactly as the file names it. No runtime adapter MAY substitute a model of its own choosing; a call that reaches a runtime without a model MUST be refused with a message, never run on a default.
- **FR-008**: The list of runtimes Fallow gives the page MUST hold, for each runtime, its model naming (FR-004) and no list of models.
- **FR-009**: When a call fails because the runtime does not accept the model, Fallow MUST show the runtime's own message where the call failed (the step or the chat), naming the agent, as for any failed call.

**The Agents page**

- **FR-010**: The Agents page MUST show the selected agent's whole file in one text editor, and MUST NOT offer a dropdown, select or input for any agent field. The project selector and "Save for" stay; they are not agent fields.
- **FR-011**: Saving MUST write the editor's text as it is, after the checks of FR-001 to FR-006; a refused save MUST show every problem at its line and write nothing.
- **FR-012**: "+ New agent" MUST ask for the name and open a template holding every required field, the name filled in and the others without a value, each line with a comment saying what to write there; the model's comment MUST give each runtime's form and where to look names up, taken from what the runtimes declare (FR-008), for the runtimes usable on this machine only (Clarifications); a comment line MUST say `limits` is optional and how it is written; a comment MUST say the instructions go below the frontmatter. The template's instructions are empty.
- **FR-013**: The page MUST list "Files Fallow could not use" with each file's problems, and MUST open such a file's text in the editor when the engineer selects it, so it can be fixed and saved (FR-011).
- **FR-014**: The agent list MUST scroll on its own at every window width, keeping the editor and its Save button in view: beside the editor above 900 px, and below it at most a fixed share of the window's height. Selecting an agent MUST keep the list's scroll position, and a focused agent MUST be scrolled into view within the list.
- **FR-015**: Every control the page keeps MUST stay reachable and usable by keyboard and labelled for screen readers, with 4.5:1 text contrast, as 008 FR-018; the problems MUST be announced when a save is refused.

**Existing files**

- **FR-016**: The shipped agents MUST name full model names (`lead`, `assistant`, `security-reviewer`: `claude-opus-5-5`; `builder`, `reviewer`, `test-writer`, `docs-writer`: `claude-sonnet-5-5`) and give every required field, `may_run: []` for `lead` and `assistant`.
- **FR-017**: On update, unedited copies of the shipped agents MUST be replaced and edited copies kept with the new default beside them as `<name>.md.new`, as 007 FR-025. Fallow MUST NOT rewrite, convert or delete any agent file on its own, valid or not.
- **FR-018**: An invalid file whose name is a shipped agent's MUST leave the shipped default (or a valid definition from an earlier place) in force, and its entry under "Files Fallow could not use" MUST say which definition is in force meanwhile.
- **FR-019**: Wherever Fallow reports that an agent cannot be used (a pipeline that names it, a project's or a session's lead, a loop step that needs it), it MUST name the agent's file and its problem when the agent exists only as an invalid file, instead of saying there is no such agent.
- **FR-020**: A loop step whose agent cannot be used when it is due MUST NOT start; the loop MUST ask the engineer, naming the file and its problem (007's behaviour for a missing runtime).
- **FR-021**: An open spec session whose lead's model or runtime changed (the update included) MUST behave as 009 FR-019a. An open solo session's next turn MUST use the assistant's definition as it is then.

**Records and privacy**

- **FR-022**: A run record (constitution V) MUST keep an agent's model name only when it has its runtime's declared form, and MUST keep nothing else written in the file.
- **FR-023**: Fallow MUST make no network request to find, list or check models (constitution V). The lookup place is shown as text or a link that opens in the engineer's browser only when they click it.

**What this replaces**

- **FR-024**: This feature replaces, and the earlier documents MUST be marked as replaced by 017 where they say otherwise: 007 contracts/agent-definition.md (the table's `model` and `may_run` rows, "empty means the runtime's default", the defaults table's models, `kind`'s values now including `assistant`); 007 contracts/runtime.md and contracts/web-api.md (`GET /api/runtimes` "the models a definition may name"); 008 FR-008, US3 acceptance scenario 2, the edge case on form fields, SC-003, and contracts/agents-api.md (the `{fields, body?}` body); 008 FR-007 for the list's scrolling.

### Key Entities

- **Agent file**: `<name>.md` in one of the three places; its frontmatter's required fields (FR-001), optional `limits`, and its instructions. Valid, or listed with its problems.
- **Model naming** (per runtime): the form of the runtime's provider's model names, and where to look them up; declared by the runtime's adapter (FR-004).
- **Agent template**: the text a new agent starts from; built from the required fields and the runtimes' model namings (FR-012).
- **File Fallow could not use**: the file's path, its problems with lines, and, for a shipped or earlier-defined name, which definition is in force meanwhile.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every shipped agent names its model by a full name; no agent definition Fallow uses holds an alias or an empty model (checked over the shipped agents and in the save path).
- **SC-002**: Fallow's source holds no list of model names for any runtime, and the Agents page holds no select or input for an agent field; its only selects are the page controls (project, "Save for").
- **SC-003**: For each required field, a file without it, or with it empty, is refused with a message naming the field and its line: 7 fields × 2 cases, all refused.
- **SC-004**: The template, filled in with only what its comments say (one value per commented line and a line of instructions), is saved at the first attempt; saved untouched, it yields one problem per required field without a value and one for the instructions, each saying what belongs there (7 problems).
- **SC-005**: After the update, 100 % of the engineer's invalid files are listed with their problems, 0 are changed on disk, and every shipped name still has a definition in force.
- **SC-006**: With 20 agents, the last agent can be selected without scrolling the page, at 1440 px and at 390 px.
- **SC-007**: Everything 007 and 014 measured still holds (constitution VIII budgets).

## Assumptions

- `claude-opus-5-5` and `claude-sonnet-5-5` are Anthropic's current full names for the models the shipped aliases `opus` and `sonnet` stood for (checked on Anthropic's models overview, 2026-11-27, and `claude --model opus` reports `claude-opus-5-5`).
- Anthropic's models overview is at `https://platform.claude.com/docs/en/models/overview` (2026-11-27). The hint names the page; if the address moves, only the `claude` adapter's hint changes.
- Claude Code accepts full model names in `--model`.
- An engineer running Claude Code through a cloud provider with other model names (Bedrock) is not served by 017's `claude` form; a runtime for that provider, with its own naming, is a later feature.
- The fake runtime's form (`fake-`) is a test convenience; Fallow's tests that put agents on the fake runtime name their models in that form.
