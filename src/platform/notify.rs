//! Desktop notifications (007 research S-14): on Windows, a notification-area icon on a hidden
//! window of its own, whose balloons open the item's page when clicked; elsewhere, nothing yet.
//!
//! What a notification says is decided by `service::notify`: the project, the feature and what
//! is needed, never code or output (FR-003a). Nothing leaves the machine.

use std::sync::Mutex;

/// One notification: its two lines and the page it opens.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub url: String,
}

/// Where notifications go.
pub trait Notify: Send + Sync {
    fn show(&self, n: &Notice);
}

/// No notifications: tests, and platforms without them.
pub struct Off;

impl Notify for Off {
    fn show(&self, _: &Notice) {}
}

/// Keeps what would have been shown (tests).
#[derive(Default)]
pub struct Recorder(pub Mutex<Vec<Notice>>);

impl Notify for Recorder {
    fn show(&self, n: &Notice) {
        if let Ok(mut v) = self.0.lock() {
            v.push(n.clone());
        }
    }
}

/// What the engineer chose in the icon's menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Choice {
    Open,
    /// Notifications on (true) or off (false).
    Notifications(bool),
    /// Quit (014: with the confirmation of FR-009).
    Stop,
    /// 014 (research R-9): Windows ends the session (sign-out, shutdown): stop, ask nothing.
    EndSession,
}

#[cfg(windows)]
pub use tray::{Tray, on_ui, request_open};

/// What to open, from the icon, a notification or another thread: `None` for the dashboard,
/// otherwise a notification's address or a route (`#/…`). Runs on the UI thread (014), with no
/// lock held: what it does may pump messages and be asked again meanwhile (review).
pub type Open = std::sync::Arc<dyn Fn(Option<String>) + Send + Sync>;

/// Fallow's app identity for its notifications (008, the engineer's choice of 2026-11-01).
/// Windows shows a notification under the identity of the process that raised it; without one,
/// the header shows a generic program icon. This sets the process's identity and registers its
/// name and icon for the current user only: the key
/// `HKCU\Software\Classes\AppUserModelId\Fallow` (`DisplayName`, `IconUri`), whose icon is a copy
/// of Fallow's icon in `data`. Nothing else is written; deleting the key and the file undoes it.
/// Call it before the notification icon is added.
#[cfg(windows)]
pub fn register_identity(data: &std::path::Path) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    const ID: &str = "Fallow";
    const ICON: &[u8] = include_bytes!("../../assets/icon/fallow.ico");
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let icon = data.join("fallow.ico");
    if std::fs::read(&icon).ok().as_deref() != Some(ICON) {
        std::fs::write(&icon, ICON).map_err(|e| format!("cannot write {}: {e}", icon.display()))?;
    }
    let key = wide(&format!("Software\\Classes\\AppUserModelId\\{ID}"));
    // SAFETY: plain Win32 calls with live, NUL-terminated wide strings; the key is closed.
    unsafe {
        let mut h: HKEY = std::ptr::null_mut();
        let e = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut h,
            std::ptr::null_mut(),
        );
        if e != 0 {
            return Err(format!(
                "cannot register Fallow's notification identity (error {e})"
            ));
        }
        let path = icon.display().to_string();
        for (name, value) in [("DisplayName", ID), ("IconUri", path.as_str())] {
            let (n, v) = (wide(name), wide(value));
            RegSetValueExW(
                h,
                n.as_ptr(),
                0,
                REG_SZ,
                v.as_ptr().cast(),
                (v.len() * 2) as u32,
            );
        }
        RegCloseKey(h);
        let id = wide(ID);
        SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }
    Ok(())
}

/// No notification identity to register on this platform.
#[cfg(not(windows))]
pub fn register_identity(_data: &std::path::Path) -> Result<(), String> {
    Ok(())
}

/// 016 (research M-10, T019): on macOS, the menu bar icon (an `NSStatusItem`, on the main
/// thread, where AppKit runs). Notifications themselves come with T027.
#[cfg(target_os = "macos")]
pub struct Tray;

