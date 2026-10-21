---

description: "Task list for 003 Agent Orchestration"
---

# Tasks: Agent Orchestration

**Input**: Design documents from `specs/003-agent-orchestration/`

**Prerequisites**: [plan.md](./plan.md), [spec.md](./spec.md), [research.md](./research.md),
[data-model.md](./data-model.md), [contracts/](./contracts/), [quickstart.md](./quickstart.md)

**Tests**: Included. The constitution's quality gates require them:
- typing into a session on another agent's behalf must be proven unreachable without approval or a
  permission;
- CI must check for listening sockets;
- every performance claim must be benchmarked.

The spec's success criteria SC-002 to SC-007 are CI tests (research R-9). Tests marked [P] are
written first and must fail before their implementation.

**Organization**: by user story, in the spec's priority order. US1 (board) is the MVP. Each later
story builds on the one before: US2 on US1's tools, US3 on US2's interests, US4 on US3's
deliveries, US5 on US4's queue.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story (US1 … US5)
- Paths follow the plan's Project Structure (single crate `src/`, `tests/`, dev-only `xtask/`).

---

## Phase 1: Setup

**Purpose**: dependencies and module skeletons.

- [X] T001 Change `Cargo.toml`:
  - move `serde_json = "1"` from `[dev-dependencies]` to `[dependencies]`;
  - add the `windows-sys` features `Win32_System_Pipes`, `Win32_Storage_FileSystem`, `Win32_System_IO` and `Win32_Security_Cryptography`;
  - run `cargo deny check` and record any change in `deny.toml` with a reason.

  Satisfies: plan Dependency budget.
- [X] T002 [P] Create the module skeletons with doc comments only, and register them in `src/lib.rs`:
  - `src/orchestration/mod.rs`, `board.rs`, `events.rs`, `delivery.rs`, `permissions.rs`, `tasks.rs`, `activity.rs`;
  - `src/orchestration/tools.rs`, `rpc.rs`, `channel.rs`, `identity.rs`, `inject.rs`, `bridge.rs`.
- [X] T003 [P] (with T021) Create the UI skeletons `src/ui/orchestration.rs` and `src/app/orchestration.rs`, and register them in `src/ui/mod.rs` and `src/app/mod.rs`.

---

## Phase 2: Foundational (blocking)

**Purpose**: settings, the protocol, the channel, identity, the bridge, per-run injection and the lazy hub that every story uses.

**⚠️ CRITICAL**: no user story work starts before this phase is done. `bench-startup` and `bench-memory --idle` stay green in CI.

- [X] T004 [P] Add the `[orchestration]` settings to `src/config/schema.rs`, with validation verbatim from contracts/config.md:

  | Setting | Range | Default |
  |---|---|---|
  | `enabled` | bool | true |
  | `request_timeout_s` | "60..=3600" | 600 |
  | `delivery_delay_ms` | "500..=30000" | 3000 |
  | `permission_default_min` | "1..=480" | 30 |
  | `wait_block_s` | "5..=55" | 45 |
  | `rate_per_min` | "1..=600" | 30 |
  | `board_messages` | "100..=1000" | 1000 |
  | `activity_entries` | "500..=5000" | 5000 |

  Add tests to `tests/config_merge.rs`.
- [X] T005 [P] Add the `orchestration` key to profiles in `src/session/profiles.rs`:
  - values `"claude" | "codex" | "gemini" | "none"`;
  - the default for user profiles is `none`;
  - the built-ins are `claude` → `claude`, `codex` → `codex`, `gemini` → `gemini`, and `aider`/`shell` → `none`;
  - a user profile that replaces a built-in by `id` and omits the key keeps the built-in's value;
  - an invalid value is reported as `file:line`.

  Add tests to `tests/profiles.rs`.
- [X] T006 [P] Write `tests/orch_rpc.rs`:
  - newline-delimited JSON-RPC 2.0 framing;
  - `initialize` answers `protocolVersion` "2025-11-25", `capabilities.tools`, and `serverInfo` `{name: "fallow", version}`;
  - `notifications/initialized` is accepted silently;
  - `ping` → `{}`;
  - `server/discover` → error `-32601`;
  - an unknown method → `-32601`;
  - malformed JSON → `-32700`;
  - an invalid request → `-32600`;
  - notifications get no reply;
  - `notifications/cancelled {requestId}` is surfaced to the caller;
  - a request before `initialize` → `-32002`.

  Satisfies: research R-1.
