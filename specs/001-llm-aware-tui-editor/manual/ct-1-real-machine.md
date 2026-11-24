# CT-1 (T054): idle memory on a real developer machine

A manual run. CI measures idle memory on quiet runners with no LLM processes; this run measures it
where Fallow is meant to live (plan.md "Carried into tasks", CT-1; research.md R-12).

**Pass**: peak RSS **< 50 MB** (Constitution I).
**Valid only if** the harness prints no `invalid:` line. It checks:
- full duration (30 s settle plus 60 one-second samples);
- the measured PID is Fallow;
- procmon cycles ≥ 90% of expected;
- **at least one LLM group detected** (`--require-llm-groups 1`).

Run it on each platform you have, and on the macOS hardware from the release blocker (T103).
Don't use the Defender-ASR-blocked machine.

## 1. Set up the machine (leave everything running during the measurement)

- [ ] At least **three agent CLIs**, each in its own terminal and project, e.g. `claude`, `codex`,
      `aider` (or `gemini`, `opencode`).
- [ ] A **local model server with a model loaded**: `ollama serve`, then `ollama run llama3.2:3b`
      (any model; keep the `run` session open so the model stays loaded).
- [ ] **Language servers**: open a project in an editor so `rust-analyzer`, `pyright` or
      `typescript-language-server` are running.
- [ ] A **browser** with a few tabs open.

## 2. Build and run (from the repository root)

```sh
cargo build --release
cargo xtask bench-memory --idle --require-llm-groups 1 --csv ct1-$(uname -s)-$(date +%Y%m%d).csv
```

Windows (PowerShell):

```powershell
cargo build --release
cargo xtask bench-memory --idle --require-llm-groups 1 --csv ct1-windows.csv
```

The run takes about 90 seconds. Don't type into the terminal while it runs.

## 3. Read the result

The last lines look like this:

```text
evidence: pid=… exe=…/fallow elapsed_s=90.0 … stats=pid=… ticks=… cycles=45
llm_groups=4 llm_processes=6 processes=612
os=linux samples=60 peak_rss_mb=… peak_at_s=… final_rss_mb=… budget_mb=50 csv=… result=PASS
```

- `result=PASS`: done. `FAIL`: peak ≥ 50 MB, a constitution violation; report it.
  `INVALID`: fix the `invalid:` reason (most often: no LLM process running) and rerun.
- To see what Fallow detected: `./target/release/fallow --snapshot-json | python3 -m json.tool | less`.

## 4. Record it

Add a row per run to research.md **R-12**, next to the CI lower bound:

| Date | Platform / hardware | Processes | LLM groups / processes | Peak RSS | Result |
|---|---|---|---|---|---|
| YYYY-MM-DD | e.g. Linux x86_64, 16 GB | `processes=` | `llm_groups=` / `llm_processes=` | `peak_rss_mb=` | PASS |

Keep the CSV with the commit that records the result. When every available platform has a
valid row, tick T054 in tasks.md.