#[cfg(target_os = "macos")]
impl Tray {
    /// Adds the menu bar icon. Called on the main thread before AppKit's loop runs; without a
    /// display there is none.
    pub fn start(
        open: Open,
        on: std::sync::Arc<std::sync::atomic::AtomicBool>,
        choose: Box<dyn Fn(Choice) + Send>,
    ) -> Result<Tray, String> {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return Err("the menu bar icon needs the main thread".into());
        };
        if !crate::platform::display::interactive() {
            return Err("no display (as over SSH)".into());
        }
        // AppKit itself first: the status bar belongs to the shared application.
        let _ = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let _ = MAC_ON.set(std::sync::Arc::clone(&on));
        mac::init();
        crate::window::mac::menu::status_item(mtm, open, on, choose);
        Ok(Tray)
    }

    /// 016 (FR-014, T030): a dot on the icon while something needs the engineer.
    pub fn set_waiting(&self, waiting: bool) {
        on_ui(Box::new(move || {
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                crate::window::mac::menu::set_waiting(mtm, waiting);
            }
        }));
    }
}

#[cfg(target_os = "macos")]
impl Notify for Tray {
    /// 016 (T027): through `UserNotifications`, while the switch is on.
    fn show(&self, n: &Notice) {
        if MAC_ON
            .get()
            .is_some_and(|on| on.load(std::sync::atomic::Ordering::Relaxed))
        {
            mac::show(n);
        }
    }
}

/// 016: the notifications switch on macOS (the menu bar icon's and the file's).
#[cfg(target_os = "macos")]
static MAC_ON: std::sync::OnceLock<std::sync::Arc<std::sync::atomic::AtomicBool>> =
    std::sync::OnceLock::new();

/// 016 (research M-9): notifications through `UserNotifications`.
#[cfg(target_os = "macos")]
#[path = "notify_mac.rs"]
pub mod mac;

/// No notification icon on this platform yet.
#[cfg(not(any(windows, target_os = "macos")))]
pub struct Tray;

#[cfg(not(any(windows, target_os = "macos")))]
impl Tray {
    pub fn start(
        _open: Open,
        _on: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _choose: Box<dyn Fn(Choice) + Send>,
    ) -> Result<Tray, String> {
        Err("notifications are not available on this platform yet".into())
    }
}

/// Nothing to open on this platform from another thread: the command line opens the page.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn request_open(_route: Option<String>) {}

/// 016: asks the main thread, from any thread, to open the dashboard (`None`) or an item.
#[cfg(target_os = "macos")]
pub fn request_open(what: Option<String>) {
    on_ui(Box::new(move || crate::window::open(what)));
}

/// 016 (research M-2): runs `f` on the main thread, where AppKit runs, from any thread; `false`
/// when AppKit does not run (no display), and `f` is dropped.
#[cfg(target_os = "macos")]
pub fn on_ui(f: Box<dyn FnOnce() + Send>) -> bool {
    crate::window::mac::app::on_ui(f)
}

/// No UI thread on this platform: `false`, `f` is dropped.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn on_ui(_f: Box<dyn FnOnce() + Send>) -> bool {
    false
}

#[cfg(not(any(windows, target_os = "macos")))]
impl Notify for Tray {
    fn show(&self, _: &Notice) {}
}

#[cfg(windows)]
mod tray {
    use super::{Choice, Notice, Notify, Open};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON, NIIF_USER, NIM_ADD,
        NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK, NOTIFYICON_VERSION_4,
        NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const CALLBACK: u32 = WM_APP + 1;
    const SHOW: u32 = WM_APP + 2;
    const QUIT: u32 = WM_APP + 3;
    /// 014: something to open (`Shared::opens`), handled on this thread.
    const OPEN: u32 = WM_APP + 4;
    /// 014: the timer that adds the icon again while Explorer had none for it (review).
    const RETRY_ICON: usize = 1;
    /// 014: calls to run on this thread (`Shared::calls`, `on_ui`).
    const CALL: u32 = WM_APP + 5;
    const NIN_SELECT: u32 = 0x400;
    const NIN_KEYSELECT: u32 = 0x401;
    const ID_OPEN: usize = 1;
    const ID_NOTIFY: usize = 2;
    const ID_STOP: usize = 3;

