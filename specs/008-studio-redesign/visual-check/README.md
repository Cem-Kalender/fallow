# Visual check (T032; SC-002, SC-005, FR-018)

Each screen 008 changes, at 1440 px and 390 px, taken on 2026-11-01 from a debug build. They are
taken with headless Chrome through its DevTools protocol, at full page height (except the
`-fold` shot, which is the 1440 × 900 window). The loops run on the scripted fake agent, made by
`tests/visual_fixture.rs` (an ignored test: see its header for how to run it). There are two
loops: `shop` is ready to merge with every story verified, and `lab` waits for the engineer
after one of US1's criteria failed.

| Screen | Mockup | 1440 px | 390 px |
|---|---|---|---|
| Loop, waiting for you after a failure | `mockup/loop-graph.html` (the live graph, T043), `mockup/loop-1440.png`, `loop-390.png` | `loop-asks-1440.png`, `loop-asks-1440-fold.png` | `loop-asks-390.png` |
| Loop, ready to merge | (same) | `loop-ready-1440.png`, `loop-ready-verified-1440.png` (What was verified open) | `loop-ready-390.png` |
| Pipelines, a checker selected | `mockup/pipelines-1440.png`, `pipelines-390.png` | `pipelines-1440.png` | `pipelines-390.png` |
| Agents | `mockup/agents-1440.png`, `agents-390.png` | `agents-1440.png` | `agents-390.png` |

## Checks

- **No sideways scrolling at 390 px.** On every screen the page is exactly as wide as the window
  (390 px). The pipeline graph scrolls inside its own box, and scrolls to the selected node.
- **Steering without scrolling (SC-002).** In `loop-asks-1440-fold.png` (1440 × 900), Note for
  next step, Limits, Pause and Stop are in the header, above the fold.
- **A failure shows itself (FR-003).** US1's failed criterion opens its panel by itself, with
  what was run, what came back and what was expected. The question banner says the same.
- **Keyboard reach (FR-018).** Tab reaches every new control in this order:
  - **Loop:** the navigation, the pipeline link, Note for next step, Limits, Pause, Stop, each
    step of the live graph that has an agent (named with its state, e.g. "reviewer: done"), the
    question's answers, then each story pill (named with its status, e.g. "US1, Add: 1 failed").
    After those come the task tree and the feed. In the graph the arrows move as in Pipelines,
    Enter picks a step, and the focus stays on it across live updates.
  - **Pipelines:** every node, then every + (each named with where it adds), then the step's
    settings and "Edit this agent in Agents →", then the caps.
  - **Agents:** Save for, Save, Runtime, Model, Kind, Returns, May run, Description, Edit as
    text, the instructions, then the agent list.
  - In the graph, Enter or Space acts, the arrows move between columns and within a stack, and
    Delete removes the step.
  - No page threw an error.
- **Contrast (FR-018).** The new text uses 007's tokens. In the graph, the lowest contrast is
  6.2:1: muted text on a selected node, and the group label and "next task" on the phase
  group's tint. All of it is above 4.5:1.

## Where the screens knowingly differ from the mockup

- **Loop:**
  - "Your last steering" appears under the header only once the engineer has steered. Neither
    demo loop has steering yet.
  - The question banner ("the loop asks you") is 007's, shown while the loop waits. The mockup's
    loop was repairing, not waiting.
  - The pipeline is the live graph of `mockup/loop-graph.html` (retaken 2026-11-01 for T043):
    the step where the loop waits for you is orange ("WAITS FOR YOU"), done steps are ticked, a
    working step pulses. The row of agent cards is gone: a click on a step shows its agent above
    the feed. On a phone the graph keeps its size and scrolls inside its box, and Stop wraps to
    its own line.
- **Pipelines:**
  - The header keeps 007's controls: the editable name, Based on, Use for this project, Delete
    and Edit as TOML. The mockup showed only the name as a title.
  - The selected step's settings are form fields (selects, a paths field, Alongside checkboxes)
    with Earlier/Later and Remove step. The mockup showed them as read-only boxes.
- **Agents:**
  - May run is a comma-separated field, not chips with "+ add".
  - The description has its own field.
  - "Reset to the default" appears only for a copy that overrides a default. The shot shows a
    default.

**Approval:** pending. The engineer compares each screen with its mockup and approves (008 T032,
with the reference mockup itself, which the engineer approves too).
