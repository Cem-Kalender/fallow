//! The application event channel (002 research R-5).
//!
//! Everything that can change the screen arrives on one `std::sync::mpsc` channel: terminal input
//! from a dedicated input thread, procmon snapshots, and session output and exits. The main loop
//! blocks on it with a timeout, so agent output wakes it at once without polling, and there is
//! still no async runtime (001 R-6).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crossterm::event::Event;

use crate::procmon::snapshot::Snapshot;

/// A session's id, shown as `#n`.
pub type SessionId = u32;

#[derive(Debug)]
pub enum AppEvent {
    /// Terminal input. `burst` is true when more input was already queued behind this event (a
    /// paste delivered as key events; Windows consoles have no bracketed paste).
    Input(Event, bool),
    Snapshot(Arc<Snapshot>),
    /// A session's screen changed.
    SessionOutput(SessionId),
    /// A session's agent exited with this code (`None`: killed by a signal or unknown).
    SessionExited(SessionId, Option<u32>),
    /// Something else wants a redraw.
    Wake,
    /// The orchestration channel (003): a hello, a tool call, a cancellation or a close.
    Hub(crate::orchestration::channel::Incoming),
}

/// Sends `SessionOutput` at most once until the main loop has drawn: a session printing a
/// million lines produces one pending event, not a million.
#[derive(Debug, Clone)]
pub struct OutputWaker {
    id: SessionId,
    tx: Sender<AppEvent>,
    pending: Arc<AtomicBool>,
}

impl OutputWaker {
    pub fn new(id: SessionId, tx: Sender<AppEvent>) -> Self {
        OutputWaker {
            id,
            tx,
            pending: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Called by the reader thread after new output was parsed.
    pub fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(AppEvent::SessionOutput(self.id));
        }
    }

    /// Called by the main loop when it handles the event.
    pub fn handled(&self) {
        self.pending.store(false, Ordering::Release);
    }

    /// A handle the main loop keeps to call `handled`.
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.pending.clone()
    }
}

/// Waits up to `timeout` for the first event, then takes everything already queued.
pub fn next_batch(rx: &Receiver<AppEvent>, timeout: Duration) -> Vec<AppEvent> {
    let mut out = Vec::new();
    match rx.recv_timeout(timeout) {
        Ok(ev) => out.push(ev),
        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return out,
    }
    while let Ok(ev) = rx.try_recv() {
        out.push(ev);
    }
    out
}

/// Starts the input thread: it blocks on terminal input and forwards every event. It ends when
/// the receiver is gone or reading fails.
pub fn spawn_input(tx: Sender<AppEvent>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("fallow-input".into())
        .spawn(move || {
            while let Ok(ev) = crossterm::event::read() {
                let burst = crossterm::event::poll(Duration::ZERO).unwrap_or(false);
                if tx.send(AppEvent::Input(ev, burst)).is_err() {
                    break;
                }
            }
        })
        .map(|_| ())
}

/// Forwards procmon snapshots onto the event channel.
pub fn forward_snapshots(rx: Receiver<Arc<Snapshot>>, tx: Sender<AppEvent>) {
    let _ = std::thread::Builder::new()
        .name("fallow-snapshots".into())
        .spawn(move || {
            while let Ok(s) = rx.recv() {
                if tx.send(AppEvent::Snapshot(s)).is_err() {
                    break;
                }
            }
        });
}

/// Limits redraws while output streams: at most one frame per `every`.
#[derive(Debug)]
pub struct FrameCap {
    every: Duration,
    last: Option<Instant>,
}

impl FrameCap {
    pub fn new(every: Duration) -> Self {
        FrameCap { every, last: None }
    }

    /// How long to wait before the next frame may be drawn (zero: now).
    pub fn wait(&self) -> Duration {
        self.last
            .map_or(Duration::ZERO, |at| self.every.saturating_sub(at.elapsed()))
    }

    pub fn drawn(&mut self) {
        self.last = Some(Instant::now());
    }
}
