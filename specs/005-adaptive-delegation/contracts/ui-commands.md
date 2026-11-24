# Contract: UI and commands (on top of 004)

## Squad tab

- A **Work** section: each piece of work from the manifest with its build decision, the lead's
  reason, builders, state and open signals; refused starts show their reason.
- A **Signals** list: open first, with the raiser, part, age and answer commit.
- The memory line: budget, in use, waiting starts and the shortfall.
- `L` names another session the lead (004), and the new lead's briefing is sent.

## Runs tab (new, in the orchestration view)

- Lists recorded runs grouped by task (names by fingerprint match against registered task sets) and
  mode, with pass rate, median time, interventions, cost per passing run where known, and the spread
  across repeats; groups with different fingerprints are kept apart (FR-019).
- `Enter` opens a run's details; `Delete` deletes a record (confirmed); the header shows the number of
  records and their size.

## Commands (palette)

| Command | Key | What it does |
|---|---|---|
| Register a task set… | | Pick a directory; Fallow validates `taskset.toml` and shows the hidden-tests notice. |
| Start a run… | | Pick a task, a mode (plain, solo, squad), components to switch off, and whether to record it. |
| Start a series… | | Pick tasks, modes and repeats (default 3); runs one at a time in shuffled order. |
| End the run | | Ends the running run now (counted as an intervention). |
| (at a run's end) | `y` | "Run 4 finished: close its sessions #12, #13, #14 and run the hidden tests?" One confirmation naming every session (Principle IV); a series continues after it. |
| Open runs | | The Runs tab. |
| Set the memory budget… | | `orchestration.memory_budget_mb`. |

New keys are checked against the known-conflicts list as in 004 (FR-041).

## Settings

| Key | Default | Meaning |
|---|---|---|
| `orchestration.memory_budget_mb` | 75% of total memory minus Fallow's | FR-037 |
| `orchestration.task_sets` | [] | registered task set directories |
| `orchestration.session_size_mb` | 700 | expected size of a session not yet measured (research R-14) |