    /// What the icon's window shares with the rest of Fallow; one icon per process.
    struct Shared {
        /// 014: opens the dashboard (`None`) or a notification's item, on this thread.
        open: Mutex<Open>,
        /// 014: what other threads asked to open, in order (`request_open`).
        opens: Mutex<VecDeque<Option<String>>>,
        /// 014: what other threads asked to run on this thread (`on_ui`).
        calls: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
        on: Arc<AtomicBool>,
        choose: Mutex<Box<dyn Fn(Choice) + Send>>,
        queue: Mutex<VecDeque<Notice>>,
        /// The page of the last balloon, and how many were shown since the last click: a click
        /// opens that page only when it can only be that balloon's, the dashboard otherwise
        /// (older balloons stay in the notification centre and their clicks come here too).
        last: Mutex<(Option<String>, u32)>,
    }

    static SHARED: OnceLock<Shared> = OnceLock::new();
    static HWND_: AtomicIsize = AtomicIsize::new(0);
    /// The message Explorer broadcasts when the taskbar comes back: the icon is added again.
    static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// Fallow's icon from its own resources (`assets/icon/fallow.res`, linked by `build.rs`), at
    /// the system's small size for the tray or its large size for a balloon, or Windows' default
    /// icon if it is missing. Each size is loaded once, at its
    /// own size (a shared load would hand every size the first one loaded), and kept for the life
    /// of the process.
    ///
    /// # Safety
    /// Plain Win32 calls.
    unsafe fn icon(large: bool) -> HICON {
        static SMALL: OnceLock<isize> = OnceLock::new();
        static LARGE: OnceLock<isize> = OnceLock::new();
        let cell = if large { &LARGE } else { &SMALL };
        *cell.get_or_init(|| {
            // SAFETY: the module of this process; resource 1 is the icon group.
            unsafe {
                let size = if large {
                    GetSystemMetrics(SM_CXICON).max(32)
                } else {
                    GetSystemMetrics(SM_CXSMICON)
                };
                let h = LoadImageW(
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::without_provenance::<u16>(1), // MAKEINTRESOURCE(1)
                    IMAGE_ICON,
                    size,
                    size,
                    LR_DEFAULTCOLOR,
                );
                if h.is_null() {
                    LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) as isize
                } else {
                    h as isize
                }
            }
        }) as HICON
    }

    /// Adds the icon to the notification area.
    ///
    /// # Safety
    /// `hwnd` is this thread's live window.
    unsafe fn add_icon(hwnd: HWND) -> bool {
        let mut d = data(hwnd);
        d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        d.uCallbackMessage = CALLBACK;
        // SAFETY: plain Win32 calls; `d` lives for them.
        unsafe {
            d.hIcon = icon(false);
            copy_into(&mut d.szTip, "Fallow");
            if Shell_NotifyIconW(NIM_ADD, &d) == 0 {
                return false;
            }
            d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &d);
        }
        true
    }

    /// 014 (research R-2): the UI thread, with its message loop: the notification icon, and what
    /// opens from it, from a notification and from other threads (`request_open`).
    pub struct Tray;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn copy_into(dst: &mut [u16], s: &str) {
        let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
        dst[..w.len()].copy_from_slice(&w);
        dst[w.len()] = 0;
    }

    fn data(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            ..Default::default()
        }
    }

    /// Opens on this thread what the icon, a notification or another thread asked for. The lock
    /// is held only to take the callback, never while it runs (review: it may pump messages,
    /// which can bring another open here).
    fn open(s: &Shared, what: Option<String>) {
        let f = s.open.lock().ok().map(|f| Arc::clone(&f));
        if let Some(f) = f {
            f(what);
        }
    }

    /// 014: asks the UI thread, from any thread, to open the dashboard (`None`) or an item.
    pub fn request_open(what: Option<String>) {
        let (Some(s), h) = (SHARED.get(), HWND_.load(Ordering::Acquire)) else {
            return;
        };
        if h == 0 {
            return;
        }
        if let Ok(mut q) = s.opens.lock() {
            q.push_back(what);
        }
        // SAFETY: posting to a window handle; a stale one only fails.
        unsafe { PostMessageW(h as HWND, OPEN, 0, 0) };
    }

    /// 014: runs `f` on the UI thread, from any thread; `false` when there is none.
    pub fn on_ui(f: Box<dyn FnOnce() + Send>) -> bool {
        let (Some(s), h) = (SHARED.get(), HWND_.load(Ordering::Acquire)) else {
            return false;
        };
        if h == 0 {
            return false;
        }
        if let Ok(mut q) = s.calls.lock() {
            q.push_back(f);
        }
        // SAFETY: posting to a window handle; a stale one only fails.
        unsafe { PostMessageW(h as HWND, CALL, 0, 0) != 0 }
    }

    impl Tray {
        /// Starts the UI thread and adds the icon. `open` opens the dashboard or an item;
        /// `on` is the notifications switch the menu turns; `choose` gets the menu's choices.
        /// The thread lives on when the icon cannot be added yet (the taskbar not ready): the
        /// icon is added when Explorer says the taskbar is there (`TaskbarCreated`).
        pub fn start(
            open: Open,
            on: Arc<AtomicBool>,
            choose: Box<dyn Fn(Choice) + Send>,
        ) -> Result<Tray, String> {
            if SHARED
                .set(Shared {
                    open: Mutex::new(open),
                    opens: Mutex::default(),
                    calls: Mutex::default(),
                    on,
                    choose: Mutex::new(choose),
                    queue: Mutex::default(),
                    last: Mutex::default(),
                })
                .is_err()
            {
                return Err("the notification icon is already there".into());
            }
            let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
            std::thread::Builder::new()
                .name("ui".into())
                .spawn(move || run(tx))
                .map_err(|e| format!("cannot start the notification icon: {e}"))?;
            rx.recv()
                .map_err(|_| "the notification icon did not start".to_string())??;
            Ok(Tray)
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            let h = HWND_.load(Ordering::Acquire);
            if h != 0 {
                // SAFETY: posting to a window handle; a stale one only fails.
                unsafe { PostMessageW(h as HWND, QUIT, 0, 0) };
            }
        }
    }

    impl Notify for Tray {
        fn show(&self, n: &Notice) {
            let (Some(s), h) = (SHARED.get(), HWND_.load(Ordering::Acquire)) else {
                return;
            };
            if h == 0 || !s.on.load(Ordering::Relaxed) {
                return;
            }
            if let Ok(mut q) = s.queue.lock() {
                q.push_back(n.clone());
            }
            // SAFETY: as in `drop`.
            unsafe { PostMessageW(h as HWND, SHOW, 0, 0) };
        }
    }

    fn run(started: std::sync::mpsc::Sender<Result<(), String>>) {
        let class = wide("FallowNotify");
        // SAFETY: plain Win32 calls with valid, live arguments; the window and the icon belong
        // to this thread, which runs their message loop.
        unsafe {
            // 014 (research R-2): a single-threaded apartment, with OLE's drag and drop, for the
            // web view the window is made with on this thread.
            windows_sys::Win32::System::Ole::OleInitialize(std::ptr::null_mut());
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            if RegisterClassExW(&wc) == 0 {
                let _ = started.send(Err("cannot register the notification window".into()));
                return;
            }
            // A hidden window, not shown: the icon's messages and its menu need one.
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if hwnd.is_null() {
                let _ = started.send(Err("cannot create the notification window".into()));
                return;
            }
            let tc = wide("TaskbarCreated");
            TASKBAR_CREATED.store(RegisterWindowMessageW(tc.as_ptr()), Ordering::Relaxed);
            // 014: without the icon yet (Explorer's taskbar not ready), the thread lives on and
            // adds it on `TaskbarCreated`.
            if !add_icon(hwnd) {
                eprintln!("fallow: the notification icon waits for the taskbar");
                SetTimer(hwnd, RETRY_ICON, 10_000, None);
            }
            HWND_.store(hwnd as isize, Ordering::Release);
            let _ = started.send(Ok(()));
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            HWND_.store(0, Ordering::Release);
        }
    }

    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        let Some(s) = SHARED.get() else {
            // SAFETY: the default handling of a message for this window.
            return unsafe { DefWindowProcW(hwnd, msg, w, l) };
        };
        match msg {
            SHOW => {
                let next = s.queue.lock().ok().and_then(|mut q| q.pop_front());
                if let Some(n) = next {
                    let mut d = data(hwnd);
                    d.uFlags = NIF_INFO;
                    // Fallow's logo in the notification, not Windows' information sign.
                    d.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
                    // SAFETY: plain Win32 calls.
                    d.hBalloonIcon = unsafe { icon(true) };
                    copy_into(&mut d.szInfoTitle, &n.title);
                    copy_into(&mut d.szInfo, &n.body);
                    // SAFETY: `d` lives for the call.
                    let shown = unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) } != 0;
                    // Counted only when shown: a click can then only be for one shown (review).
                    if shown {
                        if let Ok(mut last) = s.last.lock() {
                            *last = (Some(n.url.clone()), last.1 + 1);
                        }
                    }
                }
                0
            }
            CALLBACK => {
                // NOTIFYICON_VERSION_4: the event is in the low word of `l`.
                match (l as u32) & 0xFFFF {
                    NIN_BALLOONUSERCLICK => {
                        let url = s.last.lock().ok().and_then(|mut l| {
                            let one = l.1 == 1;
                            l.1 = 0;
                            l.0.clone().filter(|_| one)
                        });
                        open(s, url);
                    }
                    // Version 4 sends these besides the plain mouse messages: one each per click.
                    NIN_SELECT | NIN_KEYSELECT => open(s, None),
                    WM_CONTEXTMENU => menu(hwnd, s),
                    _ => {}
                }
                0
            }
            OPEN => {
                // Only the latest: earlier ones are behind it anyway (review).
                let next = s.opens.lock().ok().and_then(|mut q| {
                    let last = q.pop_back();
                    q.clear();
                    last
                });
                if let Some(what) = next {
                    open(s, what);
                }
                0
            }
            CALL => {
                // Each call taken out of the queue before it runs (it may pump messages).
                loop {
                    let next = s.calls.lock().ok().and_then(|mut q| q.pop_front());
                    match next {
                        Some(f) => f(),
                        None => break,
                    }
                }
                0
            }
            // 014 (research R-9): sign-out or shutdown: stop cleanly, ask nothing.
            WM_QUERYENDSESSION => 1,
            WM_ENDSESSION => {
                if w != 0 {
                    if let Ok(f) = s.choose.lock() {
                        f(Choice::EndSession);
                    }
                    // Windows may end the process once this returns: a moment for the engine to
                    // stop (the process exits sooner when it has).
                    std::thread::sleep(std::time::Duration::from_secs(5));
                }
                0
            }
            WM_TIMER if w == RETRY_ICON => {
                // SAFETY: this thread's window and its timer.
                unsafe {
                    if add_icon(hwnd) {
                        KillTimer(hwnd, RETRY_ICON);
                    }
                }
                0
            }
            QUIT => {
                let d = data(hwnd);
                // SAFETY: removing this window's own icon, then ending its loop.
                unsafe {
                    Shell_NotifyIconW(NIM_DELETE, &d);
                    DestroyWindow(hwnd);
                    PostQuitMessage(0);
                }
                0
            }
            m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
                // SAFETY: this thread's window.
                unsafe {
                    if add_icon(hwnd) {
                        KillTimer(hwnd, RETRY_ICON);
                    }
                }
                0
            }
            // SAFETY: as above.
            _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
        }
    }

    fn menu(hwnd: HWND, s: &Shared) {
        let on = s.on.load(Ordering::Relaxed);
        let open_ = wide("Open Fallow");
        let notify = wide("Notifications");
        let stop = wide("Quit Fallow");
        // SAFETY: a popup menu owned by this call, shown for this thread's window.
        let chosen = unsafe {
            let m = CreatePopupMenu();
            if m.is_null() {
                return;
            }
            AppendMenuW(m, MF_STRING, ID_OPEN, open_.as_ptr());
            AppendMenuW(
                m,
                MF_STRING | if on { MF_CHECKED } else { 0 },
                ID_NOTIFY,
                notify.as_ptr(),
            );
            AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(m, MF_STRING, ID_STOP, stop.as_ptr());
            let mut p = POINT { x: 0, y: 0 };
            GetCursorPos(&mut p);
            // The menu closes when the engineer clicks elsewhere only if the window is in front.
            SetForegroundWindow(hwnd);
            let c = TrackPopupMenu(
                m,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                p.x,
                p.y,
                0,
                hwnd,
                std::ptr::null(),
            );
            // So that the next click on the icon opens the menu again (as the docs ask).
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(m);
            c as usize
        };
        let choice = match chosen {
            ID_OPEN => Choice::Open,
            ID_NOTIFY => {
                s.on.store(!on, Ordering::Relaxed);
                Choice::Notifications(!on)
            }
            ID_STOP => Choice::Stop,
            _ => return,
        };
        if choice == Choice::Open {
            open(s, None);
        }
        if let Ok(f) = s.choose.lock() {
            f(choice);
        }
    }
}
