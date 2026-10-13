# Feature Specification: LLM-Aware Terminal Editor

**Feature Branch**: `001-llm-aware-tui-editor`

**Created**: 2026-10-13

**Status**: Draft

**Input**: User description: "Build a full-screen terminal UI (TUI) code editor for developers who run
multiple LLM-driven processes at once. The editor must use so little memory that the machine's RAM
stays available for those processes. [...] Out of scope for v1: plugins, debugging, AI features
inside the editor itself."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See every LLM process at a glance (Priority: P1)

A developer has several coding agents and a local model server running. They open Fallow and
immediately see a panel listing every LLM process on the machine (not only those in the current
project), with what each one is, where it runs, what it costs in memory and CPU, and how sure Fallow
is that it is an LLM process. A system summary shows total RAM, RAM used by LLM processes, RAM used
by local models, RAM used by Fallow itself, and RAM remaining.

**Why this priority**: This is the reason Fallow exists. Without it, Fallow is just another editor.
It delivers value on its own even before editing works.

**Independent Test**: Start a known agent CLI, a local model server and a script that calls a cloud
LLM API. Launch Fallow. Verify all three appear with correct type, PID, working directory, memory,
CPU, uptime, status, provider/model (where known) and confidence, and that the summary figures add
up.

**Acceptance Scenarios**:

1. **Given** an agent CLI matching a built-in signature is running, **When** the user opens the
   process panel, **Then** it is listed with type "agent CLI" and confidence "certain".
2. **Given** a local model server is running, **When** the user opens the panel, **Then** it is
   listed with type "local model server", the loaded model name if known, and its memory counts
   toward "RAM used by local models".
3. **Given** an unrecognized script holds an open connection to a known LLM API provider, **When**
   the panel refreshes, **Then** the script is listed with type "script", that provider, and
   confidence "likely".
4. **Given** an agent has spawned a shell, a language server and a test runner, **When** the user
   views the agent, **Then** the children are grouped under it and the agent row shows both its own
   and its group's total memory and CPU.
5. **Given** an LLM process exits, **When** the panel next refreshes, **Then** it disappears from
   the list and the summary updates.
6. **Given** an agent has an established connection to an LLM provider while using almost no CPU,
   **When** the panel refreshes, **Then** its status is "LLM connected". **Given** an agent with
   no such connection averages more than 2% of one core, **Then** its status is "busy". **Given**
   neither, **Then** its status is "idle".
7. **Given** a local model server has at least one established incoming connection from a client
   and uses almost no CPU (for example, because inference runs on the GPU), **When** the panel
   refreshes, **Then** its status is "serving". **Given** the last client disconnects, **Then** its
   status falls back to "busy" or "idle" based on its CPU use.
8. **Given** a local model server has a model loaded, **When** its memory is compared with the
   operating system's own process monitor at the same moment (Activity Monitor on macOS, Task
   Manager on Windows, the same footprint metric as reported by the system on Linux), **Then** the
   figure in Fallow is within 10% of the OS figure.
9. **Given** the panel is visible, **When** the user looks at a memory column, **Then** the UI
   states which footprint metric is in use on this platform.

---

### User Story 2 - Find the process that is slowing the machine (Priority: P2)

The machine feels slow. The developer switches to the process panel, sorts by group memory or CPU,
and identifies the responsible process within seconds.

**Why this priority**: It is the most time-critical scenario and builds directly on Story 1.

**Independent Test**: Run several LLM processes, one of which (with its children) consumes far more
memory than the rest. A tester unfamiliar with the setup must identify it within 10 seconds of
launching Fallow.

**Acceptance Scenarios**:

1. **Given** the panel is open, **When** the user sorts by memory, **Then** process groups are
   ordered by group total memory, highest first.
2. **Given** the panel is open, **When** the user sorts by CPU, **Then** groups are ordered by group
   total CPU, highest first.
3. **Given** available RAM falls below 10% of total, **When** the summary is visible, **Then** the
   "RAM remaining" figure is visually highlighted as low.

