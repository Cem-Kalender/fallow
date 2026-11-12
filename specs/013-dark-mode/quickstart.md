# Quickstart: checking dark mode (013)

Build and test as usual, one test file at a time when memory is short:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/wip \
  cargo test -j 1 --test web_theme -- --test-threads=2
```

The visual checks (Q1, Q3, Q4) use headless Chrome over its debugging port, as in 012's checks. The
system setting is emulated with `Emulation.setEmulatedMedia` (`prefers-color-scheme`), and the
screenshots go to `specs/013-dark-mode/checks/`.

## Q1 Following the system (US1)

1. Run Fallow with no choice made, and the emulated setting dark.
2. Open each screen of the reference set (spec FR-005). Expect each to be dark, matching the mockup's
   look, with no area left light (SC-001).
3. On the project page:
   - type half a message, open a side panel and the "/" menu;
   - switch the emulated setting to light, then back.
   - Expect the page to follow within a second, with the message, the panel and the menu as they were
     (SC-004).
4. Reload 20 times in dark, capturing the first frame each time. Expect no light ground (SC-005).

## Q2 Choosing (US2)

1. With the setting dark, choose Light in the top bar. Expect the page light at once, staying light
   when the setting changes.
2. Choose Dark, then restart Fallow 3 times, opening the page at its new address each time. Expect
   dark from the first frame (SC-006).
3. Choose System. Expect the page to follow the setting again.
4. Using the keyboard only: Tab to the control, use the arrows to choose, and check the focus outline.
   With a screen reader, expect "Theme" and the chosen option to be announced.

## Q3 The light theme is unchanged (US3)

1. Take the reference set in light before the feature (at the commit before Phase 2) and after it.
2. Compare them pixel by pixel. Expect differences only in the top bar's control (SC-003).

## Q4 Contrast (FR-012)

`cargo test --test web_theme` checks every listed pair in both themes (SC-002). By eye, check that
focus outlines and graph lines are visible on the dark screenshots.

## Q5 By hand (optional)

On the engineer's machine, set the system to dark and back while Fallow is open on a loop page.
