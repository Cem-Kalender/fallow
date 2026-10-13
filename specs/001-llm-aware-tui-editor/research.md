# Research: LLM-Aware Terminal Editor

**Feature**: [spec.md](./spec.md) | **Plan**: [plan.md](./plan.md) | **Date**: 2026-10-13

Each entry gives the decision, the reason for it, and the alternatives considered. Items marked
**VERIFY** are decided on paper but need a measurement on real hardware before `/speckit-tasks`.
The three research items the user named (R-10, R-11, R-12) are at the end, with their status.

---

## R-1: Where process fields come from (FR-011, FR-024, SC-007)

**Decision**: Use sysinfo only for enumeration, name, parent PID, owner, start time and CPU usage,
with an explicit `ProcessRefreshKind` that enables just those fields and sets `cmd`, `cwd`,
`environ`, `exe` and `memory` to `UpdateKind::Never`. A thin per-platform module owned by Fallow
(`src/procmon/platform/`) reads the command line, working directory and footprint metric.

| Field | Linux | macOS | Windows |
|---|---|---|---|
| Command line | `/proc/<pid>/cmdline` | `sysctl KERN_PROCARGS2` (see R-2) | `NtQueryInformationProcess(ProcessCommandLineInformation)`, needs only `PROCESS_QUERY_LIMITED_INFORMATION` |
| Working dir | `readlink /proc/<pid>/cwd` | `proc_pidinfo(PROC_PIDVNODEPATHINFO)` → `pvi_cdir` | PEB read, on demand only (see R-10) |
| Footprint | `/proc/<pid>/smaps_rollup` `Pss:`, fallback `statm` RSS | `proc_pid_rusage(RUSAGE_INFO_V4)` → `ri_phys_footprint` | `GetProcessMemoryInfo` → `PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize` |

**Rationale**: FR-024 and SC-007 require that Fallow never reads process environments. That
guarantee can't depend on how a general-purpose crate implements each field internally on each
OS. Owning the three sensitive or platform-specific reads keeps the guarantee auditable in about
300 lines per platform. sysinfo still provides cross-platform enumeration and CPU deltas, which
are tedious and well tested there. sysinfo's built-in `memory()` is RSS or working set on every
platform, so it can't provide the FR-019 metric anyway.

**Alternatives considered**: Use sysinfo for every field (rejected: no control over how the
command line is obtained on macOS, see R-2, and the memory metric is wrong). Drop sysinfo
completely (rejected: duplicates CPU accounting and enumeration on three OSes for no memory gain
worth having).

**Cost control**: The command line is read once per process (`OnlyIfNotSet` semantics,
implemented by Fallow's own cache keyed on PID plus start time). The footprint metric and working
directory are read only for members of detected LLM process groups, not for every process.

## R-2: macOS command line and the process environment (Principle VII, FR-024, SC-007)

**Finding**: macOS has no API that returns another process's argv without its environment.
`sysctl(KERN_PROCARGS2)` returns one buffer containing argc, the exec path, argv, **envp** and the
Apple strings, in that order. The caller has to find the argv/envp boundary itself. `ps` and every
other tool work this way. This means that reading a command line on macOS copies the target's
environment, values included, into Fallow's memory, even though Fallow never parses those values.

**Decision (approved 2026-10-13, option (a); constitution v3.0.0 Principle VII)**: Fallow uses its
own KERN_PROCARGS2 reader that:
1. Copies into a single reusable buffer owned by the reader. It allocates no strings for anything
   after argv.
2. Parses exactly `argc` strings after the exec path and ignores the rest.
3. Overwrites the entire buffer with zeros (using volatile writes, via `zeroize`) before the
   function returns, including on error paths.
4. Is called once per process lifetime (R-1 caching).

SC-007 is then tested by dumping Fallow's memory after a detection cycle and searching for a
canary. On macOS, that search must also come up empty after zeroization.

**Constitution status**: Allowed by Principle VII as amended in v3.0.0. Environment values are
never parsed, stored, displayed or logged, the buffer is wiped before the call returns, and the
SC-007 canary test runs on macOS.

**Spike S-4 (still to run)**: Test whether a deliberately truncated buffer (sized to the end of
argv, as found by a first full read of a test process) makes the kernel return only the argv
portion. **If it does, the reader uses truncated reads**, sized from argc and the argv lengths, so
envp is never copied in the common case. The wipe stays in place for the full-read fallback. S-4
runs on the physical Mac as part of the macOS release blocker (plan.md, Release blockers).

**Alternatives considered**: Use the process name only on macOS (no command line), which weakens
signature detection for interpreter-hosted agents (`node …/claude`, `python -m aider`). Use
sysinfo's reader (rejected: same exposure, and Fallow can't zero it).

