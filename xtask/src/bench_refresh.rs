//! `cargo xtask bench-refresh` (T117, plan Performance Goals, Constitution VI).
//!
//! Raises the process count to about 600 with idle children, runs Fallow in the PTY harness at
//! the default 2 s refresh, waits 30 s, then samples Fallow's own CPU time from the OS over
//! 120 s. Fails if the average exceeds 2% of one core.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::fixtures;
use crate::pty::{self, Require, Session};

const BUDGET_PCT: f64 = 2.0;

pub fn run(args: &[String]) -> i32 {
    let mut bin = pty::default_binary();
    let (mut target, mut warmup, mut secs) = (600usize, 30u64, 120u64);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut num = |name: &str| {
            it.next()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or(format!("{name} needs a number"))
        };
        let r = match a.as_str() {
            "--bin" => it
                .next()
                .map(|v| bin = PathBuf::from(v))
                .ok_or("--bin needs a path".to_string()),
            "--target" => num("--target").map(|v| target = v as usize),
            "--warmup-secs" => num("--warmup-secs").map(|v| warmup = v),
            "--secs" => num("--secs").map(|v| secs = v),
            other => Err(format!("bench-refresh: unknown argument {other:?}")),
        };
        if let Err(e) = r {
            eprintln!("{e}");
            return 2;
        }
    }

    let before = fixtures::process_count();
    let me = std::env::current_exe().expect("current exe");
    let mut children: Vec<Child> = Vec::new();
    for _ in before..target {
        match Command::new(&me)
            .arg("idle-child")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => children.push(c),
            Err(e) => {
                eprintln!("bench-refresh: spawning fixture {}: {e}", children.len());
                break;
            }
        }
    }
    std::thread::sleep(Duration::from_secs(2));
    let reached = fixtures::process_count();
    println!(
        "bench-refresh: processes before={before} spawned={} reached={reached} (target ~{target})",
        children.len()
    );
    fixtures::flush();

    let result = measure(&bin, warmup, secs, reached);
    for mut c in children {
        let _ = c.kill();
        let _ = c.wait();
    }
    result
}

fn measure(bin: &std::path::Path, warmup: u64, secs: u64, reached: usize) -> i32 {
    let mut s = match Session::spawn(bin, &[], Duration::from_secs(warmup)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bench-refresh: {e}");
            return 2;
        }
    };
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(warmup));
    let Some(t0) = fixtures::cpu_time(s.pid) else {
        eprintln!(
            "bench-refresh: INVALID: cannot read CPU time of pid {}",
            s.pid
        );
        return 2;
    };
    let m0 = Instant::now();
    std::thread::sleep(Duration::from_secs(secs));
    if s.exited() {
        eprintln!("bench-refresh: INVALID: fallow exited early");
        return 2;
    }
    let t1 = fixtures::cpu_time(s.pid).unwrap_or(t0);
    let wall = m0.elapsed();
    let outcome = s.finish();
    let pct = (t1 - t0).as_secs_f64() / wall.as_secs_f64() * 100.0;
    let problems = outcome.problems(Some(Require::Cycles {
        refresh: Duration::from_secs(2),
    }));
    let valid = problems.is_empty() && start.elapsed() >= Duration::from_secs(warmup + secs);
    let pass = valid && pct <= BUDGET_PCT;
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
        "os={} processes={reached} sample_s={:.1} cpu_s={:.3} avg_cpu_pct={pct:.3} budget_pct={BUDGET_PCT} result={result}",
        std::env::consts::OS,
        wall.as_secs_f64(),
        (t1 - t0).as_secs_f64()
    );
    fixtures::summary(&format!(
        "### Refresh CPU ({})\n\n| processes | sampled | Fallow CPU | avg | budget | result |\n|---|---|---|---|---|---|\n| {reached} | {:.0} s | {:.2} s | {pct:.2}% of one core | ≤ {BUDGET_PCT}% | **{result}** |\n",
        std::env::consts::OS,
        wall.as_secs_f64(),
        (t1 - t0).as_secs_f64()
    ));
    if !valid {
        2
    } else if pass {
        0
    } else {
        1
    }
}
