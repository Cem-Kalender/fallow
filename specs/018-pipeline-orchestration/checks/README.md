# 018 checks

## T056: the Pipelines tab at 1440 × 900 and 390 × 844 (2026-11-27)

**How it was seen**: `tests/visual_018.rs` served from a scratch root
(`FALLOW_VISUAL_ROOT=/private/tmp/claude-502/v18`): the shipped Standard, Test first, Solo and
`parallel`, two checkers of its own (`ui-checker`, `perf-checker`) and `every-point`, a pipeline
with a step at every point of FR-004 (checkers alongside each other after each task, at each
phase's end and at each checkpoint's end; `builders = 3`), every pipeline loading without a
problem; then `node specs/018-pipeline-orchestration/checks/pipelines.js <root>` driving
**headless Chrome** through DevTools, with real Tab input, on `#/pipelines/_/<name>` for
Standard, Solo, `parallel` and `every-point`. Not in Fallow's own window: the builder does not
start Fallow.app; the window's web view lays the page out with the same CSS.

**Result**: 88 checks pass, 8 fail, the same at both sizes. The 8 are one finding, "a + at every
point", for all four pipelines.

| | 1440 × 900 | 390 × 844 |
|---|---|---|
| "verifying" / "not verifying" text anywhere on the page; a pill about verifying (FR-001) | none, all four | none, all four |
| units of checkers alongside each other | `parallel`: 1 of 2; `every-point`: 3 of 2; Standard, Solo: none | the same |
| each unit drawn as branches: one column, each member its own edge in from the junction and out to the next | yes, every unit | yes |
| every + reachable by Tab (FR-029a) | 5, 4, 5, 10 of 5, 4, 5, 10 | the same |
| each focused + drawn as focused (`:focus-visible`, stroke 2.5 px in the accent) | yes | yes |
| each focused + in view in the window and the graph's box | 100 % of each | at least 81 % (Solo's first +, at the edge of the graph's scrolling box once scrolled to it); the others 100 % |
| every + labelled ("Add a step here (<point>)") | yes | yes |
| from a +, the add form's Runs and At reach every point, the checkpoint's start too | yes, all four | yes |
| the page scrolls sideways | no (0 px) | no (0 px) |
| **a + at every point** (FR-028) | **no** (below) | **no** |

**The finding: a + only where a column meets another, not at every point.** The graph puts one +
on each junction between two columns (research R-13), with the point of the column after it as
the + 's preset; a point with no step of its own has no column, so it shares the junction with
its neighbours and gets no + of its own. Seen:

- Standard and `parallel`: +s for a phase's start, before and after each task, a checkpoint's
  end and once at the end; **none for once at the start, a phase's end, a checkpoint's start**;
- Solo: as Standard, and **none for once at the end** either;
- `every-point` (a step at every point): a + for every point **but a checkpoint's start**; the +
  before its checkpoint-start step is labelled "at each story's checkpoint, at the end", as is
  the one after it (`presetOf` gives every + before a checkpoint column the checkpoint's end).

Every point can still be reached: any + opens the add form, whose "Runs" and "At" offer all
eight points, and Fallow places the step by when it runs. Whether that meets FR-028's "a + at
each point" is for the spec's owner to decide; this polish phase changes no page behaviour, so
the check is left strict and failing, and the finding is reported, not fixed.

## T057: the quickstart (2026-11-27)

Run from the builder's shell with a scratch `HOME`, on the fake runtime:

- **1 (the whole suite)**: not run here; left to CI (T044).
- **2**: `us18_designer` 3 passed; by eye through `visual_018` and `checks/pipelines.js`, above.
- **3**: `us18_points` 8 passed (three runs in a row on their own). One earlier run, while the
  headless-Chrome check and seven other test binaries ran at the same time, failed
  `checkers_alongside_each_other_at_a_phase_end_overlap_and_are_judged_together` at its
  "they overlap in time" assertion (line 269): a timing assertion under load, not reproduced.
- **4**: `us18_builders` 17 passed.
- **5**: `us18_lock` 11 passed; `cargo test --lib` 243 passed (`run::groups` among them).
- **6**: `us18_existing` 4 passed (the two new tests among them); `defs::pipeline` in the lib run.
- Also `us17_existing` 2, `web_agents` 2, `run_events` 7 passed.
- **7 (budgets)**: the `bench` job starts Fallow's window and is left to CI. Its budgets are
  unchanged: `git diff v0.4.0..HEAD -- .github xtask` is empty, so `.github/workflows/ci.yml`
  and `xtask/src/bench.rs` are as at 0.4.0.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` pass (clippy's usual note that `clippy.toml`'s MSRV differs from `Cargo.toml`'s).