- [X] T007 Implement `src/orchestration/rpc.rs` so T006 passes. Parse with `serde_json`. Keep the request id opaque (number or string). Keep a per-connection state: Uninitialized → Ready.
- [X] T008 [P] Implement `src/orchestration/identity.rs` (research R-3):
  - **Token**: 32 random bytes, from `/dev/urandom` on Unix and `ProcessPrng` on Windows; 64-char lowercase hex; constant-time compare. The token is never `Debug`-printed (a manual `Debug` impl prints `Token(…)`).
  - **Peer credentials**:
    - Linux: `SO_PEERCRED`.
    - macOS: `getpeereid` and `LOCAL_PEERPID`.
    - Windows: `GetNamedPipeClientProcessId`.
  - **Unix membership**: walk from the peer PID up the parents to the session's agent PID. Each step's start time must be ≥ the agent's start time; check again after the walk.
  - **Windows membership**: `IsProcessInJob` with the session's Job handle. Expose the handle from `src/session/platform/windows.rs`.

  Add unit tests for hex, compare and the walk over a fake parent table, including PID reuse.
- [X] T009 Implement `src/orchestration/channel.rs` (research R-2):
  - **Unix listener**:
    - at `<cache_dir>/fallow/run/<pid>.sock`;
    - the directory is created `0700` and checked with `lstat` (is a directory, not a symlink, owned by `getuid()`, mode `0700`), else "unavailable" with the reason;
    - the socket is bound with `umask(077)`;
    - a path over `sun_path` gives "unavailable", with no `/tmp` fallback;
    - stale `*.sock` files whose `connect()` fails with `ECONNREFUSED` are removed;
    - the socket is removed on exit.
  - **Windows named pipe**:
    - named `\\.\pipe\fallow-<pid>-<16 hex>`;
    - created with `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE` on the first instance, and SDDL `D:P(A;;GA;;;<user SID>)`;
    - shutdown connects to the pipe itself to wake the loop.
  - **Accept loop**: one thread. Then one thread per connection. The first line must be the hello `{"session": n, "token": "…"}`. Accept only if the uid, the token and the membership (T008) all pass. Otherwise close, add an activity entry "refused connection", and raise at most one notice per session per minute.
  - **After the hello**: forward each request as `AppEvent::Hub(LinkId, rpc::Message)` and write replies from a per-connection `Sender`.
- [X] T010 Implement `src/orchestration/bridge.rs`, and dispatch `fallow mcp-bridge` in `src/main.rs` / `src/cli.rs` before any terminal setup:
  - **Arguments**: `--endpoint`, `--fallow-pid`, `--session`, `--token`. Wrong usage prints usage and exits 2.
  - **Connection**: connect, then send the hello, then relay lines stdio ⇄ channel unchanged. On Windows, check `GetNamedPipeServerProcessId == --fallow-pid` before sending the token.
  - **Exits**: 0 when either side closes, 3 when it cannot connect or is refused.
  - **Restrictions**: it reads no config and no environment variable, and writes only relayed messages to stdout and errors to stderr.

  Add a case to `tests/cli_args.rs`. Document the subcommand in `specs/001-llm-aware-tui-editor/contracts/cli.md`.

  Satisfies: contracts/mcp-tools.md "Bridge invocation".
- [X] T011 Implement the lazy `Hub` in `src/orchestration/mod.rs` and wire it in:
  - **Hub**: the fields from data-model.md "Hub"; `Link` per session with states `Offered | Connected | Unsupported(reason) | Closed`.
  - **`Sender`**: `User | Agent { session, project, profile }`, snapshotted at send time.
  - **Limits**: as constants with the data-model "Limits" values.
  - **Lazy creation**: the hub, its listener and its threads are created when the first session with orchestration starts, never at startup (FR-005).
  - **Event loop**: add `AppEvent::Hub` in `src/app/events.rs`, dispatch it in `src/app/state.rs`, and call `hub.tick(now)` from the 002 tick.
  - **Exit**: a session exiting closes its link.

  Write `tests/orch_hub.rs::no_hub_before_the_first_orchestrated_session`.
