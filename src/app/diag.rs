//! Latency tracing for performance investigations (002 SC-006). Active only when the
//! `FALLOW_DIAG_LOG` environment variable names a file; then each line is
//! `<microseconds since start> <event> <value>`. Timings only: no screen or key content is
//! recorded.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct Log {
    start: Instant,
    out: Mutex<std::io::BufWriter<std::fs::File>>,
}

static LOG: OnceLock<Option<Log>> = OnceLock::new();

fn log() -> Option<&'static Log> {
    LOG.get_or_init(|| {
        let path = std::env::var_os("FALLOW_DIAG_LOG")?;
        let f = std::fs::File::create(path).ok()?;
        Some(Log {
            start: Instant::now(),
            out: Mutex::new(std::io::BufWriter::new(f)),
        })
    })
    .as_ref()
}

/// Records an event if tracing is on.
pub fn mark(event: &str, value: u64) {
    if let Some(l) = log() {
        let us = l.start.elapsed().as_micros();
        if let Ok(mut o) = l.out.lock() {
            let _ = writeln!(o, "{us} {event} {value}");
        }
    }
}

/// Flushes the trace (on exit).
pub fn flush() {
    if let Some(l) = log() {
        if let Ok(mut o) = l.out.lock() {
            let _ = o.flush();
        }
    }
}
