# Research: Dark mode (013)

## R-1 Where the colours are today

- `src/service/web/app.css` defines 44 named colours on `:root` and uses them almost everywhere
  (311 uses).
- It writes about 30 colours as literals. Examples:
  - text on the solid surfaces (`#FFFFFF` on `.btn`, `.nav.on`, `.you`, `.send-btn`, `.ptab.on`,
    `.ricon .dot`);
  - `.btn:hover`'s `#000000`;
  - the grey dot and link `#C9C8C1`, the arrow `#A3A29B`;
  - the dialog backdrop, the document marker `#FDF1DC`, the minus sign `#B42318`;
  - the progress bar's rest `#DCE3F8`, the phase box `#F8FAFF`;
  - the graph's grid dots `#E9E8E2` and edges `#9C9B94`;
  - three shadows;
  - a few `var(--x, #fallback)` fallbacks.
- The scripts (`app.js`, `screens/*.js`) and `index.html` hold no colour literal; the SVG icons
  draw with `currentColor`.

**Decision**: every literal becomes a named colour whose light value is exactly today's literal. In
the light theme the page then computes the same colours as before (FR-006), and the dark theme only
redefines names (FR-004). The fallbacks in `var(--x, #…)` are dropped, since every name is defined.

**Alternatives**: overriding the literals by selector in a dark block, as the mockup did
(`mockup/dark.css`). Rejected: every new rule with a literal would silently stay light, and the
selectors would have to follow every change of app.css.

## R-2 Following the system without a script

**Decision**: the dark names are defined twice in app.css:
- under `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { … } }`, for System;
- under `:root[data-theme="dark"]`, for the engineer's Dark.

The browser applies the media query itself, both at first paint and when the system setting
changes, with no reload and no script (FR-007, FR-011). `color-scheme` is set the same way, so the
browser's own scrollbars and form controls follow (edge case).

The two dark blocks must stay identical; a test checks that (T-list in tasks).

**Alternatives**:
- `matchMedia` in script: it adds a frame before the script runs, a flash on a dark system.
- One block with `:is()` across a media query: CSS cannot do that.
- A CSS preprocessor: a build step this project does not have.

## R-3 Remembering the choice across restarts, with no flash

The page's address (port and key) changes at every start (constitution V), so the browser's
storage for one address is lost: a choice kept there would not survive a restart (FR-009,
SC-006).

**Decision**:
- **Where it is kept**: Fallow keeps the choice in its data directory's `prefs.json`, as it keeps
  the editor (`src/studio/files.rs`). One field, `theme`, holds `light`, `dark` or `system`; when
  it is missing or unreadable it means `system`.
- **No flash**: when Fallow serves `index.html`, it writes `data-theme="light"` or
  `data-theme="dark"` on `<html>` for those choices, and nothing for System. The first frame is
  then right on every screen and every reload (FR-014), with no inline script, which the content
  security policy forbids.
- **Choosing**: the page calls `POST /api/theme` and sets the attribute on `<html>` at once
  (FR-008). Another open page takes the choice at its next reload (edge case).
- **A write that fails**: when the choice cannot be written, the page keeps the choice until it
  is reloaded and shows the error line it shows for any failed request (edge case: nothing
  alarming).

**Alternatives**:
- A cookie: tied to the host and port in practice, so lost with the port.
- Fallow's `settings.toml`: the engineer's hand-written config, which Fallow does not write.

## R-4 The control

**Decision**: a group of three radio buttons (System, Light, Dark), drawn as small icon segments in
the top bar after the day's cost. It is a real `<fieldset>` with `<input type="radio">` and visible
labels for screen readers (`legend` "Theme"):
- the keyboard reaches it with Tab and changes the choice with the arrow keys;
- the chosen one is marked, and screen readers announce "Theme, System, radio button, 1 of 3,
  checked";
- choosing takes one action from any screen (FR-008, FR-010, SC-007).

