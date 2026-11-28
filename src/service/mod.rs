//! The one service per user (006 research R-1, FR-035): it holds every run, starts agents and
//! serves the run page. The command line reaches it over the user-only pipe; the page over
//! loopback HTTP with a per-launch secret (`http`).

pub mod answer;
pub mod bus;
#[cfg(any(not(any(windows, target_os = "macos")), feature = "automation"))]
pub mod http;
pub mod notify;

use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use serde_json::{Value, json};

use crate::config::{ConfigPaths, Settings};
use crate::mcp::pipe;
use crate::run::engine::{self, Ctx, Engine, Msg, Reply, Request};

pub const SERVICE_FILE: &str = "service.json";

/// 014 (research R-16): a string only the `automation` test build carries; CI checks a release
/// build has none of it.
#[cfg(feature = "automation")]
pub const AUTOMATION_MARKER: &str = "automation build: the Studio's test page";

/// Where Fallow keeps its data.
pub fn data_dir() -> Result<PathBuf, String> {
    crate::config::data_dir()
        .ok_or_else(|| "no folder for Fallow's data (LOCALAPPDATA is not set)".into())
}

/// 016 (research M-14, M-22): what the service says when a Fallow from before 016 still runs on
/// the old data folder: nothing is moved while it does.
pub const OLDER_RUNNING: &str = "an older Fallow is running: stop it with `fallow stop` (its runs resume in this version), then open Fallow again";

/// Whether a service listens on the socket at `sock`: a connection is accepted. A service that
/// is slow to answer still counts (review); only a missing socket, or one nobody listens on, is
/// stale.
#[cfg(unix)]
pub fn listening(sock: &std::path::Path) -> bool {
    std::os::unix::net::UnixStream::connect(sock).is_ok()
}

/// 016 (FR-032, research M-14): before anything opens the data folder on macOS, Fallow's data
/// moves out of `~/Library/Caches` once; not while a Fallow from before 016 still runs there
/// (`OLDER_RUNNING`). Called by `fallow` before it opens the service's log there, and by the
/// service itself. Elsewhere, and with `--data-dir`, nothing.
pub fn prepare_data(data: &std::path::Path) -> Result<(), String> {
    let Some(old) = crate::config::old_data_dir() else {
        return Ok(());
    };
    if crate::config::data_dir().as_deref() != Some(data) || !old.is_dir() {
        return Ok(());
    }
    #[cfg(unix)]
    if listening(&old.join("service.sock")) {
        return Err(OLDER_RUNNING.into());
    }
    if let Some(said) = crate::config::move_old_data(&old, data)? {
        eprintln!("fallow: {said}");
    }
    Ok(())
}

/// 016 (research M-22): `fallow stop` also stops a Fallow from before 016 still running on the old
/// data folder (macOS), so the engineer can let the new version move the data; it returns once
/// that service no longer listens (up to 60 s). `false` when none listened.
pub fn stop_older() -> bool {
    #[cfg(unix)]
    if let Some(old) = crate::config::old_data_dir() {
        use std::io::Write;
        let sock = old.join("service.sock");
        let Ok(mut s) = std::os::unix::net::UnixStream::connect(&sock) else {
            return false;
        };
        if s.write_all(b"{\"kind\":\"cli\",\"request\":{\"cmd\":\"stop\"}}\n")
            .is_err()
        {
            return false;
        }
        drop(s);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while listening(&sock) && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        return true;
    }
    false
}

/// 016 (research M-22, US4 AS6, T034): the running service's version when it is not this one:
/// its own, or "an older version" for a service from before the version was given (`status`
/// without `version`). `None` when it is this version.
pub fn other_version(status: &Value) -> Option<String> {
    match status["version"].as_str() {
        Some(env!("CARGO_PKG_VERSION")) => None,
        Some(v) => Some(format!("Fallow {v}")),
        None => Some("an older Fallow".into()),
    }
}

/// 016 (research M-22, T034): a running Fallow of another version quits first, with its own Quit
/// confirmation, so this one can start; runs resume in it. `Err` when the engineer cancelled.
#[cfg(any(windows, target_os = "macos"))]
pub fn replace_other_version() -> Result<(), String> {
    let Ok(v) = request(json!({ "cmd": "status" })) else {
        return Ok(());
    };
    let Some(other) = other_version(&v).filter(|_| v["ok"] == true) else {
        return Ok(());
    };
    eprintln!(
        "fallow: {other} is running; this is {}: it quits first (it asks when work is running)",
        env!("CARGO_PKG_VERSION")
    );
    match quit(true)? {
        true => Ok(()),
        false => Err(format!(
            "{other} keeps running (Quit was cancelled); nothing was changed"
        )),
    }
}

