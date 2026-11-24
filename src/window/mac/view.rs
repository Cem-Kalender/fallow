//! 016 (research M-4 to M-7, M-18, T011): the web view in Fallow's window on macOS: WebKit's
//! `WKWebView` through `wry`, with a non-persistent data store, showing the Studio through the
//! custom scheme `fallow` (`fallow://localhost/`), answered in this process by `service::answer`.
//! Nothing listens on any port.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel};
use wry::http::{Request, Response};
use wry::{
    NewWindowResponse, RequestAsyncResponder, WebView, WebViewBuilder, WebViewBuilderExtDarwin,
    WebViewExtMacOS,
};

use crate::run::engine::Msg;
use crate::service::answer::{HEADERS, KeptTheme, PageRequest, answer};
use crate::window::nav::{self, Nav};

/// What the web view needs from the service.
#[derive(Clone)]
pub struct Service {
    pub tx: Sender<Msg>,
    pub theme: Arc<KeptTheme>,
}

/// Developer tools (WebKit's inspector): in debug and `automation` builds only (research M-7).
pub const DEVTOOLS: bool = cfg!(debug_assertions) || cfg!(feature = "automation");

/// The page's ground in 013's themes, shown until the page draws.
pub fn background(dark: bool) -> (u8, u8, u8, u8) {
    if dark {
        (0x16, 0x16, 0x14, 255)
    } else {
        (0xF6, 0xF6, 0xF3, 255)
    }
}

/// The web view, filling `window`'s content, on `url`; the page's messages go to `ipc`, and
/// `ended` runs when WebKit's web content process ends unasked.
pub fn build(
    window: &impl raw_window_handle::HasWindowHandle,
    service: &Service,
    url: &str,
    dark: bool,
    ipc: impl Fn(&str) + 'static,
    ended: impl Fn() + 'static,
) -> Result<WebView, String> {
    let s = service.clone();
    let builder = WebViewBuilder::new();
    // 016 (`automation` builds only, T047): the page's errors kept for `snapshot`.
    #[cfg(feature = "automation")]
    let builder = builder.with_initialization_script(ERRORS_JS);
    let view = builder
        // Non-persistent: WebKit writes nothing of the page to disk (research M-6).
        .with_incognito(true)
        .with_background_color(background(dark))
        .with_devtools(DEVTOOLS)
        .with_hotkeys_zoom(false)
        .with_allow_link_preview(false)
        .with_asynchronous_custom_protocol(
            "fallow".into(),
            move |_id, req: Request<Vec<u8>>, responder: RequestAsyncResponder| {
                let s = s.clone();
                // Each request on its own thread: the long poll waits up to 25 s.
                let _ = std::thread::Builder::new()
                    .name("window-req".into())
                    .spawn(move || responder.respond(respond(&s, req)));
            },
        )
        // WebKit through `wry` never says whether a navigation was the engineer's: only the
        // Studio loads; links leave through the page's `open:` message (research M-5).
        .with_navigation_handler(|url: String| {
            nav::decide(nav::ORIGIN, &url, false, false) == Nav::Load
        })
        .with_new_window_req_handler(|_url, _features| NewWindowResponse::Deny)
        .with_download_started_handler(|_url, _path| false)
        .with_on_web_content_process_terminate_handler(ended)
        .with_ipc_handler(move |req: Request<String>| ipc(req.body()))
        .with_url(url)
        .build(window)
        .map_err(|e| format!("the window's web view could not be made: {e}"))?;
    guard(&view);
    Ok(view)
}

/// One request of the page, answered by the Studio; nothing for another host.
fn respond(s: &Service, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let ours = req.uri().scheme_str() == Some("fallow") && req.uri().host() == Some("localhost");
    if !ours {
        return Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_else(|_| Response::new(Vec::new()));
    }
    let url = req
        .uri()
        .path_and_query()
        .map_or_else(|| "/".to_string(), |p| p.as_str().to_string());
    let page = PageRequest {
        method: req.method().as_str().to_string(),
        url,
        body: req.into_body(),
    };
    let a = answer(page, &s.tx, &s.theme);
    let mut b = Response::builder()
        .status(a.status)
        .header("Content-Type", a.content_type);
    for (k, v) in HEADERS {
        b = b.header(*k, *v);
    }
    b.body(a.body).unwrap_or_else(|_| Response::new(Vec::new()))
}

