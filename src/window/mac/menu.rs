//! 016 (contracts/window-macos.md §5, research M-8, FR-007, T012): the app's main menu. AppKit
//! gives a web view its editing shortcuts only through the menu (the standard selectors, sent
//! down the responder chain to the focused `WKWebView`), so Fallow builds the standard one.
//! Quit Fallow sends `terminate:`, so every Quit reaches the app delegate.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

/// What the View menu asks of the window (its owner, `host`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewCmd {
    ActualSize,
    ZoomIn,
    ZoomOut,
    Reload,
}

/// Where the View menu's choices go.
pub struct Ivars {
    pub view: fn(ViewCmd),
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; `Target` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FallowMenuTarget"]
    #[ivars = Ivars]
    pub struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(fallowActualSize:))]
        fn actual_size(&self, _sender: Option<&AnyObject>) {
            (self.ivars().view)(ViewCmd::ActualSize);
        }

        #[unsafe(method(fallowZoomIn:))]
        fn zoom_in(&self, _sender: Option<&AnyObject>) {
            (self.ivars().view)(ViewCmd::ZoomIn);
        }

        #[unsafe(method(fallowZoomOut:))]
        fn zoom_out(&self, _sender: Option<&AnyObject>) {
            (self.ivars().view)(ViewCmd::ZoomOut);
        }

        #[unsafe(method(fallowReload:))]
        fn reload(&self, _sender: Option<&AnyObject>) {
            (self.ivars().view)(ViewCmd::Reload);
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker, view: fn(ViewCmd)) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { view });
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
    }
}

/// One item; `key` empty for none, with ⌘ plus `extra` modifiers.
fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
    extra: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    // SAFETY: a new menu item with a title, an action (or none) and a key equivalent.
    let i = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    };
    if !key.is_empty() {
        i.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | extra);
    }
    i
}

fn none() -> NSEventModifierFlags {
    NSEventModifierFlags::empty()
}

fn submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: Vec<Retained<NSMenuItem>>,
) -> Retained<NSMenu> {
    let m = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for i in items {
        m.addItem(&i);
    }
    m
}

/// Sets the app's main menu; `view` gets the View menu's choices. The returned target must live
/// as long as the menu (a menu item does not keep its target).
pub fn install(mtm: MainThreadMarker, view: fn(ViewCmd)) -> Retained<Target> {
    let target = Target::new(mtm, view);
    let app = NSApplication::sharedApplication(mtm);
    let sep = || NSMenuItem::separatorItem(mtm);

    let fallow = submenu(
        mtm,
        "Fallow",
        vec![
            item(
                mtm,
                "About Fallow",
                Some(sel!(orderFrontStandardAboutPanel:)),
                "",
                none(),
            ),
            sep(),
            item(mtm, "Hide Fallow", Some(sel!(hide:)), "h", none()),
            item(
                mtm,
                "Hide Others",
                Some(sel!(hideOtherApplications:)),
                "h",
                NSEventModifierFlags::Option,
            ),
            item(
                mtm,
                "Show All",
                Some(sel!(unhideAllApplications:)),
                "",
                none(),
            ),
            sep(),
            item(mtm, "Quit Fallow", Some(sel!(terminate:)), "q", none()),
        ],
    );
    let edit = submenu(
        mtm,
        "Edit",
        vec![
            item(mtm, "Undo", Some(sel!(undo:)), "z", none()),
            item(
                mtm,
                "Redo",
                Some(sel!(redo:)),
                "z",
                NSEventModifierFlags::Shift,
            ),
            sep(),
            item(mtm, "Cut", Some(sel!(cut:)), "x", none()),
            item(mtm, "Copy", Some(sel!(copy:)), "c", none()),
            item(mtm, "Paste", Some(sel!(paste:)), "v", none()),
            item(mtm, "Select All", Some(sel!(selectAll:)), "a", none()),
        ],
    );
    let ours = |title: &str, action: Sel, key: &str| {
        let i = item(mtm, title, Some(action), key, none());
        // SAFETY: the target lives as long as the menu (returned to the caller, which keeps it).
        unsafe { i.setTarget(Some(&target)) };
        i
    };
    // ⌘= as Zoom In too (the key under + on most keyboards), hidden but working.
    let zoom_in_eq = ours("Zoom In", sel!(fallowZoomIn:), "=");
    zoom_in_eq.setHidden(true);
    zoom_in_eq.setAllowsKeyEquivalentWhenHidden(true);
    let view_menu = submenu(
        mtm,
        "View",
        vec![
            ours("Actual Size", sel!(fallowActualSize:), "0"),
            ours("Zoom In", sel!(fallowZoomIn:), "+"),
            zoom_in_eq,
            ours("Zoom Out", sel!(fallowZoomOut:), "-"),
            sep(),
            ours("Reload", sel!(fallowReload:), "r"),
        ],
    );
    let window = submenu(
        mtm,
        "Window",
        vec![
            item(
                mtm,
                "Minimize",
                Some(sel!(performMiniaturize:)),
                "m",
                none(),
            ),
            item(mtm, "Zoom", Some(sel!(performZoom:)), "", none()),
            sep(),
            item(mtm, "Close", Some(sel!(performClose:)), "w", none()),
        ],
    );
    let bar = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(""));
    for m in [&fallow, &edit, &view_menu, &window] {
        let holder = item(mtm, &m.title().to_string(), None, "", none());
        holder.setSubmenu(Some(m));
        bar.addItem(&holder);
    }
    app.setMainMenu(Some(&bar));
    app.setWindowsMenu(Some(&window));
    target
}

