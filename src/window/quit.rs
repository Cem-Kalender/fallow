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
/// manifest gives it). `true` on Quit; `false` on Cancel or when the dialog cannot be shown.
#[cfg(windows)]
pub fn confirm(names: &[String]) -> bool {
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
        if module.is_null() {
            return false;
        }
        let Some(f) = GetProcAddress(module, c"TaskDialogIndirect".as_ptr().cast()) else {
            return false;
        };
        let f: TaskDialogIndirect = std::mem::transmute(f);
        let mut cfg = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
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
}
