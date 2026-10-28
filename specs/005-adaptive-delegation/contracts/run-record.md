# Contract: the run record (on disk)

`<cache>/fallow/runs/v1/<number>.json` (the number is the file's name, not a field), written once when a recorded run's hidden tests finish. The
directory name carries the format version. Only the fields below exist: the constitution (v5.0.0,
Principle VII) forbids any text written by the engineer or an agent, session output, file contents,
test names, paths, command lines and environment values.

```json
{
  "mode": "squad",
  "switched_off": ["lead_updates"],
  "fingerprints": { "task": "9c1e…", "hidden": "07ab…" },
  "sessions": [
    { "n": 1, "role": 1, "agent": "claude", "model": "claude-opus-5-5", "version": "2.1.330" },
    { "n": 2, "role": 2, "agent": "claude", "model": "claude-opus-5-5", "version": "2.1.330" }
  ],
  "start_commit": "4f2a9c1…",
  "final_commit": "b81d0e2…",
  "started": "2026-10-29T10:12:00Z",
  "duration_s": 3120,
  "time_limit_s": 7200,
  "idle_end_s": 180,
  "parts": [
    { "n": 1, "working_s": 1400, "blocked_s": 90, "signals": 1, "repair_rounds": 0,
      "gates": ["pass"], "reviews": ["approved"] }
  ],
  "per_role": [
    { "role": 1, "working_s": 900, "blocked_s": 0, "interventions": 0, "cost_usd": 1.82, "tokens": null },
    { "role": 2, "working_s": 1400, "blocked_s": 90, "interventions": 1, "cost_usd": null, "tokens": null }
  ],
  "hidden": { "passed": 2, "failed": 0 }
}
```

| Field | Values |
|---|---|
| mode | `plain`, `solo`, `squad` |
| switched_off | component names (fixed list, spec FR-017) |
| role, n | numbers; 1 is the first session (lead or plain); names are never stored |
| agent | built-in profile id, or `custom` (never a user profile's name) |
| model, version | as the agent reported them (research R-10), else null; never a profile's configured text |
| final_commit | the head of the lead's branch when the run ended (the plain session's in plain mode); the tested state is that worktree as left |
| time_limit_s, idle_end_s | the run's settings, as durations |
| gates | `pass`, `baseline-only`, `regression`, `timed-out`, `start-failed` |
| reviews | `approved`, `changes-requested`, `incomplete` |
| hidden | counts; a run whose hidden tests could not run has every test counted as failed |
| cost_usd, tokens | numbers as reported, or null (unknown) |

The Runs tab shows task names by matching `fingerprints.task` against the registered task sets; the
record itself never holds them.