/// Logs once per selector that WebKit lacks one of the private calls Fallow uses (research
/// M-18); the window goes on without it.
fn missing(name: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SAID: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    if let Ok(mut said) = SAID.lock() {
        if said
            .get_or_insert_with(HashSet::new)
            .insert(name.to_string())
        {
            eprintln!("fallow: this macOS's WebKit has no {name}; the window goes on without it");
        }
    }
}

/// Whether `obj` answers `sel`; logs once when it does not.
fn responds(obj: &AnyObject, sel: Sel, name: &str) -> bool {
    // SAFETY: `respondsToSelector:` is NSObject's.
    let can: bool = unsafe { msg_send![obj, respondsToSelector: sel] };
    if !can {
        missing(name);
    }
    can
}

/// The web view's preferences (research M-7, M-18): fraudulent-website lookups off (Safe
/// Browsing through Apple); a hidden page keeps running, so the Studio's long poll is answered
/// while the window is minimized, covered or the screen locked.
fn guard(view: &WebView) {
    let wk = view.webview();
    // SAFETY: WKWebView's documented `configuration` and its `preferences`.
    let prefs: Retained<AnyObject> = unsafe {
        let conf: Retained<AnyObject> = msg_send![&*wk, configuration];
        msg_send![&*conf, preferences]
    };
    // SAFETY: WKPreferences' documented `fraudulentWebsiteWarningEnabled` (macOS 10.15).
    unsafe {
        let _: () = msg_send![&*prefs, setFraudulentWebsiteWarningEnabled: false];
    }
    if responds(
        &prefs,
        sel!(_setPageVisibilityBasedProcessSuppressionEnabled:),
        "_setPageVisibilityBasedProcessSuppressionEnabled:",
    ) {
        // SAFETY: answered above; takes one BOOL.
        unsafe {
            let _: () = msg_send![&*prefs, _setPageVisibilityBasedProcessSuppressionEnabled: false];
        }
    }
    if responds(
        &prefs,
        sel!(_setHiddenPageDOMTimerThrottlingEnabled:),
        "_setHiddenPageDOMTimerThrottlingEnabled:",
    ) {
        // SAFETY: answered above; takes one BOOL.
        unsafe {
            let _: () = msg_send![&*prefs, _setHiddenPageDOMTimerThrottlingEnabled: false];
        }
    }
}

/// Ends the page: WebKit's web content process ends now. `wry` 0.57.2 keeps the `WKWebView`
/// alive when its web view is dropped (it calls `retain`), so without this each opening would
/// leave one more process (research M-18, finding 1).
pub fn close(view: WebView) {
    let wk = view.webview();
    if responds(&wk, sel!(_close), "_close") {
        // SAFETY: answered above; WKWebView's `_close` takes nothing and returns nothing.
        unsafe {
            let _: () = msg_send![&*wk, _close];
        }
    }
    drop(view);
}

/// 016 (`automation` builds only, T047): keeps the page's errors (`window.onerror`, unhandled
/// rejections) from its start, for `snapshot`.
#[cfg(feature = "automation")]
const ERRORS_JS: &str = "window.__fallowErrors = []; \
    addEventListener('error', (e) => window.__fallowErrors.push(String(e.message || e))); \
    addEventListener('unhandledrejection', (e) => window.__fallowErrors.push(String(e.reason)));";

