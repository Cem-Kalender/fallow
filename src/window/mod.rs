//! 014: Fallow's own window (research R-1 to R-9). The window itself is Windows only; the
//! decisions it is made of (`nav`, `state`) are plain and tested everywhere.

pub mod nav;
pub mod state;

#[cfg(windows)]
mod frame;
#[cfg(windows)]
pub mod view;

#[cfg(windows)]
pub use host::{configure, open};

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
        /// When the last opening was asked for, for `window ready` (research R-14).
        static ASKED: Cell<Option<Instant>> = const { Cell::new(None) };
        /// The page was asked about unsaved changes before closing (research R-9).
        static CLOSING: Cell<bool> = const { Cell::new(false) };
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

    /// Shows the window on `what`'s route, making it when there is none (UI thread only).
    pub fn open(what: Option<String>) {
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
            return;
        }
        if let Some(h) = hwnd() {
            if let Some(r) = &route {
                go_to(r);
            }
            frame::front(h);
            return;
        }
        let Some(cfg) = CONFIG.get() else {
            return;
        };
        ASKED.with(|a| a.set(Some(Instant::now())));
        OPENING.with(|o| o.set(true));
        let made = make(cfg, route);
        OPENING.with(|o| o.set(false));
        if let Err(e) = made {
            message(&e);
            return;
        }
        if let Some(r) = PENDING_ROUTE.with(|p| p.borrow_mut().take()) {
            go_to(&r);
        }
    }

    fn make(cfg: &Config, route: Option<String>) -> Result<(), String> {
        // No runtime: say what to install; never a browser instead (research R-12).
        if view::runtime().is_err() {
            return Err(
                "Fallow's window needs the Microsoft Edge WebView2 Runtime, which is not installed. \
                 Install it from Microsoft (search for \"WebView2 Runtime\"), then open Fallow again."
                    .into(),
            );
        }
        let dark = frame::is_dark(crate::studio::files::theme(&cfg.data));
        let place = state::initial(state::load(&cfg.data), frame::on_screen, frame::main_area());
        let hwnd = frame::create(&place, dark)?;
        LIVE.with(|l| *l.borrow_mut() = Some(Live { hwnd, view: None }));
        let env = view::environment(&cfg.data)?;
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
                // SAFETY: this thread's window.
                unsafe { DestroyWindow(hwnd) };
                Err(e)
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
                if std::env::var_os("FALLOW_BENCH").is_some() {
                    if let Some(t) = ASKED.with(Cell::take) {
                        eprintln!("window ready {} ms", t.elapsed().as_millis());
                    }
                }
            }
            // The page's answer about unsaved changes (research R-9, contracts/window.md §4).
            "close" if CLOSING.with(Cell::get) => close_now(hwnd),
            "asking" => {
                // SAFETY: this thread's window and its timer.
                unsafe { KillTimer(hwnd, CLOSE_TIMER) };
            }
            "kept" => CLOSING.with(|c| c.set(false)),
            "theme:light" | "theme:dark" | "theme:system" => {
                frame::set_dark(hwnd, frame::is_dark(&m["theme:".len()..]));
            }
            _ => {
                if let Some(s) = m.strip_prefix("activity:") {
                    if std::env::var_os("FALLOW_BENCH").is_some() {
                        let ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis());
                        eprintln!("activity {s} {ms}");
                    }
                }
            }
        }
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
        if CLOSING.with(Cell::get) {
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

    /// No answer from the page within 3 s: it is hung or gone; the window closes.
    pub(super) fn close_timer(hwnd: HWND) {
        // SAFETY: this thread's window and its timer.
        unsafe { KillTimer(hwnd, CLOSE_TIMER) };
        if CLOSING.with(Cell::get) {
            close_now(hwnd);
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
        LIVE.with(|l| {
            let mut l = l.borrow_mut();
            if l.as_ref().is_some_and(|x| x.hwnd == hwnd) {
                *l = None;
            }
        });
    }
}
