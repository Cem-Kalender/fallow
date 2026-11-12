# Data model: Dark mode (013)

## Theme choice

- **Where**: `prefs.json` in Fallow's data directory, the file that also holds the editor (012).
  It is local to the machine and readable by the current user only, like the rest of the data
  directory (constitution V).
- **Field**: `theme`, one of `"system"`, `"light"`, `"dark"`.
  - It is missing when nothing was chosen.
  - Missing, another value, or an unreadable file all mean `system`.
- **Written**: when the engineer chooses (`POST /api/theme`), through a temporary file and a rename,
  as `prefs.json` is written today. The other fields of the file are kept.
- **Read**: each time the page's HTML is served.

## Theme

The set of named colours the page is drawn with, defined in `src/service/web/app.css`.
- **Light**: the `:root` values, today's colours exactly.
- **Dark**: the same names, redefined, in two identical blocks (research R-2).
- **The shown theme**:
  - `data-theme="dark"` on `<html>`: dark;
  - `data-theme="light"`: light;
  - no attribute (System): the system's setting, or light when the system has none.

| Choice | `<html>` as served | Shown |
|---|---|---|
| system (or none) | no `data-theme` | the system's setting |
| light | `data-theme="light"` | light |
| dark | `data-theme="dark"` | dark |