## R-3: Footprint metric details (FR-019, SC-012)

- **macOS**: `proc_pid_rusage(pid, RUSAGE_INFO_V4, &info)` → `ri_phys_footprint`. Activity
  Monitor's "Memory" column shows this value. It works without root for the same user. On Apple
  Silicon, Metal buffers in unified memory are charged to the owning task's footprint ledger, so a
  local model's weights loaded through Metal should be included. **VERIFY** in R-11.
- **Windows**: `GetProcessMemoryInfo` with `PROCESS_MEMORY_COUNTERS_EX2` (Windows 10 1809+)
  → `PrivateWorkingSetSize`. This needs only `PROCESS_QUERY_LIMITED_INFORMATION`. Task Manager's
  default "Memory (active private working set)" column uses the same quantity. Older Windows builds
  fall back to `WorkingSetSize`, and the UI labels it.
- **Linux**: `/proc/<pid>/smaps_rollup` (kernel 4.14+) → `Pss:` line. It is readable for
  same-user processes. Reading it walks the page tables, so it costs about 1 ms per GB mapped and
  is read only for LLM group members. If it can't be read, Fallow falls back to `/proc/<pid>/statm`
  resident pages × page size, and that process is marked "RSS".
- **Not covered**: Discrete-GPU VRAM (CUDA, ROCm, DirectML). The UI notes this per the spec
  assumption.

## R-4: Network connections per process (FR-022, FR-023, FR-012)

**Decision**: Write a thin platform layer and add no crate for it.

| Platform | Source | Notes |
|---|---|---|
| Linux | `/proc/net/tcp` and `tcp6` give state, local and remote address, and inode. Inodes map to PIDs by scanning `/proc/<pid>/fd` for `socket:[inode]` | Only inodes of interesting sockets (established to a provider on port 443, or to/from a local model port) are looked up. The scan stops once all are found, and the PID→inode map is cached. |
| macOS | `proc_pidinfo(PROC_PIDLISTFDS)` → `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` → `socket_fdinfo.psi.soi_proto.pri_tcp` (state, addresses) | Per process. Only the current user's processes, or all with elevation. |
| Windows | `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_ALL)` for AF_INET and AF_INET6 | One call returns the whole system table with owning PIDs. No elevation needed. |

**Rationale**: Fallow only needs TCP state, addresses and the owning PID. Crates like
`netstat2` cover this but add bindgen build dependencies and per-call `Vec<SocketInfo>`
allocations of every socket on the system, including UDP. The custom layer reuses one buffer per
refresh and filters while parsing, so memory stays flat. Each implementation is under 250 lines
and can be tested against `ss`, `lsof` and `netstat -ano` output in CI.

**Alternatives considered**: `netstat2` (rejected: allocations and build dependencies), the
`listeners` crate (listening sockets only), Linux `NETLINK_SOCK_DIAG` (more efficient than parsing
`/proc/net`, kept as a later optimization; the user named `/proc/net`).

**Rules implemented here**: A provider match needs `state == ESTABLISHED` and `remote_port == 443`
and a remote address in the provider address set (FR-022). For local model servers, Fallow finds
the server's listening ports, then established sockets whose remote end is
`127.0.0.1`/`::1`/a local interface on such a port. The other end of each is a client (FR-023),
and incoming established sockets on the server make it "serving" (FR-012).

## R-5: Provider hostname resolution (FR-022a–c, Principle VII)

**Decision**: One background thread named `dns`, started after the first frame. It calls
`std::net::ToSocketAddrs` (the system `getaddrinfo` or `GetAddrInfoW`) for each configured
provider hostname, one at a time. Results go into an in-memory cache, and the thread sleeps
until the next refresh. Lookups are forward only, with no reverse lookups (Fallow never calls
`getnameinfo`). A config setting `dns.enabled = false` stops the thread from starting. Built-in
address ranges are always loaded and used until results arrive or when lookups fail.

