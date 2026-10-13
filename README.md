# Fallow

Fallow is a full-screen terminal editor for developers who run several LLM-driven processes at
once: agent CLIs, scripts calling LLM APIs, and local model servers. Next to the editor, it shows
a live panel of every LLM process on the machine, with its memory, CPU, status and working
directory. Fallow itself stays small enough to leave that RAM to those processes.

**Status: in design.** There is no usable build yet. The specification, plan and research are in
[`specs/001-llm-aware-tui-editor/`](specs/001-llm-aware-tui-editor/), and the project's principles
are in [`.specify/memory/constitution.md`](.specify/memory/constitution.md).

Design budgets, enforced by CI benchmarks once code exists:
- under 50 MB of memory when idle, under 150 MB with a large project open;
- first screen in under 100 ms;
- no root or administrator rights needed;
- no data leaves the machine, except optional DNS lookups of known LLM provider hostnames.

## Platform support

| Platform | v1 status |
|---|---|
| Linux | Planned |
| Windows | Planned. Processes inside WSL2 are not visible to the Windows build. |
| macOS | Planned, **beta** until hardware verification passes (below) |

**macOS hardware verification (open)**: macOS releases are labeled "beta" until they have been
verified on a physical Apple Silicon Mac (M1 or later; not a GitHub-hosted runner, not an Intel
Mac). The checks are:
- memory figures vs Activity Monitor with a running local model;
- the command-line read on macOS not retaining environment data;
- the canary privacy test;
- a manual run-through in Terminal.app and iTerm2.

Details are in [plan.md, Release blockers](specs/001-llm-aware-tui-editor/plan.md#release-blockers).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
