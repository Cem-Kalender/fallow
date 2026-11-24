//! 014 (FR-009, research R-9, T017): Quit's confirmation: the work that would pause, named; asked
//! only when there is any.

use serde_json::Value;

/// The lines naming the work that would pause, from `Request::Running`'s answer.
pub fn names(running: &Value) -> Vec<String> {
    running
        .as_array()
        .map(|a| {
            a.iter()
                .map(|w| {
                    let kind = match w["kind"].as_str() {
                        Some("loop") => "Loop",
                        Some("session") => "Session",
                        _ => "Conversation",
                    };
                    let name = w["name"].as_str().unwrap_or("(unnamed)");
                    match w["project"].as_str() {
                        Some(p) if !p.is_empty() => format!("{kind} {name} ({p})"),
                        _ => format!("{kind} {name}"),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The confirmation's text for `names`.
pub fn text(names: &[String]) -> String {
    format!(
        "{}\n\nThey pause now and resume where they were when Fallow starts again.",
        names
            .iter()
            .map(|n| format!("\u{2022} {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Asks the engineer whether to quit with `names` running (a task dialog, research R-17's
/// manifest gives it), owned by Fallow's window when it is open (`owner`, its handle, or 0): the window
/// waits for it. `true` on Quit; `false` on Cancel. Without the task dialog, a plain message box
/// asks the same.
#[cfg(windows)]
pub fn confirm(owner: isize, names: &[String]) -> bool {
    let owner = owner as windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::UI::Controls::{
        TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TD_WARNING_ICON, TDCBF_CANCEL_BUTTON,
        TDF_ALLOW_DIALOG_CANCELLATION,
    };
    const QUIT: i32 = 100;
    type TaskDialogIndirect =
        unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut i32) -> i32;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let title = wide("Fallow");
    let main = wide("Quit Fallow? This work is running:");
    let content = wide(&text(names));
    let quit_label = wide("Quit");
    let buttons = [TASKDIALOG_BUTTON {
        nButtonID: QUIT,
        pszButtonText: quit_label.as_ptr(),
    }];
    // Looked up when needed, not imported: only a program with Common Controls 6 in its
    // manifest has it, and the test programs have none (they would not start).
    // SAFETY: `TaskDialogIndirect`'s documented signature; every pointer is live for the call.
    unsafe {
        let lib = wide("comctl32.dll");
        let module = LoadLibraryW(lib.as_ptr());
        let found = if module.is_null() {
            None
        } else {
            GetProcAddress(module, c"TaskDialogIndirect".as_ptr().cast())
        };
        let Some(f) = found else {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MessageBoxW,
            };
            let all = wide(&format!(
                "Quit Fallow? This work is running:\n\n{}",
                text(names)
            ));
            return MessageBoxW(
                owner,
                all.as_ptr(),
                title.as_ptr(),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
            ) == IDYES;
        };
        let f: TaskDialogIndirect = std::mem::transmute(f);
        let mut cfg = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: owner,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
            dwCommonButtons: TDCBF_CANCEL_BUTTON,
            pszWindowTitle: title.as_ptr(),
            pszMainInstruction: main.as_ptr(),
            pszContent: content.as_ptr(),
            cButtons: buttons.len() as u32,
            pButtons: buttons.as_ptr(),
            nDefaultButton: 2, // IDCANCEL: Enter does not quit by accident
            ..Default::default()
        };
        cfg.Anonymous1.pszMainIcon = TD_WARNING_ICON;
        let mut pressed = 0;
        let hr = f(
            &cfg,
            &mut pressed,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        hr == 0 && pressed == QUIT
    }
}

/// 016 (research M-11, data-model "Quit, on macOS", T020): AppKit's question whether Fallow may
/// end now (`applicationShouldTerminate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminate {
    /// End now.
    Now,
    /// Not yet: Quit is asked first (the page, then the running work).
    Cancel,
}

/// `Now` when the engine has stopped (Quit's own end) or the session is ending (logout, restart,
/// shutdown: never blocked, no dialog); `Cancel` otherwise, and Quit runs.
pub fn terminate_reply(power_off: bool, stopped: bool) -> Terminate {
    if power_off || stopped {
        Terminate::Now
    } else {
        Terminate::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_running_work_is_named_by_kind_name_and_project() {
        let running = json!([
            {"kind": "loop", "id": 3, "project": "shop", "name": "012-restock"},
            {"kind": "session", "id": "c1", "project": "shop", "name": "fix the week"},
            {"kind": "conversation", "id": "c2", "project": null, "name": "c2"},
        ]);
        let n = names(&running);
        assert_eq!(
            n,
            vec![
                "Loop 012-restock (shop)",
                "Session fix the week (shop)",
                "Conversation c2"
            ]
        );
        let t = text(&n);
        assert!(t.contains("\u{2022} Loop 012-restock (shop)"));
        assert!(t.contains("resume where they were"));
        assert!(names(&json!([])).is_empty());
    }

    #[test]
    fn appkit_may_end_fallow_only_once_stopped_or_at_the_sessions_end() {
        assert_eq!(terminate_reply(false, false), Terminate::Cancel);
        assert_eq!(terminate_reply(false, true), Terminate::Now);
        assert_eq!(terminate_reply(true, false), Terminate::Now);
        assert_eq!(terminate_reply(true, true), Terminate::Now);
    }
}
