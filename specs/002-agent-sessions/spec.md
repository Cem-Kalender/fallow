# Feature Specification: Agent Sessions and Multiple Projects

**Feature Branch**: `001-llm-aware-tui-editor` (no separate branch; see Assumptions)

**Created**: 2026-10-19

**Status**: Draft

**Input**: User description: "Agent sessions and multiple projects in Fallow. (1) Built-in terminal
panes that run agent sessions inside Fallow (embedded pseudo-terminal with a terminal emulator),
started in a chosen project, with several sessions at once that the user can switch between and
orchestrate. (2) Multi-project: several projects open at once, browsable, each with its own agent
sessions. (3) Agent-first default screen: Fallow opens on projects and agent sessions, not on the
file editor; the editor is secondary. Must respect the constitution: memory budgets (idle < 50 MB,
loaded < 150 MB), startup < 100 ms, terminal-only, least privilege, privacy, benchmarked claims."

**Relation to feature 001**: This feature builds on 001 (LLM process panel, editor, file
operations) and changes three of its decisions:
- 001 shows one project at a time (FR-002, `PATH` argument, `Ctrl-O`). This feature replaces that
  with a list of projects.
- 001's default screen is Files | Editor | LLM Processes with focus in Files. This feature makes
  the agent workspace the default screen and moves the editor to a secondary view.
- 001 never starts processes. This feature lets the user start agent sessions explicitly.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Start an agent session in a project and work with it inside Fallow (Priority: P1)

An agentic engineer opens Fallow, picks a project, and starts an agent (for example Claude Code)
in that project's folder. The agent runs in a terminal pane inside Fallow and behaves exactly as it
does in a normal terminal: its colors, prompts, menus, scrolling and key handling all work. The
engineer types to the agent, reads its output, and can leave the pane with one key to use the rest
of Fallow while the agent keeps working.

**Why this priority**: Starting and talking to agents is the main job of an IDE for agentic
workflows. Without it, the user still needs a second terminal next to Fallow.

**Independent Test**: With one project in the list, start a session with a built-in agent profile,
complete a short exchange with the agent (a prompt and a reply that edits a file), scroll back
through its output, leave the pane, come back, and confirm the session kept running and nothing
was lost.

**Acceptance Scenarios**:

1. **Given** a project is selected, **When** the user starts a session with an agent profile,
   **Then** the agent starts in that project's folder in a terminal pane and receives keyboard
   focus.
2. **Given** a session has focus, **When** the user types any key other than the session leave key,
   including `Esc`, `Tab`, `Ctrl-C` and arrow keys, **Then** the key reaches the agent unchanged.
3. **Given** a session has focus, **When** the user presses the session leave key, **Then** focus
   returns to Fallow's own panes and the agent keeps running.
4. **Given** an agent uses a full-screen interface (alternate screen, colors, cursor movement),
   **When** it runs in a session, **Then** its screen looks the same as in a standalone terminal of
   the same size.
5. **Given** a session has produced more output than fits on screen, **When** the user scrolls back,
   **Then** earlier output is shown, up to the session's scrollback limit.
6. **Given** the user resizes the terminal or the pane, **When** the size changes, **Then** the
   agent is told the new size and redraws.
7. **Given** the user pastes text into a focused session, **When** the paste arrives, **Then** the
   agent receives it as a paste, not as typed keys.
8. **Given** the agent process exits, **When** the user looks at the session, **Then** the pane
   shows that it exited and with what exit status, keeps its last output, and offers to restart it.

---

### User Story 2 - Run and oversee several agent sessions at once (Priority: P2)

The engineer runs several agents at the same time, across one or more projects. One overview shows
every session with its project, agent, state and how long it has been in that state. When an agent
finishes a step and waits for input, its session is marked as needing attention, so the engineer
switches only to the sessions that need them. Switching between sessions takes one key.

**Why this priority**: Running agents in parallel is what makes agentic work faster, and it fails
without a way to see which agent needs the user right now.

**Independent Test**: Start three sessions in two projects. Give one a task that ends with a
question, one a long task, and let the third exit. Verify the overview shows "needs attention",
"working" and "exited" correctly and that each session can be reached with one key from the
overview.

**Acceptance Scenarios**:

1. **Given** several sessions are running, **When** the user opens the session overview, **Then**
   every session is listed with its project, agent profile, state, time in that state, and the
   memory and CPU of the agent and its children.
2. **Given** a session's agent rings the terminal bell, or has produced no output for the attention
   delay while its process tree is idle, **When** the overview refreshes, **Then** the session is
   marked "needs attention".
3. **Given** a session is marked "needs attention" and is not visible, **When** its state changes,
   **Then** a notice appears in the hint bar naming the session, and the session count of sessions
   needing attention is shown on the default screen.