/// 016 (`automation` builds only, T047): sets the route and theme, waits for the page's ready
/// state (`#app` filled), runs `script` (an async function body, for a menu or dialog), and tells
/// the window `snap:<n>:<report>`: the page's errors, its width against the window's, its ground
/// colour and what `script` returned.
#[cfg(feature = "automation")]
pub fn snapshot_page(view: &WebView, n: u64, route: &str, theme: &str, script: &str) {
    let q = |v: &str| serde_json::Value::String(v.to_string()).to_string();
    let js = format!(
        "(async () => {{ \
           const sleep = (ms) => new Promise((r) => setTimeout(r, ms)); \
           window.__fallowErrors = window.__fallowErrors || []; \
           document.querySelectorAll('dialog[open]').forEach((d) => d.close()); \
           for (let i = 0; i < 2; i++) document.dispatchEvent(new KeyboardEvent('keydown', {{ key: 'Escape', bubbles: true }})); \
           document.documentElement.dataset.theme = {theme}; \
           if (location.hash !== {route}) {{ location.hash = {route}; await sleep(300); }} \
           let ready = false; for (let i = 0; i < 100; i++) {{ const a = document.getElementById('app'); if (a && a.children.length && !document.querySelector('.loading')) {{ ready = true; break; }} await sleep(100); }} \
           await sleep(900); \
           document.documentElement.dataset.theme = {theme}; \
           let result = 'ok'; \
           try {{ const r = await (async () => {{ {script} }})(); result = r === undefined ? 'ok' : String(r); }} catch (e) {{ result = 'script failed: ' + e; }} \
           await sleep(300); \
           window.ipc.postMessage('snap:{n}:' + JSON.stringify({{ \
             errors: window.__fallowErrors.slice(), \
             scrollWidth: document.documentElement.scrollWidth, \
             innerWidth: window.innerWidth, \
             background: getComputedStyle(document.body).backgroundColor, \
             theme: document.documentElement.dataset.theme, \
             ready, \
             result }})); \
           window.__fallowErrors.length = 0; \
         }})();",
        theme = q(theme),
        route = q(route),
    );
    let _ = view.evaluate_script(&js);
}

/// 016 (`automation` builds only, T047): the web view as drawn, as PNG
/// (`takeSnapshotWithConfiguration:completionHandler:`, public WebKit), given to `done` on the
/// main thread.
#[cfg(feature = "automation")]
pub fn snapshot_png(view: &WebView, done: impl FnOnce(Result<Vec<u8>, String>) + 'static) {
    use std::cell::RefCell;
    let wk = view.webview();
    let done = RefCell::new(Some(done));
    let block = block2::RcBlock::new(move |img: *mut AnyObject, _err: *mut AnyObject| {
        let Some(done) = done.borrow_mut().take() else {
            return;
        };
        if img.is_null() {
            done(Err("WebKit gave no snapshot".into()));
            return;
        }
        // SAFETY: `img` is the NSImage WebKit hands its completion handler; the calls are
        // NSImage's and NSBitmapImageRep's public API, on the main thread.
        let png = unsafe {
            let img = &*img;
            let tiff: *mut AnyObject = msg_send![img, TIFFRepresentation];
            if tiff.is_null() {
                return done(Err("the snapshot has no image data".into()));
            }
            let Some(rep_class) = objc2::runtime::AnyClass::get(c"NSBitmapImageRep") else {
                return done(Err("no NSBitmapImageRep".into()));
            };
            let rep: *mut AnyObject = msg_send![rep_class, imageRepWithData: tiff];
            if rep.is_null() {
                return done(Err("the snapshot could not be read".into()));
            }
            let Some(dict_class) = objc2::runtime::AnyClass::get(c"NSDictionary") else {
                return done(Err("no NSDictionary".into()));
            };
            let props: *mut AnyObject = msg_send![dict_class, dictionary];
            // NSBitmapImageFileTypePNG = 4.
            let data: *mut AnyObject =
                msg_send![rep, representationUsingType: 4usize, properties: props];
            if data.is_null() {
                return done(Err("the snapshot could not be made a PNG".into()));
            }
            let len: usize = msg_send![data, length];
            let bytes: *const u8 = msg_send![data, bytes];
            std::slice::from_raw_parts(bytes, len).to_vec()
        };
        done(Ok(png));
    });
    // SAFETY: public WKWebView API; a nil configuration takes the whole view; the block lives
    // as long as WebKit holds it (it is copied).
    unsafe {
        let none: *const AnyObject = std::ptr::null();
        let _: () = msg_send![
            &*wk,
            takeSnapshotWithConfiguration: none,
            completionHandler: &*block
        ];
    }
}

/// Sets the page's route.
pub fn go_to(view: &WebView, route: &str) {
    let js = format!(
        "location.hash = {};",
        serde_json::Value::String(route.to_string())
    );
    let _ = view.evaluate_script(&js);
}

/// 016 (FR-034, contracts/window-macos.md §4): the page's `open:<url>`: the default browser, for
/// `http(s)` addresses only.
pub fn open_link(url: &str) {
    let Some(u) = nav::open_link(url) else {
        return;
    };
    let ns = objc2_foundation::NSString::from_str(&u);
    if let Some(url) = objc2_foundation::NSURL::URLWithString(&ns) {
        objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url);
    }
}
