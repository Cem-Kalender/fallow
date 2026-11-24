//! 016 (research M-2, M-11): with a display, AppKit owns the service's main thread
//! (`NSApplication`'s run loop: window, menus, menu bar icon, dialogs, notifications) and the
//! engine runs whole on a thread of its own. Other threads reach the main thread through
//! `on_ui`, which queues a call on the main dispatch queue: AppKit's run loop drains it in every
//! mode, also while a menu is open or a dialog is up.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSEvent, NSEventModifierFlags, NSEventType, NSWorkspace,
    NSWorkspaceWillPowerOffNotification,
};
use objc2_foundation::{NSActivityOptions, NSNotification, NSPoint, NSProcessInfo, NSString};

use crate::window::quit::Terminate;

use crate::run::engine::Msg;

/// AppKit runs, or will: `serve` said so before the socket listens (`will_run`), so a request
/// that arrives while the main thread is still setting AppKit up is queued, not refused (review:
/// a cold `fallow` asked for the window a moment before the run loop started). Calls queued
/// before `app.run()` run once it does.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// The engine's thread has returned: the run loop ends. `RUNNING` and `ENGINE_DONE` are each
/// written by one side and read by the other: sequentially consistent, so at least one side sees
/// the other's write and the run loop never misses the engine's end (review).
static ENGINE_DONE: AtomicBool = AtomicBool::new(false);
/// The engine, for the app's own Quit.
static ENGINE: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
/// When the session's end was announced (`NSWorkspaceWillPowerOffNotification`), in Unix
/// seconds; 0 for never. Quit asks nothing for a minute after it: a logout another app cancels
/// must not turn every later ⌘Q into one without a question (review).
static POWER_OFF: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Whether the session's end was announced within the last minute.
fn powering_off() -> bool {
    let at = POWER_OFF.load(Ordering::SeqCst);
    at != 0 && unix_secs().saturating_sub(at) <= 60
}

/// AppKit ran and has ended (or the engine has stopped): Fallow is quitting.
pub fn ending() -> bool {
    ENGINE_DONE.load(Ordering::SeqCst)
        || (RAN.load(Ordering::SeqCst) && !RUNNING.load(Ordering::SeqCst))
}

/// `will_run` was called once: AppKit took the main thread.
static RAN: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The activity held while work runs (App Nap off), main thread only.
    static ACTIVITY: std::cell::RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>> =
        const { std::cell::RefCell::new(None) };
}

/// 016 (research M-11, data-model "Activity hold", T024): while any loop step, operation,
/// session turn or conversation turn runs, one activity is held so App Nap does not slow Fallow
/// with its window closed; idle sleep stays allowed. Main thread.
pub fn set_busy(busy: bool) {
    ACTIVITY.with(|a| {
        let mut a = a.borrow_mut();
        let info = NSProcessInfo::processInfo();
        match (busy, a.as_ref()) {
            (true, None) => {
                *a = Some(info.beginActivityWithOptions_reason(
                    NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                    &NSString::from_str("Fallow is running work"),
                ));
            }
            (false, Some(token)) => {
                // SAFETY: the token `beginActivityWithOptions` gave, ended once.
                unsafe { info.endActivity(token) };
                *a = None;
            }
            _ => {}
        }
    });
}

/// Opened by LaunchServices (Finder, the Dock, Spotlight, `fallow`): the window opens at start.
static OPEN_AT_START: AtomicBool = AtomicBool::new(false);

/// The window opens as soon as AppKit runs (the app was opened, not `fallow serve`).
pub fn open_at_start() {
    OPEN_AT_START.store(true, Ordering::SeqCst);
}

#[link(name = "System", kind = "dylib")]
unsafe extern "C" {
    /// The main dispatch queue (`dispatch_get_main_queue()` is this object's address).
    static _dispatch_main_q: u8;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
}

type Call = Box<dyn FnOnce() + Send>;

extern "C" fn call(context: *mut c_void) {
    // SAFETY: `context` is the `Box<Call>` `on_ui` leaked for this one call.
    let f: Box<Call> = unsafe { Box::from_raw(context.cast()) };
    objc2::rc::autoreleasepool(|_| f());
}

/// AppKit will run on the main thread (a display, the main thread): from now on `on_ui` queues
/// calls for it. `serve` calls it before its socket listens.
pub fn will_run() {
    RAN.store(true, Ordering::SeqCst);
    RUNNING.store(true, Ordering::SeqCst);
}

/// Runs `f` on the main thread, from any thread; `false` when AppKit does not run (no display,
/// or the service is ending), and `f` is dropped.
pub fn on_ui(f: Call) -> bool {
    if !RUNNING.load(Ordering::SeqCst) {
        return false;
    }
    let context = Box::into_raw(Box::new(f)).cast::<c_void>();
    // SAFETY: the main queue lives for the process; `call` takes the box back exactly once.
    unsafe { dispatch_async_f((&raw const _dispatch_main_q).cast(), context, call) };
    true
}

