# Build on macOS (T001)

Measured 2026-11-22 on the engineer's Mac: Apple M5 Pro, macOS 26.7.3, Rust 1.99.1.

## After Phase 1 and 2 (dependencies, AppKit on the main thread; no window yet)

| What | Value |
|---|---|
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok (targets: Windows x64, macOS arm64) |
| `cargo build` | ok, no warning of Fallow's |
| `cargo clippy --all-targets --all-features -- -D warnings` | ok on macOS; ok for `x86_64-pc-windows-msvc` from this Mac; `cargo check` for Linux ok |
| `cargo test --workspace --locked` | 433 passed, 0 failed, 11 ignored (before 016: 412 passed), about 11 minutes |
| Release build, from scratch, `main` before 016 | 47.8 s |
| Release build with 016's dependencies (most of the graph already built) | 52.6 s |
| `target/release/fallow`, before 016 | 6,124,080 bytes |
| `target/release/fallow`, now | 6,161,296 bytes (+37 KB) |
| `target/release` | 425 MB |
| `target/` (debug, release, test builds and two cross-checks) | 12 GB |

Dependency changes:
- `wry` 0.57.2 and `raw-window-handle` 0.6 now on macOS too.
- New on macOS: `objc2` 0.6.5, `block2` 0.6.2, `objc2-foundation`, `objc2-app-kit`, `objc2-web-kit` 0.3.2 (all already in the graph through `wry`) and `objc2-user-notifications` 0.3.2 (new).
- `url` 2.5.8 → 2.5.9 (RUSTSEC-2026-0592, reached through `wry` on macOS).
- `deny.toml`: `aarch64-apple-darwin` added to the graph's targets. `option-ext` (MPL-2.0), which `wry` brings through `dirs` on every Apple target, is allowed for that crate only.

## After Phase 3 (the window on macOS)

| What | Value |
|---|---|
| `cargo test --workspace --locked --no-fail-fast` | 434 passed, 0 failed, 11 ignored |
| `target/release/fallow` | 6,427,040 bytes (+266 KB for the window, menus and dialogs; before 016: 6,124,080) |

## After Phase 4 (close, menu bar icon, Quit, logout, App Nap)

| What | Value |
|---|---|
| `cargo test --workspace --locked --no-fail-fast --features automation` | 437 passed, 0 failed, 14 ignored (includes `window_mac`: the real window closed during a session turn) |
| `cargo test --workspace --locked --no-fail-fast` | 434 passed, 1 failed: `us12_check::the_base_moves_with_a_merge_of_the_starting_branch_and_back_on_undo`, a 30 s wait in `tests/common` under the suite's load (seen once before in Phase 2); it passes alone 3 of 3, and alone on `main` before 016. Not code 016 touches. |