---

### User Story 3 - Edit code in a project (Priority: P3)

The developer opens a project directory, browses its file tree, opens files, edits them with syntax
highlighting, saves them, and searches across the project for text.

**Why this priority**: Fallow is an editor; developers need to act on what agents are doing without
switching tools. It ranks after monitoring because existing editors already cover editing, while
the monitoring is what is new.

**Independent Test**: Open a sample project, navigate to a file, change a line, save, search the
project for a string, and open a result, without using the process panel.

**Acceptance Scenarios**:

1. **Given** Fallow is started with a directory, **When** it opens, **Then** the file tree of that
   directory is shown and files can be opened.
2. **Given** an open file of a supported language, **When** it is displayed, **Then** syntax is
   highlighted.
3. **Given** an edited file, **When** the user saves, **Then** the change is written to disk.
4. **Given** a project is open, **When** the user searches for a text string, **Then** all matching
   lines across project files are listed and each can be opened at its location.
5. **Given** a file is open with no unsaved edits and an agent changes it on disk, **When** Fallow
   notices the change, **Then** the buffer reloads and the user is told it changed.
6. **Given** a file is open with unsaved edits and an agent changes it on disk, **When** Fallow
   notices the change, **Then** the user is asked whether to keep their version or load the disk
   version; nothing is overwritten silently.

---

### User Story 4 - Discover an unknown LLM process and understand why it was flagged (Priority: P4)

The developer sees an LLM process they did not start. They select it and see which detection
signals matched and how confident each one is, so they can judge whether it really is an LLM
process.

**Why this priority**: It builds trust in the detection and covers the "unexpected process"
scenario, but it relies on Story 1's list.

**Independent Test**: Start a process that is detected only via a network signal. Select it and
verify that the details view lists the matched signal, its confidence and the provider endpoint.

**Acceptance Scenarios**:

1. **Given** a detected process, **When** the user opens its details, **Then** every matched signal
   is listed (signature, provider connection, local model connection) with its confidence.
2. **Given** a process detected only through a provider connection, **When** the user opens its
   details, **Then** the details show the provider and whether it was matched by hostname
   resolution or by a built-in address range.
3. **Given** the user adds a signature to the user config file, **When** Fallow reloads the config,
   **Then** matching processes are detected with confidence "certain".

---

### User Story 5 - Jump from a process to its work (Priority: P5)

The developer selects an LLM process and Fallow switches the file view to that process's working
directory and lists files in it that changed recently, so the developer can review what the agent
has been doing.

**Why this priority**: It connects monitoring and editing, but is useful only once both exist.

**Independent Test**: Run an agent in a directory and let it change two files. Select the agent in
the panel. Verify that the file tree shows its working directory and the two files appear in the
recent-changes list, newest first.

**Acceptance Scenarios**:

1. **Given** a process is selected, **When** the user chooses "go to working directory", **Then**
   the file tree opens that directory.
2. **Given** files in that directory changed in the last 30 minutes, **When** the directory opens,
   **Then** they are listed newest first with their modification times, labeled as changes in the
   directory rather than as changes proven to come from that process.
3. **Given** the working directory cannot be read by the current user, **When** the user tries to
   jump there, **Then** Fallow says so and stays where it was.

---

### User Story 6 - Stop or pause a runaway process, with confirmation (Priority: P6)

The developer decides a process must stop. They choose "kill" or "pause" (and later "resume"), and
Fallow asks for explicit confirmation that names the process and the action before doing anything.

**Why this priority**: It is useful but rare, and other tools can already do it. Because it is the
only feature that changes another process, it carries the highest risk.

**Independent Test**: Start a dummy LLM process, request "kill", cancel, and verify it is still
running. Request again, confirm, and verify it has stopped. On Linux and macOS, also start a dummy
that ignores SIGTERM: after 5 seconds "Force kill" is offered, cancelling leaves it running, and
confirming stops it.

