# Implementation Plan: Dark mode

**Branch**: `001-llm-aware-tui-editor` (the project's working branch) | **Date**: 2026-11-12 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/013-dark-mode/spec.md`

## Summary

- **The dark theme**: a second set of values for the page's named colours, with the approved
  mockup's palette.
- **The light theme**: stays exactly as it is. The literal colours in `app.css` become named
  colours with today's values.
- **System**: a CSS media query follows the system's setting, with no script, so the page switches
  without a reload.
- **The engineer's choice**: Light, Dark or System, from a three-way control in the top bar.
  - It is kept in Fallow's `prefs.json` beside the editor, so it survives restarts, whose address
    changes.
  - It is written on `<html>` as the page is served, so the first frame is right.
- **Contrast**: checked by a test for every pair, in both themes.

## Technical Context

**Language/Version**: Rust (edition and toolchain as in `rust-toolchain.toml`); the page is plain
JavaScript and CSS compiled into the binary (`include_str!`).

**Primary Dependencies**: none added. `tiny_http` serves the page as today.

**Storage**: `prefs.json` in Fallow's data directory (one new field, `theme`).

**Testing**: `cargo test`, with a new `tests/web_theme.rs` that reads the CSS (contrast, the two dark
blocks identical, light values unchanged) and runs the engine for the choice and the served
`<html>`. Visual checks run in headless Chrome over its debugging port, as in 012.

**Target Platform**: Windows, macOS and Linux; any current browser (Chromium, Firefox, Safari),
since `prefers-color-scheme` and `color-scheme` are supported by all of them.

**Project Type**: a local web page served by a Rust service.

**Performance Goals**: no change. The page must stay ready within 500 ms; reading `prefs.json` on
each `GET /` is one small file read.

**Constraints**:
- no inline script (the content security policy is `script-src 'self'`);
- no external resource (V);
- the light theme stays identical;
- no new dependency.

**Scale/Scope**: one stylesheet (about 730 lines, about 30 literals to name), one top-bar control,
one route, one field.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check |
|---|---|
| I. Verified, Not Trusted | Not affected. The colours of verified and not-verified results keep their difference in both themes (FR-013), so a result that is not verified is never shown in a verified one's colours. |
| II. The Engineer Decides | Not affected. |
| III. Coordination in Code | Not affected. |
| IV. Simplest Setup First | Pass: CSS names and one media query; no script for System, no framework, no build step. |
| V. Local and Private | Pass: the choice is one of three fixed values, kept in Fallow's own data directory. The page loads nothing new; no inline script, so the content security policy is unchanged. |
| VI. Least Privilege | Pass: Fallow writes only to its data directory, as for the editor. |
| VII. Runs Survive | Not affected. The choice survives restarts too (FR-009). |
| VIII. Small and Measured | Pass: no new process work beyond one small file read per page load. The existing page-ready and memory benchmarks still apply. |
| IX. Runtime-Agnostic | Not affected. |

Re-check after Phase 1: unchanged, with no violations and nothing in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/013-dark-mode/
├── spec.md
├── plan.md              # this file
├── research.md          # R-1 to R-8
├── data-model.md        # the theme choice, the shown theme
├── quickstart.md        # Q1 to Q5
├── contracts/web-api.md # GET / with data-theme, POST /api/theme, the control
├── mockup/              # the approved mockup (dark.html, dark.css, screenshots)
├── checks/              # screenshots of the visual checks (made in implementation)
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
src/service/web/app.css     # literals named (R-1); dark blocks (R-2); the control's style
src/service/web/index.html  # the control in the top bar
src/service/web/app.js      # the control: reads <html data-theme>, sets it, POST /api/theme
src/service/http.rs         # GET / writes data-theme from the choice; POST /api/theme
src/studio/files.rs         # prefs.json: theme read and written beside the editor
src/studio/mod.rs           # Req::SetTheme / GetTheme
src/run/engine.rs           # the requests routed to the studio
tests/web_theme.rs          # contrast, identical dark blocks, light values unchanged, choice
                            # kept and served
```

**Structure Decision**: everything stays in the existing service and page; the only new file is a
test.

## Complexity Tracking

No violations.
