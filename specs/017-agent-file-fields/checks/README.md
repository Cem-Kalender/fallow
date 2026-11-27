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