**Acceptance Scenarios**:

1. **Given** a selected process, **When** the user requests "kill", **Then** a confirmation shows
   the process name, PID, group size and the action, and nothing happens until the user confirms.
2. **Given** the confirmation is shown, **When** the user cancels or presses escape, **Then** the
   process is untouched.
3. **Given** a process group is selected, **When** the user confirms "kill", **Then** the
   confirmation states whether the children will also be stopped, and Fallow does exactly that.
4. **Given** the user lacks permission to signal the process, **When** they confirm, **Then** Fallow
   reports the failure and does not ask for elevation.
5. **Given** Linux or macOS and a confirmed "kill", **When** Fallow acts, **Then** it asks the
   process to stop gracefully (SIGTERM) and waits up to 5 seconds. If the process exits, the kill
   is reported as done. If it is still running after 5 seconds, Fallow offers "Force kill"
   (SIGKILL) in a **second** confirmation, which lists the processes still running. Nothing more
   happens unless the user confirms that too; cancelling leaves them running.
6. **Given** Windows, **When** the user requests "kill", **Then** the confirmation states that the
   kill is immediate and the process gets no chance to clean up or save state. Confirming
   terminates it in one step, with no separate force kill.

---

### Edge Cases

- A process's PID is reused by an unrelated process between refreshes: the old entry disappears and
  the new process is evaluated from scratch, never inheriting the old entry's details.
- A process exits while its confirmation dialog is open: confirming does nothing and the user is
  told the process is gone. Fallow never signals a PID that has since been reused.
- A process belongs to another user in default mode: it is not shown. The panel notes that other
  users' processes are hidden.
- The user config file has a syntax error: Fallow keeps working with the built-in signatures and
  shows which line is invalid.
- A user signature duplicates or conflicts with a built-in one: the user entry wins.
- A web browser connects to an LLM provider while the browser category is off: it is not listed.
- A process runs inside a container or on a remote machine: it is not listed. The panel states
  permanently that container and remote processes are not covered in v1.
- On Windows, a process runs inside WSL2: it is not visible to the Windows build. On Windows, the
  panel states this permanently.
- Provider hostname resolution is disabled, or the resolver is unreachable or fails: Fallow falls
  back to the built-in address ranges and keeps working. The details view shows which method was
  used.
- On Windows, the user pauses a process: the confirmation states how pausing on Windows differs
  from Linux and macOS (see FR-052) before the user confirms.
- No LLM processes are running: the panel shows an empty state, and the summary still shows total,
  Fallow and remaining RAM.
- Memory is shared between processes: on Linux with proportional set size readable, shared pages
  are split fairly between processes. Where Fallow falls back to resident memory, shared pages may
  be counted more than once and group totals may exceed the true footprint. The UI shows which
  metric each figure uses.
- An LLM provider sits behind shared CDN addresses: an unrelated process talking to another site on
  the same CDN can be matched as a false positive. Such matches stay "likely" and count toward the
  false-positive limit in SC-003.
- An agent keeps an idle keep-alive connection open to its provider: it shows "LLM connected" for
  as long as the connection stays established, even when no request is in flight (FR-012).
- A local model server accepts connections from clients: it shows "serving" while at least one
  client connection is established, regardless of CPU use, since inference may run on the GPU.
  A client that keeps an idle connection open keeps the server "serving", for the same reason as
  the keep-alive case above.
- A very large or binary file is opened: binary files are not opened as text, and files above a size
  limit open read-only with a warning, to protect the memory budget.
- An agent deletes or renames a file that is open in the editor: the user is told and the buffer
  stays open as unsaved.
- The terminal window is very small: the panel and editor collapse to a readable layout, never
  garbled output.

## Requirements *(mandatory)*

### Functional Requirements

**Editor**

- **FR-001**: Fallow MUST run as a full-screen application inside a terminal, with no graphical
  window.
- **FR-002**: Users MUST be able to open a project directory and browse its file tree with the
  keyboard and the mouse, opening files and expanding folders.