/// The engine's thread has returned: the run loop ends, and `serve` goes on to its clean-up.
pub fn engine_done() {
    ENGINE_DONE.store(true, Ordering::SeqCst);
    on_ui(Box::new(|| {
        if let Some(mtm) = MainThreadMarker::new() {
            super::host::engine_ended();
            stop(mtm);
        }
    }));
}

/// After a modal alert: when the engine ended meanwhile, the `stop:` it sent ended only the
/// alert's modal session (review), so the run loop is stopped again.
pub fn after_modal(mtm: MainThreadMarker) {
    if ENGINE_DONE.load(Ordering::SeqCst) {
        stop(mtm);
    }
}

/// Ends `NSApplication`'s run: `stop:` takes effect once an event has been handled, so one is
/// posted.
fn stop(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    app.stop(None);
    let wake = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        0,
        0,
    );
    if let Some(e) = wake {
        app.postEvent_atStart(&e, true);
    }
}

/// Asks the engine to stop, as `fallow stop` does; runs resume at the next start.
fn stop_engine() {
    if let Some(tx) = ENGINE.get().and_then(|t| t.lock().ok()) {
        let _ = tx.send(Msg::Shutdown);
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; `Delegate` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FallowAppDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl NSApplicationDelegate for Delegate {
        /// Every Quit reaches this: ⌘Q, the app menu, the menu bar icon, the Dock, an Apple
        /// event, and logout or shutdown (research M-11, T021, T023). The app never ends under a
        /// running engine.
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            let power_off = powering_off();
            let stopped = ENGINE_DONE.load(Ordering::SeqCst);
            match crate::window::quit::terminate_reply(power_off, stopped) {
                // Quit's own end: the run loop ends, and `serve` cleans up before the process
                // exits (rather than AppKit's exit, which would skip it).
                Terminate::Now if stopped && !power_off => {
                    stop(self.mtm());
                    NSApplicationTerminateReply::TerminateCancel
                }
                // The session ends: never blocked, no dialog. The engine stops as `fallow stop`
                // does (runs resume at the next start), within 10 s.
                Terminate::Now => {
                    stop_engine();
                    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
                    while !ENGINE_DONE.load(Ordering::SeqCst) && std::time::Instant::now() < end {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    super::host::shutdown();
                    NSApplicationTerminateReply::TerminateNow
                }
                Terminate::Cancel => {
                    super::host::quit(None);
                    NSApplicationTerminateReply::TerminateCancel
                }
            }
        }

        /// Fallow lives on with its window closed (spec US2).
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn after_last_window(&self, _sender: &NSApplication) -> bool {
            false
        }

        /// A click on the Dock icon, or Fallow opened from Finder while it runs: Open Fallow
        /// (FR-002, research M-3).
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn reopen(&self, _sender: &NSApplication, _visible: bool) -> bool {
            super::host::open(None);
            false
        }
    }

    impl Delegate {
        /// Logout, restart or shutdown is under way (T023).
        #[unsafe(method(fallowWillPowerOff:))]
        fn will_power_off(&self, _n: &NSNotification) {
            POWER_OFF.store(unix_secs().max(1), Ordering::SeqCst);
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
    }
}

/// Runs AppKit on the main thread until the engine has stopped. `engine` is the engine's channel,
/// for the app's own Quit.
pub fn run(mtm: MainThreadMarker, engine: Sender<Msg>) {
    let _ = ENGINE.set(Mutex::new(engine));
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // macOS may end an app without windows, or one that says it can be ended at once: Fallow
    // has runs to keep (research M-11).
    let info = NSProcessInfo::processInfo();
    info.disableAutomaticTermination(&NSString::from_str("Fallow runs loops and sessions"));
    info.disableSuddenTermination();
    let delegate = Delegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // Logout, restart and shutdown are announced on the workspace's own notification centre.
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the delegate answers the selector and outlives the observation (removed below).
    unsafe {
        center.addObserver_selector_name_object(
            &delegate,
            sel!(fallowWillPowerOff:),
            Some(NSWorkspaceWillPowerOffNotification),
            None,
        );
    }
    // The main menu, with or without a window (⌘Q, ⌘H); its target lives as long as AppKit runs.
    let _menu = super::menu::install(mtm, super::host::view_cmd);
    will_run();
    if OPEN_AT_START.load(Ordering::SeqCst) {
        on_ui(Box::new(|| super::host::open(None)));
    }
    // The engine may have stopped already (its `on_ui` then found no run loop).
    if !ENGINE_DONE.load(Ordering::SeqCst) {
        app.run();
    }
    RUNNING.store(false, Ordering::SeqCst);
    super::host::shutdown();
    super::menu::remove_status_item();
    set_busy(false);
    // SAFETY: the observer added above.
    unsafe { center.removeObserver(&delegate) };
    app.setDelegate(None);
    drop(delegate);
}