**Alternatives**:
- One button that cycles: it needs up to two presses, and it is unclear what comes next.
- A `<select>`: a heavy native control in the top bar.

## R-5 The switch itself

**Decision**: no transition at all. The colours change in one frame, which meets FR-015 both with
reduced motion and without, and there is nothing to time.

## R-6 Contrast, checked automatically

**Decision**: a Rust test reads `app.css` and computes the WCAG 2.x contrast ratio of every pair
the page uses for text and controls, in both themes. It lists the pairs by name (for example
`--ink` on `--panel`, `--muted` on `--ground`, `--on-ink` on `--ink`, `--bad` on `--bad-bg`), and
fails below 4.5:1 for text and 3:1 for borders of controls, focus outlines and graph lines
(FR-012, SC-002).
- The list is checked against app.css's rules, so a new `color: var(--x)` on `background: var(--y)`
  in the same rule must be listed.
- The guard sees a colour and a background only when they are in the same rule. Text on a
  parent's background is covered by listing the pair, not by the guard (analyze A1).
- The test needs no dependency: it reads hex colours, and `rgba()` only in shadows and the backdrop,
  which are not text.

Where a mockup colour fails, it is adjusted within its hue and the mockup's look (Assumptions in
the spec).

**The light theme too** (Clarifications 2026-11-12, the engineer: full AA in both themes). Today's
light theme misses FR-012 in five places. Each gets the smallest step toward black, in its own
hue, that passes on every background it sits on: `--ground`, `--panel` and `--panel-2`; the tinted ones a
control or a dot sits on (`--needs-bg` in a notice, `--hover`, `--chip`, `--accent-bg`, `--diff-add`,
the panels' tints; second review); and `--hover`/`--chip` for text. The control borders that
are on `--line` (the file search field, the gate's note box) move to `--control-line` too.

| What | Before | After (light) | Dark |
|---|---|---|---|
| Borders of controls (secondary buttons, text fields, selects, the message box, the title field, graph nodes): a new `--control-line`, so decorative lines (`--line-3` on a quote's bar) keep their grey | `#D4D3CC` (`--line-3`) | `#878680` | `#767570` |
| Graph edges, arrowheads, the "+" circles (`--edge`) | `#9C9B94` | `#878680` | `#767570` |
| The grey status dot and the phase links (`--dot-grey`) | `#C9C8C1` | `#878680` | `#767570` |
| The arrow between steps (`--arrow`) | `#A3A29B` | `#878680` | `#767570` |
| The "danger" button's border (`--bad-control`, was `--bad-line` at 1.56:1) | `#F2C4BF` | `#C1726B` | `#A85E58` |
| Faint text (`--faint`), 4.35:1 on a hovered row | `#6E6D67` | `#6A6963` | `#95948C` |

Controls whose border stays `--line`, such as the segmented tabs, option rows, agent buttons and
pills, are identified by their text. WCAG 1.4.11 asks no 3:1 boundary of them. An icon-only
button needs its icon at 3:1, not its border (third review).

The light comparison (R-7) compares against the old stylesheet with exactly these changes applied
(`checks/light-fixes.json`), so anything else that changed still shows.

## R-7 Checking the light theme is unchanged

**Decision**:
- **Values**: a test checks that each light value equals the literal it replaced, or its value
  from the table in R-6 for the five colours raised for contrast. A table of the
  old literals sits in the test, from R-1.
- **Screenshots**: the reference set is taken before and after, in headless Chrome over its
  debugging port as in 012's visual checks, at 1440 × 900. The light ones are compared pixel by
  pixel (SC-003); the only allowed difference is the control's area in the top bar.

## R-8 Content Fallow does not draw

The project's files and the agents' output are shown as text in Fallow's own elements (`.term`,
the file view, the markdown renderer), so they take the theme's colours. Images the engineer
attached (012) are shown as they are.