- **FR-003**: Users MUST be able to open, edit and save text files, with undo and redo.
- **FR-004**: Fallow MUST highlight syntax for common programming, markup and configuration
  languages (see Assumptions).
- **FR-005**: A single find command MUST search either the current file or the whole open project,
  with the scope toggled inside the find box and always shown. Project results MUST be openable at
  their line. Project search MUST run only when the user submits it, without a prebuilt
  background index, and MUST respect `.gitignore`.
- **FR-006**: Fallow MUST detect when an open file changes on disk and MUST NOT silently discard
  either the disk version or unsaved edits.
- **FR-007**: Fallow MUST warn before quitting with unsaved changes.
- **FR-008**: Users MUST be able to select text with the keyboard (Shift with the arrow and
  Home/End keys, select all) and by dragging the mouse, and to copy, cut and paste it.
  - Copy and cut MUST place the text on the system clipboard, using the terminal clipboard
    sequence (OSC 52) where the terminal supports it and the platform clipboard as a fallback.
  - Copy with no selection MUST do nothing, and the hint bar MUST point to the quit key instead.
  - Text pasted through the terminal MUST be received as a single bracketed paste and inserted
    literally.
- **FR-009**: Users MUST be able to jump to a line (and optionally a column) in the current file.

**Process panel**

- **FR-010**: Fallow MUST provide a process panel listing every detected LLM process belonging to
  the current user, regardless of the open project.
- **FR-011**: For each process, the panel MUST show: name, type (agent CLI, script, editor, local
  model server, unknown, and browser when that category is enabled), process ID, working directory,
  memory usage (FR-019), CPU usage, uptime, status (serving, LLM connected, busy or idle), LLM provider or
  local model if known, and detection confidence (certain or likely).
- **FR-012**: Each process MUST have exactly one status, chosen by the first rule that applies:
  - "serving" (local model servers only) if the server has at least one established incoming
    connection from a client, regardless of CPU use, because inference may run on the GPU while
    CPU stays low;
  - otherwise "LLM connected" if it has an established outgoing connection to an LLM provider
    endpoint (as defined in FR-022) or to a local model server, regardless of CPU use. This status
    means only that the connection exists, not that a request is in flight: Fallow cannot observe
    traffic on a connection without payload inspection or elevated privileges, and both are ruled
    out (FR-027, FR-030). The UI MUST explain this, for example in the status legend or help;
  - otherwise "busy" if its CPU use averaged over the last 10 seconds exceeds 2% of one core;
  - otherwise "idle".
- **FR-013**: Descendant processes of an LLM process MUST be grouped under it, with group totals
  for memory and CPU. Descendants that are themselves LLM processes MUST appear as nested LLM
  entries.
- **FR-014**: Users MUST be able to sort the panel by memory, CPU, uptime, name and confidence.
- **FR-015**: The panel MUST refresh automatically, every 2 seconds by default, and the interval
  MUST be user-configurable.
- **FR-016**: Fallow MUST show a system summary of total RAM, RAM used by LLM processes (group
  totals, counting each process once), RAM used by local model servers, RAM used by Fallow itself,
  and RAM remaining.
- **FR-017**: Selecting a process MUST offer to switch the file view to its working directory and
  list files there that changed in the last 30 minutes (configurable), newest first. The list MUST
  be built only when requested, and MUST say that changes are not proven to come from that process.
- **FR-018**: The panel MUST state, at all times, that processes in containers and on remote
  machines are not covered. On Windows, it MUST also state that processes inside WSL2 are not
  covered.
- **FR-019**: Per-process memory MUST use the platform's best per-process footprint metric:
  physical footprint on macOS, private working set on Windows, and proportional set size on Linux
  where it can be read, with resident set size otherwise. The UI MUST state which metric is in
  use, including per process on Linux when some processes fall back to resident set size. On
  macOS, the figure MUST include unified memory that local models use for inference, wherever the
  OS reports it as part of the process's footprint. The system summary's process figures (FR-016)
  use the same metric.

