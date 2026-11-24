//! 016 (research M-13, FR-001, FR-003, FR-004, T010): Fallow's `NSWindow`: titled, resizable,
//! at least 800 × 500, in 013's theme from its first frame (its appearance and background set
//! before it is shown), where it was last time; macOS's own window restoration and tabs off.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSBackingStoreType, NSColor, NSScreen, NSWindow, NSWindowCollectionBehavior,
    NSWindowDelegate, NSWindowStyleMask, NSWindowTabbingMode,
};
use objc2_foundation::{NSArray, NSNotification, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};

use crate::window::state::{self, MacPlace, Rect};

/// Whether 013's choice (`system`, `light`, `dark`) means dark now.
pub fn is_dark(mtm: MainThreadMarker, theme: &str) -> bool {
    match theme {
        "dark" => true,
        "light" => false,
        _ => system_dark(mtm),
    }
}

/// Whether macOS is in Dark appearance.
pub fn system_dark(mtm: MainThreadMarker) -> bool {
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: the documented appearance names.
    let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    app.effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names)
        // SAFETY: as above.
        .is_some_and(|n| &*n == unsafe { NSAppearanceNameDarkAqua })
}

fn rect(r: NSRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}

fn ns_rect(r: Rect) -> NSRect {
    NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.width, r.height))
}

/// 013's theme on the window: its appearance (the title bar, and the web view's
/// `prefers-color-scheme` under System) and its background, before the page draws.
pub fn set_theme(window: &NSWindow, theme: &str) {
    let mtm = window.mtm();
    let appearance = match theme {
        // SAFETY: the documented appearance names.
        "dark" => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }),
        "light" => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }),
        _ => None,
    };
    window.setAppearance(appearance.as_deref());
    let (r, g, b, _) = super::view::background(is_dark(mtm, theme));
    let c = |v: u8| f64::from(v) / 255.0;
    window.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
        c(r),
        c(g),
        c(b),
        1.0,
    )));
}

/// What the window's delegate tells the window's owner.
pub struct Events {
    /// The engineer asked to close it (⌘W, the red button): answer later.
    pub close_asked: fn(),
    /// It moved or was resized: its windowed place is kept for full screen (review).
    pub moved: fn(),
    /// It goes into (`true`) or has left (`false`) full screen: resizes meanwhile are the
    /// animation's, not a place to keep.
    pub full_screen: fn(bool),
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; `Delegate` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FallowWindowDelegate"]
    #[ivars = Events]
    pub struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSWindowDelegate for Delegate {
        /// The page is asked about unsaved changes first; the window closes when it lets go
        /// (research M-11, 014 research R-9).
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, _sender: &NSWindow) -> bool {
            (self.ivars().close_asked)();
            false
        }

        #[unsafe(method(windowDidMove:))]
        fn did_move(&self, _n: &NSNotification) {
            (self.ivars().moved)();
        }

        #[unsafe(method(windowDidResize:))]
        fn did_resize(&self, _n: &NSNotification) {
            (self.ivars().moved)();
        }

        #[unsafe(method(windowWillEnterFullScreen:))]
        fn will_enter_full_screen(&self, _n: &NSNotification) {
            (self.ivars().moved)();
            (self.ivars().full_screen)(true);
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn did_exit_full_screen(&self, _n: &NSNotification) {
            (self.ivars().full_screen)(false);
        }
    }
);

impl Delegate {
    pub fn new(mtm: MainThreadMarker, events: Events) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(events);
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
    }
}

/// The screens' frames and the main screen's visible frame (without menu bar and Dock).
fn screens(mtm: MainThreadMarker) -> (Vec<Rect>, Rect) {
    let all: Vec<Rect> = NSScreen::screens(mtm)
        .iter()
        .map(|s| rect(s.frame()))
        .collect();
    let main = NSScreen::mainScreen(mtm)
        .map(|s| rect(s.visibleFrame()))
        .or_else(|| all.first().copied())
        .unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
        });
    (all, main)
}

/// Where the window opens (`state::mac_opening`).
pub fn opening_place(mtm: MainThreadMarker, saved: Option<MacPlace>) -> MacPlace {
    let (all, main) = screens(mtm);
    state::mac_opening(saved, &all, main)
}

/// Makes the window at `place`, in `theme`, not shown yet; `delegate` hears its close.
pub fn create(
    mtm: MainThreadMarker,
    place: &MacPlace,
    theme: &str,
    delegate: &Delegate,
) -> Retained<NSWindow> {
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    // SAFETY: a plain titled window that Fallow releases itself (not on close).
    let window = unsafe {
        let w = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            ns_rect(place.frame),
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        w.setReleasedWhenClosed(false);
        w
    };
    window.setTitle(&NSString::from_str("Fallow"));
    window.setRestorable(false);
    window.setTabbingMode(NSWindowTabbingMode::Disallowed);
    window.setContentMinSize(NSSize::new(
        f64::from(state::MIN.0),
        f64::from(state::MIN.1),
    ));
    window.setCollectionBehavior(NSWindowCollectionBehavior::FullScreenPrimary);
    set_theme(&window, theme);
    // The frame as kept (the content rectangle above was only a start).
    window.setFrame_display(ns_rect(place.frame), false);
    window.setDelegate(Some(ProtocolObject::from_ref(delegate)));
    window
}

/// Shows the window as kept: zoomed or in full screen when it was.
pub fn show(window: &NSWindow, place: &MacPlace) {
    if place.zoomed && !window.isZoomed() {
        window.zoom(None);
    }
    front(window);
    if place.full_screen {
        window.toggleFullScreen(None);
    }
}

/// Brings the window to the front, out of the Dock when minimized, and Fallow with it.
pub fn front(window: &NSWindow) {
    if window.isMiniaturized() {
        window.deminiaturize(None);
    }
    window.makeKeyAndOrderFront(None);
    let app = NSApplication::sharedApplication(window.mtm());
    app.activate();
    // From macOS 14 `activate` is cooperative; outside a bundle no LaunchServices launch brings
    // Fallow forward, so the older call is asked too (review).
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

/// Where the window is, to keep: in full screen its windowed frame is the one kept before
/// (`kept`), as macOS gives only the screen's.
pub fn place(window: &NSWindow, kept: Option<MacPlace>) -> MacPlace {
    let full_screen = window.styleMask().contains(NSWindowStyleMask::FullScreen);
    let frame = match kept {
        Some(k) if full_screen => k.frame,
        _ => rect(window.frame()),
    };
    let screen = window
        .screen()
        .map(|s| rect(s.frame()))
        .or(kept.map(|k| k.screen))
        .unwrap_or(frame);
    MacPlace {
        frame,
        screen,
        zoomed: !full_screen && window.isZoomed(),
        full_screen,
    }
}

/// The window as the web view's parent (its content view).
pub struct Parent<'a>(pub &'a NSWindow);

impl HasWindowHandle for Parent<'_> {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let view = self.0.contentView().ok_or(HandleError::Unavailable)?;
        let h = AppKitWindowHandle::new(NonNull::from(&*view).cast());
        // SAFETY: the window and its content view outlive the web view, which is closed before
        // the window is let go.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(h)) })
    }
}
