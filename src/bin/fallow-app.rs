//! 014 (research R-18): `fallow-app.exe`, the launcher without a console: what the Start menu and
//! the installer start. It does what `fallow` does (start the service if needed, then its window)
//! and shows an error as a native message instead of on a console.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    let said = if fallow::platform::display::interactive() {
        fallow::service::open_window(None).err()
    } else {
        Some("Fallow's window needs a desktop.".to_string())
    };
    if let Some(e) = said {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let text: Vec<u16> = format!("Fallow could not open: {e}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let title: Vec<u16> = "Fallow".encode_utf16().chain(Some(0)).collect();
        // SAFETY: a modal message box with live wide strings.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        };
        std::process::exit(1);
    }
}

/// Elsewhere there is no launcher: on macOS `Fallow.app` itself is the app (016 research M-3),
/// and on Linux `fallow` opens the page.
#[cfg(not(windows))]
fn main() {
    eprintln!("fallow-app: the launcher is for Windows only; run `fallow`");
    std::process::exit(1);
}