**Cache lifetime**: The system resolver APIs don't expose record TTLs. Fallow therefore
re-resolves every **15 minutes**, which meets the spec's "at most one hour" assumption. The
"TTL if shorter" part of that assumption is not met. Getting TTLs would need a DNS client library
that sends its own queries, which is more traffic and more code, against Principle VII's spirit.
The spec assumption has been updated to say this: hostnames are re-resolved every 15 minutes,
and record TTLs are not available from the system resolver.

**Alternatives considered**: `hickory-resolver` (rejected: pulls in tokio, its own DNS client
bypasses the system resolver, and it costs several MB). Reading the OS DNS cache (rejected:
unavailable or undocumented on macOS and Linux).

## R-6: Concurrency model

**Decision**: No async runtime. Threads:
1. **main**: input events (crossterm), state updates and rendering.
2. **procmon**: refresh loop every `refresh_interval` (2 s), sends an `Arc<Snapshot>` over a
   bounded channel.
3. **dns**: R-5. Optional.
4. **watch**: notify's own watcher thread. It forwards file events over a channel.
5. **search**: spawned per search on demand, ends when the search finishes or is cancelled.
6. **jobs**: one short-lived thread per on-demand job (recent-files scan, Windows cwd read).

Channels use `std::sync::mpsc` (bounded `sync_channel`). The main loop blocks on
`crossterm::event::poll` with a timeout and drains the channels on every wake.

**Rationale**: About five threads at around 64 KB of committed stack each costs far less than a
tokio runtime (worker pool, I/O driver, timer wheel). None of the work here is I/O-concurrent at a
scale that needs async. Threads get explicit, small stack sizes (`Builder::stack_size(256 KiB)`)
except search.

## R-7: Pause and resume on Windows (FR-052)

**Decision**: `NtSuspendProcess` / `NtResumeProcess` from `ntdll`, with a
`PROCESS_SUSPEND_RESUME` handle. Process Explorer and Resource Monitor use the same mechanism.

**Documented differences from SIGSTOP/SIGCONT** (shown in the confirmation dialog and docs):
- Not a signal. The process gets no notice, and a parent shell doesn't report "Stopped".
- Suspension stacks. Each suspend increments a per-thread count. Fallow tracks its own suspends
  and resumes the same number of times. A process paused by another tool may need more resumes.
- It is racy. Threads created during suspension may run, and threads created with
  `THREAD_CREATE_FLAGS_BYPASS_PROCESS_FREEZE` are skipped.
