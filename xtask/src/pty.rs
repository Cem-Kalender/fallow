//! Runs Fallow inside a pseudo-terminal for the benchmarks (research R-13), with the R-12
//! lessons built in:
//! - a fixed 120×40 PTY, an absolute binary path and an explicit cwd;
//! - the output is drained and every `ESC[6n` cursor query is answered (ConPTY holds the child
//!   until it gets an answer);
//! - the first frame is recognized by the hint-bar marker (contracts/cli.md);
//! - output after warm-up is recorded;
//! - after sampling, the measured PID's executable must be Fallow;
//! - Fallow's own `--bench-stats` file must name the measured PID and show it was live.
//!
//! A run that fails any check is INVALID.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// contracts/cli.md requires "Ctrl-P Commands" in the first frame. Only the word is matched,
/// because ConPTY may re-encode spaces as cursor movement.
pub const FRAME_MARKER: &[u8] = b"Commands";
const CURSOR_QUERY: &[u8] = b"\x1b[6n";
const CTRL_Q: &[u8] = b"\x11";
/// Fallow's event loop wakes at least every 250 ms (research R-6).
pub const TICK: Duration = Duration::from_millis(250);

/// Which counter proves the measured Fallow was live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Require {
    /// Event-loop ticks ≥ 90% of duration ÷ 250 ms (Phase 2 default).
    Ticks,
    /// Procmon cycles ≥ 90% of duration ÷ refresh interval (default once T034 lands).
    Cycles { refresh: Duration },
}

impl Require {
    pub fn parse(s: &str) -> Result<Require, String> {
        match s {
            "ticks" => Ok(Require::Ticks),
            "cycles" => Ok(Require::Cycles {
                refresh: Duration::from_millis(2000),
            }),
            _ => Err(format!("--require must be ticks or cycles, got {s:?}")),
        }
    }
}

/// The line Fallow writes to `--bench-stats` on exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchStats {
    pub pid: u32,
    pub ticks: u64,
    pub cycles: u64,
}

impl BenchStats {
    pub fn parse(text: &str) -> Option<BenchStats> {
        let field = |k: &str| -> Option<u64> {
            text.split_whitespace()
                .find_map(|kv| kv.strip_prefix(k)?.strip_prefix('=')?.parse().ok())
        };
        Some(BenchStats {
            pid: u32::try_from(field("pid")?).ok()?,
            ticks: field("ticks")?,
            cycles: field("cycles")?,
        })
    }
}

/// A running Fallow inside a PTY.
pub struct Session {
    _master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub pid: u32,
    spawned: Instant,
    frame_after: Arc<Mutex<Option<Duration>>>,
    late_bytes: Arc<AtomicU64>,
    stats_path: PathBuf,
    watch: Arc<Mutex<Watch>>,
}

/// A byte pattern the reader looks for in the output, and when it first appeared.
#[derive(Default)]
struct Watch {
    pattern: Vec<u8>,
    seen: Option<Instant>,
}

/// What a finished session proved.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub pid: u32,
    pub elapsed: Duration,
    pub first_frame: Option<Duration>,
    pub output_after_warmup: u64,
    pub exe: String,
    pub clean_exit: bool,
    pub stats: Option<BenchStats>,
    /// The whole `--bench-stats` line, for counters beyond `BenchStats`.
    pub stats_line: String,
}

/// The extra counters Fallow writes to `--bench-stats` (loaded benchmark evidence).
pub fn stat_field(stats_line: &str, key: &str) -> Option<u64> {
    stats_line
        .split_whitespace()
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')?.parse().ok())
}

impl Session {
    /// Starts `bin` with `--bench-stats` and `args`. Output that arrives later than `warmup`
    /// after spawn is counted as evidence of activity.
    pub fn spawn(bin: &Path, args: &[&str], warmup: Duration) -> Result<Session, String> {
        let bin = std::path::absolute(bin).map_err(|e| format!("binary {}: {e}", bin.display()))?;
        if !bin.is_file() {
            return Err(format!(
                "binary {} does not exist (build it first, e.g. cargo build --release)",
                bin.display()
            ));
        }
        let cwd = std::env::current_dir().map_err(|e| format!("cwd: {e}"))?;
        static RUN: AtomicU64 = AtomicU64::new(0);
        let stats_path = std::env::temp_dir().join(format!(
            "fallow-bench-{}-{}.txt",
            std::process::id(),
            RUN.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&stats_path);

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty: {e}"))?;
        let mut cmd = CommandBuilder::new(&bin);
        cmd.arg("--bench-stats");
        cmd.arg(&stats_path);
        for a in args {
            cmd.arg(a);
        }
        // portable-pty starts children in the home directory unless told otherwise.
        cmd.cwd(&cwd);
        if cfg!(unix) {
            cmd.env("TERM", "xterm-256color");
        }
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("pty writer: {e}"))?;
        let writer = Arc::new(Mutex::new(writer));
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("pty reader: {e}"))?;