- [X] T012 Implement `src/orchestration/inject.rs` (research R-5): build the per-run additions per style, with the bridge path from `std::env::current_exe()`.
  - **`claude`**: `--mcp-config <json>` with `{"mcpServers":{"fallow":{"command":…,"args":[…]}}}`, placed before another option and never as the last argument before a positional; plus `--allowedTools mcp__fallow__*`; never `--strict-mcp-config`.
  - **`codex`**: `-c mcp_servers.fallow.command='<path>'` (TOML literal string), `-c mcp_servers.fallow.args=[…]`, `-c mcp_servers.fallow.tool_timeout_sec=120`, `-c mcp_servers.fallow.default_tools_approval_mode="approve"`.
  - **`gemini`**:
    - The env addition is `GEMINI_CLI_SYSTEM_SETTINGS_PATH=<run dir>/gemini-<n>.json`. The file is 0600 (owner-only ACL on Windows) with `"trust": true` and `"timeout": 120000`, and is removed when the session exits.
    - This is done only if no system settings file exists at the platform path. Check existence only; never read it. Otherwise the link is `Unsupported("Gemini system settings present")`.
  - **`none`**: the link is `Unsupported("profile")`.
  - **Wiring**: hook it into session start in `src/session/mod.rs` when `orchestration.enabled`.

  Add unit tests for each style's exact argument vector, including a Windows path with backslashes and spaces.
- [X] T013 [P] Implement `src/orchestration/activity.rs`: a ring of `ActivityEntry { at, actor: Sender, action, subject (≤ 200 chars), outcome }`, capped at `activity_entries` (default 5,000), oldest dropped first. Filter by session and by project. Never written anywhere (FR-023). Add unit tests.
- [X] T014 Implement the tool framework in `src/orchestration/tools.rs` (contracts/mcp-tools.md "Rules for every tool"):
  - `tools/list` with the descriptions from the contract;
  - argument validation: unknown arguments → `-32602`;
  - tool errors as `isError: true` with `{"error","message"}` and the codes `rate_limited`, `too_large`, `not_found`, `invalid`, `not_allowed`, `too_many_pending`, `unavailable`;
  - the labelled item form with the `note` text from research R-8;
  - the per-session token bucket at `rate_per_min`, excluding `event_wait` repeats and `inbox`.

  First tools: `whoami` and `session_list` (with `self: true`, `connected`, `state`).
- [X] T015 Add the `mcp-agent` persona to `xtask/src/fixtures.rs`:
  - It is a scripted MCP client. It reads the `--mcp-config` JSON (claude style) from its own argv, spawns the bridge exactly as an agent would, and runs `initialize` then the steps of a script file named in its argv: tool calls, waits, expected results.
  - It prints `ok <step>` or `fail <step>: <why>` to its terminal, and stays at an idle prompt between steps so 002 attention marks it `NeedsAttention`.
  - Add a `profiles.toml` entry template with `orchestration = "claude"` for it.
- [X] T016 Write `tests/orch_identity.rs`, run on all three OSes, with the fallow binary (`CARGO_BIN_EXE_fallow`) and the fixture:
  1. a bridge started inside session #1 with #1's token is accepted;
  2. the same arguments run from a process outside every session are refused (exit 3);
  3. a bridge inside session #2 using #1's token and session number is refused;
  4. a wrong token is refused;
  5. on Unix, the socket directory is `0700` and the socket is not reachable from another uid. Skip if the test cannot switch user, and say so.

  Done as: `tests/orch_identity.rs` covers the channel round trip with the kernel-reported peer, a refused hello, the private run directory, and the membership checks (own process, another session's process, another session's token, an outside process, another uid). A bridge run from outside every session is refused end to end in `cargo xtask orchestration` (T023).

**Checkpoint**: an `mcp-agent` session connects, `whoami` names it, and impostors are refused on all three OSes.

---

## Phase 3: User Story 1 - Board (Priority: P1) 🎯 MVP

**Goal**: agents and the engineer post and read messages on a project board and a shared board, with sender labels set by Fallow.

**Independent Test**: two `mcp-agent` sessions; A posts, B reads A's message with A as sender; the Board tab shows it within 1 s.

- [X] T017 [P] [US1] Write the board tests in `tests/orch_hub.rs`:
  - post and read: `since_id`, `limit` "1..=100" (default 20), oldest first, `newest_id`;
  - text "1..=8 KiB UTF-8"; control characters other than `\n` and `\t` are replaced with `�`;
  - "newest 1,000 per board";
  - the 24 MiB total text cap drops the oldest board messages first;
  - `project` vs `shared` (a message appears only on the chosen board);
  - the rate limit gives `rate_limited` with `retry_after_s`;
  - a user post is labelled `{"kind":"user"}`.
- [X] T018 [P] [US1] Write `tests/orch_hub.rs::sender_cannot_be_forged` (SC-004):
  - a link calls `board_post` with extra `from`, `sender` or `session` arguments → `-32602`, and nothing is stored;
  - a post whose text claims to come from another session is stored with the true sender;
  - 100% of 200 randomized attempts carry the true sender.
