//! 014 (research R-7, R-8, T010): the Win32 window around the web view: Fallow's name and icon,
//! 013's theme on its frame and background from the first frame, where it was last time, and its
//! messages handed to `super::host`.

use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetMonitorInfoW, HDC, InvalidateRect,
    MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
    MonitorFromRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::state::{Area, WindowState};

/// 013's page background (`--ground`): light and dark.
pub const LIGHT: u32 = 0x00F3_F6F6; // COLORREF is 0x00BBGGRR: #F6F6F3
pub const DARK: u32 = 0x0014_1616; // #161614

/// The background painted before the web view's first frame.
static BACKGROUND: AtomicU32 = AtomicU32::new(LIGHT);

const CLASS: &str = "FallowWindow";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Whether Windows' apps are set to dark (`AppsUseLightTheme` = 0).
pub fn system_dark() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reading one DWORD into a live buffer of its size.
    let e = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    e == 0 && value == 0
}

/// Whether 013's choice (`system`, `light`, `dark`) means dark now.
pub fn is_dark(theme: &str) -> bool {
    match theme {
        "dark" => true,
        "light" => false,
        _ => system_dark(),
    }
}

/// The frame and the background in the theme (research R-7).
pub fn set_dark(hwnd: HWND, dark: bool) {
    BACKGROUND.store(if dark { DARK } else { LIGHT }, Ordering::Relaxed);
    let on: i32 = i32::from(dark);
    // SAFETY: setting one documented attribute of this thread's window, and repainting it.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&raw const on).cast(),
            std::mem::size_of::<i32>() as u32,
        );
        InvalidateRect(hwnd, std::ptr::null(), 1);
    }
}

/// Whether a screen shows (part of) the rectangle.
pub fn on_screen(s: &WindowState) -> bool {
    let r = RECT {
        left: s.x,
        top: s.y,
        right: s.x + s.width,
        bottom: s.y + s.height,
    };
    // SAFETY: a plain query.
    !unsafe { MonitorFromRect(&r, MONITOR_DEFAULTTONULL) }.is_null()
}

/// The main screen's work area.
pub fn main_area() -> Area {
    // SAFETY: plain queries into a live, sized structure.
    unsafe {
        let m = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(m, &mut info);
        let w = info.rcWork;
        Area {
            left: w.left,
            top: w.top,
            right: w.right,
            bottom: w.bottom,
        }
    }
}

/// Where the window is now: its restored rectangle and whether it is maximised.
pub fn placement(hwnd: HWND) -> Option<WindowState> {
    // SAFETY: a plain query into a live, sized structure.
    unsafe {
        let mut p: WINDOWPLACEMENT = std::mem::zeroed();
        p.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if GetWindowPlacement(hwnd, &mut p) == 0 {
            return None;
        }
        let r = p.rcNormalPosition;
        Some(WindowState {
            x: r.left,
            y: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
            maximized: p.showCmd == SW_SHOWMAXIMIZED as u32,
        })
    }
}

/// The client area's size, in physical pixels.
pub fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a plain query.
    unsafe { GetClientRect(hwnd, &mut r) };
    (
        (r.right - r.left).max(0) as u32,
        (r.bottom - r.top).max(0) as u32,
    )
}

/// Fallow's icon from its resources, at `size` pixels.
fn icon(size: i32) -> HICON {
    // SAFETY: resource 1 of this module is Fallow's icon group (build.rs).
    unsafe {
        let h = LoadImageW(
            GetModuleHandleW(std::ptr::null()),
            std::ptr::without_provenance::<u16>(1),
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        );
        if h.is_null() {
            LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
        } else {
            h
        }
    }
}

/// Makes and shows the window at `s`, in the theme, in front.
pub fn create(s: &WindowState, dark: bool) -> Result<HWND, String> {
    let class = wide(CLASS);
    let title = wide("Fallow");
    BACKGROUND.store(if dark { DARK } else { LIGHT }, Ordering::Relaxed);
    // SAFETY: plain Win32 calls on this, the UI thread, with live wide strings; the class is
    // registered once (a second registration fails harmlessly).
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc_),
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: icon(GetSystemMetrics(SM_CXICON)),
            hIconSm: icon(GetSystemMetrics(SM_CXSMICON)),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            s.x,
            s.y,
            s.width,
            s.height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err("Fallow's window could not be made".into());
        }
        set_dark(hwnd, dark);
        ShowWindow(
            hwnd,
            if s.maximized {
                SW_SHOWMAXIMIZED
            } else {
                SW_SHOWNORMAL
            },
        );
        front(hwnd);
        Ok(hwnd)
    }
}

/// Brings the window to the front, restoring it when minimised.
pub fn front(hwnd: HWND) {
    // SAFETY: plain calls on this thread's window.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            let mut r = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            // SAFETY: painting this window's background on the DC Windows gave.
            unsafe {
                GetClientRect(hwnd, &mut r);
                let brush = CreateSolidBrush(BACKGROUND.load(Ordering::Relaxed));
                FillRect(w as HDC, &r, brush);
                DeleteObject(brush);
            }
            1
        }
        WM_SIZE => {
            super::host::resized(hwnd);
            0
        }
        WM_DPICHANGED => {
            // The suggested rectangle for the new screen's scale (research R-8).
            // SAFETY: `l` points to a RECT for this message.
            unsafe {
                let r = &*(l as *const RECT);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            0
        }
        WM_SETTINGCHANGE => {
            // SAFETY: for this message `l` is null or a NUL-terminated wide string.
            let what = unsafe { wide_at(l as *const u16) };
            if what == "ImmersiveColorSet" {
                super::host::system_theme_changed(hwnd);
            }
            // SAFETY: the default handling.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        WM_CLOSE => {
            super::host::close_asked(hwnd);
            0
        }
        WM_TIMER if w == super::host::CLOSE_TIMER => {
            super::host::close_timer(hwnd);
            0
        }
        WM_DESTROY => {
            super::host::destroyed(hwnd);
            0
        }
        // SAFETY: the default handling.
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

/// A NUL-terminated wide string, or empty.
///
/// # Safety
/// `p` is null or a NUL-terminated wide string.
unsafe fn wide_at(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    // SAFETY: as the caller promises.
    unsafe {
        while *p.add(n) != 0 && n < 256 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }
}
