//! 016 (T014, research M-11): Fallow's native messages on macOS (`NSAlert`), asked app-modally on
//! the main thread.

use std::cell::Cell;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSApplication, NSModalResponse, NSWindow,
};
use objc2_foundation::NSString;

/// Asks with `buttons` (the first is the default); the index of the one chosen, or `None` when
/// the alert was ended otherwise (Fallow stopping meanwhile).
pub fn ask(
    mtm: MainThreadMarker,
    style: NSAlertStyle,
    title: &str,
    text: &str,
    buttons: &[&str],
) -> Option<usize> {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(style);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(text));
    for b in buttons {
        alert.addButtonWithTitle(&NSString::from_str(b));
    }
    NSApplication::sharedApplication(mtm).activate();
    let r = alert.runModal();
    super::app::after_modal(mtm);
    usize::try_from(r - NSAlertFirstButtonReturn)
        .ok()
        .filter(|i| *i < buttons.len())
}

/// A message with OK.
pub fn message(mtm: MainThreadMarker, title: &str, text: &str) {
    let _ = ask(mtm, NSAlertStyle::Warning, title, text, &["OK"]);
}

/// WebKit's web content process for the window ended unasked: `true` to reload the Studio,
/// `false` to close the window. The service and its runs are untouched.
pub fn page_ended(mtm: MainThreadMarker) -> bool {
    ask(
        mtm,
        NSAlertStyle::Warning,
        "Fallow's window stopped showing the Studio",
        "The page in the window ended. Your loops and sessions are still running.",
        &["Reload", "Close"],
    ) == Some(0)
}

/// Says why Fallow cannot start when it was opened from Finder, the Dock or Spotlight (no
/// terminal shows its error), before AppKit runs its loop.
pub fn cannot_start(text: &str) {
    if let Some(mtm) = MainThreadMarker::new() {
        message(mtm, "Fallow cannot start", text);
    }
}

/// 016 (FR-009, T021): Quit's confirmation naming the work that would pause (`names`), as a
/// sheet on Fallow's window when it is open, else app-modal; `done` hears `true` for Quit and
/// `false` for Cancel (or an alert ended otherwise).
pub fn confirm_quit(
    mtm: MainThreadMarker,
    window: Option<&NSWindow>,
    names: &[String],
    done: fn(bool),
) {
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&NSString::from_str("Quit Fallow? This work is running:"));
    alert.setInformativeText(&NSString::from_str(&crate::window::quit::text(names)));
    alert.addButtonWithTitle(&NSString::from_str("Quit"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    NSApplication::sharedApplication(mtm).activate();
    match window.filter(|w| w.isVisible()) {
        Some(w) => {
            let once = Cell::new(Some(done));
            let block = RcBlock::new(move |r: NSModalResponse| {
                if let Some(f) = once.take() {
                    f(r == NSAlertFirstButtonReturn);
                }
            });
            alert.beginSheetModalForWindow_completionHandler(w, Some(&block));
        }
        None => {
            let r = alert.runModal();
            super::app::after_modal(mtm);
            done(r == NSAlertFirstButtonReturn);
        }
    }
}

/// 016 (T026): macOS's open panel for one folder, as a sheet on Fallow's window when it is open,
/// else app-modal; `done` hears the folder, or `None` when cancelled.
pub fn pick_folder(title: &str, done: impl FnOnce(Option<std::path::PathBuf>) + 'static) {
    let Some(mtm) = MainThreadMarker::new() else {
        done(None);
        return;
    };
    let panel = objc2_app_kit::NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    panel.setMessage(Some(&NSString::from_str(title)));
    panel.setPrompt(Some(&NSString::from_str("Add")));
    let chosen = |p: &objc2_app_kit::NSOpenPanel, r: NSModalResponse| {
        (r == objc2_app_kit::NSModalResponseOK)
            .then(|| p.URL().and_then(|u| u.path()))
            .flatten()
            .map(|s| std::path::PathBuf::from(s.to_string()))
    };
    NSApplication::sharedApplication(mtm).activate();
    match super::host::window() {
        Some(w) if w.isVisible() => {
            let once = Cell::new(Some(done));
            let p = panel.clone();
            let block = RcBlock::new(move |r: NSModalResponse| {
                if let Some(f) = once.take() {
                    f(chosen(&p, r));
                }
            });
            panel.beginSheetModalForWindow_completionHandler(&w, &block);
        }
        _ => {
            let r = panel.runModal();
            super::app::after_modal(mtm);
            done(chosen(&panel, r));
        }
    }
}
