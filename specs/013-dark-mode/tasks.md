---
description: "Task list for 013 dark mode"
---

# Tasks: Dark mode

**Input**: Design documents from `specs/013-dark-mode/` (spec, plan, research R-1 to R-8, data-model,
contracts/web-api.md, quickstart Q1 to Q5)

**Tests**: the spec asks for automated contrast checks (SC-002) and pixel comparisons of the light
theme (SC-003), so tests are included.

**Build and test**: `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/wip`,
`-j 1`, one test file per command, `--test-threads=2`.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Write the visual check: `tests/visual_013.rs` and `specs/013-dark-mode/checks/shots.js`.
  - **The demo** (`tests/visual_013.rs`, an ignored test): on the fake runtime, it serves a page at a fixed address. It holds a loop ready to merge, a loop asking the engineer, and a solo session with two turns, a suggested feature and a command waiting for approval.
  - **The shots** (`shots.js`, Node, headless Chrome over its debugging port, as in 012's checks): every screen and state of the reference set (spec FR-005), at 1440 × 900, in a chosen theme (`light`, `dark`, or `system-dark`/`system-light` with an emulated `prefers-color-scheme`).
  - **The light comparison** (`--compare <old.css>`): each screen is shot twice on the same page, once with app.css as served and once with the stylesheet from before this feature (`git show d9a7685:src/service/web/app.css`) put in its place through DevTools. The two are compared pixel by pixel. The same page at the same moment makes the comparison exact; two separate runs would differ in times and ids.
  - **Then**: run it once in light before any change to the page, to check that the script itself works.

---

## Phase 2: Foundational (the light theme in names)

**Purpose**: every colour comes from a name, with today's values; nothing on screen changes.

- [X] T002 In `src/service/web/app.css`, replace every literal colour outside `:root` with a new named colour defined on `:root` with exactly that literal's value (research R-1). Examples: `--on-ink: #FFFFFF`, `--solid-hover: #000000`, `--dot-grey: #C9C8C1` (also `.link`), `--arrow: #A3A29B`, `--backdrop: rgba(28, 28, 26, .35)`, `--marker: #FDF1DC`, `--minus: #B42318`, `--bar-rest: #DCE3F8`, `--phase-bg: #F8FAFF`, `--grid-dot: #E9E8E2`, `--edge: #9C9B94`, and `--shadow-menu`, `--shadow-panel`. Drop the `var(--x, #fallback)` fallbacks (every name is defined). Add `color-scheme: light` to `:root`.
- [X] T003 [P] New test file `tests/web_theme.rs`, first tests (reading `src/service/web/app.css`):
  - outside the `:root` and dark blocks, no rule holds a hex colour or `rgb()`/`rgba()`;
  - every `var(--x)` used is defined on `:root`;
  - each light value named in T002 equals the literal it replaced (a table of the old literals in the test).

**Checkpoint**: the page looks exactly as before (spot-check the dashboard and a loop page with `shots.js light`).

---

## Phase 3: User Story 1 - Fallow follows the system's dark setting (P1) 🎯 MVP

**Goal**: with the system dark and no choice made, every screen is dark from the first frame, and it follows the system without a reload.

**Independent Test**: quickstart Q1.

- [X] T004 [US1] In `src/service/web/app.css`, add the dark values of every name (research R-2):
  - the palette of `specs/013-dark-mode/mockup/dark.css`, with `--on-ink: #161614` and `--solid-hover: #FFFFFF`;
  - the mockup's dark values for the colours named in T002 (`--dot-grey: #55544D`, `--arrow: #6E6D66`, `--backdrop: rgba(0, 0, 0, .6)`, `--marker: #3A2F18`, `--minus: #F2A098`, `--bar-rest: #2A3358`, `--phase-bg: #1B2036`, `--grid-dot: #2E2D29`, `--edge: #6E6D66`, darker shadows);
  - and `color-scheme: dark`;
  - written twice, identically: in `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { … } }` and in `:root[data-theme="dark"] { … }`.
  - Also add `:root[data-theme="light"] { color-scheme: light; }`.
  - No transition on colours (R-5).
- [X] T005 [P] [US1] In `tests/web_theme.rs`:
  - the two dark blocks are identical;
  - every name on `:root` has a dark value (FR-004).
- [X] T006 [US1] In `tests/web_theme.rs`, contrast (research R-6, FR-012, SC-002):
  - **The pairs**: list every text-on-background pair the stylesheet uses. Among them: `--ink`, `--ink-2`, `--body`, `--muted` and `--faint` on `--ground`, `--panel`, `--panel-2`, `--hover` and `--chip`; `--on-ink` on `--ink`; `--accent` on `--accent-bg`; each status colour on its `-bg`; `--diff-add-ink` on `--diff-add`; `--diff-del-ink` on `--diff-del`; `--term-ink` on `--term`.
  - **The thresholds**: at least 4.5:1, in both themes. Control borders (`--line-3`), the focus outline (`--accent`) and graph lines (`--edge`) need at least 3:1 against `--panel` and `--ground`.
  - **A guard**: a rule pairing `color: var(--x)` with `background: var(--y)` that the list lacks fails the test.
  - **Fixes**: where a mockup colour fails, adjust it within its hue in `src/service/web/app.css` (both blocks), and note each change in research R-6.
- [X] T007 [US1] Visual check (quickstart Q1):
  - run `shots.js system` with the emulated setting dark, and save to `specs/013-dark-mode/checks/dark/`;
  - check each screen for any area, text or line left light (SC-001), against the mockup's look;
  - on the project page, switch the emulated setting with a half-written message, a side panel and the "/" menu open, and check that nothing is lost (SC-004);
  - capture the first frame of 20 reloads in dark (SC-005);
  - check that the stage, status and legend colours and the diff's added and removed lines stay distinguishable from each other in dark (FR-013, analyze E2);
  - check that selects, checkboxes and scrollbars are drawn dark (edge case, analyze E3).
  - Fix what is found in `src/service/web/app.css`.

**Checkpoint**: Q1 passes.

---

## Phase 4: User Story 3 - The light theme stays as it is (P1)

**Goal**: an engineer in light mode sees no change.

**Independent Test**: quickstart Q3.

- [X] T008 [US3] Run `shots.js light --compare <the stylesheet at d9a7685>` after Phases 2 and 3, into `specs/013-dark-mode/checks/light/`. Compare against the old stylesheet with the light contrast fixes of research R-6 applied (`--fixes specs/013-dark-mode/checks/light-fixes.json`). Expect every screen the same (SC-003; the control does not exist yet). Fix any difference in `src/service/web/app.css`.

**Checkpoint**: Q3 passes with zero differing pixels.

---

## Phase 5: User Story 2 - The engineer chooses the theme (P2)

**Goal**: Light, Dark or System from any screen, applied at once, remembered across restarts, with no flash.

**Independent Test**: quickstart Q2.

- [X] T009 [P] [US2] In `src/studio/files.rs`, add `theme: Option<String>` to `Prefs` (data model: `"system"`, `"light"` or `"dark"`; missing, another value or an unreadable file mean `system`), with `theme(data) -> &'static str` and `set_theme(data, t) -> Result<&'static str, String>`. `set_theme` refuses anything else with "the theme is system, light or dark" and keeps the other fields of `prefs.json` (written through a temporary file and a rename, as `write_prefs` does).
- [X] T010 [US2] Route the choice:
  - `Req::GetTheme` and `Req::SetTheme { theme }` in `src/studio/mod.rs`, answered from `files::theme` and `files::set_theme` with the studio's data directory, and handled in `src/run/engine.rs` where the other `Req`s are;
  - in `src/service/http.rs`:
    - `POST /api/theme` `{theme}` answers `{theme}`, or 409;
    - `GET /` asks `GetTheme` and serves `index.html` with `<html lang="en" data-theme="light">` or `data-theme="dark"`, unchanged for System (contracts/web-api.md);
    - if the ask fails, it serves as System.
- [X] T011 [US2] The control (contracts/web-api.md, research R-4):
  - in `src/service/web/index.html`: `<fieldset class="theme-pick">` with `<legend class="sr">Theme</legend>` and three radio inputs `name="theme"` (values `system`, `light`, `dark`), labelled with an icon, a visually hidden name ("System", "Light", "Dark") and a `title`, after `#header-status`;
  - in `src/service/web/app.js`: on start, check the input matching `document.documentElement.dataset.theme` (none means `system`). On `change`, set or remove `data-theme` on `<html>` at once, then `POST /api/theme`. On an error, keep the choice and show the error line (`alertLine`);
  - in `src/service/web/app.css`: style it as three small segments with the active one marked and a visible focus outline, all from named colours.
- [X] T012 [P] [US2] In `tests/web_theme.rs`, through the engine (as `tests/common` starts it):
  - the choice is kept in `prefs.json`, survives a new engine on the same data, and keeps the editor field;
  - `POST /api/theme` refuses `"blue"`;
  - `GET /` holds `data-theme="dark"` after choosing dark, and no `data-theme` for System.
  - Also extend `tests/web_markdown.rs`'s check that `index.html` has no inline script, if the control adds markup there.
- [X] T013 [US2] Visual and keyboard check (quickstart Q2):
  - choosing each option from two screens;
  - Dark kept over 3 restarts (SC-006), with the first frame dark;
  - Light staying light when the emulated setting turns dark;
  - keyboard only: Tab to the control, arrows to choose, focus outline visible;
  - choosing with a half-written message, an open side panel and a scrolled conversation keeps all three (FR-011, analyze E1). A menu that closes on any click outside it (the "/" menu, the session dropdown) closes as it would for any click, and that is expected;
  - the accessibility tree names the group "Theme" and the checked option.
  - Then run `shots.js light --compare` again: the only differing area is the control (SC-003).
  - Save the control's screenshots in `specs/013-dark-mode/checks/`.
  - Done 2026-11-12 with `shots.js --pick` (16 checks), `--switch` and `--reloads`. Screenshots are in `checks/dark/`, plus `checks/light-dashboard.png` and `checks/theme-pick-focus.png`.
    - **Restarts:** a restart of Fallow keeping the choice is checked by `tests/web_theme.rs`, through a new engine on the same data. The first frame is checked by 10 reloads in Dark with the system light.
    - **Light comparison:** it hides the control in both shots, since the old stylesheet has none, and is otherwise pixel-identical.
    - **Found and fixed:** the side panel and the session dropdown closed when the control was clicked or took the focus. Both now stay open (FR-011). The "/" menu, a typing popup, closes when the message box loses focus, as analyze E1 expected.
    - **Review fixes:**
      - a theme of the wrong type in `prefs.json` no longer loses the editor;
      - `GET /` no longer waits on the engine: the page server keeps the theme, asking the engine once and updating it on each `POST /api/theme`. A first try that limited the wait to 200 ms served System on a busy engine, and 9 of 10 reloads in Dark showed a light frame; with the kept theme, none did;
      - browsers' radio restore is off;
      - choices are saved in order;
      - a choice clicked before the script runs is taken.

**Checkpoint**: Q2 passes.

---

## Phase 6: Polish

- [X] T014 [P] Add a line on themes to the page section of `README.md`.
- [X] T015 Run the whole suite one file at a time, and `cargo clippy --all-targets -- -D warnings`.
  - Done 2026-11-12: all 66 test files pass and the 159 unit tests; clippy is clean.
  - `us11_update` was run test by test: as one file it takes over ten minutes, and in the background it was stopped for low memory.
- [X] T016 Quickstart Q5 by hand with the engineer (optional): the system switched while a loop page is open. Done by the engineer on 2026-11-12: it looked right.

---

## Dependencies & Execution Order

- **Setup (Phase 1)**: T001 first, **before any change to the page** (it records the baseline).
- **Foundational (Phase 2)**: T002, then T003 (T003 can be written alongside).
- **US1 (Phase 3)**: needs Phase 2. T004 → T005, T006 → T007.
- **US3 (Phase 4)**: needs Phases 2 and 3 (it checks that the dark blocks changed nothing in light).
- **US2 (Phase 5)**: needs Phase 2. T009 → T010 → T011; T012 after T010; T013 last.
- **Polish (Phase 6)**: after all stories.

### Parallel opportunities

- T003 beside T002; T005 beside T006's palette work; T009 beside T011's markup; T012 beside T011.

## Implementation Strategy

1. **MVP**: Phases 1 to 4. Fallow follows the system's dark setting, and the light theme is
   proven unchanged.
2. **Then**: Phase 5 (the choice) and Phase 6.
3. **Per phase**: a fresh reviewer, its confirmed findings fixed, then commit and push.
