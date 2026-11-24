//! 016 (data-model "Window", contracts/window-macos.md §1, research M-11, T014, T018): the window
//! on macOS, on the main thread: at most one, made when asked for, ended with its web view when
//! closed. 014's states (closed → opening → open → closing → closed) as on Windows.
//!
//! What the page and WebKit tell the window arrives inside WebKit's own calls; it is handled on
//! the main queue afterwards (`on_ui`), so the web view is never ended from inside its own call.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;
use wry::WebView;

use super::{alert, frame, menu, view};
use crate::window::{nav, state};

/// 016 (`automation` builds only, T047): a snapshot's answer: the page's report with the PNG
/// (base64) added.
#[cfg(feature = "automation")]
pub type SnapReply = std::sync::mpsc::Sender<Result<serde_json::Value, String>>;

/// What the window needs from the service, set once at its start.
struct Config {
    data: PathBuf,
    service: view::Service,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Sets what the window needs; the service calls it before AppKit opens anything.
pub fn configure(data: PathBuf, service: view::Service) {
    let _ = CONFIG.set(Config { data, service });
}

/// The window that is open.
struct Live {
    window: Retained<NSWindow>,
    _delegate: Retained<frame::Delegate>,
    view: Option<WebView>,
    /// The place it opened at (its windowed frame, kept while in full screen).
    kept: Option<state::MacPlace>,
}

thread_local! {
    static LIVE: RefCell<Option<Live>> = const { RefCell::new(None) };
    /// When the last opening was asked for, for `window ready` (014 research R-14).
    static ASKED: Cell<Option<Instant>> = const { Cell::new(None) };
    static LAUNCHED: Cell<Option<u128>> = const { Cell::new(None) };
    /// The page was asked about unsaved changes before closing (014 research R-9).
    static CLOSING: Cell<bool> = const { Cell::new(false) };
    /// Each close asked counts up; a 3 s timer acts only for its own (`asking` counts up too).
    static CLOSE_ASK: Cell<u64> = const { Cell::new(0) };
    /// The page's zoom (View menu).
    static ZOOM: Cell<f64> = const { Cell::new(1.0) };
    /// In full screen, or going in or out: the windowed place is not taken then.
    static FULL_SCREEN: Cell<bool> = const { Cell::new(false) };
    /// Quit is under way (014 research R-9); who asked waits for the outcome.
    static QUITTING: Cell<bool> = const { Cell::new(false) };
    /// Quit has passed the page: its alert is up (or the service is stopping); a close now
    /// waits for it.
    static QUIT_PAST_PAGE: Cell<bool> = const { Cell::new(false) };
    /// 016 (`automation` builds only, T047): the snapshot under way: its number and who waits.
    #[cfg(feature = "automation")]
    static SNAP: RefCell<Option<(u64, SnapReply)>> = const { RefCell::new(None) };
    static QUIT_REPLY: RefCell<Vec<std::sync::mpsc::Sender<bool>>> = const { RefCell::new(Vec::new()) };
}

/// The route in what was asked for: `#/…` itself, or after the `#` of a notification's address.
fn route_of(what: Option<String>) -> Option<String> {
    let w = what?;
    let i = w.find("#/")?;
    Some(w[i..].to_string())
}

fn mtm() -> Option<MainThreadMarker> {
    MainThreadMarker::new()
}

/// The window, when it is open (for sheets on it). Main thread.
pub fn window() -> Option<Retained<NSWindow>> {
    LIVE.with(|l| l.borrow().as_ref().map(|l| l.window.clone()))
}

/// The menu bar icon's, a notification's, the Dock's or the socket's request without an answer:
/// shows the window on `what`'s route; an error is shown as a message (main thread only).
pub fn open(what: Option<String>) {
    if let Err(e) = open_for(what, None) {
        if let Some(mtm) = mtm() {
            alert::message(mtm, "Fallow", &e);
        }
    }
}

/// Shows the window on `what`'s route, making it when there is none; answers once it exists
/// (contracts/window-macos.md §1). `launched`: the command line's start, in Unix ms.
pub fn open_for(what: Option<String>, launched: Option<u128>) -> Result<(), String> {
    let Some(mtm) = mtm() else {
        return Err("Fallow's window is asked for off the main thread".into());
    };
    let route = route_of(what);
    let shown = LIVE.with(|l| {
        l.borrow().as_ref().map(|live| {
            if let (Some(r), Some(v)) = (&route, &live.view) {
                view::go_to(v, r);
            }
            frame::front(&live.window);
        })
    });
    if shown.is_some() {
        return Ok(());
    }
    if !crate::platform::display::interactive() {
        return Err(
            "Fallow was started without a display: stop it (`fallow stop`) and open Fallow from the Mac's desktop"
                .into(),
        );
    }
    let Some(cfg) = CONFIG.get() else {
        return Err("Fallow's window is not set up".into());
    };
    ASKED.with(|a| a.set(Some(Instant::now())));
    LAUNCHED.with(|l| l.set(launched));
    make(mtm, cfg, route)
}

fn make(mtm: MainThreadMarker, cfg: &Config, route: Option<String>) -> Result<(), String> {
    let theme = crate::studio::files::theme(&cfg.data);
    let dark = frame::is_dark(mtm, theme);
    let saved = state::load_mac(&cfg.data);
    let place = frame::opening_place(mtm, saved);
    let delegate = frame::Delegate::new(
        mtm,
        frame::Events {
            close_asked: || {
                crate::platform::notify::on_ui(Box::new(close_asked));
            },
            moved,
            full_screen: |on| FULL_SCREEN.with(|f| f.set(on)),
        },
    );
    let window = frame::create(mtm, &place, theme, &delegate);
    let url = format!("{}{}", nav::HOME, route.unwrap_or_default());
    let built = view::build(
        &frame::Parent(&window),
        &cfg.service,
        &url,
        dark,
        |m| {
            let m = m.to_string();
            crate::platform::notify::on_ui(Box::new(move || page_said(&m)));
        },
        || {
            crate::platform::notify::on_ui(Box::new(page_ended));
        },
    );
    let v = match built {
        Ok(v) => v,
        Err(e) => {
            window.setDelegate(None);
            window.close();
            return Err(e);
        }
    };
    ZOOM.with(|z| z.set(1.0));
    FULL_SCREEN.with(|f| f.set(false));
    LIVE.with(|l| {
        *l.borrow_mut() = Some(Live {
            window: window.clone(),
            _delegate: delegate,
            view: Some(v),
            kept: Some(place),
        });
    });
    frame::show(&window, &place);
    LIVE.with(|l| {
        if let Some(v) = l.borrow().as_ref().and_then(|l| l.view.as_ref()) {
            let _ = v.focus();
        }
    });
    Ok(())
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// A message from the page (014 contracts/window.md §4, 016 §4); anything else is ignored.
fn page_said(m: &str) {
    match m {
        "ready" => {
            // A page drawn (again) while it was asked about its changes has none: the close
            // goes on.
            if CLOSING.with(Cell::get) {
                page_let_go();
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
        "close" if CLOSING.with(Cell::get) => page_let_go(),
        // A question is open: the 3 s timer stops; for Quit, the window comes forward so the
        // question is seen (review).
        "asking" => {
            CLOSE_ASK.with(|c| c.set(c.get() + 1));
            if QUITTING.with(Cell::get) {
                LIVE.with(|l| {
                    if let Some(live) = l.borrow().as_ref() {
                        frame::front(&live.window);
                    }
                });
            }
        }
        "kept" => {
            CLOSING.with(|c| c.set(false));
            if QUITTING.with(Cell::get) {
                quit_done(false);
            }
        }
        "theme:light" | "theme:dark" | "theme:system" => LIVE.with(|l| {
            if let Some(live) = l.borrow().as_ref() {
                frame::set_theme(&live.window, &m["theme:".len()..]);
            }
        }),
        _ => {
            #[cfg(feature = "automation")]
            if let Some(rest) = m.strip_prefix("snap:") {
                snapped(rest);
                return;
            }
            if let Some(url) = m.strip_prefix("open:") {
                view::open_link(url);
            } else if let Some(s) = m.strip_prefix("activity:") {
                if std::env::var_os("FALLOW_BENCH").is_some() {
                    eprintln!("activity {s} {}", unix_ms());
                }
            }
        }
    }
}

/// The window moved or was resized (or is going full screen): its windowed place is kept, so
/// the frame saved from full screen is the latest one (review). Inside AppKit's call: no borrow
/// is held elsewhere then.
fn moved() {
    if FULL_SCREEN.with(Cell::get) {
        return;
    }
    LIVE.with(|l| {
        if let Ok(mut l) = l.try_borrow_mut() {
            if let Some(live) = l.as_mut() {
                let p = frame::place(&live.window, live.kept);
                if !p.full_screen {
                    live.kept = Some(p);
                }
            }
        }
    });
}

/// The engineer closes the window (⌘W, the red button): the page is asked about unsaved changes
/// first; it answers `close`, or `asking` then `close` or `kept`; without an answer within 3 s
/// the window closes anyway (014 research R-9).
fn close_asked() {
    // Quit's alert is up: the window waits for it.
    if CLOSING.with(Cell::get) || QUIT_PAST_PAGE.with(Cell::get) {
        return;
    }
    ask_page();
}

/// Asks the page about its unsaved changes (`Fallow.closeAsked`), with the 3 s timer; without a
/// page that can answer, it lets go at once.
fn ask_page() {
    let asked = LIVE.with(|l| {
        l.borrow().as_ref().and_then(|l| l.view.as_ref()).map(|v| {
            v.evaluate_script("Fallow.closeAsked && Fallow.closeAsked();")
                .is_ok()
        })
    });
    if asked != Some(true) {
        close_now();
        return;
    }
    CLOSING.with(|c| c.set(true));
    let ask = CLOSE_ASK.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    let _ = std::thread::Builder::new()
        .name("close-timer".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            crate::platform::notify::on_ui(Box::new(move || {
                if CLOSING.with(Cell::get) && CLOSE_ASK.with(Cell::get) == ask {
                    page_let_go();
                }
            }));
        });
}

/// The page let go (it answered `close`, did not answer within 3 s, or was drawn again): Quit
/// goes on past the page, or the window closes.
fn page_let_go() {
    CLOSING.with(|c| c.set(false));
    CLOSE_ASK.with(|c| c.set(c.get() + 1));
    if QUITTING.with(Cell::get) && !QUIT_PAST_PAGE.with(Cell::get) {
        after_page();
    } else {
        close_now();
    }
}

/// 016 (FR-009, research M-11, T021): Quit, from ⌘Q, the app menu, the menu bar icon, the Dock
/// (all through `applicationShouldTerminate`) or `fallow quit` (`reply` hears whether Fallow
/// quits). The page's unsaved changes first, then the running work named in an alert, then the
/// engine stops as `fallow stop` does; AppKit's loop ends once it has.
pub fn quit(reply: Option<std::sync::mpsc::Sender<bool>>) {
    if let Some(r) = reply {
        QUIT_REPLY.with(|q| q.borrow_mut().push(r));
    }
    if QUIT_PAST_PAGE.with(Cell::get) {
        // The alert is up: bring it forward.
        if let Some(mtm) = mtm() {
            objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
        }
        return;
    }
    if QUITTING.with(Cell::get) && CLOSING.with(Cell::get) {
        // The page is being asked: it answers that question.
        return;
    }
    QUITTING.with(|q| q.set(true));
    if LIVE.with(|l| l.borrow().as_ref().is_some_and(|l| l.view.is_some())) {
        ask_page();
    } else {
        after_page();
    }
}

/// Quit, once the page let go: the window's place kept, the running work asked for on a worker
/// thread (the main thread never waits on the engine) and named in an alert.
fn after_page() {
    let Some(cfg) = CONFIG.get() else {
        quit_done(false);
        return;
    };
    QUIT_PAST_PAGE.with(|q| q.set(true));
    LIVE.with(|l| {
        if let Some(live) = l.borrow().as_ref() {
            let _ = state::save_mac(&cfg.data, &frame::place(&live.window, live.kept));
        }
    });
    let tx = cfg.service.tx.clone();
    let spawned = std::thread::Builder::new()
        .name("quit".into())
        .spawn(move || {
            // An error is not "nothing runs": then the alert asks without the list.
            let names = match crate::service::ask(&tx, crate::run::engine::Request::Running) {
                crate::run::engine::Reply::Ok(v) => crate::window::quit::names(&v),
                crate::run::engine::Reply::Err(_) => {
                    vec!["(Fallow could not tell what is running)".to_string()]
                }
            };
            let asked = crate::platform::notify::on_ui(Box::new(move || {
                if names.is_empty() {
                    quit_done(true);
                    return;
                }
                let Some(mtm) = mtm() else {
                    quit_done(false);
                    return;
                };
                let window = LIVE.with(|l| l.borrow().as_ref().map(|l| l.window.clone()));
                alert::confirm_quit(mtm, window.as_deref(), &names, quit_done);
            }));
            if !asked {
                // AppKit ended meanwhile: the service is stopping anyway.
                crate::platform::notify::on_ui(Box::new(|| quit_done(false)));
            }
        });
    if spawned.is_err() {
        quit_done(false);
    }
}

/// Quit's outcome: told to who asked; when Fallow quits and nobody asked over the socket, the
/// engine stops now (a socket that asked stops it once its answer is written).
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

/// 016 (`automation` builds only, T047): the window at `route` in `theme`, after `script`: the
/// page's report and the web view's picture, to `reply`. Needs the window open.
#[cfg(feature = "automation")]
pub fn snapshot(route: &str, theme: &str, script: &str, reply: SnapReply) {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let sent = LIVE.with(|l| {
        let l = l.borrow();
        let Some(live) = l.as_ref() else {
            return false;
        };
        let Some(v) = live.view.as_ref() else {
            return false;
        };
        frame::set_theme(&live.window, theme);
        view::snapshot_page(v, n, route, theme, script);
        true
    });
    if sent {
        SNAP.with(|s| *s.borrow_mut() = Some((n, reply)));
    } else {
        let _ = reply.send(Err("no window: open it first".into()));
    }
}

/// The page's `snap:<n>:<report>`: the picture is taken now, then both go to who asked.
#[cfg(feature = "automation")]
fn snapped(rest: &str) {
    let Some((n, report)) = rest.split_once(':') else {
        return;
    };
    let Some((want, reply)) = SNAP.with(|s| s.borrow_mut().take()) else {
        return;
    };
    if n.parse::<u64>().ok() != Some(want) {
        SNAP.with(|s| *s.borrow_mut() = Some((want, reply)));
        return;
    }
    let mut report: serde_json::Value = match serde_json::from_str(report) {
        Ok(v) => v,
        Err(e) => {
            let _ = reply.send(Err(format!("the page's report: {e}")));
            return;
        }
    };
    let taken = LIVE.with(|l| {
        l.borrow()
            .as_ref()
            .and_then(|live| live.view.as_ref())
            .map(|v| {
                let reply = reply.clone();
                view::snapshot_png(v, move |png| {
                    let _ = reply.send(png.map(|png| {
                        report["png"] = serde_json::Value::String(base64(&png));
                        report
                    }));
                });
            })
            .is_some()
    });
    if !taken {
        let _ = reply.send(Err("the window closed".into()));
    }
}

/// Standard base64, for the PNG in a JSON answer.
#[cfg(feature = "automation")]
fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// 016 (`automation` builds only, T042): minimizes the window into the Dock, for the check that
/// a hidden page keeps getting the Studio's changes.
#[cfg(feature = "automation")]
pub fn perform_miniaturize() -> bool {
    LIVE.with(|l| {
        l.borrow()
            .as_ref()
            .map(|live| live.window.miniaturize(None))
            .is_some()
    })
}

/// 016 (`automation` builds only, T025): closes the window as its red button does, for the
/// checks that drive the real window.
#[cfg(feature = "automation")]
pub fn perform_close() -> bool {
    LIVE.with(|l| {
        l.borrow()
            .as_ref()
            .map(|live| live.window.performClose(None))
            .is_some()
    })
}

/// Closes the window: its place kept, the page ended (its web content process with it), the
/// window let go. The service and its runs stay (research M-11).
fn close_now() {
    // A question to the page still open (the page died after `asking`): it ends with the window,
    // and a Quit that waited on it goes on (review).
    let was_asking = CLOSING.with(|c| c.replace(false));
    CLOSE_ASK.with(|c| c.set(c.get() + 1));
    if was_asking && QUITTING.with(Cell::get) && !QUIT_PAST_PAGE.with(Cell::get) {
        after_page();
    }
    let Some(live) = LIVE.with(|l| l.borrow_mut().take()) else {
        return;
    };
    // An alert on the window (Quit's) ends with it: its answer is Cancel.
    if let Some(sheet) = live.window.attachedSheet() {
        live.window.endSheet(&sheet);
    }
    if let Some(cfg) = CONFIG.get() {
        let p = frame::place(&live.window, live.kept);
        if let Err(e) = state::save_mac(&cfg.data, &p) {
            eprintln!("fallow: {e}");
        }
    }
    if let Some(v) = live.view {
        view::close(v);
    }
    live.window.setDelegate(None);
    live.window.close();
}

/// The engine has stopped (Quit's own end, or `fallow stop` while Quit's alert was up): who asked
/// to quit hears that Fallow quits, and an alert still up ends (review). Main thread, before
/// AppKit's loop stops.
pub fn engine_ended() {
    if QUITTING.with(Cell::get) || !QUIT_REPLY.with(|q| q.borrow().is_empty()) {
        quit_done(true);
    }
    LIVE.with(|l| {
        if let Some(live) = l.borrow().as_ref() {
            if let Some(sheet) = live.window.attachedSheet() {
                live.window.endSheet(&sheet);
            }
        }
    });
}

/// Fallow stops: the window closes as it would (its place kept, its page ended) before the
/// process ends, never in a thread-local's clean-up at exit (main thread, once AppKit's loop has
/// ended).
pub fn shutdown() {
    CLOSING.with(|c| c.set(false));
    close_now();
}

/// WebKit's web content process ended unasked: the native message, with Reload or Close; the
/// engine is untouched.
fn page_ended() {
    let Some(mtm) = mtm() else { return };
    if LIVE.with(|l| l.borrow().is_none()) {
        return;
    }
    if alert::page_ended(mtm) {
        LIVE.with(|l| {
            if let Some(v) = l.borrow().as_ref().and_then(|l| l.view.as_ref()) {
                let _ = v.reload();
            }
        });
    } else {
        close_now();
    }
}

/// The View menu (contracts/window-macos.md §5): the page's zoom, and Reload on the current
/// route.
pub fn view_cmd(cmd: menu::ViewCmd) {
    LIVE.with(|l| {
        let l = l.borrow();
        let Some(v) = l.as_ref().and_then(|l| l.view.as_ref()) else {
            return;
        };
        if cmd == menu::ViewCmd::Reload {
            let _ = v.reload();
            return;
        }
        let z = ZOOM.with(|z| {
            let next = match cmd {
                menu::ViewCmd::ZoomIn => (z.get() * 1.1).min(3.0),
                menu::ViewCmd::ZoomOut => (z.get() / 1.1).max(0.5),
                _ => 1.0,
            };
            z.set(next);
            next
        });
        let _ = v.zoom(z);
    });
}
