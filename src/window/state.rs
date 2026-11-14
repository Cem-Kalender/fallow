//! 014 (research R-8, data-model "Window state"): where the window was, kept in `window.json` in
//! Fallow's data folder, and where it opens: there again when a screen still shows it, otherwise
//! centred on the main screen.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The restored (not maximised) rectangle, in physical pixels of the virtual screen, and whether
/// the window was maximised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

/// The smallest window kept (data-model: at least 800 × 500).
pub const MIN: (i32, i32) = (800, 500);
/// The window's size when it has no place yet.
pub const DEFAULT: (i32, i32) = (1280, 800);

/// A screen's work area (the screen without the taskbar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

const FILE: &str = "window.json";

/// The kept state, or `None` when there is none or it cannot be read.
pub fn load(data: &Path) -> Option<WindowState> {
    let text = std::fs::read_to_string(data.join(FILE)).ok()?;
    let s: WindowState = serde_json::from_str(&text).ok()?;
    Some(WindowState {
        width: s.width.max(MIN.0),
        height: s.height.max(MIN.1),
        ..s
    })
}

/// Keeps the state; a failure only loses the place.
pub fn save(data: &Path, s: &WindowState) -> Result<(), String> {
    let text = serde_json::to_string(s).map_err(|e| e.to_string())?;
    std::fs::write(data.join(FILE), text)
        .map_err(|e| format!("cannot keep the window's place: {e}"))
}

/// Where the window opens: `saved` when a screen still shows it (`on_screen`), otherwise the
/// default size, times `scale` (the screen's DPI over 96), fitted to the main screen's work area
/// `main`, centred, not maximised.
pub fn initial(
    saved: Option<WindowState>,
    on_screen: impl Fn(&WindowState) -> bool,
    main: Area,
    scale: f64,
) -> WindowState {
    if let Some(s) = saved.filter(|s| on_screen(s)) {
        return s;
    }
    let (aw, ah) = (main.right - main.left, main.bottom - main.top);
    let px = |v: i32| (f64::from(v) * scale.max(1.0)).round() as i32;
    let width = px(DEFAULT.0).min(aw).max(px(MIN.0).min(aw));
    let height = px(DEFAULT.1).min(ah).max(px(MIN.1).min(ah));
    WindowState {
        x: main.left + (aw - width) / 2,
        y: main.top + (ah - height) / 2,
        width,
        height,
        maximized: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Area = Area {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    #[test]
    fn a_kept_place_on_a_screen_is_used_and_otherwise_the_window_is_centred() {
        let kept = WindowState {
            x: 2000,
            y: 100,
            width: 1000,
            height: 700,
            maximized: true,
        };
        assert_eq!(initial(Some(kept), |_| true, MAIN, 1.0), kept);
        let centred = initial(Some(kept), |_| false, MAIN, 1.0);
        assert_eq!(
            centred,
            WindowState {
                x: 320,
                y: 120,
                width: 1280,
                height: 800,
                maximized: false
            }
        );
        assert_eq!(initial(None, |_| true, MAIN, 1.0), centred);
        // At 150 % the window is 1.5 times larger, still fitted.
        let big = initial(None, |_| true, MAIN, 1.5);
        assert_eq!((big.width, big.height), (1920, 1040));
    }

    #[test]
    fn a_small_screen_gets_a_window_that_fits() {
        let small = Area {
            left: 0,
            top: 0,
            right: 1024,
            bottom: 728,
        };
        let s = initial(None, |_| true, small, 1.0);
        assert_eq!((s.width, s.height), (1024, 728));
        assert_eq!((s.x, s.y), (0, 0));
    }

    #[test]
    fn the_place_is_kept_and_read_back_and_bad_files_are_ignored() {
        let dir = std::env::temp_dir().join(format!("fallow-window-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir), None);
        let s = WindowState {
            x: -1200,
            y: 40,
            width: 900,
            height: 600,
            maximized: false,
        };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), Some(s));
        // Too small is grown to the smallest window.
        std::fs::write(
            dir.join(FILE),
            r#"{"x":0,"y":0,"width":10,"height":10,"maximized":false}"#,
        )
        .unwrap();
        assert_eq!(load(&dir).map(|s| (s.width, s.height)), Some(MIN));
        std::fs::write(dir.join(FILE), "not json").unwrap();
        assert_eq!(load(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