**Detection**

- **FR-020**: Fallow MUST detect LLM processes by matching the process name and command line
  against a built-in signature list. A match yields confidence "certain".
- **FR-021**: Users MUST be able to add, override and disable signatures in a user-editable config
  file. User entries take precedence over built-in ones.
- **FR-022**: Fallow MUST detect processes with a connection to a known LLM API provider and mark
  them "likely" unless another signal makes them "certain". A provider match requires an
  established TCP connection to port 443 of a provider address. Listening sockets, connections
  being opened or closed, and other ports do not count. A provider match on its own never yields
  "certain".
- **FR-022a**: To recognize provider endpoints, Fallow MAY resolve the configured list of provider
  hostnames (built-in defaults plus user config) through the system resolver, using forward
  lookups only. It MUST cache the results, and MUST NOT resolve any other hostname or perform
  reverse lookups of the user's connections.
- **FR-022b**: Users MUST be able to disable provider hostname resolution with a config setting.
  When it is disabled or fails, Fallow MUST match endpoints against built-in provider address
  ranges instead.
- **FR-022c**: Provider hostname resolution MUST NOT delay the first interactive screen. Until
  results are available, Fallow uses the built-in address ranges.
- **FR-023**: Fallow MUST identify local model servers by signature, and MUST detect processes
  connected to a local model server as LLM processes using that server's model ("likely").
- **FR-024**: Fallow MUST NOT request the environment of any process, including to check whether
  an API key variable exists. Environment variables are not a detection signal. Fallow MUST NOT
  parse, store, display or log environment variable values. Where the operating system's only
  command-line API also returns the environment (macOS), the returned buffer MUST be wiped before
  the read returns (constitution Principle VII).
- **FR-025**: Every listed process MUST show each signal that matched and that signal's confidence.
- **FR-026**: Web browsers MUST be excluded from detection by default. Users MUST be able to enable
  them as a separate "browser" category.
- **FR-027**: Detection MUST use only process metadata and connection endpoints. Fallow MUST NOT
  inspect network payloads.

**Privileges and safety**

- **FR-030**: By default, Fallow MUST run without elevated privileges and show only the current
  user's processes.
- **FR-031**: When the user launches Fallow with elevated privileges and enables "all users" mode,
  Fallow MUST show all users' processes, label each with its owner, and show a persistent indicator
  that elevated mode is active. Fallow MUST NOT prompt for elevation itself.
- **FR-032**: Kill, pause and resume MUST each require an explicit confirmation that names the
  process, PID, action and whether child processes are affected. There MUST be no "don't ask
  again" option.
  - On Linux and macOS, "kill" MUST first request a graceful stop (SIGTERM) and wait up to 5
    seconds. Forcing (SIGKILL) MUST only follow a second, separate confirmation for the
    processes still running.
  - On Windows, "kill" terminates immediately, and its confirmation MUST say that the process
    cannot clean up.
- **FR-033**: Before acting, Fallow MUST verify that the PID still refers to the same process that
  was confirmed.
- **FR-034**: Fallow MUST NOT change any process, file or setting outside its own configuration and
  cache, except for:
  - user-confirmed kill, pause and resume;
  - saving files the user edited;
  - file operations the user starts explicitly in the Files pane (FR-068).
- **FR-035**: No user data, file contents, process details or network payloads may leave the
  machine. The only outbound traffic Fallow may cause is the provider hostname resolution in
  FR-022a.

**Resource budgets** (from the constitution)

- **FR-040**: Fallow MUST use less than 50 MB of resident memory when idle with the process panel
  active, and less than 150 MB with the reference large project open.
- **FR-041**: Fallow MUST show its first interactive screen in under 100 ms.
- **FR-042**: Fallow MUST NOT index or scan in the background unless the user explicitly enables it.

**Platforms**

- **FR-050**: Fallow v1 MUST support Linux, macOS and Windows, with the same features on each
  except where this spec documents a difference.
