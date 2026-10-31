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
    Stop,
}

#[cfg(windows)]
pub use tray::Tray;

/// No notification icon on this platform yet.
#[cfg(not(windows))]
pub struct Tray;

#[cfg(not(windows))]
impl Tray {
    pub fn start(
        _home: String,
        _on: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _choose: Box<dyn Fn(Choice) + Send>,
    ) -> Result<Tray, String> {
        Err("notifications are not available on this platform yet".into())
    }
}

#[cfg(not(windows))]
impl Notify for Tray {
    fn show(&self, _: &Notice) {}
}

#[cfg(windows)]
mod tray {
    use super::{Choice, Notice, Notify};
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
    const NIN_SELECT: u32 = 0x400;
    const NIN_KEYSELECT: u32 = 0x401;
    const ID_OPEN: usize = 1;
    const ID_NOTIFY: usize = 2;
    const ID_STOP: usize = 3;

    /// What the icon's window shares with the rest of Fallow; one icon per process.
    struct Shared {
        home: String,
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

    /// The notification icon, on a thread of its own with its message loop.
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

    fn open(url: &str) {
        let _ = crate::platform::open_browser(url);
    }

    impl Tray {
        /// Adds the icon. `home` is the page "Open Fallow" opens; `on` is the notifications
        /// switch the menu turns; `choose` gets the menu's choices.
        pub fn start(
            home: String,
            on: Arc<AtomicBool>,
            choose: Box<dyn Fn(Choice) + Send>,
        ) -> Result<Tray, String> {
            if SHARED
                .set(Shared {
                    home,
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
                .name("notify".into())
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
            if !add_icon(hwnd) {
                DestroyWindow(hwnd);
                let _ = started.send(Err("cannot add the notification icon".into()));
                return;
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
                    if let Ok(mut last) = s.last.lock() {
                        *last = (Some(n.url.clone()), last.1 + 1);
                    }
                    // SAFETY: `d` lives for the call.
                    unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
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
                        open(url.as_deref().unwrap_or(&s.home));
                    }
                    // Version 4 sends these besides the plain mouse messages: one each per click.
                    NIN_SELECT | NIN_KEYSELECT => open(&s.home),
                    WM_CONTEXTMENU => menu(hwnd, s),
                    _ => {}
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
                unsafe { add_icon(hwnd) };
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
        let stop = wide("Stop Fallow");
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
            open(&s.home);
        }
        if let Ok(f) = s.choose.lock() {
            f(choice);
        }
    }
}