        let spawned = Instant::now();
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn: {e}"))?;
        drop(pair.slave);
        let pid = child.process_id().ok_or("no child pid")?;

        let frame_after = Arc::new(Mutex::new(None));
        let late_bytes = Arc::new(AtomicU64::new(0));
        let watch = Arc::new(Mutex::new(Watch::default()));
        {
            let (writer, frame_after, late_bytes, watch) = (
                writer.clone(),
                frame_after.clone(),
                late_bytes.clone(),
                watch.clone(),
            );
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let mut tail: Vec<u8> = Vec::with_capacity(8192 + 64);
                let mut frame_seen = false;
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let at = spawned.elapsed();
                    if at >= warmup {
                        late_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    }
                    tail.extend_from_slice(&buf[..n]);
                    if !frame_seen && contains(&tail, FRAME_MARKER) {
                        frame_seen = true;
                        if let Ok(mut f) = frame_after.lock() {
                            *f = Some(at);
                        }
                    }
                    if let Ok(mut w) = watch.lock() {
                        if w.seen.is_none() && !w.pattern.is_empty() && contains(&tail, &w.pattern)
                        {
                            w.seen = Some(Instant::now());
                        }
                    }
                    let queries = count(&tail, CURSOR_QUERY);
                    if queries > 0 {
                        if let Ok(mut w) = writer.lock() {
                            for _ in 0..queries {
                                let _ = w.write_all(b"\x1b[1;1R");
                            }
                            let _ = w.flush();
                        }
                        // Forget the answered queries but keep enough for a split marker.
                        if let Some(p) = rfind(&tail, CURSOR_QUERY) {
                            tail.drain(..p + CURSOR_QUERY.len());
                        }
                    }
                    let keep = tail.len().saturating_sub(32);
                    tail.drain(..keep);
                }
            });
        }
        Ok(Session {
            _master: pair.master,
            child,
            writer,
            pid,
            spawned,
            frame_after,
            late_bytes,
            stats_path,
            watch,
        })
    }

    /// Types `bytes` into the terminal.
    pub fn send(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Starts watching the output for `pattern` (at most 32 bytes), forgetting earlier sightings.
    pub fn expect(&self, pattern: &[u8]) {
        assert!(pattern.len() <= 32, "the reader keeps a 32-byte tail");
        if let Ok(mut w) = self.watch.lock() {
            w.pattern = pattern.to_vec();
            w.seen = None;
        }
    }

    /// When the expected pattern appeared, waiting up to `timeout`.
    pub fn seen(&mut self, timeout: Duration) -> Option<Instant> {
        let until = Instant::now() + timeout;
        loop {
            if let Some(at) = self.watch.lock().ok().and_then(|w| w.seen) {
                return Some(at);
            }
            if Instant::now() >= until || self.exited() {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Time from spawn to the first frame, waiting up to `timeout`.
    pub fn wait_frame(&mut self, timeout: Duration) -> Option<Duration> {
        let until = Instant::now() + timeout;
        loop {
            if let Some(d) = self.frame_after.lock().ok().and_then(|f| *f) {
                return Some(d);
            }
            if Instant::now() >= until || matches!(self.child.try_wait(), Ok(Some(_))) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Checks the executable (the PID must be Fallow by now), then quits with Ctrl-Q, waits up
    /// to 5 s for a clean exit and reads the stats file.
    pub fn finish(mut self) -> Outcome {
        let exe = crate::rss::exe_path(self.pid).unwrap_or_else(|| "<unreadable>".into());
        let elapsed = self.spawned.elapsed();
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(CTRL_Q);
            let _ = w.flush();
        }
        let until = Instant::now() + Duration::from_secs(5);
        let mut clean_exit = false;
        while Instant::now() < until {
            if let Ok(Some(status)) = self.child.try_wait() {
                clean_exit = status.success();
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !clean_exit {
            let _ = self.child.kill();
        }
        let stats_line = std::fs::read_to_string(&self.stats_path).unwrap_or_default();
        let stats = BenchStats::parse(&stats_line);
        let _ = std::fs::remove_file(&self.stats_path);
        Outcome {
            pid: self.pid,
            elapsed,
            first_frame: self.frame_after.lock().ok().and_then(|f| *f),
            output_after_warmup: self.late_bytes.load(Ordering::Relaxed),
            exe,
            clean_exit,
            stats,
            stats_line,
        }
    }
}

impl Outcome {
    /// Every reason this run cannot count. Empty means valid.
    pub fn problems(&self, require: Option<Require>) -> Vec<String> {
        let mut p = Vec::new();
        if self.first_frame.is_none() {
            p.push("no first frame (marker \"Commands\" never seen)".to_string());
        }
        let stem = Path::new(&self.exe)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned());
        if stem.as_deref() != Some("fallow") {
            p.push(format!(
                "measured PID {} is not fallow (exe {})",
                self.pid, self.exe
            ));
        }
        if !self.clean_exit {
            p.push("fallow did not exit cleanly on Ctrl-Q within 5 s".to_string());
        }
        match self.stats {
            None => p.push("no --bench-stats file written".to_string()),
            Some(s) if s.pid != self.pid => {
                p.push(format!("stats pid {} != measured pid {}", s.pid, self.pid))
            }
            Some(s) => match require {
                None => {}
                Some(Require::Ticks) => {
                    let expected = self.elapsed.as_secs_f64() / TICK.as_secs_f64();
                    if (s.ticks as f64) < 0.9 * expected {
                        p.push(format!("ticks {} < 90% of expected {expected:.0}", s.ticks));
                    }
                }
                Some(Require::Cycles { refresh }) => {
                    let expected = self.elapsed.as_secs_f64() / refresh.as_secs_f64();
                    if (s.cycles as f64) < 0.9 * expected {
                        p.push(format!(
                            "cycles {} < 90% of expected {expected:.0}",
                            s.cycles
                        ));
                    }
                }
            },
        }
        p
    }

    pub fn evidence(&self) -> String {
        format!(
            "pid={} exe={} elapsed_s={:.1} first_frame_ms={} output_after_warmup_bytes={} clean_exit={} stats={}",
            self.pid,
            self.exe,
            self.elapsed.as_secs_f64(),
            self.first_frame
                .map_or("none".into(), |d| format!("{:.1}", ms(d))),
            self.output_after_warmup,
            self.clean_exit,
            self.stats.map_or("none".into(), |s| format!(
                "pid={} ticks={} cycles={}",
                s.pid, s.ticks, s.cycles
            )),
        )
    }
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Appends Markdown to the GitHub Actions job summary when running in CI.
pub fn step_summary(markdown: &str) {
    if let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            let _ = writeln!(f, "{markdown}");
        }
    }
}

/// The release binary in this workspace's target directory.
pub fn default_binary() -> PathBuf {
    PathBuf::from("target")
        .join("release")
        .join(format!("fallow{}", std::env::consts::EXE_SUFFIX))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bench_stats() {
        let s = BenchStats::parse("pid=42 ticks=360 cycles=0\n").unwrap();
        assert_eq!(
            s,
            BenchStats {
                pid: 42,
                ticks: 360,
                cycles: 0
            }
        );
        assert!(BenchStats::parse("pid=42 ticks=1").is_none());
    }

    fn outcome(ticks: u64, pid: u32) -> Outcome {
        Outcome {
            pid: 7,
            elapsed: Duration::from_secs(90),
            first_frame: Some(Duration::from_millis(20)),
            output_after_warmup: 0,
            exe: "/x/target/release/fallow".into(),
            clean_exit: true,
            stats: Some(BenchStats {
                pid,
                ticks,
                cycles: 0,
            }),
            stats_line: String::new(),
        }
    }

    #[test]
    fn liveness_and_identity() {
        assert!(
            outcome(324, 7).problems(Some(Require::Ticks)).is_empty(),
            "90% of 360"
        );
        assert_eq!(outcome(323, 7).problems(Some(Require::Ticks)).len(), 1);
        assert_eq!(
            outcome(400, 8).problems(Some(Require::Ticks)).len(),
            1,
            "wrong pid"
        );
        let cycles = Require::Cycles {
            refresh: Duration::from_secs(2),
        };
        assert_eq!(
            outcome(400, 7).problems(Some(cycles)).len(),
            1,
            "no cycles yet"
        );
        let mut o = outcome(400, 7);
        o.exe = "/usr/bin/xtask".into();
        assert_eq!(o.problems(None).len(), 1);
    }

    #[test]
    fn counts_queries() {
        assert_eq!(count(b"a\x1b[6nb\x1b[6n", CURSOR_QUERY), 2);
        assert_eq!(rfind(b"a\x1b[6nb", CURSOR_QUERY), Some(1));
    }
}