- **FR-051**: The Windows build MUST NOT show processes running inside WSL2. This is a documented
  limitation shown in the UI (FR-018).
- **FR-052**: Pause and resume MUST be available on all three platforms. Where Windows behaves
  differently from Linux and macOS (for example, in how a paused process or its children react),
  the difference MUST be stated in the pause confirmation and in the user documentation, not
  hidden.
- **FR-053**: Fallow MUST be built, tested and benchmarked against the resource budgets
  (FR-040, FR-041) in CI on all three platforms.

**Usability**

- **FR-060**: A bar of key hints MUST always be visible, showing the most relevant keys for the
  focused pane and current mode.
- **FR-061**: A help overlay MUST list every key binding, grouped by pane, and the known terminal
  conflicts. It MUST explain that while Fallow captures the mouse, the terminal's own text
  selection needs a modifier held while dragging (usually Shift), and that the optional Alt
  shortcuts on macOS need the terminal's "Option as Meta" setting. It MUST close with escape.
- **FR-062**: A command palette MUST give access to every command by name, with type-to-filter,
  and MUST show each command's key binding next to it.
- **FR-063**: Users MUST be able to use the mouse to select items (files, processes, search
  results, editor positions), scroll any scrollable pane, and resize panes by dragging their
  borders, in terminals that report mouse events. Dragging in the editor MUST select text, and
  clicking a process panel column header MUST sort by that column (clicking again reverses).
  Every mouse action MUST also be reachable from the keyboard or the command palette.
- **FR-064**: No default key binding may lose its function because the default settings of
  Windows Terminal, macOS Terminal.app or iTerm2 intercept it. A terminal binding that intercepts
  a key but produces the same effect is acceptable if documented; for example, Windows Terminal's
  Ctrl-V paste arrives as a bracketed paste. Every interception MUST be listed in the known
  terminal conflicts, with an alternative where the function would be lost, and verified on each
  platform before release.
- **FR-065**: Tab and Shift-Tab MUST be the primary way to move between panes. Direct pane
  shortcuts (Alt with a number) MAY exist as optional extras. In the editor, where Tab indents,
  the way to leave with the keyboard MUST be shown in the hint bar.
- **FR-066**: Help MUST be reachable with F1, with "?" when focus is not in a text input, and
  from the command palette. The hint bar MUST always show a way to open help that does not need
  F1, because F-keys need an extra modifier on many keyboards.
- **FR-067**: Fallow MUST request unambiguous modifier-key reporting (the kitty keyboard protocol)
  from terminals that support it. It MUST fall back to standard key decoding where the terminal
  doesn't support it, and MUST restore the terminal's mode on exit. Requesting the protocol MUST
  NOT delay the first frame (FR-041).
- **FR-068**: In the Files pane, users MUST be able to create a file, create a folder, rename and
  delete. Delete MUST require an explicit confirmation that shows the path and, for folders, how
  many items they contain, with Cancel as the default choice.

### Key Entities

- **LLM Process**: A running process judged to be LLM-driven. It has a PID, name, command line,
  owner, working directory, start time, memory (with the footprint metric used), CPU, status (serving,
  LLM connected, busy or idle; "serving" applies only to local model servers), type, provider or model if known, confidence, and a list of matched
  signals.
- **Process Group**: An LLM process plus all of its descendants. It has group memory and CPU totals.
  It may contain nested LLM processes.
- **Detection Signal**: One piece of evidence for a process: its kind (signature, provider
  connection, local model connection), its confidence, and what matched (signature name, provider
  and match method, model server).
- **Signature**: A rule that identifies a known LLM tool by name or command-line pattern. It has a
  resulting type and optional provider or model. It comes from either the built-in list or the user
  config.
- **Provider Endpoint**: A known LLM API provider with its configured hostnames, the cached
  addresses they resolved to, and built-in fallback address ranges.
