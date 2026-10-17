//! `cargo xtask bench-new-process` (T118, SC-004, Constitution VI).
//!
//! Runs `fallow --snapshot-stream`, starts a stand-in that matches a built-in signature (a copy
//! of this binary named `ollama`) at a random offset within a refresh interval, and measures
//! the time until its PID first appears in the stream and, after it is stopped, until it is
//! gone. 10 repetitions; any time over two refresh intervals (4 s) fails.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::fixtures;
use crate::pty;

const REPS: usize = 10;
const REFRESH: Duration = Duration::from_secs(2);
const LIMIT: Duration = Duration::from_secs(4);
const WAIT: Duration = Duration::from_secs(15);

pub fn run(args: &[String]) -> i32 {
    let mut bin = pty::default_binary();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bin" => match it.next() {
                Some(v) => bin = PathBuf::from(v),
                None => return usage("--bin needs a path"),
            },
            other => return usage(&format!("unknown argument {other:?}")),
        }
    }
    let bin = std::path::absolute(&bin).unwrap_or(bin);
    let dir = fixtures::temp_dir("newproc");
    let stand_in = fixtures::copy_self_as(&dir, "ollama");

    // Long enough for 10 repetitions of up to 2 s offset + 2 × 4 s.
    let mut fallow = match Command::new(&bin)
        .args(["--snapshot-stream", "150", "--no-dns"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bench-new-process: cannot start {}: {e}", bin.display());
            return 2;
        }
    };
    let (tx, rx) = mpsc::channel::<(Instant, String)>();
    let out = fallow.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send((Instant::now(), line)).is_err() {
                return;
            }
        }
    });
    // The first line shows the stream is running.
    if rx.recv_timeout(WAIT).is_err() {
        eprintln!("bench-new-process: INVALID: no output from fallow --snapshot-stream");
        let _ = fallow.kill();
        return 2;
    }

    let has = |line: &str, pid: u32| line.contains(&format!("{{\"pid\":{pid},"));
    let mut appear = Vec::new();
    let mut disappear = Vec::new();
    let mut invalid = None;
    for rep in 0..REPS {
        let offset = REFRESH.mul_f64(fixtures::random_bytes(1)[0] as f64 / 256.0);
        std::thread::sleep(offset);
        while rx.try_recv().is_ok() {}
        let started = Instant::now();
        let mut child = match Command::new(&stand_in)
            .args(["run", "stand-in"])
            .stdout(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                invalid = Some(format!("spawning stand-in: {e}"));
                break;
            }
        };
        let pid = child.id();
        let seen = wait_for(&rx, started, |l| has(l, pid));
        let stopped = Instant::now();
        let _ = child.kill();
        let _ = child.wait();
        let gone = wait_for(&rx, stopped, |l| !has(l, pid));
        match (seen, gone) {
            (Some(a), Some(d)) => {
                println!(
                    "rep {:2}: offset {:4.0} ms  appeared after {:5.0} ms  disappeared after {:5.0} ms",
                    rep + 1,
                    offset.as_secs_f64() * 1000.0,
                    pty::ms(a),
                    pty::ms(d)
                );
                appear.push(a);
                disappear.push(d);
            }
            _ => {
                invalid = Some(format!(
                    "rep {}: stand-in pid {pid} not seen or never gone within {WAIT:?}",
                    rep + 1
                ));
                break;
            }
        }
        fixtures::flush();
    }
    let _ = fallow.kill();
    let _ = fallow.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let max = |v: &[Duration]| v.iter().max().copied().unwrap_or_default();
    let (ma, md) = (max(&appear), max(&disappear));
    let valid = invalid.is_none() && appear.len() == REPS;
    let pass = valid && ma <= LIMIT && md <= LIMIT;
    let result = if !valid {
        "INVALID"
    } else if pass {
        "PASS"
    } else {
        "FAIL"
    };
    if let Some(why) = &invalid {
        println!("invalid: {why}");
    }
    println!(
        "os={} reps={} max_appear_ms={:.0} max_disappear_ms={:.0} limit_ms={:.0} result={result}",
        std::env::consts::OS,
        appear.len(),
        pty::ms(ma),
        pty::ms(md),
        pty::ms(LIMIT)
    );
    fixtures::summary(&format!(
        "### New/exited process latency ({})\n\n| reps | max appear | max disappear | limit | result |\n|---|---|---|---|---|\n| {} | {:.0} ms | {:.0} ms | {:.0} ms | **{result}** |\n",
        std::env::consts::OS,
        appear.len(),
        pty::ms(ma),
        pty::ms(md),
        pty::ms(LIMIT)
    ));
    if !valid {
        2
    } else if pass {
        0
    } else {
        1
    }
}

/// Time from `since` to the first stream line received after `since` that satisfies `ok`.
fn wait_for(
    rx: &mpsc::Receiver<(Instant, String)>,
    since: Instant,
    ok: impl Fn(&str) -> bool,
) -> Option<Duration> {
    while since.elapsed() < WAIT {
        let Ok((at, line)) = rx.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        if at >= since && ok(&line) {
            return Some(at - since);
        }
    }
    None
}

fn usage(msg: &str) -> i32 {
    eprintln!("bench-new-process: {msg}\nusage: cargo xtask bench-new-process [--bin <path>]");
    2
}