- [X] T019 [US1] Implement `src/orchestration/board.rs`: `BoardId = Shared | Project(ProjectId)`; `BoardMessage { id: u64 increasing across boards, sender, at: SystemTime, text }`; the caps above; activity entries `Post`/`Read`.
- [X] T020 [US1] Add the tools `board_post` and `board_read` to `src/orchestration/tools.rs`, returning labelled items (research R-8).
- [X] T021 [US1] Build the orchestration overlay in `src/ui/orchestration.rs` and `src/app/orchestration.rs`:
  - **Layout**: the top counts line (requests · waiting · queued (blocked) · permissions with time left); the tabs Requests, Board, Queue, Permissions, Activity, switched with `Tab`/`Shift-Tab`; `Esc` closes.
  - **Board tab**: the selected project's board, `b` toggles shared; sender, local time and text per message; `↑↓ PgUp PgDn` scroll; `w` writes a message as the user (prompt).
  - **Keys and palette**: bind `o` in the Projects and Sessions contexts in `src/app/keymap.rs`. Add the palette entries "Orchestration: open" and "Write on the board" in `src/app/commands.rs`.
  - **Freshness**: redraw on every hub change, so a post is visible within one frame (SC-002).
- [X] T022 [US1] Show the link state in `src/ui/sessions.rs`: `no orchestration (<reason>)` for `Unsupported` and for `enabled = false`. Add a session details popup on `i` (link state, client name and version from `initialize`) in `src/app/workspace.rs`. Add render tests in `src/ui/workspace.rs`.
- [X] T023 [US1] Create `xtask/src/orchestration.rs` and register `cargo xtask orchestration` in `xtask/src/main.rs`:
  - start Fallow in the PTY harness with two `mcp-agent` sessions (A, B);
  - A posts 50 messages and B polls `board_read`; measure post-to-read latency. The fixture timestamps the post, and B reports when it sees it.
  - **Gate**: p95 < 1,000 ms (SC-002).
  - print `os=… post_read_p95_ms=… result=PASS|FAIL`.
- [X] T024 [US1] Add a step "Orchestration (003)" running `cargo xtask orchestration` to `.github/workflows/ci.yml` on all three OSes, with `timeout-minutes: 10`.

**Checkpoint**: quickstart M1, M2 and M12 pass. CI is green with the SC-002 gate.

---

## Phase 4: User Story 2 - Events (Priority: P2)

**Goal**: agents signal named events and wait for them; the engineer sees who waits for what.

**Independent Test**: B waits for `pr-123-merged`, A signals it, and B's wait ends with A as sender. A wait with a time limit ends `timed_out`. The overview shows "waiting for …".

- [X] T025 [P] [US2] Write the event tests in `tests/orch_hub.rs`:
  - name `[A-Za-z0-9._:/#-]{1,64}`, case-sensitive;
  - signal message "≤ 2 KiB";
  - sticky: a wait on a signalled event ends at once;
  - signalling again replaces sender, time and message;
  - "at most 500 events per scope (oldest signalled first out)";
  - the wait outcomes:
    - `timeout_s` passing → `timed_out`;
    - the block limit `wait_block_s` → `still_waiting` with `waited_s`, and the overview's `since` survives the repeat call;
    - `notifications/cancelled` removes the waiter with no reply;
    - a session exiting removes its waits;
  - interests "max 16" per link;
  - a user signal is labelled user.
- [X] T026 [US2] Implement `src/orchestration/events.rs`:
  - `Event`, `Signal`, `Wait`, and interests per data-model.md;
  - parked replies held by the hub and answered on signal, deadline or block limit from `hub.tick`;
  - `notifications/progress` every 15 s while parked, when the request carried a `progressToken`;
  - activity `Signal`/`Wait`/`WaitEnded`.
- [X] T027 [US2] Add the tools `event_signal`, `event_wait` (with `wake_when_idle`, default true, recording an interest) and `event_status` to `src/orchestration/tools.rs`.
- [X] T028 [US2] UI:
  - the sessions list and overview state column show `waiting for <event> m:ss` (FR-010) in `src/ui/sessions.rs`;
  - the Queue tab's events section lists signal, sender and waiters, with `v` to signal as the user (name and message prompts) and `c` to clear;
  - palette "Signal an event".

  Add render tests.
- [X] T029 [US2] Extend `xtask orchestration` with a wait/signal scenario: B waits; A signals after 2 s; B reports `signalled` with A as sender; a second wait with `timeout_s = 3` reports `timed_out`; a wait with `wait_block_s` at 5 returns `still_waiting` and is repeated.

