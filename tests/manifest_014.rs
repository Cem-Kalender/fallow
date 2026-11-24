//! 014 (research R-17, T002): both executables carry the application manifest: Common Controls
//! 6.0 (Quit's task dialog), per-monitor DPI awareness, Windows 10 and 11, no elevation. Test
//! binaries get no manifest, so this reads the built executables themselves.

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn both_executables_carry_the_application_manifest() {
    for exe in [
        env!("CARGO_BIN_EXE_fallow"),
        env!("CARGO_BIN_EXE_fallow-app"),
    ] {
        let bytes = std::fs::read(exe).unwrap();
        let has = |s: &str| bytes.windows(s.len()).any(|w| w == s.as_bytes());
        for part in [
            "Microsoft.Windows.Common-Controls",
            "PerMonitorV2",
            "{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}",
            "asInvoker",
        ] {
            assert!(has(part), "{exe} has no {part:?} in its manifest");
        }
    }
}

/// The manifest is the one Windows applies: `fallow.exe` starts and answers with it (a
/// manifest Windows rejects stops the program before `main`, error 14001).
#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn fallow_starts_with_its_manifest() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_fallow"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
}
