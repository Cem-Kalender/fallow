//! `cargo xtask diag-sessions [--out DIR]`: why is key echo slow while sessions print? (002
//! SC-006 investigation, research R-9 addendum).
//!
//! 1. **Baseline without Fallow**: the same load (7 `flood-agent rate:1000` stand-ins and one
//!    `echo-agent`), each in its own pseudo-terminal, read and discarded by plain threads. Key
//!    echo latency here is what the machine itself gives.
//! 2. **Fallow**: `bench-sessions` run 2 with `FALLOW_DIAG_SAMPLE=DIR`, which records a
//!    `sample` profile of Fallow on macOS while the keys are typed.
//!
//! Writes a summary to stdout and the job summary; the profile goes to DIR.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

type Writer = std::sync::Arc<std::sync::Mutex<Box<dyn Write + Send>>>;
type Agent = (
    Writer,
    Receiver<Vec<u8>>,
    Box<dyn portable_pty::Child + Send + Sync>,
    Box<dyn portable_pty::MasterPty + Send>,
);

/// An xtask persona in its own pseudo-terminal. The reader answers the cursor-position query
/// ConPTY sends at startup (it blocks the child until answered).
fn pty_agent(args: &[&str]) -> Result<Agent, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new(me);
    cmd.args(args);
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer: Writer = std::sync::Arc::new(std::sync::Mutex::new(
        pair.master.take_writer().map_err(|e| e.to_string())?,
    ));
    let (tx, rx) = channel();
    let w = writer.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            if contains(&buf[..n], b"\x1b[6n") {
                if let Ok(mut w) = w.lock() {
                    let _ = w.write_all(b"\x1b[1;1R");
                    let _ = w.flush();
                }
            }
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Ok((writer, rx, child, pair.master))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Key echo latency with no Fallow involved, under the same flood.
fn baseline(keys: usize) -> Result<Vec<Duration>, String> {
    let mut flooders = Vec::new();
    for _ in 0..7 {
        let (_w, rx, child, master) = pty_agent(&["flood-agent", "rate:1000"])?;
        // Drain and discard, like a reader that does no parsing.
        std::thread::spawn(move || while rx.recv().is_ok() {});
        flooders.push((child, master));
    }
    let (w, rx, mut echo, _m) = pty_agent(&["echo-agent"])?;
    let mut seen = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while !contains(&seen, b"echo-ready") && Instant::now() < until {
        if let Ok(c) = rx.recv_timeout(Duration::from_millis(100)) {
            seen.extend(c);
        }
    }
    std::thread::sleep(Duration::from_secs(1));
    while rx.try_recv().is_ok() {}
    let mut times = Vec::with_capacity(keys);
    for i in 0..keys {
        let want: &[u8] = if i % 2 == 0 { b"OOOOOOOO" } else { b"XXXXXXXX" };
        let t0 = Instant::now();
        {
            let mut w = w.lock().map_err(|_| "writer lock")?;
            w.write_all(b"k").map_err(|e| e.to_string())?;
            w.flush().ok();
        }
        let mut got = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(c) => {
                    got.extend(c);
                    if contains(&got, want) {
                        break;
                    }
                }
                Err(_) => return Err(format!("baseline key {i}: no echo")),
            }
        }
        times.push(t0.elapsed());
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = echo.kill();
    for (mut c, _) in flooders {
        let _ = c.kill();
    }
    Ok(times)
}

fn p(v: &[Duration], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort();
    let i = ((s.len() as f64 * q).ceil() as usize).clamp(1, s.len()) - 1;
    s[i].as_secs_f64() * 1000.0
}

pub fn run(args: &[String]) -> i32 {
    let mut out = PathBuf::from("target").join("diag");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--out" {
            if let Some(o) = it.next() {
                out = PathBuf::from(o);
            }
        }
    }
    let _ = std::fs::create_dir_all(&out);
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    println!("diag-sessions: os={} cores={cores}", std::env::consts::OS);
    let base = match baseline(50) {
        Ok(t) => t,
        Err(e) => {
            println!("baseline: ERROR {e}");
            Vec::new()
        }
    };
    if !base.is_empty() {
        println!(
            "baseline (no Fallow, 7 flood + 1 echo in plain PTYs): p50_ms={:.1} p95_ms={:.1} max_ms={:.1}",
            p(&base, 0.5),
            p(&base, 0.95),
            p(&base, 1.0)
        );
    }
    // Fallow under the same load, with a profile when the platform has a sampler.
    // SAFETY-free: environment variable for a child we start next.
    unsafe {
        std::env::set_var("FALLOW_DIAG_SAMPLE", &out);
    }
    let code = crate::bench_sessions::run(&[]);
    crate::pty::step_summary(&format!(
        "### diag-sessions ({}, {cores} cores)\n\nbaseline without Fallow: p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms\n\nbench-sessions exit code: {code}\n",
        std::env::consts::OS,
        if base.is_empty() { 0.0 } else { p(&base, 0.5) },
        if base.is_empty() { 0.0 } else { p(&base, 0.95) },
        if base.is_empty() { 0.0 } else { p(&base, 1.0) },
    ));
    0
}
