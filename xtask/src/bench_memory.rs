//! `cargo xtask bench-memory --idle` (T024, FR-040, SC-005, research R-13).
//!
//! Starts Fallow with no project, waits 30 s, samples RSS every second for 60 s and fails if
//! the peak is 50 MB or more. The run must also be valid (pty.rs): first frame, measured PID is
//! Fallow, clean exit, and the liveness counter chosen with `--require` (ticks until procmon
//! exists, then cycles).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::pty::{self, Require, Session};
use crate::rss;

const SETTLE: Duration = Duration::from_secs(30);
const SAMPLES: u64 = 60;
const BUDGET_MB: f64 = 50.0;

struct Args {
    bin: PathBuf,
    csv: PathBuf,
    require: Require,
    settle: Duration,
    samples: u64,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut idle = false;
    let mut a = Args {
        bin: pty::default_binary(),
        csv: PathBuf::from(format!("bench-memory-idle-{}.csv", std::env::consts::OS)),
        require: Require::Ticks,
        settle: SETTLE,
        samples: SAMPLES,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--idle" => idle = true,
            "--bin" => a.bin = value("--bin")?.into(),
            "--csv" => a.csv = value("--csv")?.into(),
            "--require" => a.require = Require::parse(&value("--require")?)?,
            // For quick local checks only; CI uses the defaults.
            "--settle-secs" => {
                a.settle = Duration::from_secs(
                    value("--settle-secs")?
                        .parse()
                        .map_err(|_| "--settle-secs needs a number")?,
                )
            }
            "--samples" => {
                a.samples = value("--samples")?
                    .parse()
                    .map_err(|_| "--samples needs a number")?
            }
            "--loaded" => {
                return Err(
                    "--loaded is not implemented yet (needs the editor and the fixture)".into(),
                );
            }
            other => return Err(format!("bench-memory: unknown argument {other:?}")),
        }
    }
    if !idle {
        return Err("bench-memory: say which profile: --idle".into());
    }
    Ok(a)
}

pub fn run(args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "{e}\nusage: cargo xtask bench-memory --idle [--bin <path>] [--csv <path>] [--require ticks|cycles]"
            );
            return 2;
        }
    };
    let total = a.settle + Duration::from_secs(a.samples);
    println!(
        "bench-memory --idle: {} (settle {} s, {} samples at 1 s, budget peak < {BUDGET_MB} MB, liveness {:?})",
        a.bin.display(),
        a.settle.as_secs(),
        a.samples,
        a.require
    );
    let mut s = match Session::spawn(&a.bin, &[], a.settle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bench-memory: {e}");
            return 2;
        }
    };
    let pid = s.pid;
    let start = Instant::now();
    let mut samples: Vec<(u64, u64)> = Vec::with_capacity(a.samples as usize);
    for i in 1..=a.samples {
        let second = a.settle.as_secs() + i;
        let target = start + Duration::from_secs(second);
        if let Some(d) = target.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
        if s.exited() {
            eprintln!("bench-memory: INVALID: fallow exited early at {second} s");
            return 2;
        }
        let Some(bytes) = rss::rss_bytes(pid) else {
            eprintln!("bench-memory: INVALID: cannot read RSS of pid {pid}");
            return 2;
        };
        samples.push((second, bytes));
    }
    let outcome = s.finish();
    debug_assert!(outcome.elapsed >= total);

    let mut csv = String::from("second,rss_bytes\n");
    for (sec, b) in &samples {
        csv.push_str(&format!("{sec},{b}\n"));
    }
    if let Err(e) = std::fs::write(&a.csv, csv) {
        eprintln!(
            "bench-memory: warning: cannot write {}: {e}",
            a.csv.display()
        );
    }

    let mb = |b: u64| b as f64 / 1_048_576.0;
    let (peak_at, peak) = samples
        .iter()
        .copied()
        .max_by_key(|(_, b)| *b)
        .unwrap_or((0, 0));
    let last = samples.last().map_or(0, |(_, b)| *b);
    let problems = outcome.problems(Some(a.require));
    let valid = problems.is_empty();
    let pass = valid && mb(peak) < BUDGET_MB;
    let result = if !valid {
        "INVALID"
    } else if pass {
        "PASS"
    } else {
        "FAIL"
    };
    println!("evidence: {}", outcome.evidence());
    for p in &problems {
        println!("invalid: {p}");
    }
    println!(
        "os={} samples={} peak_rss_mb={:.2} peak_at_s={peak_at} final_rss_mb={:.2} budget_mb={BUDGET_MB} csv={} result={result}",
        std::env::consts::OS,
        samples.len(),
        mb(peak),
        mb(last),
        a.csv.display()
    );
    pty::step_summary(&format!(
        "### Idle memory ({})\n\n| peak RSS | at | final | budget | liveness | result |\n|---|---|---|---|---|---|\n| {:.2} MB | {peak_at} s | {:.2} MB | < {BUDGET_MB} MB | {} | **{result}** |\n",
        std::env::consts::OS,
        mb(peak),
        mb(last),
        outcome.stats.map_or("no stats".into(), |st| format!(
            "ticks={} cycles={}",
            st.ticks, st.cycles
        )),
    ));
    if !valid {
        2
    } else if pass {
        0
    } else {
        1
    }
}