// 016 (research M-10, contracts/window-macos.md §6, FR-008, T019): the menu bar icon.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::AllocAnyThread;
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize};

use crate::platform::notify::{Choice, Open};

/// The template images (black on transparent; macOS colours them), drawn by
/// `assets/icon/make_menubar.swift`, built in so the icon shows outside a bundle too.
const ICON: &[u8] = include_bytes!("../../../assets/icon/menubar@2x.png");
const ICON_DOT: &[u8] = include_bytes!("../../../assets/icon/menubar-dot@2x.png");

/// What the menu bar icon's menu does.
pub struct StatusIvars {
    open: Open,
    on: Arc<AtomicBool>,
    choose: Box<dyn Fn(Choice) + Send>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; `StatusTarget` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FallowStatusTarget"]
    #[ivars = StatusIvars]
    pub struct StatusTarget;

    unsafe impl NSObjectProtocol for StatusTarget {}

    impl StatusTarget {
        #[unsafe(method(fallowOpen:))]
        fn open(&self, _sender: Option<&AnyObject>) {
            (self.ivars().open)(None);
            (self.ivars().choose)(Choice::Open);
        }

        #[unsafe(method(fallowNotifications:))]
        fn notifications(&self, sender: Option<&AnyObject>) {
            let on = !self.ivars().on.load(Ordering::Relaxed);
            self.ivars().on.store(on, Ordering::Relaxed);
            if let Some(item) = sender.and_then(|s| s.downcast_ref::<NSMenuItem>()) {
                item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
            }
            (self.ivars().choose)(Choice::Notifications(on));
        }
    }
);

/// The icon, its menu's target and its two images, kept on the main thread.
struct Status {
    item: Retained<NSStatusItem>,
    _menu: Retained<NSMenu>,
    _target: Retained<StatusTarget>,
    plain: Option<Retained<NSImage>>,
    dot: Option<Retained<NSImage>>,
}

thread_local! {
    static STATUS: RefCell<Option<Status>> = const { RefCell::new(None) };
}

fn template(png: &[u8]) -> Option<Retained<NSImage>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))?;
    image.setSize(NSSize::new(18.0, 18.0));
    image.setTemplate(true);
    Some(image)
}

/// Adds the menu bar icon: Open Fallow, Notifications (checked while `on`), Quit Fallow
/// (`terminate:`, so it reaches the app delegate's Quit). Main thread.
pub fn status_item(
    mtm: MainThreadMarker,
    open: Open,
    on: Arc<AtomicBool>,
    choose: Box<dyn Fn(Choice) + Send>,
) {
    let checked = on.load(Ordering::Relaxed);
    let target = StatusTarget::alloc(mtm).set_ivars(StatusIvars { open, on, choose });
    // SAFETY: NSObject's `init`.
    let target: Retained<StatusTarget> = unsafe { msg_send![super(target), init] };
    let ours = |title: &str, action: Sel| {
        let i = item(mtm, title, Some(action), "", none());
        // SAFETY: the target is kept with the icon.
        unsafe { i.setTarget(Some(&target)) };
        i
    };
    let notify = ours("Notifications", sel!(fallowNotifications:));
    notify.setState(if checked {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
    let menu = submenu(
        mtm,
        "Fallow",
        vec![
            ours("Open Fallow", sel!(fallowOpen:)),
            notify,
            NSMenuItem::separatorItem(mtm),
            item(mtm, "Quit Fallow", Some(sel!(terminate:)), "", none()),
        ],
    );
    let status = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let plain = template(ICON);
    if let Some(b) = status.button(mtm) {
        b.setImage(plain.as_deref());
        if plain.is_none() {
            b.setTitle(&NSString::from_str("Fallow"));
        }
    }
    status.setMenu(Some(&menu));
    STATUS.with(|s| {
        *s.borrow_mut() = Some(Status {
            item: status,
            _menu: menu,
            _target: target,
            plain,
            dot: template(ICON_DOT),
        });
    });
}

/// 016 (FR-014, T030): the icon with a dot while something needs the engineer. Main thread.
pub fn set_waiting(mtm: MainThreadMarker, waiting: bool) {
    STATUS.with(|s| {
        if let Some(s) = s.borrow().as_ref() {
            if let Some(b) = s.item.button(mtm) {
                b.setImage(if waiting {
                    s.dot.as_deref()
                } else {
                    s.plain.as_deref()
                });
            }
        }
    });
}

/// Removes the icon (the service stops). Main thread.
pub fn remove_status_item() {
    STATUS.with(|s| {
        if let Some(s) = s.borrow_mut().take() {
            NSStatusBar::systemStatusBar().removeStatusItem(&s.item);
        }
    });
}