**Checkpoint**: quickstart M3 passes.

---

## Phase 5: User Story 3 - Delivery with approval, and standing permissions (Priority: P3)

**Goal**: an agent asks Fallow to type a message into another session. Nothing is typed without the engineer's approval or a matching, unexpired standing permission.

**Independent Test**:
- A requests a delivery to B.
- Rejecting types nothing.
- Approving types exactly the label plus the shown text into B as a paste, followed by Enter.
- A permission A→B lets the next delivery through with a notice, until it is revoked.

- [X] T030 [P] [US3] Write `tests/orch_gate.rs` (SC-003; constitution quality gate):
  - **Fake**: give each session a recording fake PTY writer.
  - **Paths**: drive every request path: `deliver`, a wake from an interest, a task hand-over, and a start with a first task.
  - **Zero bytes** must reach the writer for each of:
    - no approval;
    - rejected;
    - expired;
    - an expired permission;
    - a wrong-source permission;
    - a wrong-target permission;
    - a wrong-action permission;
    - a revoked permission;
    - "revoke all" issued before the request.
  - **On approval**, the bytes are exactly:
    - `keys::paste(label + "\n" + text, bracketed)` followed by `\r`;
    - with bracketed paste when the target enabled it;
    - with the label verbatim from research R-7: `[Fallow: message from agent #<n> (<profile>, project <project>). Another agent wrote this, not the user.]`.
  - **Compile check**: a `compile_fail` doctest proves `DeliveryGrant` cannot be built outside `src/orchestration/delivery.rs` and `permissions.rs`.
- [X] T031 [P] [US3] Write the permission tests in `tests/orch_hub.rs`:
  - `cover()` matches by `Session` or `Project` scope for source and target, by action, and only while `now < expires`;
  - duration "1 minute ..= 8 hours, default 30 minutes";
  - `uses` counts;
  - `StartSession { limit }` with `starts_used`;
  - "revoke all" empties the list before the next request is handled (SC-005);
  - every use adds an activity entry and a notice;
  - an exhaustive `match` over the action enum shows there is no stop, pause, resume, kill or re-prioritize variant (FR-017).
