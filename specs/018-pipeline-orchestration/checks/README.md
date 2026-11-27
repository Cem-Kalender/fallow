# 018 checks

## T056: the Pipelines tab at 1440 × 900 and 390 × 844 (2026-11-27)

**How it was seen**: `tests/visual_018.rs` served from a scratch root
(`FALLOW_VISUAL_ROOT=<a short scratch folder>`): the shipped Standard, Test first, Solo and
`parallel`, two checkers of its own (`ui-checker`, `perf-checker`) and `every-point`, a pipeline
with a step at every point of FR-004 (checkers alongside each other after each task, at each
phase's end and at each checkpoint's end; `builders = 3`), every pipeline loading without a
problem; then `node specs/018-pipeline-orchestration/checks/pipelines.js <root>` driving
**headless Chrome** through DevTools, with real Tab input, on `#/pipelines/_/<name>` for
Standard, Solo, `parallel` and `every-point`. Not in Fallow's own window: the builder does not
start Fallow.app; the window's web view lays the page out with the same CSS.

**Result (2026-11-27, after the designer fix)**: all 112 checks pass, at both sizes. The check
now also asserts that each point's + is labelled with exactly that point and opens the add form
preset to it (16 checks more than the first run's 96).

| | 1440 × 900 | 390 × 844 |
|---|---|---|
| "verifying" / "not verifying" text anywhere on the page; a pill about verifying (FR-001) | none, all four | none, all four |
| units of checkers alongside each other | `parallel`: 1 of 2; `every-point`: 3 of 2; Standard, Solo: none | the same |
| each unit drawn as branches: one column, each member its own edge in from the junction and out to the next | yes, every unit | yes |
| **a + at every point** (FR-028): once at the start, each phase's start, before and after each task, each phase's end, each checkpoint's start and end, once at the end | **yes**, 8 +s for each of the four | **yes** |
| each point's + labelled with exactly that point ("Add a step here (at each story's checkpoint, at the start)") | yes | yes |
| each point's + opens the add form preset to that point (Runs and At) | yes | yes |
| every + reachable by Tab (FR-029a) | 8 of 8, all four | the same |
| each focused + drawn as focused (`:focus-visible`, stroke 2.5 px in the accent) | yes | yes |
| each focused + in view in the window and the graph's box | 100 % of each | 100 % of each |
| from a +, the add form's Runs and At reach every point | yes, all four | yes |
| the page scrolls sideways | no (0 px) | no (0 px) |

**What changed** (research R-13, amended): a point with no step is now a narrow slot column
holding its own +, inside its groups, the edge running through it; a point with steps has its +
on the junction before its first column; each + says exactly its point. Only the designer page
(`src/service/web/screens/pipelines.js`) changed; the loop page's graph, the pipeline format and
the TOML the designer writes are as before (`us18_designer`, `us18_existing` pass unchanged).

**The first run (before the fix)**: 88 checks passed, 8 failed, the same at both sizes, one
finding, "a + at every point", for all four pipelines: the graph put one + on each junction
between two columns, so a point with no step had no + of its own (Standard and `parallel`: once
at the start, a phase's end, a checkpoint's start; Solo also once at the end; `every-point`: a
checkpoint's start, the + before its checkpoint-start step being labelled "at each story's
checkpoint, at the end").

## T057: the quickstart (2026-11-27)

Run from the builder's shell with a scratch `HOME`, on the fake runtime:

- **1 (the whole suite)**: not run here; left to CI (T044).
- **2**: `us18_designer` 3 passed; by eye through `visual_018` and `checks/pipelines.js`, above.
- **3**: `us18_points` 8 passed (three runs in a row on their own). One earlier run, while the
  headless-Chrome check and seven other test binaries ran at the same time, failed
  `checkers_alongside_each_other_at_a_phase_end_overlap_and_are_judged_together` at its
  "they overlap in time" assertion (line 269): a timing assertion under load. Fixed: each
  checker now takes 8 s (was 2 s) and the overlap is measured from the steps' own
  `step_started` / `step_finished` in the loop's log, as `us18_builders`' SC-002 test is;
  `us18_points` then passed 5 runs in a row.
- **4**: `us18_builders` 17 passed.
- **5**: `us18_lock` 11 passed; `cargo test --lib` 243 passed (`run::groups` among them).
- **6**: `us18_existing` 4 passed (the two new tests among them); `defs::pipeline` in the lib run.
- Also `us17_existing` 2, `web_agents` 2, `run_events` 7 passed.
- **7 (budgets)**: the `bench` job starts Fallow's window and is left to CI. Its budgets are
  unchanged: `git diff v0.4.0..HEAD -- .github xtask` is empty, so `.github/workflows/ci.yml`
  and `xtask/src/bench.rs` are as at 0.4.0.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` pass (clippy's usual note that `clippy.toml`'s MSRV differs from `Cargo.toml`'s).
