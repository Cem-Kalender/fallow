//! The one service per user (006 research R-1, FR-035): it holds every run, starts agents and
//! serves the run page. The command line reaches it over the user-only pipe; the page over
//! loopback HTTP with a per-launch secret (`http`).

pub mod answer;
pub mod bus;
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

/// Where Fallow keeps its data.
pub fn data_dir() -> Result<PathBuf, String> {
    crate::config::data_dir()
        .ok_or_else(|| "no folder for Fallow's data (LOCALAPPDATA is not set)".into())
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
    crate::platform::private_dir(&data)?;
    let paths = ConfigPaths::resolve(config_dir);
    match paths.prepare() {
        Ok(Some(said)) => eprintln!("fallow: {said}"),
        Ok(None) => {}
        Err(e) => eprintln!("fallow: {e}"),
    }
    if let Some(dir) = &paths.dir {
        match crate::defs::install_defaults(dir) {
            Ok(news) => {
                for n in news {
                    eprintln!(
                        "fallow: an update changed the default {n}; your edited copy is kept, the new one is {n}.new"
                    );
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
    let web = http::Web::start(tx.clone(), secret.clone())?;
    let port = web.port;
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
            // SAFETY: no other thread of this process reads or writes these variables, and the
            // web view's environment is made later, on the UI thread.
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
    #[cfg(not(windows))]
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
            crate::platform::notify::Choice::Stop => {
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
        page: Some(home),
    };
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
            "port": port,
            "started": crate::run::time::rfc3339(crate::run::time::unix_now()),
        })
        .to_string(),
    )
    .map_err(|e| format!("cannot write {SERVICE_FILE}: {e}"))?;
    eprintln!("Fallow is running: http://127.0.0.1:{port}/ (open it with `fallow`)");
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
    engine.serve(rx);
    drop(tray);
    drop(listener);
    drop(web);
    let _ = std::fs::remove_file(data.join(SERVICE_FILE));
    Ok(())
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
    let answer = cli_request(req, tx, port, secret);
    let _ = pipe::write_line(&mut *writer, &answer.to_string());
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
        Some("url") => {
            json!({ "ok": true, "url": page_url(port, secret, req["path"].as_str().unwrap_or("/")) })
        }
        Some("status") => reply(ask(tx, Request::List)),
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
            r => {
                crate::platform::notify::request_open(r.map(str::to_string));
                json!({ "ok": true })
            }
        },
        // 014 (FR-009): what Quit would pause; with `confirmed`, stop as `stop` does and say
        // which process to wait for.
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
fn service_pid() -> Option<u32> {
    let data = data_dir().ok()?;
    std::fs::read_to_string(data.join(SERVICE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["pid"].as_u64())
        .map(|p| p as u32)
}

/// 014 (research R-2, contracts/cli.md): starts the service if needed and asks it for its
/// window (on `route`, `#/…`), allowing it to come to the front.
#[cfg(windows)]
pub fn open_window(route: Option<&str>) -> Result<(), String> {
    ensure_running()?;
    if let Some(pid) = service_pid() {
        // SAFETY: a plain call; a failure only leaves the window behind others.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) };
    }
    let mut req = json!({ "cmd": "window" });
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

/// Starts the service in the background unless it is running (`fallow`, `fallow run`).
pub fn ensure_running() -> Result<(), String> {
    if request(json!({ "cmd": "status" })).is_ok() {
        return Ok(());
    }
    let exe = service_exe()?;
    let data = data_dir()?;
    crate::platform::private_dir(&data)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data.join("service.log"))
        .map_err(|e| format!("cannot open the service log: {e}"))?;
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
    let mut engine = Engine::load(data, ctx)?;
    engine.studio.endpoint = Some(suffix);
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