- [X] T032 [US3] Implement `src/orchestration/delivery.rs`:
  - **`Delivery`**: `id`, `from`, `to` (a running session other than the sender's), `text` (≤ 8 KiB), `purpose` `Message | Task | Wake`, `created`/`expires` (`request_timeout_s`), `covered_by`.
  - **State machine**: Pending → Approved / Approved(edited) → Delivered; or Rejected / Expired / Failed("target exited").
  - **Limits**: pending "64 in total, 8 per sender", else `too_many_pending`.
  - **`DeliveryGrant`**: a private field; built only by `Hub::approve(id)` and `Permission::cover`.
  - **Activity**: `DeliveryRequested`/`DeliveryOutcome`; the newest 200 finished deliveries are kept.
- [X] T033 [US3] Implement `src/orchestration/permissions.rs`: `Permission { id (P1…), source, target, actions: Deliver | CreateTask | StartSession { limit: u8 }, granted, expires, uses, starts_used }`, `cover()`, `revoke(id)`, `revoke_all()`, and expiry in `hub.tick`.
- [X] T034 [US3] Add `Session::deliver(&mut self, grant: DeliveryGrant, text: &str)` in `src/session/mod.rs` as the only path from the hub to a PTY write:
  - record the time of the last user key sent to each session;
  - an approved delivery waits while the target has focus and its last key is newer than `delivery_delay_ms` (FR-014), and is typed when the user leaves the session or pauses;
  - a target exiting fails the delivery with "target exited".
- [X] T035 [US3] Add the tools `deliver` and `inbox` to `src/orchestration/tools.rs`. `deliver` returns `pending` or `approved` at once. `inbox` returns the outcomes of deliveries, tasks and starts since the last call.
- [X] T036 [US3] Build the Requests and Permissions tabs in `src/ui/orchestration.rs` and `src/app/orchestration.rs` (contracts/ui-commands.md):
  - **Requests tab**: sender → target, the action and its expiry, and the exact text including the label. Keys: `y` approve, `e` edit then approve (edits the text, not the label), `x` reject, `g` the grant dialog pre-filled from the request (approve plus grant).
  - **Grant dialog**: From and To as session or "any session of project"; Allow as deliver, create tasks, or start sessions up to N; For in minutes "max 480". It has no option for destructive actions.
  - **Permissions tab**: `Delete` revoke, `Shift-X` revoke all, `a` grant new.
  - **Hint bar** (`src/ui/hints.rs`): `o N requests` in both views. With a session focused, only the count is shown and the overlay never opens by itself.
  - **Notices** from contracts/ui-commands.md "Elsewhere".
  - **Palette**: "Orchestration: review requests", "Grant standing permission…", "Revoke all permissions".
- [X] T037 [US3] Turn interests into wake deliveries:
  - **Trigger**: when an event a session registered interest in is signalled while that session is in no wait and is `NeedsAttention` (002), create a `Delivery { purpose: Wake }`.
  - **Text**: "Event <name> was signalled by <sender>. <message>".
  - **Rules**: it follows the normal approval and permission rules, in `src/orchestration/events.rs`.

  Add tests.
- [X] T038 [US3] Extend `xtask orchestration` with a delivery scenario:
  - A requests a delivery to B. The harness checks that B's screen shows nothing new.
  - It presses `o`, then `x`. A's `inbox` says rejected.
  - A requests again. The harness presses `y`, and B's screen shows the label and the text.
  - The harness grants `g` for 5 minutes. A's next delivery arrives without a key press. `Shift-X` revokes all, and the next one is pending again.

**Checkpoint**: quickstart M4 to M7 pass. The SC-003 and SC-005 tests are green.

---

## Phase 6: User Story 4 - Task queue (Priority: P4)

**Goal**: tasks are queued for a session or the next free session of a profile in a project. They are handed over when the target is free and the preconditions are met, and advance as agents report done or failed.

**Independent Test**: three tasks for B, the second with precondition `deploy-ok`. T1 is handed over; T2 stays blocked until the event; reports move the queue; the Queue tab shows every state.

- [X] T039 [P] [US4] Write `tests/orch_queue.rs`:
  - **States**: Queued ⇄ Blocked → InProgress → Done / Failed(note); any non-final state → Cancelled, which is never handed over; paused tasks are skipped.
  - **Free target**: running, connected, `NeedsAttention`, not waiting, no own task `InProgress`; one task per target at a time.
  - **Preconditions**: "max 8", all must be signalled.
  - **`NextFree { profile, project }`**: waits when no session is free, and never starts one.
  - **Target exits while in progress**: → Queued for NextFree, Failed("target exited") for Session.
  - **Reports**: only the assigned session may report (`not_allowed`); note "≤ 2 KiB"; text "1..=8 KiB".
  - **Approval**: agent-created tasks need approval unless a `CreateTask` permission covers them (US4.5).
  - **Limits**: "500" tasks.
- [X] T040 [US4] Implement `src/orchestration/tasks.rs`:
  - `Task` per data-model.md;
  - queue advancement in vector order, on every hub change and on 002 attention changes (called from `src/app/state.rs`);
  - hand-over through a `Delivery { purpose: Task }`, under the US3 rules;
  - activity `TaskChanged`.
- [X] T041 [US4] Add the tools `task_create` (target `{"session": n}` or `{"profile", "project"}`, `preconditions`), `task_list` (`mine`, default true) and `task_report` to `src/orchestration/tools.rs`. Agent-created tasks enter the queue as `pending_approval` and show on the Requests tab.
- [X] T042 [US4] Build the Queue tab editing in `src/ui/orchestration.rs` and `src/app/orchestration.rs`:
  - `n` new task (text, target, preconditions);
  - `e` edit;
  - `Shift-↑`/`Shift-↓` reorder;
  - `Space` pause/resume;
  - `Delete` cancel;
  - state, target, preconditions ("blocked: deploy-ok") and note per task;
  - palette "New task".
- [X] T043 [US4] Extend `xtask orchestration` with the PR scenario end to end (SC-001, without a real model):
  - the engineer queues "merge feature/x" for B with precondition `pr-123-merged`;
  - B waits; A signals `pr-123-merged`;
  - the task is handed over with one approval key press;
  - B reports done, and the queue shows Done.
  - Gate: at most one approval was needed.

**Checkpoint**: quickstart M8 and M9 pass.

---

## Phase 7: User Story 5 - Agents request new sessions (Priority: P5)

**Goal**: an agent asks Fallow to start a session (profile, project, first task); it starts only after approval or under a `StartSession` permission with a limit, within 002's limits.

**Independent Test**: the approval prompt shows profile, project and task; approval starts the session with the task queued; request N+1 under a limit-N permission prompts again; at the session limit the request is refused with the reason.

- [X] T044 [P] [US5] Write the start-request tests in `tests/orch_hub.rs`:
  - `StartRequest` states Pending → Approved / Rejected / Expired / Refused(reason);
  - unknown profile or project, or a missing project → `invalid`;
  - 002 FR-007's `max_sessions` and the FR-029 memory guard are checked at request and again at approval time;
  - `starts_used` against `limit`;
  - the first task is queued for `Session(new id)`.
- [X] T045 [US5] Implement `StartRequest` in `src/orchestration/mod.rs` and the `session_start` tool in `src/orchestration/tools.rs`:
  - an approved or covered request starts the session through the same 002 start path as the start menu, in `src/app/workspace.rs`, with the project's default "Where" (worktree for git projects);
  - activity `StartRequested`/`StartOutcome`.
- [X] T046 [US5] Show start requests on the Requests tab (profile, project, first task) in `src/ui/orchestration.rs`. Include "start sessions, up to N" in the grant dialog. Extend `xtask orchestration`: a start request is approved, and the new `mcp-agent` session receives its first task.

**Checkpoint**: quickstart M10 and M11 pass.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [X] T047 [P] Build the Activity tab in `src/ui/orchestration.rs`: newest first, `f` filters to the selected session or project, `/` searches. Add the palette entry "Activity log". Add render tests.
- [X] T048 [P] Update `src/ui/help.rs`:
  - an "Agent orchestration" section: tools, approvals, permissions, what is never allowed;
  - the one-step setup for other agents (the `orchestration` key in `profiles.toml`, FR-004);
  - conflict rows for `o` and `i` ("none known").

  Update `tests/keymap_conflicts.rs` so that every 003 key is in the table and bound.
- [X] T049 Extend `xtask/src/egress.rs` with the listening-socket check during an orchestrated run (SC-006; constitution quality gate). Add one `mcp-agent` session to the run. Per OS:
  - Linux: strace `bind`/`listen` on `AF_INET`/`AF_INET6` is a violation; `AF_UNIX` is noted.
  - Windows: `GetExtendedTcpTable` LISTEN rows and UDP rows owned by Fallow.
  - macOS: `lsof -a -p <pid> -iTCP -sTCP:LISTEN` and UDP.

  Add unit tests for the judge.
- [X] T050 Extend `xtask/src/canary_sessions.rs`:
  - An `mcp-agent` session posts the canary to a board, puts it in a task, and asks for it to be delivered.
  - After quit, every file Fallow wrote is searched for the canary: config, cache, and `run/`, which must be empty.
  - On macOS, the core dump check also runs.

  Satisfies: SC-006.
- [X] T051 Extend `xtask/src/bench_memory.rs`:
  - `--loaded` adds the hub, filled through four `mcp-agent` sessions: the shared board to 1,000 × 8 KiB messages, and the activity log to 5,000 entries;
  - the 150 MB gate stays.

  Satisfies: SC-007.
- [X] T052 [P] Update the docs:
  - `README.md`: orchestration, the tools, approvals and permissions, supported agents, known gaps such as Aider and Gemini with system settings;
  - `src/cli.rs` USAGE: `mcp-bridge` is internal;
  - `specs/001-llm-aware-tui-editor/plan.md`: the macOS hardware checklist gains 003 M1–M13.
- [X] T053 Resolve the research **verify** items and record the results in `specs/003-agent-orchestration/research.md`:
  - the Codex `default_tools_approval_mode` value, and whether progress resets its timeout;
  - Gemini's timeout behaviour;
  - the Unix ancestry walk through wrappers that exit (`npx`, shell shims); if it fails, fall back to process-group membership in `src/orchestration/identity.rs`;
  - whether Claude Code shows any trust prompt for `--mcp-config` servers.
- [X] T054 Run the quickstart manual scenarios M1–M13 on Windows with Claude Code (and Codex if installed). Record the results in `specs/003-agent-orchestration/quickstart.md` "Results", with the CI run numbers of the automated gates. Needs the engineer at the terminal.

---

## Phase 9: Findings from the manual run (T054, 2026-10-21)

**Purpose**: fixes found while the engineer ran the quickstart on Windows with Claude Code
(2.1.304) in `C:\Repos\FallowSandbox`. They block the next release.

- [X] T055 [P] Wrap the label line in the Requests tab like the text below it: today a long label (`[Fallow: message from agent #1 (Claude Code, project FallowSandbox). Another agent wrote this, not the use…`) runs past the right edge and is cut off, so the engineer cannot read the whole text that would be typed (FR-012: "the exact text"). Fix in `src/ui/orchestration.rs` `requests_lines` (use `wrap` for the label too); add a render test at 100 columns in `src/ui/workspace.rs` that finds the label's last words.
- [ ] T057 Close the "deliver `/exit`" loophole, or prove it closed. **Claude Code: proven closed (2026-10-21, quickstart M11)**: the label line comes first, so `/exit` is not read as a command. Still open: the same check with Codex and Gemini when available, then decide whether the hardening below is worth keeping as defense in depth. An agent asked to stop another session proposed delivering `/exit` (Claude Code's quit command). Under a standing "deliver" permission that would be a de facto stop without per-action confirmation (FR-017, constitution IV). Expected to be harmless: the label is always the first line of the same paste, and agent CLIs read slash commands only at the start of the input. Verify with Claude Code (approve one delivery of exactly `/exit`; the target must stay open) and record it in quickstart.md. If the target quits, make deliveries whose text has a line starting with `/` or `!` never covered by a permission (per-action approval only, with a warning in the prompt) in `src/orchestration/mod.rs` `request_delivery`, with a test in `tests/orch_gate.rs`. Also consider letting an agent withdraw its own pending requests (the agent noted it cannot).
- [X] T058 [P] Tell agents how to finish a task. A fresh session (#4, started by #1 with the first task "say hi") did the task and said "it's done" but never called `task_report`, so T2 stayed "in progress". The label typed with a task should end with how to report it: `[Fallow: task T2 … When you are done, report it with Fallow's task_report tool (task 2).]` (and for agents without the tools: "…tell the user"). Change `label` in `src/orchestration/delivery.rs` for `Purpose::Task`, update research R-7 and the label test in `tests/orch_gate.rs`.
- [X] T059 [P] Make the "Signal an event as you" prompt explain itself: the engineer pressed `v` and read the prompt as a confirmation. Title: "Signal an event: type its name (and an optional message), then Enter" in `src/app/editing.rs`.
- [X] T056 [P] Describe session states to agents in their own words: `session_list` (and `whoami`) pass Fallow's 002 state labels, so an agent read "needs attention" as a problem with the other session. Map them in `src/orchestration/tools.rs`: `NeedsAttention` → `"idle"` (at its prompt, waiting for input), `Working` → `"working"`, `Starting` → `"starting"`, exited/failed → `"exited"`; keep the UI's labels unchanged. Update contracts/mcp-tools.md and a test in `tests/orch_hub.rs`.

---

## Dependencies & Execution Order

### Phase dependencies

- Setup (Phase 1) → Foundational (Phase 2) → user stories in order.
- US1 depends on Phase 2.
- US2 depends on US1 (tool framework, overlay, xtask harness).
- US3 depends on US2 (interests become wake deliveries; T037).
- US4 depends on US3: hand-over is a delivery.
- US5 depends on US4: the first task is a queued task.
- Polish depends on the stories it verifies. T049–T051 need US3 and US4 for full coverage.

### Key task-level dependencies

- T006 → T007; T008 → T009 → T010 → T016
- T011 → T012, T014; T015 → T016, T023
- T030, T031 → T032, T033 → T034 → T035 → T036 → T038
- T026 → T037; T040 → T043; T045 → T046

### Within each story

Tests marked [P] first; they must fail before implementation. Then the hub module, then the tools,
then the UI, then the xtask scenario and CI.

## Parallel Example: Phase 2

```text
T004 src/config/schema.rs   T005 src/session/profiles.rs   T006 tests/orch_rpc.rs
T008 src/orchestration/identity.rs   T013 src/orchestration/activity.rs
```

## Parallel Example: User Story 3

```text
# Tests together:
T030 tests/orch_gate.rs   T031 tests/orch_hub.rs (permissions)
```

## Implementation Strategy

### MVP first (User Story 1)

1. Phase 1 and Phase 2. The identity tests (T016) are green on all three OSes before any tool
   exists.
2. Phase 3: the board. **Stop and validate** with quickstart M1, M2 and M12 against Claude Code,
   and with the SC-002 gate in CI.

### Incremental delivery

US1 (board) → US2 (events) → US3 (delivery and permissions: the riskiest step, fully gated by
T030) → US4 (queue) → US5 (session starts). Each increment keeps every constitution gate green in
CI.

## Notes

- [P] tasks touch different files and have no dependency on unfinished tasks.
- Commit after each task or logical group; push to `001-llm-aware-tui-editor`.
- Never write board, event, task, delivery, permission or activity content anywhere, and never log
  it (FR-023). Never print or log a token.