- Windows with a GUI turn "Not responding". Console processes stop printing.
- Child processes are not suspended (on Unix, SIGSTOP doesn't stop children either). Pausing a
  group means suspending each member, and the dialog says so.
- The API is undocumented but has been stable since Windows XP.

**Alternatives considered**: Looping `SuspendThread` over a thread snapshot (more racy, same
result). `DebugActiveProcess` (attaches a debugger, which is intrusive and EDR-visible).
`NtCreateProcessStateChange` (Windows 11+ only; it auto-resumes when Fallow exits, a nicer safety
property but different semantics and not available on Windows 10). Job-object freeze (needs the
process to be placed in a job, which modifies it).

## R-8: Editor components

- **Buffers**: `ropey`. Memory is about the file size plus around 10% node overhead, edits are
  O(log n), and it scales to the 10 MB read-only threshold.
- **Highlighting**: `tree-sitter` plus `tree-sitter-highlight`, with the grammar crates for the 15
  languages in the spec assumption linked statically. "Lazy" means the grammar's parse tables
  (in the binary's read-only data) are only paged in, and its highlight query is only compiled,
  when the first file of that language opens. Unused grammars cost binary size (estimated
  +20–30 MB on disk) but no resident memory. Highlighting runs only for the visible range plus a
  margin, and the parse tree is kept per open buffer.
- **File watching**: `notify` with the platform backend. Fallow watches the open files and the
  directories expanded in the tree, non-recursively, plus the selected process's working directory
  while it is shown. It never watches whole project trees recursively.
- **Search**: `grep-searcher` + `grep-regex` + `ignore` (ripgrep's library crates). Search runs on
  demand on a search thread, respects `.gitignore`, streams results to the UI, can be cancelled,
  and holds at most 10,000 results in memory.
- **Recent files (FR-017)**: an on-demand `ignore::Walk` of the working directory with mtime
  within 30 minutes. It is capped at 50,000 entries visited and 200 results, and the UI says when
  a cap was hit.
- **Undo**: per buffer, an edit log capped at 10 MB per buffer. The oldest entries are dropped
  first.

## R-9: Configuration

- **Format**: TOML through `toml` + `serde`. Three logical files, each with built-in defaults
  embedded in the binary through `include_str!` and an optional user file of the same name in
  the platform config dir (`dirs::config_dir()/fallow/`): `settings.toml`, `signatures.toml`,
  `providers.toml`.
- **Merging**: user entries override built-in entries with the same `id`, and `enabled = false`
  disables a built-in entry. A parse error in a user file keeps the defaults and shows the file,
  line and message (spec edge case).
- **Reload**: by command (palette "Reload configuration"), not by watching.
- Schemas are in [contracts/config.md](./contracts/config.md).

## R-10 (user research item 1): Windows working directory and antivirus/EDR

**Finding**: Windows has no API for another process's current directory. It has to be read from
the target's PEB: `NtQueryInformationProcess(ProcessBasicInformation)` → `PebBaseAddress` →
`ReadProcessMemory` of `ProcessParameters` → `CurrentDirectory.DosPath`. This needs
`PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ`. Process Explorer, System Informer and
sysinfo all do this. Searching found no vendor documentation saying that reading another
process's PEB *read-only* raises an alert. Public EDR detection logic centers on
`WriteProcessMemory`, `VirtualAllocEx` and handles opened to LSASS or other protected processes.
Sysmon (event ID 10, ProcessAccess) and similar telemetry *do* record handles opened with
`PROCESS_VM_READ`, and some products score many such accesses from an unsigned binary as
suspicious. Fallow can't rule out alerts across all products from desk research.

**Decision**:
- Read the Windows cwd **only on demand**: when a process is selected, when the user jumps to it,
  or once when a process is first detected as an LLM process. Never in the 2-second loop, and
  never for processes that are not detected LLM processes.
- Never open `PROCESS_VM_READ` handles to protected or system processes, to LSASS, or to
  processes of other users in default mode.
- Read exactly the `CurrentDirectory` `UNICODE_STRING` and its buffer. Never read the
  `Environment` pointer's target.
- Config `windows.read_working_directory = "on_detect" | "on_select" | "never"` (default
  `on_detect`).
- **Fallback** when the setting is `never` or the read fails (access denied, or blocked by
  security software): the working directory shows "unavailable". "Go to working directory" offers
  the executable's directory and a typed path instead, and the UI explains why.
**Defender on GitHub-hosted runners (verified 2026-10-13)**: Defender is **not** active by default.
The `windows-2025` Packer template (`images/windows/templates/build.windows-2025.pkr.hcl`) runs
`images/windows/scripts/build/Configure-WindowsDefender.ps1` as its first provisioning script.
That script sets, among other things:
- `DisableRealtimeMonitoring`, `DisableBehaviorMonitoring`, `DisableIOAVProtection`,
  `DisableScriptScanning` and `DisableIntrusionPreventionSystem` to `$true`;
- `MAPSReporting = 0` and `SubmitSamplesConsent = 2`;
- `ExclusionPath = C:\, D:\`, so the whole disk is excluded and the checkout on `D:\` would never
  be scanned;
- `ForceDefenderPassiveMode = 1` when the ATP policy key exists.

A check on the stock image would pass trivially, so it would prove nothing.

**CI procedure (`xtask defender-check`, Windows job only)**:
1. **Enable**: `Set-MpPreference -DisableRealtimeMonitoring $false -DisableBehaviorMonitoring
   $false -DisableIOAVProtection $false -DisableScriptScanning $false`, then
   `Remove-MpPreference -ExclusionPath C:\, D:\`. Then try `Update-MpSignature`; if it fails, log
   a warning and continue on the image's signatures.
2. **Verify it is really on**: `Get-MpComputerStatus` must report `AMRunningMode = "Normal"` (not
   `Passive Mode` or `EDR Block Mode`), `RealTimeProtectionEnabled`, `BehaviorMonitorEnabled`
   and `IoavProtectionEnabled` all `True`, and `Get-MpPreference` must show no exclusion that
   covers the Fallow binary or the workspace. **If any check fails, the job fails.** A run with
   protection off never counts as a pass, including when the Defender service is missing or
   `Set-MpPreference` throws.
3. **Exercise**: start the LLM-process fixtures, then run `fallow --snapshot-json` twice with
   `windows.read_working_directory = "on_detect"` and once with `on_select` while a selection is
   simulated, so the PEB cwd read runs against each fixture.
4. **Judge**: fail on any `Get-MpThreatDetection` entry, or any Defender operational-log event
   (`Microsoft-Windows-Windows Defender/Operational`, IDs 1006, 1007, 1015, 1116, 1117), created
   after step 2.

Other EDR products can't be tested in CI. The release notes will state this as a known
limitation.

**Status**: Resolved as a design. Defender is verified in CI with real-time protection
explicitly enabled and checked. Coverage of other EDRs remains a documented risk.

## R-11 (user research item 2): macOS footprint vs Activity Monitor with a running local model

**Decision (2026-10-13)**: S-2 is **no longer a planning gate**. It becomes a task in the macOS
platform module, with SC-012 as its acceptance test. It runs on a physical Apple Silicon Mac
(M1 or later) before the macOS release. Until then, macOS ships as "beta" (plan.md, Release
blockers).

**GitHub-hosted macOS runners are not suitable for S-2.** They are virtual machines without Metal
GPU acceleration, so a local model can't run Metal-backed inference there. They also can't
reproduce how unified memory is charged on real hardware, and Activity Monitor (a GUI app) isn't
usable in a headless runner. CI still runs the macOS unit tests, the benchmarks and the SC-007
canary test, but not SC-012.

**Method** (run on the physical Mac):
1. Run Ollama (Metal) and load a model: 3B (`ollama run llama3.2:3b`) on a Mac with 8 GB, 8B
   (`ollama run llama3.1:8b`) otherwise. Keep it busy with a repeated prompt.
2. Every 5 s for 2 minutes, record (a) `ri_phys_footprint` from a 30-line Rust probe using the
   R-3 call, and (b) Activity Monitor's Memory value via `top -l 1 -stats pid,mem -pid <pid>`.
   `top`'s MEM column is phys_footprint. Also record a screenshot of Activity Monitor at 3
   points.
3. Pass: every pair is within 10%. Results are recorded in `research/s2-macos-footprint.md`.

**Expectation**: Pass, since both read the same kernel ledger. The risk is in how Ollama's
runner subprocess is attributed. The model may be loaded by `ollama runner`, a child of
`ollama serve`. Fallow groups children, so the comparison has to be per PID *and* per group.

**Status**: **NOT YET VERIFIED.** This is a macOS release blocker, not a planning gate. Results are
to be recorded here.

## R-12 (user research item 3): Is 50 MB idle realistic?

**Desk estimate** (idle: process panel live, no project open, 80×24 to 250×70 terminal):

| Component | Estimated RSS |
|---|---|
| Binary pages touched at idle (code + rodata; grammars untouched) | 3–6 MB |
| Rust std, allocator, 5 threads × small stacks | 1–2 MB |
| ratatui: 2 cell buffers at 250×70 (~17.5k cells × ~40 B × 2) | ~1.5 MB |
| sysinfo `System` with ~600 processes (name, user, CPU fields only) | 1–3 MB |
| Fallow command-line cache (~600 entries, ~200 B avg) | ~0.2 MB |
| Connection buffers (Windows full TCP table, Linux `/proc/net` read buffer) | 0.5–2 MB |
| Config, signatures, provider sets, DNS cache | < 0.5 MB |
| **Total** | **~8–16 MB** |

With a large project open, the extra cost is tree expansion, a few open buffers (ropey plus
tree-sitter trees, about 3–5× file size for trees), the grammar pages touched and search results.
That is estimated at 20–60 MB, well inside 150 MB.

**Planned verification**: Spike S-3, a ~300-line prototype (`spikes/idle-budget/`, not shipped)
with the real crate set: ratatui+crossterm drawing a static panel, sysinfo with the R-1 refresh
kind on a 2 s loop, the platform connection reader, and one tree-sitter grammar linked but unused.
Measure peak RSS over 5 minutes on all three platforms with the same harness as the CI benchmark.
Pass: at most 25 MB, leaving half the budget for the full product.

**Who runs it**:
- **Windows**: the user, locally, after installing the Rust toolchain. Run
  `cargo run --release` in `spikes/idle-budget/`, then the measurement harness against it.
- **Linux and macOS**: a CI workflow (`.github/workflows/spike-idle-budget.yml`) on
  `ubuntu-24.04` and `macos-15`. It runs on pushes that touch the spike or the workflow, and by
  manual `workflow_dispatch`. It builds the spike and measures with the same harness, and
  uploads the RSS series as an artifact. GitHub-hosted runners are fine here: the measurement is
  Fallow's own RSS and needs no GPU.

Pass: peak RSS ≤ 25 MB over 5 minutes on each platform. Results are recorded here.

**Implementation**: `spikes/idle-budget/` is its own Cargo workspace with two crates.
- `idle-spike`: a ratatui 0.30/crossterm UI with mouse capture, bracketed paste, and kitty flags
  on Unix. sysinfo 0.39 is limited to CPU and user. It uses the Fallow-owned command-line,
  footprint and TCP readers from R-1 to R-4, a 2 s procmon thread, a DNS thread, and
  tree-sitter-rust linked but unused.
- `idle-measure`: the harness. It runs a 120×40 PTY, reads OS RSS every 1 s for 300 s, and
  exits 1 above the limit.

**Results**

Recorded run: [38414881466](https://github.com/Cem-Kalender/fallow/actions/runs/38414881466),
commit `0259825`, 2026-10-13. rustc 1.99.0, release profile (thin LTO, `panic = "abort"`,
stripped), 301 one-second samples, 120×40 PTY.

| Platform | Peak RSS | Peak after 30 s | Final | Result | RSS source |
|---|---|---|---|---|---|
| Linux (`ubuntu-24.04`) | **5.71 MB** (at 29 s) | 5.71 MB | 5.71 MB | **PASS** | `/proc/<pid>/status` VmRSS |
| macOS (`macos-15`) | **7.97 MB** (at 72 s) | 7.97 MB | 7.97 MB | **PASS** | `proc_pidinfo` `pti_resident_size` |
| Windows (`windows-2025`) | **11.51 MB** (at 1 s) | 11.51 MB | 11.50 MB | **PASS** | `WorkingSetSize`, which includes shared DLL pages; that partly explains why it's higher than the others |

**Run validity evidence (from the CI logs of run 38414881466)**

| Check | Linux | macOS | Windows |
|---|---|---|---|
| Measurement lasted the full 300 s: harness wall clock | 300.0 s | 300.1 s | 300.0 s |
| Same, from the GitHub step timestamps | 14:37:37→14:42:37 (300 s) | 14:37:58→14:42:58 (300 s) | 14:38:51→14:43:52 (301 s) |
| Harness PID / measured PID | 3762 / 3763 | 3134 / 3135 | 4172 / 3404 |
| PID written by the spike itself (`--stats`) | 3763 ✔ matches | 3135 ✔ matches | 3404 ✔ matches |
| Executable of the measured PID, read at the end of sampling | `…/target/release/idle-spike` ✔ | `…/target/release/idle-spike` ✔ | `…\target\release\idle-spike.exe` ✔ |
| Procmon refresh cycles (expected about 150 at 2 s; at least 135 required) | 148 | 145 | 150 |
| Processes seen per cycle: min / max / last | 245 / 261 / 248 | 441 / 516 / 491 | 140 / 154 / 146 |
| Processes of the runner's own user (max) | 54 | 263 | 38 |
| Command lines cached (last) | 53 | 261 | 31 |
| Interesting TCP connections (max) | 9 | 10 | 13 |
| LLM processes detected (max) | 0 | 0 | 0 |
| First frame drawn / output after 30 s | yes / 4,937 bytes | yes / 6,640 bytes | yes / 1,239 bytes |
| Spike exited cleanly on `q` and wrote its stats | yes | yes | yes |

The harness now fails a run as INVALID (exit 2) unless all of the following hold:
- the spike-written PID equals the measured PID;
- the measured executable is `idle-spike`;
- the full duration elapsed;
- at least 90% of the expected refresh cycles ran;
- the first frame was drawn and output continued after warm-up.

**Consistency across runs**: The previous valid run,
[38411827336](https://github.com/Cem-Kalender/fallow/actions/runs/38411827336) (commit
`83febda`, before the evidence checks existed), measured 5.60, 7.59 and 11.46 MB, within 0.4 MB
of the recorded run. In run
[38413742547](https://github.com/Cem-Kalender/fallow/actions/runs/38413742547) the executable
check fired too early on Linux and macOS. It read `/proc/<pid>/exe` and `proc_pidpath` right
after `fork`, before `exec`, and saw the harness binary. The spike-written PID still matched the
measured PID on all three platforms, so the measurement was correct and only the check was
early. The check now runs at the end of sampling.

**These figures are a lower bound.** CI runners are quiet machines:
- 140–516 processes in total and 38–263 for the runner's own user;
- about 10 relevant TCP connections;
- **no LLM processes** at all, so the footprint reads, LLM grouping and per-LLM snapshot rows
  were never exercised.

A real developer machine with several agents, a local model server, language servers and a
browser will have more processes, more connections and non-empty LLM groups. Fallow's idle RSS
will be higher there. The highest CI peak, 11.51 MB, is under half the 25 MB spike limit and
under a quarter of the 50 MB constitution budget, at the low end of the desk estimate
(8–16 MB). That leaves room, but it doesn't prove the budget holds under real load. **Task
carried into tasks.md**: re-measure on a real developer machine once detection is implemented
(plan.md, "Carried into tasks").

**Invalid earlier result (kept for the record)**: Run
[38409989505](https://github.com/Cem-Kalender/fallow/actions/runs/38409989505) reported Windows
at a flat 2.28 MB, with only two distinct values in 301 samples. The cause was ConPTY: at startup
it sends a cursor-position request (`ESC[6n`) to the host terminal and holds the child until it
gets an answer. The harness didn't answer, so the spike was stalled before its first frame. The
harness now answers these requests, and runs without proof of liveness are marked INVALID (see
above). That run's Linux (5.61 MB) and macOS (7.58 MB) figures match the valid runs. **Lessons
for the CI benchmark (R-13)**:
- The harness must answer terminal queries.
- It must prove that the measured PID is the product, after `exec`.
- It must prove that the binary was live: first frame, continued output, and refresh cycles.

A low memory figure alone isn't evidence.

**Context: why Windows is measured in CI.** The local run planned for 2026-10-13 was blocked. The
development machine's Microsoft Defender Attack Surface Reduction policy, rule
`01443614-CD74-433A-B99E-2ECDC07BFC25` ("Block executable files from running unless they meet a
prevalence, age, or trusted list criterion"), blocks every freshly compiled executable, cargo
build scripts included. No Rust code can be built on that machine. The Windows figure therefore
comes from the `windows-2025` CI runner, and development moves elsewhere (plan.md, Development
environment).

**Product implication (new finding)**: The same ASR rule is common in managed enterprise
fleets. It will block an unsigned, low-prevalence Fallow binary for users on such machines. The
release plan needs code signing for Windows and macOS; tracked as a follow-up for tasks.

**Status**: **VERIFIED 2026-10-13: PASS on all three platforms** (peaks of 5.71, 7.97 and 11.51
MB, run 38414881466). Duration, the measured PID and procmon activity are confirmed from the CI
logs. The planning gate is closed. The figures are a CI lower bound; re-measuring on a real
developer machine is a task.

## R-13: Benchmarks in CI (Principle VI, FR-053)

- **Harness**: `xtask` binary (workspace member, dev-only). It launches the release binary inside
  a pseudo-terminal (`portable-pty`, a dev dependency only) at a fixed 120×40 size.
- **Startup**: time from spawn to the first byte sequence the harness recognizes as the first
  complete frame (the key-hint bar's text, see contracts/cli.md). 30 runs after 3 warm-ups. The
  build fails if **p95 ≥ 100 ms**.
- **Idle memory**: start, wait 30 s (15 refresh cycles), sample RSS every second for 60 s. Fails if
  the max is 50 MB or more. RSS is read from the OS (Linux `/proc/<pid>/status` VmRSS, macOS
  `proc_pidinfo` task info, Windows `WorkingSetSize`), as the constitution requires RSS.
- **Loaded memory**: same, with the reference large project fixture open, a search run and five
  files from different languages opened by scripted keys. Fails at 150 MB or more.
- **Fixture** (resolves constitution TODO(LARGE_PROJECT_FIXTURE)): a pinned commit of a public
  polyglot repository, about 50,000 files and 1 GB, fetched and cached in CI by hash. The exact
  repository is chosen during tasks. Candidates are `rust-lang/rust` at a tag, or a synthetic tree
  generated by `xtask gen-fixture` with fixed seeds. The synthetic tree is the recommended choice
  for reproducibility and license clarity.
- **Runners** (resolves TODO(REFERENCE_HARDWARE)): GitHub-hosted `ubuntu-24.04`, `macos-15` (arm64)
  and `windows-2025` standard runners.
- **Noise**: shared runners vary. Startup uses p95, not the max, and fails only after one
  automatic re-run of the benchmark job.
