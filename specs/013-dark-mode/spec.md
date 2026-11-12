# Feature Specification: Dark mode

**Feature Branch**: `001-llm-aware-tui-editor` (the project's working branch; the spec lives in `specs/013-dark-mode`)

**Created**: 2026-11-02

**Status**: Draft; reference set updated on 2026-11-12 for 012 (solo sessions)

**Input**: User description: "Dark mode for Fallow Studio (013; first numbered 010, then 012), agreed with the engineer on 2026-11-02 after the mockup (`mockup/dark.html`, 'looks absolutely perfect'). Small feature, built on 009 as it is. A dark theme with the look of the mockup; by default Fallow follows the system's light or dark setting and switches with it without a reload; the engineer can choose Light, Dark or System, remembered on that machine across restarts; every screen and state is covered; text and controls meet WCAG AA contrast in both themes; no flash of the light theme on opening or reloading; the light theme does not change. Out of scope: more themes or custom colours, the standalone app (014), changing the layout."

## Context

Fallow's page has one look: light, warm and paper-like. The engineer works with the system in dark mode, and Fallow is the one bright window. On 2026-11-02 the engineer asked whether dark mode is easy, and approved a mockup: the engineer's own Fallow, six screens taken as they are and with a draft dark palette laid over them (`mockup/dark.html`, palette in `mockup/dark.css`).

The approved look:

- **Ground and panels**: a warm near-black ground, panels a step lighter, lines a step lighter again; the same warmth as the light theme, not a cold or pure black.
- **Text**: off-white, with the secondary and faint text in warm greys.
- **Colours**: the accent and the status colours (needs you, done, spec, failure, warning) lighter, so they read on dark; their tinted backgrounds (badges, pills, the loop graph's phase boxes) dark tints of the same hue.
- **Solid surfaces turn light**: the primary buttons, the active tab and navigation item, the engineer's own chat bubbles and the send button are light with dark text (in the light theme they are near-black with white text).
- **Secondary buttons** stay the colour of the panel, with light text and a visible border.
- **Code, logs and terminal output** stay dark, as they already are in the light theme.

Fallow will be its own app window later (014), which will follow the system's theme through this feature.

**What does not change**: the layout, the light theme (it looks exactly as today), the content of any screen, and how Fallow works.

## Clarifications

### Session 2026-11-12

- Q: Today's light theme misses WCAG AA in a few places: control borders (1.5:1), graph lines and step arrows (about 2.6:1), the grey status dot (1.7:1), and faint text on a hovered row (4.35:1). FR-006 (the light theme unchanged) and FR-012 (AA in both themes) cannot both hold. Which wins? → A: Full AA in both themes. The light theme changes only where it misses FR-012: the borders of controls, graph lines, arrows and grey dots get a darker grey of the same warm hue, and faint text a shade darker. Nothing else in it changes.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Fallow follows the system's dark setting (Priority: P1)

The engineer's system is set to dark. The engineer opens Fallow and every screen is dark, readable and complete: the dashboard, a project with its conversation, a spec session, a loop, pipelines, agents, the data page, with their dialogs, menus and panels. When the engineer switches the system to light, Fallow turns light without a reload, and back.

**Why this priority**: it is the feature: one dark window among the others is the complaint, and following the system needs no choice from the engineer.

**Independent Test**: with the system (or the browser's emulated setting) dark, open every screen and state named in FR-005 and check that it is dark and that every text and control is readable; switch the setting to light and back with a screen open and check that the page follows within a second, with nothing typed or open lost.

**Acceptance Scenarios**:

1. **Given** the system is set to dark and the engineer has not chosen a theme in Fallow, **When** the engineer opens Fallow, **Then** the page is dark from the first frame it shows.
2. **Given** a project page open with a half-written message, an open side panel and the "/" menu open, **When** the system switches from dark to light, **Then** the page turns light within a second, and the message, the panel and the menu stay as they were.
3. **Given** the dark theme, **When** the engineer opens a dialog ("Leave your changes?", a confirmation), the "/" menu, a side panel, a file with its diff, a pipeline graph and a loop graph, **Then** each is dark, and every text, border, icon and focus outline in it is readable.
4. **Given** the dark theme, **When** the engineer reloads the page or Fallow restarts, **Then** no light frame is shown before the dark page.

---

### User Story 2 - The engineer chooses the theme (Priority: P2)

The engineer wants Fallow light while the system is dark (or the other way round). From any screen, the engineer chooses Light, Dark or System. The choice applies at once to the whole page, and Fallow remembers it after a restart.

**Why this priority**: following the system covers most of the need; the choice is for the engineer who wants Fallow different from the rest of the system, for example for a screen share or a bright room.

**Independent Test**: choose each of the three options from two different screens; check that the page changes at once, that a reload and a restart of Fallow keep the choice, and that "System" follows the system's setting again.

**Acceptance Scenarios**:

1. **Given** the system is dark, **When** the engineer chooses Light, **Then** the page turns light at once, and stays light when the system changes.
2. **Given** the engineer chose Dark, **When** Fallow is restarted and the page is opened at its new address, **Then** the page is dark from the first frame.
3. **Given** the engineer chose Light or Dark, **When** the engineer chooses System, **Then** the page follows the system's setting again, at once and when it changes.
4. **Given** the theme control, **When** the engineer uses it with the keyboard only, **Then** the engineer can reach it, see which option is chosen, and choose another, and a screen reader announces the control and the chosen option.

---

### User Story 3 - The light theme stays as it is (Priority: P1)

An engineer who uses Fallow in light mode sees no change at all.

**Why this priority**: the light theme was designed and approved screen by screen in 007 to 009; a dark theme must not cost it anything.

**Independent Test**: take every screen of the reference set (FR-005) in the light theme before and after the feature, and compare them pixel by pixel.

**Acceptance Scenarios**:

1. **Given** the light theme, **When** each screen of the reference set is shown, **Then** it is identical to the same screen before this feature, except for the new theme control and the colours raised to meet FR-012.

### Edge Cases

- **The system has no light or dark setting** (or the setting cannot be read): System means light.
- **Two pages of Fallow open at once**: a choice made in one applies to the other no later than its next reload; neither page loses what is typed or open.
- **The theme changes while a dialog, the "/" menu, a side panel or a drawer is open, or while the lead is answering**: they stay open, with their content, focus and scroll position; the working indicator keeps running.
- **Reduced motion**: the switch between themes has no animated transition when the system asks for reduced motion (and no long one otherwise).
- **Colours that carry meaning** (stages, statuses, the loop graph's legend, diffs' added and removed lines): each stays distinguishable from the others in the dark theme, as in the light one, and never depends on colour alone where it does not today.
- **Content Fallow does not draw** (the project's own files shown as text, agents' output): shown in Fallow's own colours for text and code, so it is readable in both themes.
- **Fallow's saved choice cannot be read or written** (for example the data directory is read-only): the page follows the system and says nothing alarming; choosing still applies to the page until it is reloaded.
- **Scrollbars and form controls drawn by the browser** (selects, checkboxes, scrollbars) follow the theme as far as the browser allows; they never stay bright white in the dark theme.

## Requirements *(mandatory)*

### Functional Requirements

**The dark theme**

- **FR-001**: Fallow's page MUST have a dark theme with the approved look of `mockup/dark.html` and the palette of `mockup/dark.css`: a warm near-black ground, panels and lines a step lighter, off-white text with warm greys for secondary text, lighter accent and status colours on dark tints of their hue.
- **FR-002**: In the dark theme, the solid surfaces MUST be light with dark text: the primary buttons, the active navigation item and tab, the engineer's own chat bubbles, the send button and the count badges on the side rail. Secondary buttons MUST keep the panel's colour with light text and a visible border.
- **FR-003**: Code, logs and terminal output MUST stay dark in both themes, as today.
- **FR-004**: Every colour the page shows MUST come from the theme, so that nothing stays light in the dark theme: no text, background, border, line, icon, graph edge, shadow or backdrop keeps a light-theme colour.

**Coverage**

- **FR-005**: The dark theme MUST cover every screen and state of the page. The reference set, checked in both themes, is at least:
  - the dashboard (with and without a running loop and something that needs the engineer), and the data page;
  - a project page with its solo session (012): the session dropdown with "+ New session" and the closed sessions, the conversation with a turn's activity, an attached image and waiting messages, an approval card (a command, a read) with the folder's facts, the "/" menu, the side panels (Changes, Gates, Branches, proposals, what the assistant knows), a suggested feature card started and not started, the "Make this a feature" and close dialogs, a closed session's read-only view, and the Files tab with a file and its diff;
  - a spec session with its documents, the step bar, the lock area and the Files drawer;
  - a loop page with its graph in each step state, the activity, the tasks, the gates and the commits;
  - Pipelines (list, graph, selected step, TOML editor) and Agents (list, form, instructions);
  - the dialogs ("Leave your changes?", confirmations), alerts and error lines, empty states, and the error shown when a screen cannot be loaded;
  - form fields (text, select, checkbox), focus outlines, the working indicator, badges, pills, stage labels and tooltips.
- **FR-006**: The light theme MUST stay exactly as it is today: every screen of the reference set looks the same as before this feature, except for the new theme control and the colours raised to meet FR-012 (Clarifications, 2026-11-12).

**Choosing the theme**

- **FR-007**: By default (the System choice), Fallow MUST follow the system's light or dark setting, and MUST change theme when that setting changes, without a reload and within one second.
- **FR-008**: The engineer MUST be able to choose Light, Dark or System from every screen, with one control in the same place on each. The choice MUST apply to the whole page at once.
- **FR-009**: The choice MUST be remembered on that machine for Fallow, across reloads and restarts of Fallow (whose page address changes at every start), until the engineer changes it. System is the default when nothing was chosen.
- **FR-010**: The theme control MUST be usable with the keyboard alone, MUST show which option is chosen, and MUST be announced with its name and chosen option by a screen reader.
- **FR-011**: Changing the theme, by the engineer or by the system, MUST NOT lose or reset anything on the page: what is typed, what is open (dialogs, menus, panels, drawers, files), the focus, the scroll positions and running indicators.

**Readability and comfort**

- **FR-012**: In both themes, normal text MUST have a contrast ratio of at least 4.5:1 against its background, and large text (at least 18.66 px bold or 24 px), icons, borders of controls, focus outlines and the parts of graphs that carry meaning at least 3:1.
- **FR-013**: Colours that tell things apart (stages, statuses, the graph's legend, added and removed lines) MUST stay distinguishable from each other in the dark theme, and MUST NOT be the only way to tell them apart where the light theme does not rely on colour alone.
- **FR-014**: Opening or reloading the page in the dark theme MUST NOT show a light frame first, on any screen.
- **FR-015**: Switching themes MUST NOT animate when the system asks for reduced motion; otherwise any transition MUST be short (at most 200 ms).

### Key Entities

- **Theme choice**: the engineer's choice of Light, Dark or System; one per machine for Fallow; System when nothing was chosen.
- **Theme**: the set of colours the page is drawn with, light or dark; the shown theme is the choice, or the system's setting when the choice is System.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In the dark theme, 100% of the screens and states of the reference set (FR-005) show no area, text or line left in a light-theme colour, checked on screenshots of each.
- **SC-002**: 100% of the text and control colour pairs used by the page pass the contrast thresholds of FR-012 in both themes, checked automatically for every pair.
- **SC-003**: In the light theme, the screenshots of the reference set before and after the feature differ only in the new theme control and in the colours raised to meet FR-012.
- **SC-004**: When the system setting changes, the page shows the other theme within 1 second, with nothing typed or open lost, on every screen of the reference set.
- **SC-005**: Over 20 reloads in the dark theme, no captured frame shows the light ground.
- **SC-006**: The engineer's theme choice survives 3 restarts of Fallow in a row.
- **SC-007**: The engineer can switch the theme in at most 2 actions from any screen.

## Assumptions

- The approved mockup's palette (`mockup/dark.css`) is the starting point; individual colours may be adjusted to pass FR-012 and FR-013, without changing the look the engineer approved.
- The theme control sits in the top bar, next to the day's cost, so that it is on every screen; its exact form (one button that cycles, or a small menu of three) is settled in the plan, within FR-008 to FR-010.
- Fallow's page address (port and key) changes at every start, so the choice cannot rely on what the browser keeps for one address; Fallow keeps it with its own settings on the machine (as it keeps other choices such as desktop notifications). It holds only one of three fixed values and is local, in line with principle V.
- No new dependency is needed: the page already draws every colour from a shared set of named colours, and the system's setting is available to the page.
- Desktop notifications are drawn by the operating system and follow its theme; they are not part of this feature.
- The standalone app (014) will use the same themes and the System choice; nothing in this feature depends on it.