/// 016 (research M-14, M-22, T034): a Fallow from before 016 still running on the old data
/// folder (macOS) quits with the engineer's word (`confirm` hears the work that would pause;
/// asked only when there is any), so this one can move the data and start. `Ok(false)` when the
/// engineer said no; `Ok(true)` when none runs or it has quit.
#[cfg(unix)]
pub fn quit_older(confirm: impl Fn(&[String]) -> bool) -> Result<bool, String> {
    use std::io::{BufRead, Write};
    let Some(old) = crate::config::old_data_dir() else {
        return Ok(true);
    };
    let sock = old.join("service.sock");
    if !listening(&sock) {
        return Ok(true);
    }
    let ask = |req: Value| -> Result<Value, String> {
        let mut s = std::os::unix::net::UnixStream::connect(&sock)
            .map_err(|e| format!("cannot reach the older Fallow: {e}"))?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .map_err(|e| e.to_string())?;
        writeln!(s, "{}", json!({ "kind": "cli", "request": req }))
            .map_err(|e| format!("the older Fallow closed the connection: {e}"))?;
        let mut line = String::new();
        std::io::BufReader::new(s)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&line).map_err(|e| e.to_string())
    };
    let running = ask(json!({ "cmd": "quit" }))?;
    let names = crate::window::quit::names(&running["running"]);
    if !names.is_empty() && !confirm(&names) {
        return Ok(false);
    }
    ask(json!({ "cmd": "quit", "confirmed": true }))?;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while listening(&sock) {
        if std::time::Instant::now() > end {
            return Err("the older Fallow did not stop within 60 s".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(true)
}

/// 016: asks in the terminal whether the older Fallow may quit with `names` running; no
/// terminal (a script), no.
#[cfg(unix)]
pub fn ask_in_terminal(names: &[String]) -> bool {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprintln!("An older Fallow is running with this work:");
    for n in names {
        eprintln!("  \u{2022} {n}");
    }
    eprint!("Quit it? It pauses now and resumes in this version. [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok()
        && matches!(line.trim(), "y" | "Y" | "yes" | "Yes")
}

/// The service's log in `data`, opened for `fallow`'s background service, after the one-time
/// move of the data folder (`prepare_data`: the log must not be what makes the new folder look
/// used, review).
pub fn service_log(data: &std::path::Path) -> Result<std::fs::File, String> {
    prepare_data(data)?;
    crate::platform::private_dir(data)?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("service.log"))
        .map_err(|e| format!("cannot open the service log: {e}"))
}

/// Sends a request to the engine and waits for its reply.
pub fn ask(tx: &Sender<Msg>, r: Request) -> Reply {
    let (rtx, rrx) = mpsc::channel();
    if tx.send(Msg::Request(r, rtx)).is_err() {
        return Reply::Err("the service is stopping".into());
    }
    rrx.recv()
        .unwrap_or_else(|_| Reply::Err("the service is stopping".into()))
}

/// Runs the service in the foreground (`fallow serve`).
/// `config_dir` and `data_dir` replace Fallow's own folders (for trying Fallow out and for
/// tests); by default they are the platform's.
pub fn serve(
    config_dir: Option<&std::path::Path>,
    data_dir_override: Option<PathBuf>,
) -> Result<(), String> {
    let data = match data_dir_override {
        Some(d) => d,
        None => data_dir()?,
    };
    prepare_data(&data)?;
    crate::platform::private_dir(&data)?;
    let paths = ConfigPaths::resolve(config_dir);
    match paths.prepare() {
        Ok(Some(said)) => eprintln!("fallow: {said}"),
        Ok(None) => {}
        Err(e) => eprintln!("fallow: {e}"),
    }
    if let Some(dir) = &paths.dir {
        match crate::defs::install_defaults_said(dir) {
            Ok(said) => {
                for s in said {
                    eprintln!("fallow: {s}");
                }
            }
            Err(e) => eprintln!("fallow: {e}"),
        }
    }
    let (settings, notices) = Settings::load(paths.settings().as_deref());
    for n in &notices {
        eprintln!("{n}");
    }
    let (tx, rx) = mpsc::channel::<Msg>();
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot find Fallow's own executable: {e}"))?;
    // The pipe first: it is what makes this the only service.
    let secret = crate::platform::random::hex(32).map_err(|e| e.to_string())?;
    // 014 (research R-3, R-15), 016 (FR-015, FR-018, T038): no web server on Windows or macOS,
    // unless this is the `automation` test build and `FALLOW_AUTOMATION=1` asks for one; on Linux
    // the Studio's page as before, until its own window.
    #[cfg(not(any(windows, target_os = "macos")))]
    let web = Some(http::Web::start(tx.clone(), secret.clone())?);
    // The marker CI looks for: never in a release build (research R-16).
    #[cfg(all(any(windows, target_os = "macos"), feature = "automation"))]
    eprintln!("fallow: {AUTOMATION_MARKER}");
    #[cfg(all(any(windows, target_os = "macos"), feature = "automation"))]
    let web = if std::env::var_os("FALLOW_AUTOMATION").is_some_and(|v| v == "1") {
        Some(http::Web::start(tx.clone(), secret.clone())?)
    } else {
        None
    };
    #[cfg(any(not(any(windows, target_os = "macos")), feature = "automation"))]
    let port = web.as_ref().map_or(0, |w| w.port);
    #[cfg(all(any(windows, target_os = "macos"), not(feature = "automation")))]
    let port = 0u16;
    // The notification icon (research S-14); without it, Fallow goes on without notifications.
    // The menu's switch is kept across restarts: a file says it is off.
    let off_file = data.join("notifications-off");
    let notify_on = Arc::new(std::sync::atomic::AtomicBool::new(!off_file.exists()));
    let home = page_url(port, &secret, "/");
    let tx_tray = std::sync::Mutex::new(tx.clone());
    // The header of a notification shows Fallow's name and icon, not a generic program's.
    if let Err(e) = crate::platform::notify::register_identity(&data) {
        eprintln!("fallow: {e}");
    }
    // 014: what the icon, a notification and `fallow` open: Fallow's window on Windows (research
    // R-2), the page elsewhere (a notification's address as it is, a route joined to the page's
    // address).
    #[cfg(windows)]
    let opener: crate::platform::notify::Open = {
        // Inherited from a shell, these could give the web view a debugging port or another
        // runtime (research R-6).
        for v in crate::window::view::WEBVIEW2_VARIABLES {
            // SAFETY: on Windows the process environment is guarded by the C runtime, so this
            // is sound with other threads running; nothing of Fallow's reads these variables but
            // the web view's environment, made later on the UI thread (review).
            unsafe { std::env::remove_var(v) };
        }
        crate::window::configure(
            data.clone(),
            crate::window::view::Service {
                tx: tx.clone(),
                theme: Arc::default(),
            },
        );
        Arc::new(crate::window::open)
    };
    // 016 (research M-2): Fallow's window on macOS, on the main thread.
    #[cfg(target_os = "macos")]
    let opener: crate::platform::notify::Open = {
        crate::window::configure(
            data.clone(),
            crate::window::mac::view::Service {
                tx: tx.clone(),
                theme: Arc::default(),
            },
        );
        Arc::new(crate::window::open)
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let opener: crate::platform::notify::Open = {
        let home_open = home.clone();
        Arc::new(move |what: Option<String>| {
            let url = match what {
                Some(r) if r.starts_with("#/") => format!("{home_open}{r}"),
                Some(u) if u.starts_with(&home_open) => u,
                _ => home_open.clone(),
            };
            let _ = crate::platform::open_browser(&url);
        })
    };
    let tray = crate::platform::notify::Tray::start(
        opener,
        Arc::clone(&notify_on),
        Box::new(move |c| match c {
            // 014 (FR-009): Quit asks the page and names the running work (on this, the UI
            // thread); elsewhere it stops as before.
            #[cfg(windows)]
            crate::platform::notify::Choice::Stop => crate::window::quit(None),
            #[cfg(not(windows))]
            crate::platform::notify::Choice::Stop => {
                if let Ok(t) = tx_tray.lock() {
                    let _ = t.send(Msg::Shutdown);
                }
            }
            crate::platform::notify::Choice::EndSession => {
                if let Ok(t) = tx_tray.lock() {
                    let _ = t.send(Msg::Shutdown);
                }
            }
            crate::platform::notify::Choice::Notifications(true) => {
                let _ = std::fs::remove_file(&off_file);
            }
            crate::platform::notify::Choice::Notifications(false) => {
                let _ = std::fs::write(&off_file, "");
            }
            crate::platform::notify::Choice::Open => {}
        }),
    );
    let tray: Option<Arc<crate::platform::notify::Tray>> = match tray {
        Ok(t) => Some(Arc::new(t)),
        Err(e) => {
            eprintln!("fallow: no desktop notifications: {e}");
            None
        }
    };
    let ctx = Ctx {
        slots: Arc::default(),
        settings,
        tx: tx.clone(),
        exe,
        pid: std::process::id(),
        config: paths.dir.clone(),
        endpoint: None,
        notify: match &tray {
            Some(t) => Arc::clone(t) as Arc<dyn crate::platform::notify::Notify>,
            None => Arc::new(crate::platform::notify::Off),
        },
        notify_on,
        // 014: on Windows (016: and macOS) a notification carries the item's route only (the
        // window opens it); elsewhere the page's address with its secret, as before.
        page: if cfg!(any(windows, target_os = "macos")) {
            None
        } else {
            Some(home)
        },
    };
    // 016 (review): AppKit will take the main thread: requests for the window that arrive
    // before its loop runs are queued for it.
    #[cfg(target_os = "macos")]
    if objc2::MainThreadMarker::new().is_some() && crate::platform::display::interactive() {
        crate::window::mac::app::will_run();
    }
    let tx_pipe = tx.clone();
    let url_secret = secret.clone();
    let listener = pipe::listen(
        &pipe::service_endpoint()?,
        Arc::new(move |conn| pipe_conn(conn, &tx_pipe, port, &url_secret)),
    )?;
    let engine = Engine::load(data.clone(), ctx)?;
    let resumed: Vec<u64> = engine
        .runs
        .values()
        .filter(|r| !r.state.stage.is_final())
        .map(|r| r.state.id)
        .collect();
    std::fs::write(
        data.join(SERVICE_FILE),
        json!({
            "pid": std::process::id(),
            "port": (port != 0).then_some(port),
            "started": crate::run::time::rfc3339(crate::run::time::unix_now()),
        })
        .to_string(),
    )
    .map_err(|e| format!("cannot write {SERVICE_FILE}: {e}"))?;
    // Never the page's secret in the log (014 FR-016; the log is a file).
    if port == 0 {
        eprintln!("Fallow is running (its window opens with `fallow`)");
    } else if cfg!(any(windows, target_os = "macos")) {
        eprintln!("Fallow is running; automation page on port {port}");
    } else {
        eprintln!("Fallow is running: http://127.0.0.1:{port}/ (open it with `fallow`)");
    }
    if !resumed.is_empty() {
        eprintln!(
            "Resuming run(s) {}",
            resumed
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    run_engine(engine, rx, tx.clone())?;
    drop(tray);
    drop(listener);
    #[cfg(any(not(any(windows, target_os = "macos")), feature = "automation"))]
    drop(web);
    let _ = std::fs::remove_file(data.join(SERVICE_FILE));
    Ok(())
}

/// Runs the engine until it stops. 016 (research M-2): on macOS with a display, AppKit takes the
/// main thread and the engine runs whole on a thread of its own; without one (SSH), and
/// elsewhere, the engine runs on this thread as before.
fn run_engine(engine: Engine, rx: mpsc::Receiver<Msg>, tx: Sender<Msg>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if let Some(mtm) =
        objc2::MainThreadMarker::new().filter(|_| crate::platform::display::interactive())
    {
        let h = std::thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                engine.serve(rx);
                crate::window::mac::app::engine_done();
            })
            .map_err(|e| format!("cannot start the engine's thread: {e}"))?;
        busy_watch(tx.clone());
        crate::window::mac::app::run(mtm, tx);
        let _ = h.join();
        return Ok(());
    }
    let _ = tx;
    engine.serve(rx);
    Ok(())
}

/// 016 (research M-11, data-model "Activity hold", T024, T030): every 5 s, whether any loop
/// step, operation, session turn or conversation turn runs (`Request::Running`), App Nap held
/// off while one does; and whether anything needs the engineer, the menu bar icon's dot. Ends with the engine. A poll, not the engine's events: the engine's many
/// event sources would each need a hook, and App Nap only begins after the app has sat unseen
/// for a while, so 5 s is soon enough.
#[cfg(target_os = "macos")]
fn busy_watch(tx: Sender<Msg>) {
    let _ = std::thread::Builder::new()
        .name("busy".into())
        .spawn(move || {
            let mut was = false;
            let mut was_waiting = false;
            loop {
                let busy = match ask(&tx, Request::Running) {
                    Reply::Ok(v) => v.as_array().is_some_and(|a| !a.is_empty()),
                    Reply::Err(_) => return,
                };
                if busy != was {
                    was = busy;
                    crate::platform::notify::on_ui(Box::new(move || {
                        crate::window::mac::app::set_busy(busy);
                    }));
                }
                // 016 (FR-014, research M-10, T030): the menu bar icon's dot while the "Needs
                // you" list holds anything; changed when its count leaves or reaches 0.
                // The count only, from memory: the dashboard would run git for every project
                // (review).
                let waiting = match ask(&tx, Request::NeedsYou) {
                    Reply::Ok(v) => v.as_u64().unwrap_or(0) > 0,
                    Reply::Err(_) => return,
                };
                if waiting != was_waiting {
                    was_waiting = waiting;
                    crate::platform::notify::on_ui(Box::new(move || {
                        if let Some(mtm) = objc2::MainThreadMarker::new() {
                            crate::window::mac::menu::set_waiting(mtm, waiting);
                        }
                    }));
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        });
}

/// 014 (contracts/window.md §1): the window shown or made on the UI thread; the answer once it
/// exists, or why it cannot be. 016: on macOS's main thread.
#[cfg(any(windows, target_os = "macos"))]
fn open_on_ui(route: Option<String>, launched: Option<u64>) -> Value {
    let (otx, orx) = mpsc::channel();
    let asked = crate::platform::notify::on_ui(Box::new(move || {
        let _ = otx.send(crate::window::open_for(route, launched.map(u128::from)));
    }));
    if !asked {
        // 016 (contracts/window-macos.md §1): on macOS, AppKit does not run without a display.
        let error = if cfg!(target_os = "macos") {
            "Fallow was started without a display: stop it (`fallow stop`) and open Fallow from the Mac's desktop"
        } else {
            "Fallow has no window thread (no desktop?)"
        };
        return json!({ "ok": false, "error": error });
    }
    match orx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(Ok(())) => json!({ "ok": true }),
        Ok(Err(e)) => json!({ "ok": false, "error": e }),
        Err(_) => json!({ "ok": false, "error": "the window did not open" }),
    }
}

/// Other platforms: the page opens from the command line (`url`).
#[cfg(not(any(windows, target_os = "macos")))]
fn open_on_ui(route: Option<String>, _launched: Option<u64>) -> Value {
    crate::platform::notify::request_open(route);
    json!({ "ok": true })
}

/// One connection on the pipe: the command line asks one thing and gets one answer; an agent's
/// MCP bridge serves its tools (`step_conn`).
fn pipe_conn(conn: pipe::Conn, tx: &Sender<Msg>, port: u16, secret: &str) {
    let peer = conn.peer.pid;
    let mut reader = BufReader::new(conn.reader);
    let mut writer = conn.writer;
    let mut line = Vec::new();
    if !matches!(pipe::read_line(&mut reader, &mut line), Ok(true)) || line.len() > pipe::MAX_LINE {
        return;
    }
    let Ok(hello) = serde_json::from_slice::<Value>(&line) else {
        return;
    };
    if hello["kind"] == "step" {
        if let Some(token) = hello["token"].as_str() {
            step_conn(token, peer, reader, writer, tx);
        }
        return;
    }
    if hello["kind"] != "cli" {
        return;
    }
    let req = &hello["request"];
    let mut answer = cli_request(req, tx, port, secret);
    // 014 (review): Quit stops the service only once its answer is written, so `fallow quit`
    // always hears it.
    let stop_after = answer
        .as_object_mut()
        .and_then(|o| o.remove("stop_after"))
        .is_some_and(|v| v == true);
    let _ = pipe::write_line(&mut *writer, &answer.to_string());
    if stop_after {
        let _ = tx.send(Msg::Shutdown);
    }
}

/// How long a tool call may wait for its answer (an approval waits up to 10 minutes).
const TOOL_WAIT: std::time::Duration = std::time::Duration::from_secs(11 * 60);

/// An agent's MCP bridge (006 research R-5, 007 contracts/mcp-tools.md): the token must be live
/// and the bridge one of the agent's own processes; then MCP over the pipe, each tool call
/// answered by the engine.
fn step_conn(
    token: &str,
    peer: u32,
    mut reader: BufReader<Box<dyn std::io::Read + Send>>,
    mut writer: Box<dyn std::io::Write + Send>,
    tx: &Sender<Msg>,
) {
    use crate::mcp::rpc;
    let tools = match ask(
        tx,
        Request::Studio(crate::studio::Req::ToolOpen {
            token: token.to_string(),
            pid: peer,
        }),
    ) {
        Reply::Ok(v) => v,
        Reply::Err(_) => return,
    };
    let mut server = rpc::Server::new(tools).with_instructions(Some(
        "Fallow's tools for this step of a Fallow loop or chat.".into(),
    ));
    let mut line = Vec::new();
    loop {
        line.clear();
        match pipe::read_line(&mut reader, &mut line) {
            Ok(true) if line.len() <= pipe::MAX_LINE => {}
            _ => return,
        }
        let text = String::from_utf8_lossy(&line);
        let out = match server.handle(&text) {
            rpc::Action::Reply(l) => l,
            rpc::Action::Call {
                id,
                name,
                arguments,
                ..
            } => {
                let (rtx, rrx) = mpsc::channel();
                let call = Request::ToolCall {
                    token: token.to_string(),
                    name,
                    args: arguments,
                };
                if tx.send(Msg::Request(call, rtx)).is_err() {
                    return;
                }
                let content = |text: String, error: bool| json!({ "content": [{ "type": "text", "text": text }], "isError": error });
                match rrx.recv_timeout(TOOL_WAIT) {
                    Ok(Reply::Ok(v)) => rpc::result_line(&id, content(v.to_string(), false)),
                    Ok(Reply::Err(e)) => rpc::result_line(&id, content(e, true)),
                    Err(_) => rpc::result_line(&id, content("no answer in time".into(), true)),
                }
            }
            rpc::Action::Cancelled(_) | rpc::Action::None => continue,
        };
        if pipe::write_line(&mut *writer, &out).is_err() {
            return;
        }
    }
}

fn page_url(port: u16, secret: &str, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}?k={secret}")
}

fn cli_request(req: &Value, tx: &Sender<Msg>, port: u16, secret: &str) -> Value {
    let reply = |r: Reply| match r {
        Reply::Ok(v) => json!({ "ok": true, "value": v }),
        Reply::Err(e) => json!({ "ok": false, "error": e }),
    };
    match req["cmd"].as_str() {
        // 014: the page's address with its secret, where there is a page (Linux, and the Windows
        // and macOS `automation` test builds with FALLOW_AUTOMATION=1).
        Some("url") if port == 0 => {
            json!({ "ok": false, "error": "the Studio is in Fallow's window: run `fallow`" })
        }
        Some("url") => {
            json!({ "ok": true, "url": page_url(port, secret, req["path"].as_str().unwrap_or("/")) })
        }
        // 016 (research M-22, contracts/window-macos.md §1): the version too, on every platform,
        // so a newer `fallow` can tell an older service apart.
        Some("status") => {
            let mut a = reply(ask(tx, Request::List));
            a["version"] = json!(env!("CARGO_PKG_VERSION"));
            a
        }
        Some("stop") => {
            let _ = tx.send(Msg::Shutdown);
            json!({ "ok": true })
        }
        // 014 (contracts/window.md): show the window, or bring it to the front (the page until
        // the window exists, T013), on an item's route when given.
        Some("window") => match req["route"].as_str() {
            Some(r) if !r.starts_with("#/") => {
                json!({ "ok": false, "error": "a route starts with #/" })
            }
            r => open_on_ui(r.map(str::to_string), req["launched"].as_u64()),
        },
        // 014 (FR-009): what Quit would pause; with `confirmed`, stop as `stop` does and say
        // which process to wait for.
        // 014 (`fallow quit`): Quit as the icon does it, in the service's own window and dialog;
        // the answer says whether Fallow quits, and its process to wait for.
        #[cfg(any(windows, target_os = "macos"))]
        Some("quit") if req["ask"] == true => {
            let (qtx, qrx) = std::sync::mpsc::channel();
            if !crate::platform::notify::on_ui(Box::new(move || crate::window::quit(Some(qtx)))) {
                // 016 (review): asked while Fallow is already quitting: it quits.
                #[cfg(target_os = "macos")]
                if crate::window::mac::app::ending() {
                    return json!({ "ok": true, "quit": true, "pid": std::process::id() });
                }
                return json!({
                    "ok": false,
                    "error": "Fallow runs without a desktop, so it cannot ask: use `fallow stop`"
                });
            }
            match qrx.recv_timeout(std::time::Duration::from_secs(1800)) {
                // The service stops after this answer is written (`stop_after`).
                Ok(quits) => json!({
                    "ok": true,
                    "quit": quits,
                    "pid": std::process::id(),
                    "stop_after": quits,
                }),
                Err(_) => json!({ "ok": false, "error": "no answer" }),
            }
        }
        // 016 (`automation` builds only, T025): the window closed as its red button closes it,
        // for the checks that drive the real window.
        #[cfg(all(target_os = "macos", feature = "automation"))]
        Some("window-close") => {
            let (ctx, crx) = mpsc::channel();
            crate::platform::notify::on_ui(Box::new(move || {
                let _ = ctx.send(crate::window::mac::host::perform_close());
            }));
            match crx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(true) => json!({ "ok": true }),
                _ => json!({ "ok": false, "error": "no window to close" }),
            }
        }
        // 016 (`automation` builds only, T047): the window at a route in a theme: the page's
        // report (errors, width, ground colour) and the web view's picture (PNG, base64).
        #[cfg(all(target_os = "macos", feature = "automation"))]
        Some("snapshot") => {
            let route = req["route"].as_str().unwrap_or("#/").to_string();
            let theme = match req["theme"].as_str() {
                Some("dark") => "dark",
                _ => "light",
            };
            let script = req["script"].as_str().unwrap_or("").to_string();
            let (stx, srx) = mpsc::channel();
            crate::platform::notify::on_ui(Box::new(move || {
                crate::window::mac::host::snapshot(&route, theme, &script, stx);
            }));
            match srx.recv_timeout(std::time::Duration::from_secs(60)) {
                Ok(Ok(v)) => json!({ "ok": true, "value": v }),
                Ok(Err(e)) => json!({ "ok": false, "error": e }),
                Err(_) => json!({ "ok": false, "error": "the page did not answer within 60 s" }),
            }
        }
        // 016 (`automation` builds only, T042): the window minimized into the Dock.
        #[cfg(all(target_os = "macos", feature = "automation"))]
        Some("window-minimize") => {
            let (mtx, mrx) = mpsc::channel();
            crate::platform::notify::on_ui(Box::new(move || {
                let _ = mtx.send(crate::window::mac::host::perform_miniaturize());
            }));
            match mrx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(true) => json!({ "ok": true }),
                _ => json!({ "ok": false, "error": "no window to minimize" }),
            }
        }
        Some("quit") if req["confirmed"] == true => {
            let _ = tx.send(Msg::Shutdown);
            json!({ "ok": true, "pid": std::process::id() })
        }
        Some("quit") => match ask(tx, Request::Running) {
            Reply::Ok(v) => json!({ "ok": true, "running": v }),
            Reply::Err(e) => json!({ "ok": false, "error": e }),
        },
        _ => json!({ "ok": false, "error": "unknown request" }),
    }
}

/// The command line's side: one request to the running service.
pub fn request(req: Value) -> Result<Value, String> {
    let data = data_dir()?;
    let pid = std::fs::read_to_string(data.join(SERVICE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["pid"].as_u64())
        .map(|p| p as u32);
    let (r, mut w) = pipe::connect(&pipe::service_endpoint()?, pid)?;
    pipe::write_line(
        &mut *w,
        &json!({ "kind": "cli", "request": req }).to_string(),
    )
    .map_err(|e| format!("the service closed the connection: {e}"))?;
    let mut reader = BufReader::new(r);
    let mut line = Vec::new();
    match pipe::read_line(&mut reader, &mut line) {
        Ok(true) => serde_json::from_slice(&line).map_err(|e| e.to_string()),
        _ => Err("the service did not answer".into()),
    }
}

/// The service's program: `fallow` beside the running one (`fallow-app` starts it too, 014).
fn service_exe() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = format!("fallow{}", std::env::consts::EXE_SUFFIX);
    Ok(match me.parent() {
        Some(dir) if me.file_name().is_some_and(|n| n != name.as_str()) => dir.join(name),
        _ => me,
    })
}

/// The running service's process id, from its service file.
#[cfg(windows)]
fn service_pid() -> Option<u32> {
    let data = data_dir().ok()?;
    std::fs::read_to_string(data.join(SERVICE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["pid"].as_u64())
        .map(|p| p as u32)
}

/// 014 (research R-2, contracts/cli.md): starts the service if needed and asks it for its
/// window (on `route`, `#/…`), allowing it to come to the front. 016 (research M-3): on macOS,
/// inside `Fallow.app` the service is the app, opened through LaunchServices, so the Dock icon,
/// notifications and permissions are Fallow's.
#[cfg(any(windows, target_os = "macos"))]
pub fn open_window(route: Option<&str>) -> Result<(), String> {
    let launched = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    replace_other_version()?;
    // A Fallow from before 016 on the old data folder: quits on the engineer's word (T034).
    #[cfg(target_os = "macos")]
    if !quit_older(ask_in_terminal)? {
        return Err(OLDER_RUNNING.into());
    }
    // Inside the bundle, always through LaunchServices: it brings Fallow to the front, which an
    // app may not do by itself from macOS 14 on (cooperative activation); a running Fallow gets
    // a reopen.
    #[cfg(target_os = "macos")]
    match crate::platform::bundle() {
        Some(app) => open_app(&app)?,
        None => ensure_running()?,
    }
    #[cfg(windows)]
    {
        ensure_running()?;
        if let Some(pid) = service_pid() {
            // SAFETY: a plain call; a failure only leaves the window behind others.
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) };
        }
    }
    let mut req = json!({ "cmd": "window", "launched": launched });
    if let Some(r) = route {
        req["route"] = json!(r);
    }
    let v = request(req)?;
    if v["ok"] == true {
        Ok(())
    } else {
        Err(v["error"]
            .as_str()
            .unwrap_or("the service did not open its window")
            .to_string())
    }
}

