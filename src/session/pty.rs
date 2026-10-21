//! Running an agent in a pseudo-terminal (002 research R-7).
//!
//! `portable-pty` gives a PTY on Linux and macOS and a ConPTY on Windows. Per session:
//! - a reader thread reads output, parses it into the terminal, writes the replies the agent
//!   waits for (R-6) and wakes the main loop once;
//! - a waiter thread reports the exit.
//!
//! No shell is involved: the program gets its arguments as given.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::platform::Group;
use super::term::Term;
use crate::app::events::{AppEvent, OutputWaker, SessionId};

/// What to start.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Environment additions; the rest of the environment is inherited unread.
    pub env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// A running agent.
pub struct Pty {
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Writer,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pub pid: Option<u32>,
    group: Group,
}

impl Pty {
    /// Whether process `pid` belongs to this session (003 research R-3): in its Job Object.
    #[cfg(windows)]
    pub fn contains(&self, pid: u32) -> bool {
        self.group.contains(pid)
    }
}

impl std::fmt::Debug for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pty(pid {:?})", self.pid)
    }
}

fn size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Starts the agent. Output goes into `term`; `waker` wakes the main loop; the exit is sent as
/// `AppEvent::SessionExited(id, …)`.
pub fn spawn(
    spec: &SpawnSpec,
    id: SessionId,
    term: Arc<Mutex<Term>>,
    signals: Arc<Mutex<Vec<super::term::Signal>>>,
    waker: OutputWaker,
    events: Sender<AppEvent>,
) -> Result<Pty, String> {
    let pair = native_pty_system()
        .openpty(size(spec.rows, spec.cols))
        .map_err(|e| format!("cannot open a pseudo-terminal: {e}"))?;
    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "fallow");
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut group = Group::new().map_err(|e| format!("cannot set up the session: {e}"))?;
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("cannot start {}: {e}", spec.program.display()))?;
    drop(pair.slave);
    let pid = child.process_id();
    if let Some(pid) = pid {
        if let Err(e) = group.adopt(pid) {
            let _ = child.kill();
            return Err(format!("cannot track the agent's processes: {e}"));
        }
    }
    let killer = child.clone_killer();
    let writer: Writer = Arc::new(Mutex::new(
        pair.master
            .take_writer()
            .map_err(|e| format!("pty writer: {e}"))?,
    ));
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("pty reader: {e}"))?;

    let w = writer.clone();
    std::thread::Builder::new()
        .name(format!("fallow-session-{id}-out"))
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                crate::app::diag::mark("read", u64::from(id));
                let (replies, sigs) = match term.lock() {
                    Ok(mut t) => {
                        t.process(&buf[..n]);
                        (t.take_replies(), t.take_signals())
                    }
                    Err(_) => break,
                };
                // Signals go through their own small queue, so the main thread never waits on
                // this session's terminal while it parses (002 SC-006 investigation).
                if !sigs.is_empty() {
                    if let Ok(mut q) = signals.lock() {
                        q.extend(sigs);
                    }
                }
                if !replies.is_empty() {
                    if let Ok(mut w) = w.lock() {
                        let _ = w.write_all(&replies);
                        let _ = w.flush();
                    }
                }
                waker.wake();
            }
            waker.wake();
        })
        .map_err(|e| format!("reader thread: {e}"))?;

    std::thread::Builder::new()
        .name(format!("fallow-session-{id}-wait"))
        .spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code());
            let _ = events.send(AppEvent::SessionExited(id, code));
        })
        .map_err(|e| format!("waiter thread: {e}"))?;

    Ok(Pty {
        master: Some(pair.master),
        writer,
        killer,
        pid,
        group,
    })
}

impl Pty {
    /// Sends bytes to the agent.
    pub fn write(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
        crate::app::diag::mark("written", bytes.len() as u64);
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        if let Some(m) = &self.master {
            let _ = m.resize(size(rows, cols));
        }
    }

    /// Graceful stop: SIGTERM to the group (Unix), closing the pseudoconsole (Windows).
    pub fn terminate(&mut self) {
        self.group.terminate();
        if cfg!(windows) {
            self.close_master();
        }
    }

    /// Forced stop of the agent and everything it started.
    pub fn kill(&mut self) {
        self.group.kill();
        let _ = self.killer.kill();
    }

    /// Closes the pseudo-terminal off the calling thread: on Windows `ClosePseudoConsole` can
    /// block until the output is drained, which the reader thread keeps doing.
    fn close_master(&mut self) {
        if let Some(m) = self.master.take() {
            let _ = std::thread::Builder::new()
                .name("fallow-session-close".into())
                .spawn(move || drop(m));
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // A session that is closed or discarded never leaves its agent running.
        self.group.terminate();
        self.close_master();
        self.group.kill();
        let _ = self.killer.kill();
    }
}
