//! 016 (research M-9, FR-013, FR-014, data-model "Notifications permission", T027): Fallow's
//! notifications on macOS through `UserNotifications`. macOS gives an app signed ad hoc
//! provisional permission without a prompt: notifications go quietly to Notification Center;
//! "Show banners" in the notifications setting asks for full permission while the engineer is
//! there to answer. A refusal is macOS's to keep: Fallow never asks again, and the "Needs you"
//! inbox and the menu bar icon's dot carry what is waiting.
//!
//! Only from an app bundle: `UNUserNotificationCenter` refuses (and throws) for a program that
//! is not one, so outside `Fallow.app` notifications are off, said once in the service's log.

use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::mpsc::channel;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, define_class, msg_send};
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSettings, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::Notice;

/// What macOS lets Fallow do with its notifications (data-model "Notifications permission").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Fallow has not needed one yet.
    NotAsked,
    /// Granted without a prompt: delivered quietly to Notification Center.
    Provisional,
    /// macOS shows them.
    Allowed,
    /// The engineer declined, or macOS does not allow it for this app.
    Refused,
    /// Not an app bundle (a development build): no system notifications.
    Unavailable,
}

impl Permission {
    /// Its name for the page.
    pub fn name(self) -> &'static str {
        match self {
            Permission::NotAsked => "not_asked",
            Permission::Provisional => "provisional",
            Permission::Allowed => "allowed",
            Permission::Refused => "refused",
            Permission::Unavailable => "unavailable",
        }
    }
}

/// Whether this process is `Fallow.app` (decided once).
fn bundled() -> bool {
    static B: OnceLock<bool> = OnceLock::new();
    *B.get_or_init(|| {
        let b = super::super::bundle().is_some();
        if !b {
            eprintln!(
                "fallow: not running as Fallow.app, so no system notifications (the \"Needs you\" inbox and the menu bar icon show what is waiting)"
            );
        }
        b
    })
}

fn center() -> Option<Retained<UNUserNotificationCenter>> {
    bundled().then(UNUserNotificationCenter::currentNotificationCenter)
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; `Delegate` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "FallowNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Fallow in front: still shown, as a banner and in the list.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _n: &UNNotification,
            done: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            done.call((
                UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,
            ));
        }

        /// A click: the window opens on the item (its route in the notification's `userInfo`).
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &block2::DynBlock<dyn Fn()>,
        ) {
            let info = response.notification().request().content().userInfo();
            let key = NSString::from_str("route");
            // SAFETY: NSDictionary's `objectForKey:`; the value Fallow put there is a string.
            let route: Option<Retained<NSString>> =
                unsafe { msg_send![&*info, objectForKey: &*key] };
            super::request_open(route.map(|r| r.to_string()));
            done.call(());
        }
    }
);

/// Sets Fallow as the notifications' delegate, so a click opens the item; before AppKit's loop
/// runs, so a click that launched Fallow is heard too.
pub fn init() {
    let Some(c) = center() else { return };
    let d: Retained<Delegate> = {
        let this = Delegate::alloc().set_ivars(());
        // SAFETY: NSObject's `init`.
        unsafe { msg_send![super(this), init] }
    };
    c.setDelegate(Some(ProtocolObject::from_ref(&*d)));
    // The center keeps its delegate weakly: Fallow's lives as long as the process.
    std::mem::forget(d);
    refresh();
}

fn status(c: &UNUserNotificationCenter) -> Option<UNAuthorizationStatus> {
    let (tx, rx) = channel();
    let block = RcBlock::new(move |s: NonNull<UNNotificationSettings>| {
        // SAFETY: macOS hands a live settings object for the block's call.
        let _ = tx.send(unsafe { s.as_ref() }.authorizationStatus());
    });
    c.getNotificationSettingsWithCompletionHandler(&block);
    rx.recv_timeout(Duration::from_secs(3)).ok()
}

