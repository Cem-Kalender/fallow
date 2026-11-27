# 017 checks

## T001: the agent list with 20 agents, before 017 changes it (research R-7, "To verify first")

**How it was seen** (2026-11-27): the 013 demo (`tests/visual_013.rs`, served on the fake
runtime from a scratch root) with 20 valid checker agents `agent-01` … `agent-20` written to its
`config/agents/` beforehand (as `FALLOW_VISUAL_AGENTS=20` now does for
`tests/visual_fixture.rs`), 27 agents in all with the 7 shipped ones; the Agents page (`#/agents`)
opened in **headless Chrome** through DevTools at 1440 × 900 and at 390 × 844, the list
(`.ag-side`) scrolled to its end by script. Not in Fallow's own window: the builder was not to
start Fallow.app; the window's web view lays the page out with the same CSS, so the numbers
below are expected to hold there, but that is not verified.

**1440 × 900 (over 900 px)**: the list does scroll on its own. `.ag-side` is
`position: sticky; overflow-y: auto`, 818 px high (`max-height: calc(100vh - var(--top-h) -
24px)` with `--top-h: 58px`) holding 3060 px of agents; scrolling it to its end left the page at
`scrollY` 0. No ancestor clips or scrolls, so `position: sticky` is not broken. **But** the list
starts 135 px below the window's top (the top bar and the page's heading), not 58 + 12 px, so
its bottom is at 949 px, 49 px below the 900 px window, and the page itself is 977 px high: with
the list scrolled to its end, the last agent (bottom at 949 px) is out of view until the page is
scrolled by about 77 px. Cause: the height subtracts only the top bar (`--top-h`), not the
heading above the list; the sticky `top: 12px` only takes over once the page scrolls.

**390 × 844 (below 900 px)**: the list does not scroll on its own. `.ag-side` is `static`,
`overflow-y: visible`, no `max-height`: it is 2649 px high, in the flow of the page, and the
editor starts at 2925 px, more than three screens down (the engineer's complaint).

**For T040**: below 900 px, a `max-height` and `overflow-y: auto` (research R-7); over 900 px,
a height that keeps the whole list inside the window when the page is not scrolled (for
example measured from the list's own top, or the page's heading counted in), so the last agent
can be selected without scrolling the page (SC-006).

## T038-T040: after the fix (2026-11-27)

**What changed** (`src/service/web/app.css`, `src/service/web/screens/agents.js`):
- over 900 px the list keeps `position: sticky` and its own scroll, and its `max-height` now
  counts from its own top: `calc(100vh - var(--ag-top) - 12px)`, where `--ag-top` is the
  layout's top in the page, measured by `agents.js` at each render and on resize (fallback
  140 px). The 135 px above the list (top bar, page padding, project row) are now subtracted,
  not only `--top-h`;
- below 900 px `.ag-side` gets `max-height: 40vh; overflow-y: auto; overscroll-behavior:
  contain` (research R-7);
- a focused agent (Tab, or the page focusing the one chosen) is scrolled into view within the
  list only, never the page (`focus({preventScroll: true})` and the list's own `scrollTop`).

**How it was seen**: `tests/visual_017.rs` (20 agents `agent-01` … `agent-20`, the 7 shipped
ones and one file Fallow cannot use, `old-checker.md` with `model: haiku`; one project) served
from a scratch root, and `node specs/017-agent-file-fields/checks/agents.js <root>` driving
**headless Chrome** through DevTools, with real wheel, click and Tab input. As for T001, not in
Fallow's own window (the builder does not start Fallow.app); the window's web view lays the page
out with the same CSS.

**Before the fix** (the same script against the unchanged page): 1440 × 900 failed "the whole
list is inside the window" (list 135-953 px of 900) and the Tab check (3 focused agents below
the window); 390 × 844 failed 10 checks: the list did not scroll (0 px to scroll, 2766 px
high), the editor's text started at 3236 px, and the wheel scrolled the page (scrollY 2904).

**After the fix**: every check passes at both sizes.

| | 1440 × 900 | 390 × 844 |
|---|---|---|
| list on screen, page unscrolled | 135-888 px (753 px high, 2564 px to scroll) | 256-594 px (338 px = 40 % of 844, 2436 px to scroll) |
| wheel to the list's end | page `scrollY` 0 | page `scrollY` 0 |
| editor's Save button | 154-194 px | 669-709 px |
| last agent (`security-reviewer`) and `agent-20` | in the window, selected without page scroll, list position kept | the same |
| `#ag-text` starts at | (beside the list) | 840 px of 844: within the first screen, with little to spare |
| Tab through 27 items | each visible within the list | the same |
| `select` elements | `ag-project`, `ag-scope` | the same |
| `old-checker.md` under "Files Fallow could not use" | opens with "line 5: model \"haiku\" …" and "line 8: may_run is required …", the overview's address a link, Save for "all my projects" | the same |

Below 900 px the editor's text box starts only a few pixels inside the first screen, because the
editor's heading (name, Save for, Save, description, "Used in", the file name) wraps on a narrow
window; its Save button is well inside it.

## T044: the quickstart before 0.4.0 (2026-11-27)

- **Q1, Q2, Q4 (automated)**: `cargo test --lib` (214 passed, the `defs::agent` and `claude.rs`
  unit tests among them) and `cargo test --features automation --test us6_defs --test
  us7_runtime --test agent_claude --test us8_agent_fields --test us17_existing --test us9_leads`
  pass, with a scratch `HOME`.
- **Q3 (automated part)**: `us8_agent_fields` passes. Its by-hand steps, and Q4's, need
  Fallow's window and were not done (the builder does not start Fallow.app).
- **Q5**: recorded above (T038-T040), every check passing at both sizes; no page code changed
  since. A rerun from the builder's sandboxed shell hung at Chrome's first page, before any check.
- **Q6**: `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --all-features
  -- -D warnings` pass; the full `cargo test --workspace --features automation` and the budget
  benchmarks (`cargo xtask bench`, which start Fallow's window) are left to CI.