/// 014 (`fallow quit`): Quit as the icon does it. `Ok(false)` when the engineer cancelled;
/// `Ok(true)` when Fallow quit, or was not running. With `wait`, returns once its process and
/// every other `fallow` process of this user (the agents' bridges) have exited, up to 60 s each.
/// 016 (contracts/cli-macos.md): on macOS the service shows its `NSAlert` itself.
#[cfg(any(windows, target_os = "macos"))]
pub fn quit(wait: bool) -> Result<bool, String> {
    if request(json!({ "cmd": "status" })).is_err() {
        return Ok(true);
    }
    #[cfg(windows)]
    if let Some(pid) = service_pid() {
        // Its dialog may come to the front (review).
        // SAFETY: a plain call; a failure only leaves the dialog behind others.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) };
    }
    let v = request(json!({ "cmd": "quit", "ask": true }))?;
    if v["ok"] != true {
        return Err(v["error"].as_str().unwrap_or("Quit failed").to_string());
    }
    if v["quit"] != true {
        return Ok(false);
    }
    if wait {
        if let Some(pid) = v["pid"].as_u64() {
            if !crate::platform::job::wait_exit(pid as u32, std::time::Duration::from_secs(60)) {
                return Err("Fallow did not exit within 60 s".into());
            }
        }
        let me = std::process::id();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let name = if cfg!(windows) {
            "fallow.exe"
        } else {
            "fallow"
        };
        while crate::platform::job::others_named(name, me) > 0 {
            if std::time::Instant::now() > end {
                return Err("Fallow's agent bridges did not exit within 60 s".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    Ok(true)
}

/// 016 (research M-3): asks LaunchServices to open `Fallow.app` (`app`), which becomes the
/// service and opens its window, and waits up to 10 s for its socket.
#[cfg(target_os = "macos")]
fn open_app(app: &std::path::Path) -> Result<(), String> {
    // The data folder's move and an older Fallow are said here, in the terminal.
    #[cfg(unix)]
    if crate::config::old_data_dir().is_some_and(|o| listening(&o.join("service.sock"))) {
        return Err(OLDER_RUNNING.into());
    }
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg(app)
        .status()
        .map_err(|e| format!("cannot open {}: {e}", app.display()))?;
    if !status.success() {
        return Err(format!("macOS could not open {}", app.display()));
    }
    for _ in 0..400 {
        if request(json!({ "cmd": "status" })).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Err(format!(
        "Fallow did not start; see {}",
        data_dir()?.join("service.log").display()
    ))
}

/// Starts the service in the background unless it is running (`fallow`, `fallow run`).
pub fn ensure_running() -> Result<(), String> {
    if request(json!({ "cmd": "status" })).is_ok() {
        return Ok(());
    }
    let exe = service_exe()?;
    let data = data_dir()?;
    let log = service_log(&data)?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP: it outlives this command, with a console
        // that is never shown. Not DETACHED_PROCESS: without any console, every console program
        // it starts (git, the gates, the agents' commands) would open a window of its own.
        cmd.creation_flags(0x0800_0000 | 0x0000_0200);
    }
    // 015: the service outlives this command: none of its pipes.
    crate::platform::no_inherited_files(&mut cmd);
    cmd.spawn()
        .map_err(|e| format!("cannot start the service: {e}"))?;
    // Every 25 ms for 10 s: the start counts toward the window's 1 s (research R-14).
    for _ in 0..400 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        if request(json!({ "cmd": "status" })).is_ok() {
            return Ok(());
        }
    }
    Err(format!(
        "the service did not start; see {}",
        data.join("service.log").display()
    ))
}

/// For tests: an engine on its own data folder, without pipe or page.
pub fn engine_for_tests(
    data: PathBuf,
    settings: Settings,
) -> Result<(Sender<Msg>, std::thread::JoinHandle<()>), String> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let ctx = Ctx {
        slots: Arc::default(),
        settings,
        tx: tx.clone(),
        exe: std::env::current_exe().unwrap_or_default(),
        pid: std::process::id(),
        config: None,
        endpoint: None,
        notify: Arc::new(crate::platform::notify::Off),
        notify_on: Arc::default(),
        page: None,
    };
    let engine = Engine::load(data, ctx)?;
    let h = std::thread::spawn(move || engine.serve(rx));
    Ok((tx, h))
}

/// For tests: an engine that also serves agents' MCP bridges, on a pipe of its own, so tests can
/// drive the tools end to end. `exe` is Fallow's binary (`mcp-bridge`); `config` the engineer's
/// configuration folder.
pub struct ToolEngine {
    pub tx: Sender<Msg>,
    pub handle: std::thread::JoinHandle<()>,
    _listener: pipe::Listener,
}

pub fn engine_with_tools(
    data: PathBuf,
    settings: Settings,
    config: Option<PathBuf>,
    exe: PathBuf,
    approval_wait: Option<std::time::Duration>,
) -> Result<ToolEngine, String> {
    engine_with_notify(
        data,
        settings,
        config,
        exe,
        approval_wait,
        Arc::new(crate::platform::notify::Off),
    )
}

/// `engine_with_tools` whose notifications go to `notify`, with the page at
/// `http://fallow.test/`.
pub fn engine_with_notify(
    data: PathBuf,
    settings: Settings,
    config: Option<PathBuf>,
    exe: PathBuf,
    approval_wait: Option<std::time::Duration>,
    notify: Arc<dyn crate::platform::notify::Notify>,
) -> Result<ToolEngine, String> {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let suffix = format!(
        "test-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let (tx, rx) = mpsc::channel::<Msg>();
    let ctx = Ctx {
        slots: Arc::default(),
        settings,
        tx: tx.clone(),
        exe,
        pid: std::process::id(),
        config,
        endpoint: Some(suffix.clone()),
        notify,
        notify_on: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        page: Some("http://fallow.test/".into()),
    };
    let tx_pipe = tx.clone();
    let listener = pipe::listen(
        &format!("{}-{suffix}", pipe::service_endpoint()?),
        Arc::new(move |conn| pipe_conn(conn, &tx_pipe, 0, "")),
    )?;
    // The `/` menu reads a home of the test's own, never the engineer's `~/.claude`.
    let home = data.with_file_name("home");
    let mut engine = Engine::load(data, ctx)?;
    engine.studio.endpoint = Some(suffix);
    engine.studio.home = Some(home);
    if let Some(w) = approval_wait {
        engine.studio.approval_wait = w;
    }
    let handle = std::thread::spawn(move || engine.serve(rx));
    Ok(ToolEngine {
        tx,
        handle,
        _listener: listener,
    })
}

pub use engine::summary;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_gives_the_version() {
        let dir = std::env::temp_dir().join(format!("fallow-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, h) = engine_for_tests(dir.clone(), Settings::default()).unwrap();
        let a = cli_request(&json!({ "cmd": "status" }), &tx, 0, "");
        assert_eq!(a["ok"], true, "{a}");
        assert_eq!(a["version"], env!("CARGO_PKG_VERSION"));
        assert!(a["value"].is_array(), "{a}");
        // 016 (T030): the menu bar dot's count, from memory.
        assert!(matches!(ask(&tx, Request::NeedsYou), Reply::Ok(v) if v == json!(0)));
        let _ = tx.send(Msg::Shutdown);
        let _ = h.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_version_is_told_apart_from_this_one() {
        let ours = env!("CARGO_PKG_VERSION");
        assert_eq!(other_version(&json!({ "ok": true, "version": ours })), None);
        assert_eq!(
            other_version(&json!({ "ok": true, "version": "0.0.1-old" })),
            Some("Fallow 0.0.1-old".into())
        );
        // A service from before `status` gave its version.
        assert_eq!(
            other_version(&json!({ "ok": true, "value": [] })),
            Some("an older Fallow".into())
        );
    }
}