4. **Given** the overview is open, **When** the user selects a session and confirms, **Then** that
   session's pane is shown with focus.
5. **Given** a session has focus, **When** the user presses the next-session or previous-session
   key, **Then** focus moves to the next or previous session without passing through the overview.
6. **Given** two sessions are open, **When** the user chooses a split view, **Then** both are visible
   side by side and each can receive focus.
7. **Given** a session is selected, **When** the user stops it, **Then** Fallow asks for
   confirmation naming the session, the agent and the process count, and only then asks the agent
   to stop; a forced stop needs its own confirmation (as 001 FR-032/FR-033).

"Orchestrate" in this feature means starting, watching, switching and stopping sessions, with
attention alerts (clarified 2026-10-19). Agents coordinating with each other (messages between
sessions, waiting on events, task hand-off) is feature 003, which builds on these sessions.

---

### User Story 3 - Agent-first default screen (Priority: P3)

When Fallow starts, it shows the agent workspace: the list of projects, the sessions of the
selected project (or the active session), and the LLM process panel with the system summary. The
file editor is not on screen until the user asks for it. One key switches to the editor view for
the selected project and back.

**Why this priority**: The engineer's first question on opening Fallow is "what are my agents
doing and where do I start the next one", not "which file do I edit".

**Independent Test**: Start Fallow with two projects in its list and one running session from an
earlier start (if sessions survive restarts) or none. Verify the first screen shows projects,
sessions and processes, that no editor or file tree is visible, and that one key opens the editor
for the selected project and the same key returns.

**Acceptance Scenarios**:

1. **Given** Fallow starts, **When** the first screen appears, **Then** it shows the project list,
   the selected project's sessions, and the LLM process panel with the system summary, and focus is
   on the project list.
2. **Given** the agent workspace is shown, **When** the user presses the editor view key, **Then**
   the Files pane and editor of the selected project are shown, as in 001 User Story 3.
3. **Given** the editor view is shown, **When** the user presses the same key, **Then** the agent
   workspace is shown again with its previous selection and focus.
4. **Given** the hint bar is visible on the default screen, **When** no text input has focus,
   **Then** it shows how to start a session, how to switch sessions, how to open the editor view,
   and how to open help.

---

### User Story 4 - Keep several projects open and browse between them (Priority: P4)

The engineer works on several repositories. Fallow keeps a list of their projects. They add a
project by path, remove it from the list, and move between projects; each project shows its own
sessions, its own file tree in the editor view, and the LLM processes whose working directory is
inside it.

**Why this priority**: Agentic engineers commonly run agents in several repositories at once.
It depends on Stories 1 and 3 to be useful, which is why it ranks after them.

**Independent Test**: Add three projects, start a session in two of them, restart Fallow, and
verify the three projects are still listed in the same order, that each shows only its own
sessions and processes, and that removing a project from the list leaves its folder untouched.

**Acceptance Scenarios**:

1. **Given** the project list is focused, **When** the user adds a project by entering a folder
   path, **Then** it is added to the list and selected; a path that is not a readable folder is
   rejected with the reason.
2. **Given** a project is in the list, **When** the user removes it, **Then** Fallow asks for
   confirmation if it has running sessions, removes it from the list only, and never changes the
   folder on disk.
3. **Given** projects are in the list, **When** Fallow is restarted, **Then** the same projects are
   listed in the same order and the last selected project is selected again.
4. **Given** a project is selected, **When** the user looks at the LLM process panel, **Then** they
   can filter it to processes whose working directory is inside that project, and the filter is
   shown while active.
5. **Given** Fallow is started with a folder argument, **When** it opens, **Then** that folder is
   added to the project list (if not already there) and selected.
6. **Given** a project has running sessions, **When** the list is shown, **Then** each project row
   shows its number of sessions and how many need attention.

---

### User Story 5 - Choose which agents can be started (Priority: P5)

The engineer chooses which agents appear as profiles: built-in profiles for common agent CLIs, a
plain shell, and their own profiles with a name, a command, arguments and optional environment
additions.

**Why this priority**: Built-in profiles cover the common case; custom profiles matter once the
basics work.

**Independent Test**: Add a custom profile in the configuration, reload the configuration, start a
session with it, and verify the command and arguments used.

**Acceptance Scenarios**:

1. **Given** no custom profiles exist, **When** the user starts a session, **Then** the built-in
   profiles whose command is found on the system are offered, plus "shell".
2. **Given** a custom profile is configured, **When** the configuration is reloaded, **Then** the
   profile is offered and starts with exactly the configured command and arguments.
3. **Given** a profile's command cannot be found or fails to start, **When** the user starts it,
   **Then** the session shows the error and no other session is affected.

### Edge Cases