- **Local Model Server**: An LLM process that serves a model locally. It has a listening endpoint
  and loaded model names where they can be determined.
- **System Summary**: A snapshot of total RAM, LLM RAM, local model RAM, Fallow RAM and remaining
  RAM.
- **Project**: An opened directory with its file tree and open buffers.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In a usability test, a developer unfamiliar with the setup identifies the process
  group using the most memory within 10 seconds of launching Fallow.
- **SC-002**: On a reference set of at least 20 LLM tools (agent CLIs, local model servers, scripts
  using provider APIs), Fallow detects at least 95%, and 100% of the tools covered by built-in
  signatures are marked "certain".
- **SC-003**: On a reference set of at least 20 common non-LLM developer processes (shells, build
  tools, browsers with the browser category off), Fallow lists at most one as an LLM process.
- **SC-004**: A newly started LLM process appears in the panel within two refresh intervals (4
  seconds by default). An exited process disappears within the same window.
- **SC-005**: Fallow's resident memory stays under 50 MB when idle and under 150 MB with the
  reference large project open, as measured by an automated benchmark in CI on Linux, macOS and
  Windows.
- **SC-006**: Fallow shows its first interactive screen in under 100 ms, as measured by an automated
  benchmark in CI on Linux, macOS and Windows.
- **SC-007**: With a unique canary value placed in an environment variable of a monitored
  process, the canary never appears in Fallow's memory (checked after a full detection cycle),
  screen output, logs, config, cache or crash output. The test runs on Linux, macOS and Windows.
- **SC-008**: In automated tests, no kill, pause or resume reaches a process without a recorded
  confirmation for that exact process.
- **SC-009**: During a full test session, Fallow's only outbound traffic is DNS queries for the
  configured provider hostnames. With resolution disabled, it makes no outbound traffic at all.
- **SC-010**: A full-project text search over the reference large project returns its first
  results in under 1 second.
- **SC-011**: In a usability test, a developer new to Fallow, without documentation, opens a file
  in a project and sorts the process panel by memory within 2 minutes.
- **SC-012**: For a running local model server, Fallow's memory figure is within 10% of the
  operating system's own process monitor on each of the three platforms.

## Assumptions

- Users are developers comfortable with terminal applications and keyboard-driven interfaces.
- "Memory usage" means the per-process footprint metric defined in FR-019. On macOS with unified
  memory, it includes memory that local models use for inference wherever the OS reports it.
  Dedicated GPU memory (VRAM) on discrete GPUs is not counted in v1, and the UI says so.
- Fallow's own budgets (FR-040, SC-005) stay defined as resident set size, because that is what
  the constitution specifies. The "Fallow itself" figure in the system summary uses the FR-019
  metric so that it can be compared with the other figures.
- "RAM remaining" means the memory the operating system reports as available.
- Syntax highlighting covers, at minimum: Python, JavaScript/TypeScript, Rust, Go, C/C++, Java,
  C#, Shell, JSON, YAML, TOML, Markdown, HTML and CSS. Highlighting support loads only when a file
  of that language is opened.
- The size limit above which files open read-only defaults to 10 MB.
- The built-in signature and provider lists ship with Fallow and are updated with new releases. No
  automatic online updates, per the constitution's privacy principle.
- Provider hostname resolution is on by default. Provider hostnames are re-resolved every 15
  minutes, and results are cached in memory in between. Record TTLs are not available from the
  system resolver, so they are not used.
- "Editor" type covers editors with built-in LLM features (for example AI-enabled IDEs) when they
  match a signature.
- Pause and resume use the operating system's standard suspend and continue mechanism. Whether a
  paused process recovers cleanly depends on that process.
- The reference large project and the reference process sets (SC-002 and SC-003) will be defined as
  pinned fixtures in the benchmark suite, as required by the constitution.
- Out of scope for v1: plugins, debugging, AI features inside the editor, container processes,
  WSL2 processes on Windows, remote machines, environment-variable detection, and
  dedicated GPU memory accounting.