/// What macOS lets Fallow do now (waits up to 3 s for macOS's answer; not on the main thread).
pub fn permission() -> Permission {
    let Some(c) = center() else {
        return Permission::Unavailable;
    };
    match status(&c) {
        Some(UNAuthorizationStatus::NotDetermined) => Permission::NotAsked,
        Some(UNAuthorizationStatus::Provisional) => Permission::Provisional,
        Some(UNAuthorizationStatus::Denied) => Permission::Refused,
        Some(_) => Permission::Allowed,
        None => Permission::NotAsked,
    }
}

fn request(c: &UNUserNotificationCenter, options: UNAuthorizationOptions) -> bool {
    let (tx, rx) = channel();
    let block = RcBlock::new(move |granted: Bool, _e: *mut NSError| {
        let _ = tx.send(granted.as_bool());
    });
    c.requestAuthorizationWithOptions_completionHandler(options, &block);
    // A prompt may wait for the engineer: long enough for them to answer.
    rx.recv_timeout(Duration::from_secs(300)).unwrap_or(false)
}

/// "Show banners": asks for full permission (macOS's prompt), while the engineer is at the
/// setting. Not on the main thread.
pub fn ask_banners() -> Permission {
    if let Some(c) = center() {
        request(
            &c,
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        );
    }
    remember(permission())
}

/// Shows `n` (008's content rule: project, feature, what is needed), asking for provisional
/// permission the first time; nothing when refused. On a thread of its own: macOS answers
/// asynchronously.
pub fn show(n: &Notice) {
    if !bundled() {
        return;
    }
    let n = n.clone();
    let _ = std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            let Some(c) = center() else { return };
            let allowed = match status(&c) {
                Some(UNAuthorizationStatus::NotDetermined) => request(
                    &c,
                    UNAuthorizationOptions::Alert
                        | UNAuthorizationOptions::Sound
                        | UNAuthorizationOptions::Provisional,
                ),
                Some(UNAuthorizationStatus::Denied) | None => false,
                Some(_) => true,
            };
            remember(permission());
            if !allowed {
                return;
            }
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&n.title));
            content.setBody(&NSString::from_str(&n.body));
            let key = NSString::from_str("route");
            let value = NSString::from_str(&n.url);
            // SAFETY: NSDictionary's class constructor with one string key and value, and
            // UNMutableNotificationContent's `setUserInfo:` taking it.
            unsafe {
                let info: Retained<AnyObject> = msg_send![
                    objc2::class!(NSDictionary),
                    dictionaryWithObject: &*value,
                    forKey: &*key
                ];
                let _: () = msg_send![&*content, setUserInfo: &*info];
            }
            let id = NSString::from_str(&format!(
                "fallow-{}",
                crate::platform::random::hex(8).unwrap_or_default()
            ));
            let req =
                UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
            c.addNotificationRequest_withCompletionHandler(&req, None);
        });
}

/// The last permission macOS gave (`None` until first read).
static LAST: std::sync::Mutex<Option<Permission>> = std::sync::Mutex::new(None);

fn remember(p: Permission) -> Permission {
    if let Ok(mut l) = LAST.lock() {
        *l = Some(p);
    }
    p
}

/// The last permission macOS gave, for the page (the engine never waits on macOS); refreshed in
/// the background each time it is read, and at once by "Show banners" and the first
/// notification (review).
pub fn cached() -> Permission {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BUSY: AtomicBool = AtomicBool::new(false);
    if !bundled() {
        return Permission::Unavailable;
    }
    if !BUSY.swap(true, Ordering::SeqCst) {
        let spawned = std::thread::Builder::new()
            .name("notify-permission".into())
            .spawn(|| {
                remember(permission());
                BUSY.store(false, Ordering::SeqCst);
            });
        if spawned.is_err() {
            BUSY.store(false, Ordering::SeqCst);
        }
    }
    LAST.lock()
        .ok()
        .and_then(|l| *l)
        .unwrap_or(Permission::NotAsked)
}

/// Reads macOS's answer once at start, so the first project page says it.
pub fn refresh() {
    let _ = cached();
}
