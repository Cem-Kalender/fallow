//! 014 (research R-8, data-model "Window state"), 016 (research M-13): where the window was, kept in `window.json` in
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

/// 016 (research M-13, data-model "Window state"): a rectangle in macOS's points, in its global
/// coordinates (origin at the bottom left of the main screen).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// 016: where the window was on macOS: its frame, the frame of the screen it was on, and whether
/// it was zoomed or in full screen. A `window.json` with Windows' fields reads as missing here,
/// and this one as missing on Windows (one file per platform's fields).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MacPlace {
    pub frame: Rect,
    pub screen: Rect,
    pub zoomed: bool,
    pub full_screen: bool,
}

/// 016: the kept macOS place, or `None` when there is none, it cannot be read, or it holds
/// another platform's fields.
pub fn load_mac(data: &Path) -> Option<MacPlace> {
    let text = std::fs::read_to_string(data.join(FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// 016: keeps the macOS place; a failure only loses the place.
pub fn save_mac(data: &Path, p: &MacPlace) -> Result<(), String> {
    let text = serde_json::to_string(p).map_err(|e| e.to_string())?;
    std::fs::write(data.join(FILE), text)
        .map_err(|e| format!("cannot keep the window's place: {e}"))
}

/// 016 (FR-003): where the window opens on macOS: `saved` when one of the current `screens`
/// contains its frame's centre, grown to at least 800 × 500; otherwise 1280 × 800 (fitted to it)
/// centred on the main screen's visible frame `main_visible`, neither zoomed nor in full screen.
pub fn mac_opening(saved: Option<MacPlace>, screens: &[Rect], main_visible: Rect) -> MacPlace {
    let min = (f64::from(MIN.0), f64::from(MIN.1));
    if let Some(s) = saved {
        let (cx, cy) = (
            s.frame.x + s.frame.width / 2.0,
            s.frame.y + s.frame.height / 2.0,
        );
        if screens.iter().any(|r| r.contains(cx, cy)) {
            return MacPlace {
                frame: Rect {
                    width: s.frame.width.max(min.0),
                    height: s.frame.height.max(min.1),
                    ..s.frame
                },
                ..s
            };
        }
    }
    let m = main_visible;
    let width = f64::from(DEFAULT.0).min(m.width).max(min.0.min(m.width));
    let height = f64::from(DEFAULT.1).min(m.height).max(min.1.min(m.height));
    MacPlace {
        frame: Rect {
            x: m.x + ((m.width - width) / 2.0).round(),
            y: m.y + ((m.height - height) / 2.0).round(),
            width,
            height,
        },
        screen: m,
        zoomed: false,
        full_screen: false,
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

    const MAC_MAIN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 944.0,
    };

    fn mac_kept() -> MacPlace {
        MacPlace {
            frame: Rect {
                x: 1600.0,
                y: 100.0,
                width: 1000.0,
                height: 700.0,
            },
            screen: Rect {
                x: 1512.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            zoomed: true,
            full_screen: false,
        }
    }

    #[test]
    fn on_macos_a_kept_frame_on_a_current_screen_is_used() {
        let kept = mac_kept();
        let screens = [MAC_MAIN, kept.screen];
        assert_eq!(mac_opening(Some(kept), &screens, MAC_MAIN), kept);
    }

    #[test]
    fn on_macos_without_a_place_or_its_screen_the_window_is_centred() {
        let centred = MacPlace {
            frame: Rect {
                x: 116.0,
                y: 72.0,
                width: 1280.0,
                height: 800.0,
            },
            screen: MAC_MAIN,
            zoomed: false,
            full_screen: false,
        };
        assert_eq!(mac_opening(None, &[MAC_MAIN], MAC_MAIN), centred);
        // The second screen is gone: centred on the main one, not zoomed.
        assert_eq!(
            mac_opening(Some(mac_kept()), &[MAC_MAIN], MAC_MAIN),
            centred
        );
        // A small screen gets a window that fits.
        let small = Rect {
            x: 0.0,
            y: 25.0,
            width: 1024.0,
            height: 700.0,
        };
        let s = mac_opening(None, &[small], small);
        assert_eq!(
            (s.frame.x, s.frame.y, s.frame.width, s.frame.height),
            (0.0, 25.0, 1024.0, 700.0)
        );
    }

    #[test]
    fn on_macos_a_kept_frame_is_at_least_the_smallest_window() {
        let mut kept = mac_kept();
        kept.frame = Rect {
            x: 1700.0,
            y: 200.0,
            width: 300.0,
            height: 200.0,
        };
        let o = mac_opening(Some(kept), &[MAC_MAIN, kept.screen], MAC_MAIN);
        assert_eq!((o.frame.width, o.frame.height), (800.0, 500.0));
        assert_eq!((o.frame.x, o.frame.y), (1700.0, 200.0));
    }

    #[test]
    fn each_platform_reads_only_its_own_fields() {
        let dir = std::env::temp_dir().join(format!("fallow-window-mac-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load_mac(&dir), None);
        save_mac(&dir, &mac_kept()).unwrap();
        assert_eq!(load_mac(&dir), Some(mac_kept()));
        // A macOS file is missing for Windows, and a Windows file for macOS.
        assert_eq!(load(&dir), None);
        save(
            &dir,
            &WindowState {
                x: 0,
                y: 0,
                width: 900,
                height: 600,
                maximized: false,
            },
        )
        .unwrap();
        assert_eq!(load_mac(&dir), None);
        std::fs::write(dir.join(FILE), "not json").unwrap();
        assert_eq!(load_mac(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
