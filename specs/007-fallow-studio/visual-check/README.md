# Visual check (T085; SC-007, SC-008)

> The screenshots (`.png`) were removed from the repository on 2026-11-14, before it became public. The `.html` mockups and the check scripts that make them are kept; run them to take the screenshots again locally.

Each screen at 1440 px and 390 px, taken on 2026-10-31 from a debug build, with two demo
projects whose loops run on the scripted fake agent (one waits for an answer). They are taken
with headless Chrome through its DevTools protocol, at full page height. None of them scrolls
sideways at 390 px.

| Screen | Mockup | 1440 px | 390 px |
|---|---|---|---|
| 1. Dashboard | `mockup/Main.dc.html` | `dashboard-1440.png` | `dashboard-390.png` |
| 2. Spec session | `mockup/Spec.dc.html` | `spec-1440.png` | `spec-390.png` |
| 3. Loop | `mockup/Run.dc.html` | `loop-1440.png` | `loop-390.png` |
| 4. Pipeline designer | `mockup/Pipeline.dc.html` | `pipelines-1440.png` | `pipelines-390.png` |
| 5. Project | `mockup/Project.dc.html` | `project-1440.png` | `project-390.png` |

Where the screens knowingly differ from the mockups:
- **Pipeline designer:**
  - Each step has Change, ↑, ↓ and Remove buttons. The pipeline's name can be edited, there is a
    project selector, and there is a TOML view.
  - The definition shows `kind:` where the mockup showed `edits:`, as contracts/agent-definition.md
    has it.
  - The gates may be removed, with the "not verified by Fallow" warning (FR-015). The mockup said
    Fallow's checks could not be removed.
- **Spec session:** the document's line numbers are darker than the mockup's #A3A29B, which has
  only 2.6:1 contrast (FR-032 asks for 4.5:1).

**Approval:** pending. The engineer compares each screen with its mockup and approves (T085).
