# Data Model: LLM-Aware Terminal Editor

**Feature**: [spec.md](./spec.md) | **Research**: [research.md](./research.md)

All state is in memory. Nothing is persisted except the user's own config files and files the
user saves. Process identity is always **(PID, start time)**, never the PID alone. This
satisfies the PID-reuse edge cases and FR-033.

## Process monitoring

### ProcessKey
| Field | Type | Notes |
|---|---|---|
| pid | u32 | |
| start_time | u64 | Seconds since epoch, from the OS. Together with pid, it identifies the process. |

### ProcessRecord (one per visible process, per snapshot)
| Field | Type | Source | Notes |
|---|---|---|---|
| key | ProcessKey | sysinfo | |
| parent | Option<ProcessKey> | sysinfo | |
| name | CompactString | sysinfo | |
| cmdline | Option<Arc<[Box<str>]>> | platform (R-1/R-2) | Cached per key and read once. |
| owner | Option<Uid> | sysinfo | Shown only in all-users mode (FR-031). |
| cpu_samples | [f32; 5] ring | sysinfo | Last 5 refreshes (10 s at the default interval), as % of one core. |
| footprint | Option<Footprint> | platform (R-3) | Only for LLM group members. |
| cwd | CwdState | platform | See below. Only for LLM group members. |

**Validation**: Records without a start time are dropped. The record's user must equal the current
user unless all-users mode is on (FR-030).

### Footprint
| Field | Type | Notes |
|---|---|---|
| bytes | u64 | |
| metric | enum `PhysFootprint` / `PrivateWorkingSet` / `Pss` / `Rss` / `WorkingSet` | Shown in the UI (FR-019). |

### CwdState
enum `Unknown` / `Pending` / `Known(PathBuf)` / `Unavailable(reason)`. On Windows, a value
moves from `Unknown` to `Pending` to `Known` or `Unavailable` per the R-10 policy.

### Connection (per snapshot, filtered)
| Field | Type | Notes |
|---|---|---|
| owner | ProcessKey | |
| local | SocketAddr | |
| remote | SocketAddr | |
| state | enum `Established` / `Listen` / `Other` | Only these three are kept. |

Only connections that could matter are kept:
- Established connections to port 443 of an address in the provider set.
- Listening sockets of local model servers.
- Established loopback or local connections to or from those listening ports.

### DetectionSignal
| Field | Type | Notes |
|---|---|---|
| kind | enum `Signature{id}` / `ProviderConnection{provider_id, via: Resolved|Range}` / `LocalModelConnection{server: ProcessKey}` | No environment-based kind exists (FR-024). |
| confidence | enum `Certain` / `Likely` | Signature → Certain. Provider or local-model connection → Likely. |

### LlmProcess
| Field | Type | Notes |
|---|---|---|
| key | ProcessKey | |
| kind | enum `AgentCli` / `Script` / `Editor` / `LocalModelServer` / `Unknown` / `Browser` | From the signature, or `Script` for an interpreter matched by connection, else `Unknown`. |
| signals | SmallVec<[DetectionSignal; 2]> | At least one (FR-025). |
| confidence | Confidence | Highest among the signals. |
| provider | Option<ProviderId> | From the signature or the connection. |
| model | Option<CompactString> | For local model servers, from the signature's model hint or the command line. Otherwise from the server it connects to. |
| status | Status | See the state machine. |

**Validation**:
- A process whose only matching signature has `kind = Browser` is dropped unless
  `detection.browsers = true` (FR-026).
- A provider match requires `Established` and remote port 443 (FR-022).

### ProcessGroup
| Field | Type | Notes |
|---|---|---|
| root | ProcessKey | An LLM process that has no LLM ancestor. |
| members | Vec<ProcessKey> | All descendants, including nested LLM processes. |
| nested | Vec<ProcessKey> | Descendants that are themselves LLM processes (FR-013). |
| total_footprint | u64 | Each member counted once. |
| total_cpu | f32 | |

### Status (FR-012): evaluated each snapshot, first match wins

```text
            ┌────────────────────────────────────────────────────────────┐
            │ kind == LocalModelServer AND ≥1 established incoming       │──▶ Serving
            │ connection on one of its listening ports                   │
            └────────────────────────────────────────────────────────────┘
                 │ else
            ┌────────────────────────────────────────────────────────────┐
            │ ≥1 established outgoing connection to a provider:443       │──▶ LlmConnected
            │ or to a local model server's listening port                │
            └────────────────────────────────────────────────────────────┘
                 │ else
            ┌────────────────────────────────────────────────────────────┐
            │ mean(cpu_samples) > 2.0                                    │──▶ Busy
            └────────────────────────────────────────────────────────────┘
                 │ else                                                  ──▶ Idle
```

Status is not sticky. It is recomputed from scratch every snapshot, so any status can follow any
other.

### SystemSummary (FR-016)
| Field | Type | Notes |
|---|---|---|
| total_ram | u64 | OS |
| available_ram | u64 | OS "available" |
| llm_ram | u64 | Sum of the group totals (members unique across groups) |
| local_model_ram | u64 | Sum of the group totals whose root is a `LocalModelServer` |
| fallow_ram | Footprint | Fallow's own footprint, same metric as the other figures (spec assumption) |
| low_memory | bool | `available_ram < 10% of total_ram` |

