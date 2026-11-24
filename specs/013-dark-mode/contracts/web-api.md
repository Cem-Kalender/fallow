# Contract: web API changes (013)

These changes go on top of the web API contracts of 007 to 012. The general rules stay the same:
- every reply is `{ok:true, value}` or `{ok:false, error}` (409);
- the per-launch secret is required;
- non-GET requests need the same origin.

## The page

| Request | Answer |
|---|---|
| `GET /` | `index.html` as today, with the engineer's choice on `<html>`: `data-theme="light"` or `data-theme="dark"`, and no attribute for System (research R-3). Nothing else in the page changes. |

## The theme choice

| Request | Body | Answer |
|---|---|---|
| `POST /api/theme` | `{theme: "system" \| "light" \| "dark"}` | `{theme}`. A value outside the three is refused (409 "the theme is system, light or dark"); a write that fails answers 409 with the reason. |

The page:
- reads the current choice from `<html>` (no attribute means System);
- sets the attribute itself at once when the engineer chooses, before the answer arrives;
- on a 409, keeps what was chosen until reload and shows the error line.

## The control (top bar)

- **Markup**: `<fieldset class="theme-pick">` with `<legend class="sr">Theme</legend>`, then three
  `<input type="radio" name="theme">`, each with a `<label>`. The labels show icons for System, Light
  and Dark, each with a visually hidden name and a `title`.
- **Use**:
  - Tab reaches the group, and the arrow keys move the choice;
  - the chosen one is drawn as the active segment, with a visible focus outline;
  - choosing calls `POST /api/theme`.
- **Place**: the top bar, after `#header-status`, so it is in the same place on every screen.
