# Contract: the page (routes, regions, the graph)

## Routes

| Route | Screen |
|---|---|
| `#/pipelines`, `#/pipelines/<project>`, `#/pipelines/<project>/<pipeline>` | Pipelines: the list of pipelines on the left; the graph over the rest of the width; under it the selected step's settings, its agent in brief and what the pipeline guarantees; the third form opens that pipeline (`_` for no project) |
| `#/agents`, `#/agents/<project>`, `#/agents/<project>/<agent>` | Agents: agents grouped by kind, the agent editor; the third form opens that agent (`_` for no project) |
| `#/p/<project>/f/<feature>/loop` | The loop page (007), with 008's regions |

The navigation is Dashboard · Spec sessions · Loops · Pipelines · Agents. Pipelines and Agents keep the selected project between them. 007's `#/pipelines/<project>` links keep working.

## The loop page's regions, top to bottom

1. **Header:**
   - project, lock and pipeline; the feature and its stage;
   - totals (time, cost, tasks, repairs);
   - Merge or Merge anyway, with "What was verified" beside it;
   - Note, Limits, Pause or Go on (not at ready or done), and Stop.
   - The latest steering line sits under the header.
2. **Notice:** the done-unverified notice (007).
3. **The pipeline strip** (007).
4. **The question banner** (007), when a question waits.
5. **"Verified by Fallow":** one pill per story with criteria. Under it, at most one open evidence panel, which opens by itself when a story fails. "What was verified" (in the header) opens a dialog with every story's evidence instead.
6. **Body:**
   - the task tree;
   - the agent cards, the task detail and the feed;
   - the side column: gates vs baseline, questions, commits. It holds no story card and no steering card.

## The graph (research R-6)

- **Columns, left to right:**
  1. Spec session, Lock (both fixed);
  2. "For each phase in tasks.md": `at = start` phase steps, then the per-task steps in pipeline order, then `at = end` phase steps;
  3. "At each story's checkpoint": `at = start` steps, then the checkers; a `parallel_with` set is one column of stacked nodes;
  4. `once` steps;
  5. Merge (fixed).
- **Edges:**
  - one forward edge between consecutive columns, which branches into a stacked column and joins after it;
  - a dotted "next task" back-edge over the per-task steps;
  - a red dashed back-edge for `on_fail = repair` (to the builder, labelled "fails → repair, same session · ≤ N", with N from the pipeline's caps);
  - "→ you" for `on_fail = engineer`.
- **Nodes:** each shows when the step runs (EACH TASK, EACH PHASE, CHECKPOINT, ONCE, ALWAYS), the agent, and one line about it. It has a left bar coloured by role: maker blue, checker green, Fallow (`fallow:gates`) grey, the engineer's fixed steps orange.
- **+ on a forward edge:** adds a step. Its `when` and `at` are preset from where the + is; the engineer picks the agent.
- **Selecting a node:** shows its settings under the graph (agent, when, at, paths, alongside, on failure). Removing a node removes the step; fixed nodes cannot be removed.
- **Keyboard:**
  - every node and + is focusable, with an `aria-label` (step, when, agent);
  - arrow keys move between nodes in layout order;
  - Enter selects, Delete removes.
- **Size:** the layout is computed in a fixed coordinate space and scaled to its box. Below 700 px it scrolls inside the box, never the page.
