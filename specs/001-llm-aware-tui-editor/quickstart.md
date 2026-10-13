# Quickstart & Validation Guide

How to build Fallow and prove that the feature works end to end. Commands use the `xtask` harness
from the [plan](./plan.md). Contracts: [cli](./contracts/cli.md), [config](./contracts/config.md),
[ui-commands](./contracts/ui-commands.md).

## Prerequisites

- Rust stable (see `rust-toolchain.toml`) on Linux, macOS or Windows.
- For scenario checks: at least one agent CLI (e.g. Claude Code), Ollama with a small model, and
  Python 3 for the scripted LLM client.

## Build and run

```sh
cargo build --release
./target/release/fallow            # idle: process panel only
./target/release/fallow ~/src/app  # with a project
```

## Automated gates (also run in CI on all three platforms)

```sh
cargo test --workspace                 # unit + integration
cargo xtask bench-startup              # p95 < 100 ms        (FR-041, SC-006)
cargo xtask bench-memory --idle        # max RSS < 50 MB     (FR-040, SC-005)
cargo xtask bench-memory --loaded      # max RSS < 150 MB    (FR-040, SC-005)
cargo xtask detect-fixtures            # SC-002 / SC-003 reference sets
cargo xtask canary                     # SC-007
cargo xtask egress                     # SC-009
cargo xtask defender-check             # Windows only: enables + verifies real-time protection, fails on any detection (R-10)
```

## Manual validation scenarios

| # | Scenario | Steps | Expected |
|---|---|---|---|
| V1 | See every LLM process (US1) | Start an agent CLI in `~/a`, `ollama serve` + `ollama run <model>`, and `python scripts/llm_client.py` (it calls a provider API in a loop). Run `fallow`. | All three are listed with type, PID, cwd, memory with metric label, CPU, uptime, status and confidence. The agent is `certain`, the script `likely`, ollama `local model server` with its model name. The summary adds up. |
| V2 | Statuses (FR-012) | While the model answers a prompt from a client | ollama shows `serving` and the client shows `LLM connected`. After the client exits, ollama shows `busy` or `idle`. |
| V3 | Grouping (US1-4) | In the agent, run a shell command that starts a test runner | The children appear under the agent, and the group total is at least the agent's own figure. |
| V4 | Footprint parity (SC-012) | Compare the ollama group figure with Activity Monitor or Task Manager (or `smem -P ollama` on Linux) | Within 10%. |
| V5 | Find the culprit (US2, SC-001) | Press `m` | The largest group is first. |
| V6 | Explain detection (US4) | Select the script and press `Enter` | It shows "provider connection: <provider> via resolved \| range, likely". |
| V7 | Jump to the work (US5) | Select the agent and press `g` | The tree opens in `~/a`, and the recent changes list shows files newest first with the disclaimer. |
| V8 | Kill with confirmation (US6) | Select the script, press `Shift-K`, then `Esc` | The process is still running. Press `Shift-K` and `y`: on Linux/macOS it receives SIGTERM and exits; on Windows the dialog says the kill is immediate with no cleanup. |
| V8b | Force kill (Linux/macOS) | Run `python -c "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(600)"` as a detected process, then `Shift-K`, `y` | After 5 s a second "Force kill" dialog appears. `Esc`: still running. Repeat and confirm: it stops. |
| V9 | Windows pause note (FR-052) | On Windows, select a process and press `p` | The dialog contains the Windows difference text. |
| V10 | Editor basics (US3) | Open a file, edit, `Ctrl-S`. Then `Ctrl-F`, type a string, `Tab` to "whole project", `Enter`, and open a hit. Then `Ctrl-G 10`. | The file is saved, the scope label switches, results appear, the hit opens at its line, and the cursor goes to line 10. |
| V15 | Selection and clipboard (FR-008) | `Shift-→` ×5, `Ctrl-C`, paste in another app. Then `Ctrl-C` with no selection. Then paste a multi-line block with the terminal's paste. | The other app receives the 5 characters. With no selection, nothing is copied and the hint shows "Quit: Ctrl-Q". The pasted block is inserted verbatim with no auto-indent. |
| V16 | File operations (FR-068) | In Files, `a` to create `x.txt`, `r` to rename it to `y.txt`, then `d` and `Esc`, then `d` and `y` | The file is created and renamed. The first delete is cancelled and the file remains; the second deletes it. |
| V11 | Agent edits an open file (FR-006) | With unsaved edits, let the agent change the same file | The keep-or-load prompt appears and nothing is lost. |
| V12 | DNS off (FR-022b) | `fallow --no-dns` with the V1 script running | The script is still detected "via range" (if its provider has ranges), and no DNS queries are sent (check with `cargo xtask egress --no-dns`). |
| V13 | Bad config (edge case) | Add a syntax error to `signatures.toml`, then run "Reload configuration" | An error with file:line is shown, and built-in detection still works. |
| V14 | Discoverability (SC-011) | A new user with no docs: open a file and sort by memory | Done within 2 minutes using the hint bar, `F1` or `Ctrl-P`. |
