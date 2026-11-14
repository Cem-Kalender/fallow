//! 014 (research R-13): whether this process can show a window: on Windows, whether it runs on
//! the interactive window station (`WinSta0`); over SSH or as a Windows service it does not.

/// Whether this process can show a window to the engineer.
#[cfg(windows)]
pub fn interactive() -> bool {
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

/// Other platforms: the page opens in the browser, as 006 has it.
#[cfg(not(windows))]
pub fn interactive() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_runs_on_a_window_station_or_says_it_does_not() {
        // On a desktop this is true; on a CI service account it may not be. Either way it answers.
        let _ = super::interactive();
    }
}