### Snapshot
`{ taken_at, groups: Vec<ProcessGroup>, llm: HashMap<ProcessKey, LlmProcess>, records:
HashMap<ProcessKey, ProcessRecord> (LLM group members only), summary: SystemSummary,
notes: Vec<Note> }`. Notes carry the container, remote and WSL2 statements (FR-018) and any
degraded-mode warnings. The procmon thread sends snapshots to the UI as `Arc<Snapshot>`.

## Detection configuration

### Signature
| Field | Type | Notes |
|---|---|---|
| id | String | Unique, and the merge key for user overrides |
| enabled | bool | default true |
| kind | Kind | |
| match.name | Option<glob> | Case-insensitive on Windows and macOS |
| match.cmdline | Option<regex> | Matched against the argv joined with spaces |
| provider | Option<ProviderId> | |
| model_from_cmdline | Option<regex> | One capture group gives the model name |
| local_server | Option<{ default_ports: [u16] }> | Only for `LocalModelServer` |

**Validation**: at least one of `match.name` or `match.cmdline`. Regexes compile at load time,
and invalid ones are reported with file and line.

**Matching**: every specified matcher of one signature must match (AND); a process is detected
when any enabled signature matches (OR). See contracts/config.md.

### Provider
| Field | Type | Notes |
|---|---|---|
| id | String | e.g. `anthropic`, `openai` |
| display_name | String | |
| hostnames | Vec<String> | The only names the DNS thread may resolve (FR-022a) |
| ranges | Vec<IpNet> | Built-in fallback ranges (FR-022b) |
| enabled | bool | |

### ResolvedProviderSet
`HashMap<IpAddr, (ProviderId, via: Resolved|Range)>` plus a sorted range list. It is swapped
atomically when the DNS thread finishes a pass. It is never populated from any other source.

## Process control

### ControlRequest (FR-032, FR-033)
| Field | Type | Notes |
|---|---|---|
| action | enum `Kill` / `ForceKill` / `Pause` / `Resume` | `Kill` = SIGTERM on Linux/macOS, `TerminateProcess` on Windows. `ForceKill` = SIGKILL, Linux/macOS only, created only from `AwaitingForceConfirmation`. |
| targets | Vec<ProcessKey> | Root plus members when acting on a group |
| include_children | bool | Shown in the dialog |
| platform_note | Option<&'static str> | Windows pause or resume text (FR-052) |

**Lifecycle**:
```text
Requested ──▶ AwaitingConfirmation ──(cancel / esc / target exited)──▶ Discarded
                     │ confirm
                     ▼
              Revalidating: every target's (pid, start_time) still matches?
                     │ no ──▶ Aborted ("process is gone")
                     │ yes
                     ▼
               Executing ──▶ Done(per-target result: Ok | PermissionDenied | Gone | Error)
```
No path reaches `Executing` without passing through `AwaitingConfirmation`. Tests assert this
(SC-008).

**Kill on Linux/macOS** (graceful first):
```text
Executing (SIGTERM) ──▶ WaitingForExit (poll every 250 ms, up to 5 s)
        │ all targets exited ──▶ Done(Ok)
        │ some still alive
        ▼
AwaitingForceConfirmation (second dialog, remaining targets only)
        │ cancel / esc ──▶ Done(StillRunning)
        │ confirm (new ConfirmedToken for ForceKill)
        ▼
Revalidating ──▶ Executing (SIGKILL) ──▶ Done
```
A target that exits or changes `(pid, start_time)` during the wait or while the second dialog is
open becomes `Gone` and is never sent SIGKILL. On Windows, `Kill` is a single
`TerminateProcess` with no force step.

## Editor

### Project
`{ root: PathBuf, tree: LazyTree, open_buffers: Vec<BufferId> }`. A `LazyTree` node lists its
children only when expanded.

### Buffer
| Field | Type | Notes |
|---|---|---|
| path | Option<PathBuf> | |
| text | ropey::Rope | |
| read_only | bool | True for files over 10 MB |
| language | Option<LanguageId> | From the extension |
| syntax | Option<SyntaxState> | Created on first display |
| undo | EditLog | Capped at 10 MB |
| dirty | bool | |
| disk_state | enum `InSync` / `ChangedOnDisk` / `DeletedOnDisk` | Driven by notify (FR-006) |

**Transitions on disk change (FR-006)**:
- `dirty = false` and the file changed: reload, show a notice, stay `InSync`.
- `dirty = true` and the file changed: go to `ChangedOnDisk` and prompt "keep mine / load
  disk". Nothing is written until the user chooses.
- The file was deleted or renamed: go to `DeletedOnDisk`, show a notice, keep the buffer, and set
  `dirty = true`.

### SearchSession
`{ query, options, results: Vec<SearchHit> (cap 10,000), state: Running|Done|Cancelled|Capped }`

### RecentChanges (FR-017)
`{ dir, window: Duration (30 min), entries: Vec<(PathBuf, SystemTime)> sorted desc (cap 200),
capped: bool, disclaimer: "changes in this directory, not proven to come from this process" }`

## Settings (runtime)
See [contracts/config.md](./contracts/config.md). Key fields: `refresh_interval_ms` (2000),
`recent_window_minutes` (30), `dns.enabled` (true), `dns.refresh_minutes` (15),
`detection.browsers` (false), `windows.read_working_directory` (`on_detect`),
`editor.read_only_threshold_mb` (10).
