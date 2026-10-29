//! The one service per user (006 research R-1, FR-035): it holds every run, starts agents and
//! serves the run page. The command line reaches it over the user-only pipe; the page over
//! loopback HTTP with a per-launch secret (`http`).

pub mod http;

use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use serde_json::{Value, json};

use crate::config::{Autonomy, ConfigPaths, RoleChoice, Settings};
use crate::mcp::pipe;
use crate::run::engine::{self, Create, Ctx, Engine, Msg, Reply, Request};

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
    let (settings, notices) = Settings::load(paths.settings().as_deref());
    for n in &notices {
        eprintln!("{n}");
    }
    let (tx, rx) = mpsc::channel::<Msg>();
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot find Fallow's own executable: {e}"))?;
    let ctx = Ctx {
        settings,
        tx: tx.clone(),
        exe,
        pid: std::process::id(),
    };
    // The pipe first: it is what makes this the only service.
    let secret = crate::platform::random::hex(32).map_err(|e| e.to_string())?;
    let web = http::Web::start(tx.clone(), secret.clone())?;
    let port = web.port;
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
    drop(listener);
    drop(web);
    let _ = std::fs::remove_file(data.join(SERVICE_FILE));
    Ok(())
}

/// One connection on the pipe: the command line asks one thing and gets one answer. Agent
/// steps' bridges are refused until their tools arrive (user story 2).
fn pipe_conn(conn: pipe::Conn, tx: &Sender<Msg>, port: u16, secret: &str) {
    let mut reader = BufReader::new(conn.reader);
    let mut writer = conn.writer;
    let mut line = Vec::new();
    if !matches!(pipe::read_line(&mut reader, &mut line), Ok(true)) || line.len() > pipe::MAX_LINE {
        return;
    }
    let Ok(hello) = serde_json::from_slice::<Value>(&line) else {
        return;
    };
    if hello["kind"] != "cli" {
        return;
    }
    let req = &hello["request"];
    let answer = cli_request(req, tx, port, secret);
    let _ = pipe::write_line(&mut *writer, &answer.to_string());
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
        Some("run") => {
            let mut roles = std::collections::BTreeMap::new();
            for r in req["roles"].as_array().into_iter().flatten() {
                if let (Some(role), Some(agent)) = (r["role"].as_str(), r["agent"].as_str()) {
                    roles.insert(
                        role.to_string(),
                        RoleChoice {
                            agent: agent.to_string(),
                            model: r["model"].as_str().unwrap_or_default().to_string(),
                        },
                    );
                }
            }
            let c = Create {
                repo: PathBuf::from(req["repo"].as_str().unwrap_or(".")),
                task: req["task"].as_str().unwrap_or_default().to_string(),
                target: req["target"].as_str().map(str::to_string),
                autonomy: req["autonomy"].as_str().and_then(Autonomy::parse),
                limit_usd: req["limit_usd"].as_f64(),
                limit_min: req["limit_min"].as_u64(),
                roles,
            };
            match ask(tx, Request::Create(c)) {
                Reply::Ok(v) => {
                    let id = v["id"].as_u64().unwrap_or(0);
                    json!({ "ok": true, "id": id, "url": page_url(port, secret, &format!("/runs/{id}")) })
                }
                other => reply(other),
            }
        }
        Some("stop") => {
            let _ = tx.send(Msg::Shutdown);
            json!({ "ok": true })
        }
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

/// Starts the service in the background unless it is running (`fallow`, `fallow run`).
pub fn ensure_running() -> Result<(), String> {
    if request(json!({ "cmd": "status" })).is_ok() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
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
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
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
        settings,
        tx: tx.clone(),
        exe: std::env::current_exe().unwrap_or_default(),
        pid: std::process::id(),
    };
    let engine = Engine::load(data, ctx)?;
    let h = std::thread::spawn(move || engine.serve(rx));
    Ok((tx, h))
}

pub use engine::summary;
