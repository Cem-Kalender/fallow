//! 014 (research R-13), 016 (research M-12): whether this process can show a window: on Windows,
//! whether it runs on the interactive window station (`WinSta0`); on macOS, whether its security
//! session has access to the window server. Over SSH, as a Windows service or as a launchd daemon
//! it does not.

/// Whether this process can show a window to the engineer.
#[cfg(windows)]
pub fn interactive() -> bool {
    if said_none() {
        return false;
    }
    use windows_sys::Win32::System::StationsAndDesktops::{
        GetProcessWindowStation, GetUserObjectInformationW, UOI_NAME,
    };
    let mut name = [0u16; 64];
    let mut needed = 0u32;
    // SAFETY: a query into a live buffer of the given size; the station handle needs no close.
    let ok = unsafe {
        let ws = GetProcessWindowStation();
        !ws.is_null()
            && GetUserObjectInformationW(
                ws,
                UOI_NAME,
                name.as_mut_ptr().cast(),
                (name.len() * 2) as u32,
                &mut needed,
            ) != 0
    };
    let n = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    ok && String::from_utf16_lossy(&name[..n]).eq_ignore_ascii_case("WinSta0")
}

/// 016 (T040): `FALLOW_NO_DISPLAY=1` makes Fallow behave as without a display (as over SSH), for
/// the tests of that path. It can only take the window away, never give one.
fn said_none() -> bool {
    std::env::var_os("FALLOW_NO_DISPLAY").is_some_and(|v| v == "1")
}

/// Whether this process can show a window to the engineer: its security session (the login
/// session it belongs to) has graphic access (`SessionGetInfo`, Security framework). An SSH login
/// gets a session without it.
#[cfg(target_os = "macos")]
pub fn interactive() -> bool {
    if said_none() {
        return false;
    }
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SessionGetInfo(session: u32, session_id: *mut u32, attributes: *mut u32) -> i32;
    }
    /// `callerSecuritySession`: the caller's own session.
    const CALLER: u32 = u32::MAX;
    /// `sessionHasGraphicAccess`.
    const GRAPHIC_ACCESS: u32 = 0x0010;
    let mut id = 0u32;
    let mut attributes = 0u32;
    // SAFETY: a query into two live integers.
    let status = unsafe { SessionGetInfo(CALLER, &mut id, &mut attributes) };
    status == 0 && attributes & GRAPHIC_ACCESS != 0
}

/// Other platforms: the page opens in the browser, as 006 has it.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn interactive() -> bool {
    !said_none()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_runs_on_a_window_station_or_says_it_does_not() {
        // On a desktop this is true; on a CI service account it may not be. Either way it answers.
        let _ = super::interactive();
    }
}
