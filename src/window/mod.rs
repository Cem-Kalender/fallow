//! 014: Fallow's own window (research R-1 to R-9); 016: on macOS too (`mac`). The window itself
//! is Windows and macOS only; the decisions it is made of (`nav`, `state`, `quit`) are plain and
//! tested everywhere.

pub mod nav;
pub mod quit;
pub mod state;

#[cfg(target_os = "macos")]
pub mod mac;

#[cfg(windows)]
mod frame;
#[cfg(windows)]
pub mod view;

#[cfg(windows)]
pub use host::{configure, open, open_for, quit};
#[cfg(target_os = "macos")]
pub use mac::host::{configure, open, open_for, quit};

/// The window on the UI thread (`platform::notify`'s thread): at most one, made when asked for,
/// dropped with its web view when closed (research R-2, R-9).
#[cfg(windows)]
mod host {
    use std::cell::{Cell, RefCell};
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, KillTimer, MB_ICONERROR, MB_OK, MessageBoxW, SetTimer,
    };
    use wry::WebView;
    use wry::dpi::{PhysicalPosition, PhysicalSize};

    use super::{frame, nav, state, view};

    /// What the window needs from the service, set once at its start.
    struct Config {
        data: PathBuf,
        service: view::Service,
    }

    static CONFIG: OnceLock<Config> = OnceLock::new();

    /// Sets what the window needs; the service calls it before the UI thread opens anything.
    pub fn configure(data: PathBuf, service: view::Service) {
        let _ = CONFIG.set(Config { data, service });
    }

    /// The window that is open, on this thread.
    struct Live {
        hwnd: HWND,
        view: Option<WebView>,
    }

    thread_local! {
        static LIVE: RefCell<Option<Live>> = const { RefCell::new(None) };
        /// The web view is being made (it pumps messages meanwhile): what arrives then only
        /// brings the window forward, its route kept for when it is made (research R-2).
        static OPENING: Cell<bool> = const { Cell::new(false) };
        static PENDING_ROUTE: RefCell<Option<String>> = const { RefCell::new(None) };
        /// When the last opening was asked for, for `window ready` (research R-14): the
        /// launcher's start (Unix ms) when it said, else when the UI thread was asked.
        static ASKED: Cell<Option<Instant>> = const { Cell::new(None) };
        static LAUNCHED: Cell<Option<u128>> = const { Cell::new(None) };
        /// The window was closed while it was being made: closed once made (review).
        static CLOSE_AFTER_OPEN: Cell<bool> = const { Cell::new(false) };
        /// The page was asked about unsaved changes before closing (research R-9).
        static CLOSING: Cell<bool> = const { Cell::new(false) };
        /// Quit is under way (research R-9); who asked waits for the outcome.
        static QUITTING: Cell<bool> = const { Cell::new(false) };
        /// Quit has passed the page: its dialog is up (or the service is stopping); a close now
        /// only closes (review).
        static QUIT_PAST_PAGE: Cell<bool> = const { Cell::new(false) };
        static QUIT_REPLY: RefCell<Vec<std::sync::mpsc::Sender<bool>>> = const { RefCell::new(Vec::new()) };
    }

    /// The timer that closes a window whose page did not answer within 3 s (research R-9).
    pub(super) const CLOSE_TIMER: usize = 0x0140;

    /// The route in what was asked for: `#/…` itself, or after the `#` of a notification's
    /// address.
    fn route_of(what: Option<String>) -> Option<String> {
        let w = what?;
        let i = w.find("#/")?;
        Some(w[i..].to_string())
    }

    fn hwnd() -> Option<HWND> {
        LIVE.with(|l| l.borrow().as_ref().map(|l| l.hwnd))
    }

    fn message(text: &str) {
        let t: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let c: Vec<u16> = "Fallow".encode_utf16().chain(Some(0)).collect();
        // SAFETY: a modal message box with live wide strings.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                t.as_ptr(),
                c.as_ptr(),
                MB_OK | MB_ICONERROR,
            )
        };
    }

    /// What the error of a window that could not open says when Fallow is quitting meanwhile:
    /// nothing to tell the engineer.
    const QUITTING_MEANWHILE: &str = "Fallow is quitting";

    /// The icon's, a notification's or the pipe's request without an answer: shows the window
    /// on `what`'s route; an error is shown as a message (UI thread only).
    pub fn open(what: Option<String>) {
        if let Err(e) = open_for(what, None) {
            // The missing runtime said so already, with its download page.
            if e != QUITTING_MEANWHILE && !e.contains("WebView2 Runtime") {
                message(&e);
            }
        }
    }

    /// Shows the window on `what`'s route, making it when there is none; answers once it exists
    /// (contracts/window.md §1). `launched`: the launcher's start, in Unix ms (research R-14).
    pub fn open_for(what: Option<String>, launched: Option<u128>) -> Result<(), String> {
        let route = route_of(what);
        // Being made: forward only, the route kept for when the page is there (this comes
        // first: the window exists before its web view).
        if OPENING.with(Cell::get) {
            if route.is_some() {
                PENDING_ROUTE.with(|p| *p.borrow_mut() = route);
            }
            if let Some(h) = hwnd() {
                frame::front(h);
            }
            return Ok(());
        }
        if let Some(h) = hwnd() {
            if let Some(r) = &route {
                go_to(r);
            }
            frame::front(h);
            return Ok(());
        }
        if !crate::platform::display::interactive() {
            return Err(
                "Fallow was started without a desktop: stop it (`fallow stop`) and start it again from the desktop"
                    .into(),
            );
        }
        let Some(cfg) = CONFIG.get() else {
            return Err("Fallow's window is not set up".into());
        };
        ASKED.with(|a| a.set(Some(Instant::now())));
        LAUNCHED.with(|l| l.set(launched));
        OPENING.with(|o| o.set(true));
        let made = make(cfg, route);
        OPENING.with(|o| o.set(false));
        made?;
        if let Some(r) = PENDING_ROUTE.with(|p| p.borrow_mut().take()) {
            go_to(&r);
        }
        // Closed while it was being made: closed now that it is (review).
        if CLOSE_AFTER_OPEN.with(Cell::take) {
            if let Some(h) = hwnd() {
                close_asked(h);
            }
        }
        Ok(())
    }

    fn make(cfg: &Config, route: Option<String>) -> Result<(), String> {
        // No runtime: say what to install, offer Microsoft's page; never a browser instead of
        // the window (research R-12).
        if view::runtime().is_err() {
            let text: Vec<u16> = "Fallow's window needs the Microsoft Edge WebView2 Runtime, which is not installed.\n\nOpen Microsoft's download page in your browser?"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let title: Vec<u16> = "Fallow".encode_utf16().chain(Some(0)).collect();
            // SAFETY: a modal message box with live wide strings.
            let yes = unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    title.as_ptr(),
                    windows_sys::Win32::UI::WindowsAndMessaging::MB_YESNO
                        | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONWARNING,
                )
            } == windows_sys::Win32::UI::WindowsAndMessaging::IDYES;
            if yes {
                let _ = crate::platform::open_browser(
                    "https://developer.microsoft.com/microsoft-edge/webview2/",
                );
            }
            return Err(
                "Fallow's window needs the Microsoft Edge WebView2 Runtime, which is not installed"
                    .into(),
            );
        }
        let dark = frame::is_dark(crate::studio::files::theme(&cfg.data));
        let place = frame::opening_place(state::load(&cfg.data));
        let hwnd = frame::create(&place, dark)?;
        LIVE.with(|l| *l.borrow_mut() = Some(Live { hwnd, view: None }));
        crate::platform::picker::OWNER.store(hwnd as isize, std::sync::atomic::Ordering::Release);
        let env = match view::environment(&cfg.data) {
            Ok(e) => e,
            Err(e) => {
                CLOSE_AFTER_OPEN.with(Cell::take);
                // SAFETY: this thread's window.
                unsafe { DestroyWindow(hwnd) };
                return Err(if view::quitting(&e) {
                    QUITTING_MEANWHILE.to_string()
                } else {
                    e
                });
            }
        };
        let url = format!("{}{}", nav::HOME, route.unwrap_or_default());
        let parent = Parent(hwnd);
        let built = view::build(
            &parent,
            env,
            &cfg.service,
            &url,
            bounds(hwnd),
            dark,
            move |m| page_said(hwnd, m),
        );
        match built {
            Ok(v) => {
                LIVE.with(|l| {
                    if let Some(live) = l.borrow_mut().as_mut() {
                        live.view = Some(v);
                    }
                });
                Ok(())
            }
            Err(e) => {
                // The window may be gone already (closed while being made).
                if self::hwnd() == Some(hwnd) {
                    // SAFETY: this thread's window.
                    unsafe { DestroyWindow(hwnd) };
                }
                Err(if view::quitting(&e) || CLOSE_AFTER_OPEN.with(Cell::take) {
                    QUITTING_MEANWHILE.to_string()
                } else {
                    e
                })
            }
        }
    }

    /// The window as the web view's parent.
    struct Parent(HWND);

    impl raw_window_handle::HasWindowHandle for Parent {
        fn window_handle(
            &self,
        ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            let h = std::num::NonZeroIsize::new(self.0 as isize)
                .ok_or(raw_window_handle::HandleError::Unavailable)?;
            let raw = raw_window_handle::RawWindowHandle::Win32(
                raw_window_handle::Win32WindowHandle::new(h),
            );
            // SAFETY: the window outlives the web view, which is dropped before it is destroyed.
            Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
        }
    }

    fn bounds(hwnd: HWND) -> wry::Rect {
        let (w, h) = frame::client_size(hwnd);
        wry::Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(w, h).into(),
        }
    }

    /// Sets the page's route.
    fn go_to(route: &str) {
        let js = format!(
            "location.hash = {};",
            serde_json::Value::String(route.to_string())
        );
        LIVE.with(|l| {
            if let Some(v) = l.borrow().as_ref().and_then(|l| l.view.as_ref()) {
                let _ = v.evaluate_script(&js);
            }
        });
    }

    /// A message from the page (contracts/window.md §4); anything else is ignored.
    fn page_said(hwnd: HWND, m: &str) {
        match m {
            "ready" => {
                // A page drawn (again) while it was asked about its changes has none: the close
                // or Quit goes on (review).
                if CLOSING.with(Cell::get) {
                    page_let_go(hwnd);
                }
                let asked = ASKED.with(Cell::take);
                let launched = LAUNCHED.with(Cell::take);
                if std::env::var_os("FALLOW_BENCH").is_some() {
                    let ms = match (launched, asked) {
                        (Some(l), _) => unix_ms().saturating_sub(l),
                        (None, Some(t)) => t.elapsed().as_millis(),
                        _ => return,
                    };
                    eprintln!("window ready {ms} ms");
                }
            }
            // The page's answer about unsaved changes (research R-9, contracts/window.md §4).
            "close" if CLOSING.with(Cell::get) => page_let_go(hwnd),
            "asking" => {
                // SAFETY: this thread's window and its timer.
                unsafe { KillTimer(hwnd, CLOSE_TIMER) };
            }
            "kept" => {
                CLOSING.with(|c| c.set(false));
                if QUITTING.with(Cell::get) {
                    quit_done(false);
                }
            }
            "theme:light" | "theme:dark" | "theme:system" => {
                frame::set_dark(hwnd, frame::is_dark(&m["theme:".len()..]));
            }
            _ => {
                // 016 (FR-034, contracts/window-macos.md §4): the engineer clicked a link outside
                // the Studio.
                if let Some(url) = m.strip_prefix("open:") {
                    if let Some(u) = nav::open_link(url) {
                        let _ = crate::platform::open_browser(&u);
                    }
                    return;
                }
                if let Some(s) = m.strip_prefix("activity:") {
                    if std::env::var_os("FALLOW_BENCH").is_some() {
                        eprintln!("activity {s} {}", unix_ms());
                    }
                }
            }
        }
    }

    fn unix_ms() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
    }

    /// The page let go (it answered `close`, or did not answer within 3 s, or was drawn again):
    /// Quit goes on past the page, or the window closes.
    fn page_let_go(hwnd: HWND) {
        CLOSING.with(|c| c.set(false));
        // SAFETY: this thread's window and its timer.
        unsafe { KillTimer(hwnd, CLOSE_TIMER) };
        if QUITTING.with(Cell::get) && !QUIT_PAST_PAGE.with(Cell::get) {
            after_page();
        } else {
            close_now(hwnd);
        }
    }

    /// 1. The window got the focus: the page gets it (review: `build_as_child` leaves it to us).
    pub(super) fn focused() {
        LIVE.with(|l| {
            if let Some(v) = l.borrow().as_ref().and_then(|l| l.view.as_ref()) {
                let _ = v.focus();
            }
        });
    }

    /// 1. The window moved: WebView2 is told, for its popups and accessibility (review).
    pub(super) fn moved() {
        LIVE.with(|l| {
            if let Some(v) = l.borrow().as_ref().and_then(|l| l.view.as_ref()) {
                use wry::WebViewExtWindows;
                // SAFETY: a WebView2 call on the thread that made it.
                let _ = unsafe { v.controller().NotifyParentWindowPositionChanged() };
            }
        });
    }

    pub(super) fn resized(hwnd: HWND) {
        LIVE.with(|l| {
            if let Some(v) = l.borrow().as_ref().and_then(|l| l.view.as_ref()) {
                let _ = v.set_bounds(bounds(hwnd));
            }
        });
    }

    pub(super) fn system_theme_changed(hwnd: HWND) {
        let Some(cfg) = CONFIG.get() else { return };
        if crate::studio::files::theme(&cfg.data) == "system" {
            frame::set_dark(hwnd, frame::system_dark());
        }
    }

    /// The engineer closes the window: the page is asked about unsaved changes first (009's
    /// question does not run when a host closes its web view); it answers `close`, or `asking`
    /// then `close` or `kept`; without an answer within 3 s the window closes anyway
    /// (research R-9).
    pub(super) fn close_asked(hwnd: HWND) {
        if OPENING.with(Cell::get) {
            CLOSE_AFTER_OPEN.with(|c| c.set(true));
            return;
        }
        // Quit's dialog is up: the window waits for it (it owns the dialog).
        if QUIT_PAST_PAGE.with(Cell::get) {
            return;
        }
        let asked = LIVE.with(|l| {
            l.borrow().as_ref().and_then(|l| l.view.as_ref()).map(|v| {
                v.evaluate_script("Fallow.closeAsked && Fallow.closeAsked();")
                    .is_ok()
            })
        });
        if asked == Some(true) {
            CLOSING.with(|c| c.set(true));
            // SAFETY: this thread's window and its timer.
            unsafe { SetTimer(hwnd, CLOSE_TIMER, 3000, None) };
        } else {
            close_now(hwnd);
        }
    }

    /// No answer from the page within 3 s: it is hung or gone; the window closes, or Quit goes
    /// on.
    pub(super) fn close_timer(hwnd: HWND) {
        if CLOSING.with(Cell::get) {
            page_let_go(hwnd);
        } else {
            // SAFETY: this thread's window and its timer.
            unsafe { KillTimer(hwnd, CLOSE_TIMER) };
        }
    }

    /// 014 (FR-009, research R-9): Quit, from the icon, the system menu or `fallow quit`
    /// (`reply` hears whether Fallow quits). The page's unsaved changes first, then the running
    /// work named (asked on a worker thread, the dialog there too), then the service stops.
    pub fn quit(reply: Option<std::sync::mpsc::Sender<bool>>) {
        if let Some(r) = reply {
            QUIT_REPLY.with(|q| q.borrow_mut().push(r));
        }
        if QUITTING.with(Cell::get) {
            // Asked again while the page was asked: ask it again, which recovers a page that
            // died meanwhile (no answer: the 3 s timer); past the page, nothing to do.
            if QUIT_PAST_PAGE.with(Cell::get) {
                return;
            }
        }
        QUITTING.with(|q| q.set(true));
        let asked = LIVE.with(|l| {
            l.borrow().as_ref().and_then(|l| {
                l.view.as_ref().map(|v| {
                    (
                        l.hwnd,
                        v.evaluate_script("Fallow.closeAsked && Fallow.closeAsked();")
                            .is_ok(),
                    )
                })
            })
        });
        match asked {
            Some((hwnd, true)) => {
                CLOSING.with(|c| c.set(true));
                // SAFETY: this thread's window and its timer.
                unsafe { SetTimer(hwnd, CLOSE_TIMER, 3000, None) };
            }
            _ => after_page(),
        }
    }

    /// Quit, once the page let go: the window's place kept, the running work asked for and named
    /// on a worker thread (the UI thread never waits on the engine), and the service stopped on
    /// the engineer's word.
    fn after_page() {
        let Some(cfg) = CONFIG.get() else {
            quit_done(false);
            return;
        };
        QUIT_PAST_PAGE.with(|q| q.set(true));
        let owner = hwnd();
        if let Some(p) = owner.and_then(frame::placement) {
            let _ = state::save(&cfg.data, &p);
        }
        let owner = owner.map_or(0, |h| h as isize);
        let tx = cfg.service.tx.clone();
        let spawned = std::thread::Builder::new()
            .name("quit".into())
            .spawn(move || {
                // An error is not "nothing runs": then the dialog asks without the list (review).
                let names = match crate::service::ask(&tx, crate::run::engine::Request::Running) {
                    crate::run::engine::Reply::Ok(v) => super::quit::names(&v),
                    crate::run::engine::Reply::Err(_) => {
                        vec!["(Fallow could not tell what is running)".to_string()]
                    }
                };
                let go = names.is_empty() || super::quit::confirm(owner, &names);
                crate::platform::notify::on_ui(Box::new(move || quit_done(go)));
            });
        if spawned.is_err() {
            quit_done(false);
        }
    }

    /// Quit's outcome: told to who asked; when Fallow quits and nobody asked over the pipe, the
    /// service stops now (a pipe that asked stops it once its answer is written).
    fn quit_done(quits: bool) {
        QUITTING.with(|q| q.set(false));
        QUIT_PAST_PAGE.with(|q| q.set(false));
        let replies = QUIT_REPLY.with(|q| std::mem::take(&mut *q.borrow_mut()));
        let told = replies.iter().filter(|r| r.send(quits).is_ok()).count();
        if quits && told == 0 {
            if let Some(cfg) = CONFIG.get() {
                let _ = cfg.service.tx.send(crate::run::engine::Msg::Shutdown);
            }
        }
    }

    /// Windows ends the session: the window's place kept; the service stops through the icon's
    /// thread, asking nothing (research R-9).
    pub(super) fn session_ends(hwnd: HWND) {
        if let (Some(cfg), Some(p)) = (CONFIG.get(), frame::placement(hwnd)) {
            let _ = state::save(&cfg.data, &p);
        }
    }

    /// Closes the window: its place kept, the web view dropped (its processes end), the window
    /// destroyed. The service and its runs stay (research R-9).
    fn close_now(hwnd: HWND) {
        CLOSING.with(|c| c.set(false));
        // SAFETY: this thread's window and its timer.
        unsafe { KillTimer(hwnd, CLOSE_TIMER) };
        if let (Some(cfg), Some(p)) = (CONFIG.get(), frame::placement(hwnd)) {
            if let Err(e) = state::save(&cfg.data, &p) {
                eprintln!("fallow: {e}");
            }
        }
        let view = LIVE.with(|l| l.borrow_mut().as_mut().and_then(|l| l.view.take()));
        drop(view);
        // SAFETY: this thread's window.
        unsafe { DestroyWindow(hwnd) };
    }

    pub(super) fn destroyed(hwnd: HWND) {
        let _ = crate::platform::picker::OWNER.compare_exchange(
            hwnd as isize,
            0,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        );
        LIVE.with(|l| {
            let mut l = l.borrow_mut();
            if l.as_ref().is_some_and(|x| x.hwnd == hwnd) {
                *l = None;
            }
        });
    }
}