- **Fallow quits while sessions run**: `Ctrl-Q` shows a confirmation that lists the running
  sessions (project, profile, process count). Confirming stops every agent with the graceful stop
  of FR-012, then Fallow exits. Cancelling keeps everything running. Sessions never outlive
  Fallow, and there is no background helper (clarified 2026-10-19).
- **Agent floods output**: an agent prints faster than Fallow can draw. Fallow stays responsive to
  keys, drops intermediate frames rather than falling behind, and never loses scrollback within the
  limit.
- **Scrollback limit reached**: the oldest lines are dropped first; the pane says how many lines
  are kept.
- **Agent needs a key Fallow uses**: every key except the session leave key goes to the agent. If
  the leave key itself must be sent to the agent, a documented two-key sequence sends it.
- **Very small terminal**: below the 001 minimum sizes, panes collapse to tabs (001 layout rules);
  a session pane always gets the full available width and the agent is told the real size.
- **Project folder deleted or renamed while listed**: the project row shows "missing"; its running
  sessions keep running; the user can remove it or point it to the new path.
- **Two sessions in the same project edit the same file**: Fallow does not coordinate agents; the
  open-file rules of 001 FR-006 apply to any buffer open in the editor.
- **Same agent started outside Fallow**: it still appears in the LLM process panel (001); it is not
  shown as a session and Fallow does not attach to it.
- **Session started without elevation needs elevation**: Fallow never elevates; the agent sees the
  same permissions as Fallow (001 FR-030).
- **Unsupported terminal feature**: if an agent uses a terminal feature the emulator does not
  support, the output may look different but input and the session keep working; known gaps are
  listed in help.

## Requirements *(mandatory)*

### Functional Requirements

**Agent sessions**

- **FR-001**: Users MUST be able to start an agent session from a profile in a selected project.
  The agent runs in that project's folder, inside a terminal pane in Fallow.
- **FR-002**: A session MUST emulate a terminal well enough that the agent CLIs with built-in
  profiles look and behave as in a standalone terminal of the same size: colors (including 256 and
  24-bit), cursor movement, alternate screen, line editing, bracketed paste, mouse reporting when
  the agent asks for it, and window size changes.
- **FR-003**: While a session has focus, every key except the session leave key MUST be sent to
  the agent. The leave key and the way to send it to the agent MUST be listed in help and in the
  hint bar.
- **FR-004**: Each session MUST keep a scrollback history of at least 10,000 lines that the user
  can scroll and search. Older lines are dropped first. The limit MUST be configurable.
- **FR-005**: Users MUST be able to select and copy text from a session pane with the keyboard and
  the mouse, using the same copy behavior as the editor (001 FR-008).
- **FR-006**: When a session's agent exits, the pane MUST show the exit status and keep the last
  output, and users MUST be able to restart the same profile in the same project or close the
  session.
- **FR-007**: Users MUST be able to run at least 8 sessions at the same time across all projects.
- **FR-008**: Fallow MUST show a session overview listing every session with project, profile,
  state (working, needs attention, exited), time in state, and the memory and CPU of the agent's
  process group (using the metrics of 001 FR-019).
- **FR-009**: A session MUST be marked "needs attention" when its agent rings the bell, or when it
  has produced no output for the attention delay (default 5 s, configurable) while its process
  group uses less than the busy threshold of 001 FR-012. Users MUST be told when a hidden session
  starts needing attention.
- **FR-010**: Users MUST be able to switch to the next or previous session with one key, and to any
  session from the overview.
- **FR-011**: Users MUST be able to show two sessions side by side.
- **FR-012**: Stopping a session MUST follow 001's control rules (FR-032, FR-033): a confirmation
  that names the session, the agent and the number of processes, then a graceful stop, and a
  separately confirmed forced stop if needed. Quitting Fallow with running sessions asks once for
  all of them and then stops them the same way; sessions never outlive Fallow.
- **FR-013**: Sessions MUST be linked to the LLM process panel: the agent's process group is
  labelled with its session and project, and selecting it can jump to the session.

**Projects**

- **FR-014**: Fallow MUST keep a list of projects (folders) that users can add to by path, remove
  from, and reorder. Removing a project never changes its folder.
- **FR-015**: The project list, its order and the last selected project MUST be remembered across
  restarts, stored only in Fallow's own configuration directory.
- **FR-016**: Each project MUST show its sessions, its file tree in the editor view, and, on
  request, the LLM processes whose working directory is inside it.
- **FR-017**: A folder given on the command line MUST be added to the project list and selected.
- **FR-018**: A project whose folder is missing MUST be shown as missing without affecting other
  projects or its running sessions.

**Default screen and views**

- **FR-019**: Fallow MUST start on the agent workspace: project list, sessions of the selected
  project, and the LLM process panel with the system summary. Focus starts on the project list.
