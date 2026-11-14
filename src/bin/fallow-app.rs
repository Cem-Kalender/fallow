//! 014 (research R-18): `fallow-app.exe`, the launcher without a console: what the Start menu and
//! the installer start. It does what `fallow` does (start the service if needed, then its window)
//! and shows an error as a native message instead of on a console. Filled in by T013.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {}
