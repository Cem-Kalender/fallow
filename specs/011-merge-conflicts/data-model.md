# Data model: Merge conflicts between loops

All new fields are optional with defaults, so logs and `projects.json` from before 011 load
unchanged.

## Loop events (the run log, `src/run/events.rs`)

| Event | Fields | Meaning |
|---|---|---|
| `Updated` | `n` (1, 2, …), `target`, `target_commit`, `conflicts: [path]`, `docs_changed: [path]`, `remote: bool`, `working: Option<commit>` | An update started (written before the merge is computed). With conflicts, `working` is the working commit under `refs/fallow/update/<run>-<n>` the repair starts on (R-2). With `docs_changed`, nothing is merged. |
| `UpdateMerged` | `n`, `commit` | Fallow's two-parent merge commit is on the loop's branch. Folded (R-3): every story's phases back to Pending with `reopened`, `outcome` cleared, `fresh_seq` set; `affected` untouched; `base_commit` = the target commit. |
| `UpdateDropped` | `n`, `why` | Dropped (pull request merged or closed, loop stopped, documents changed); its working ref deleted. |
| `PushedForward` | `from`, `to` | The loop's branch on the remote moved forward; `Publication.commit` = `to`. |
| `LeftPullRequest` | `unchecked: [commit]` | "Leave the pull request as it is" at the limit. |
| `Touched` | `files: [path]` | Files a verified step changed (computed on the step's thread). |
| `Overtaken` | `n`, `target_commit` | The target moved again while the loop held its turn (R-6). |
| `Lined` | `at` | Joined the merge line of its target. |
| `Unlined` | `why` | Left it. |
| `Overlap` | `with` (run id), `files: [path]` | Another unfinished loop touches the same files. |
| `WaitsFor` | `run` (run id), `from` (`lock` or `engineer`) | Does not start steps until that loop is merged, stopped or deleted. |
| `WaitEnded` | `run`, `why` | The wait is over. |
| `Merged` (existing) | + `unchecked: [commit]` | Target commits a "Merge without the latest check" was not verified with. |
| `Published` (existing) | + `unchecked: [commit]` | The same for "Publish without the latest check". |
| `LoopCreated` (existing) | + `update_limit` (default 3), + `run_after: Option<run id>` | The lock's settings. |
| `Steering` (existing) | + kinds `Update`, `MergeUnchecked`, `PublishUnchecked`, `LeavePullRequest`, `WaitFor`, `StopWaiting` | Recorded steering actions (II). |

## LoopState (`src/run/state.rs`)

| Field | Type | From |
|---|---|---|
| `update` | `Option<UpdateState { n, target_commit, conflicts, remote, working, started_seq }>` | `Updated` until the loop is ready (or, published, pushed forward) again, or `UpdateDropped` |
| `fresh_seq` | `u64` | `Relocked` and `UpdateMerged`; read by `repairs_of`, `allowed_more`, `fresh_since` instead of `relocked_seq` |
| `updates` | `u32` | count of `Updated` |
| `update_limit` | `u32` | `LoopCreated`, default 3 |
| `overtaken` | `u32` | `Overtaken`; reset by `Merged`, `Published`, `PushedForward`, `Steering::Update` |
| `base_commit` | `String` | the target commit last merged in; the lock commit before any update. The gate baseline is looked up by this commit. |
| `lined_at` | `Option<timestamp>` | `Lined` / `Unlined` |
| `touched` | `BTreeSet<path>` | files of verified steps |
| `task_paths` | `BTreeSet<path>` | paths named in the locked tasks |
| `overlaps` | `Vec<Overlap { with, files }>` | `Overlap` |
| `waits_for` | `Option<run id>` | `WaitsFor` / `WaitEnded` |
| `moved` | `Option<Moved { target_commit, commits, conflicts, from }>` (not logged: the last R-1 result) | the target watch |

## Engine (in memory, rebuilt on load)

- `lines: BTreeMap<(repo, target), Vec<(at, run id)>>` from `lined_at`, ordered by `at` then run id.
- `moved` results per loop from the target watch thread (R-1), with the time of the read.

## Lock record (`src/project/mod.rs`)

`LockRec` gains `update_limit` and `run_after` (both optional).

## Host (`src/publish/host/mod.rs`)

`PrState` gains `mergeable: Option<Mergeable { Mergeable, Conflicting, Unknown }>` and `behind: bool`;
the protection read gains `strict: Option<bool>` (requires up to date).

## Rules (state transitions)

- Ready → (target moved) → Free: `Updated`; Normal/Strict: `NeedsEngineer [bring up to date, stop]`.
- `Updated` (no conflict, no docs) → `UpdateMerged` → Building (stories reopened) → … → Ready.
- `Updated` (conflicts) → repair step on `update-<n>` from the working commit → verified (no markers) → Fallow's merge commit → `UpdateMerged` → as above.
- `Updated` (docs_changed) → `NeedsEngineer [lock again, stop]`; nothing merged.
- Ready + target moved again while holding the turn → `Overtaken`; `overtaken == update_limit` →
  `NeedsEngineer [verify again on the latest <target>, merge without the latest check (if no
  conflict), stop]`.
- Publishing + pull request conflicting/behind-and-required → as Ready, with the remote target; the
  stage stays Publishing throughout; `finish()` hands over to `push_forward` → `PushedForward`; limits
  and caps become `NeedsEngineer` without a stage change, never offering "stop".