- **FR-020**: One key MUST switch between the agent workspace and the editor view of the selected
  project; switching back restores the previous selection and focus in each view.
- **FR-021**: The hint bar on the agent workspace MUST show how to start a session, switch
  sessions, open the editor view and open help (001 FR-060, FR-066).
- **FR-022**: Every new command MUST be in the command palette with its key, and every new default
  key MUST be checked against the known terminal conflicts (001 FR-062, FR-064).

**Profiles**

- **FR-023**: Fallow MUST provide built-in profiles for common agent CLIs and a plain shell, and
  offer only the built-in profiles whose command is found.
- **FR-024**: Users MUST be able to define profiles with a name, command, arguments and environment
  additions in Fallow's configuration, validated like the existing configuration files, with errors
  reported as file and line.

**Constitution and safety**

- **FR-025**: Fallow MUST start a process only when the user explicitly starts or restarts a
  session. It MUST NOT start sessions automatically at startup.
- **FR-026**: Session output and scrollback MUST be kept in memory only. Fallow MUST NOT write them
  to disk, log them, or send them anywhere (Principle VII).
- **FR-027**: Fallow MUST NOT read, parse, store, display or log the environment of a session's
  agent. Configured environment additions are passed to the agent and shown only as names in the
  profile view.
- **FR-028**: Starting Fallow MUST NOT be slowed by the project list or by sessions: the first
  frame stays under 100 ms (001 FR-041), with the project list loaded after the first frame if
  needed.
- **FR-029**: Fallow's resident memory MUST stay under 50 MB with the project list loaded and no
  session, and under 150 MB with the reference large project open and 4 sessions each holding a
  full default scrollback. Agent processes themselves are not counted.
- **FR-030**: Sessions MUST work on Linux, macOS and Windows.

### Key Entities

- **Project**: a folder the user works in. Name (folder name unless renamed), path, position in the
  list, missing or present. Has sessions, a file tree and an editor state.
- **Agent profile**: what can be started. Name, command, arguments, environment additions (names
  shown, values never displayed), built-in or user-defined, available or not found.
- **Session**: one running (or exited) agent in a terminal pane. Project, profile, state (working,
  needs attention, exited with status), time in state, size, scrollback, and its link to the
  agent's process group in the LLM process panel.
- **Workspace view**: which view is shown (agent workspace or editor view), the selected project
  and session, and the layout (single session or split).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new user starts an agent session in a project within 30 seconds of opening Fallow,
  using only the hint bar, help or command palette.
- **SC-002**: Each built-in agent profile's first screen in a Fallow session matches the same agent
  in a standalone terminal of the same size, cell for cell, apart from documented gaps.
- **SC-003**: Keys typed into a focused session reach the agent with no perceptible delay: the
  agent's echo of a typed character appears within 50 ms on the reference hardware.
- **SC-004**: A session that starts waiting for input is marked "needs attention" within the
  attention delay plus 2 seconds in at least 95% of trials.
- **SC-005**: Switching from one session to another takes one key press and the new session is
  fully drawn within 100 ms.
- **SC-006**: With 8 sessions running, Fallow stays responsive: a key press in the focused session
  is shown within 100 ms while the other sessions produce continuous output.
- **SC-007**: Fallow's resident memory stays under 50 MB at idle with the project list loaded, and
  under 150 MB with the reference large project and 4 sessions with full scrollback. Startup stays
  under 100 ms. All three are measured in CI on all three platforms.
- **SC-008**: In a test agent that prints a secret canary, the canary never appears in any file
  Fallow writes or in anything it sends over the network.
- **SC-009**: After a restart, 100% of listed projects are restored in order with the last
  selection.

## Assumptions

- This feature is specified on branch `001-llm-aware-tui-editor`, because all work for now happens
  on that branch (user instruction). It can move to its own branch when planning starts.
- The editor, file operations, LLM process panel and system summary of 001 are reused unchanged
  except for the three changes listed under "Relation to feature 001".
- "Reference hardware" and the "reference large project" are those of the constitution and 001
  research R-13.
- Built-in profiles cover at least Claude Code, Codex CLI, Gemini CLI and Aider, plus a plain
  shell. The exact list is decided in planning.
- A session runs with Fallow's own user, permissions and environment, plus the profile's additions.
  Fallow passes the environment on without reading it.
- Notifications are inside Fallow only (hint bar, counts, row markers). Desktop notifications are
  out of scope.
- Sessions on remote machines, in containers and in WSL2 are out of scope, as in 001.
- The agent workspace is a terminal UI like the rest of Fallow (Principle III); the embedded
  terminal emulator is part of Fallow, not an external program.
- The memory budget for sessions counts Fallow's own process only. Each agent's own memory is
  shown in the LLM process panel, not counted against Fallow's budget.
