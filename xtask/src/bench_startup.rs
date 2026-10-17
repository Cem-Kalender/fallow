//! `cargo xtask bench-startup` (T023, FR-041, SC-006, research R-13).
//!
//! Time from spawn to the first frame, 3 warm-ups then 30 measured runs. The budget is met when
//! the p95 is under 100 ms (constitution v3.0.1). Shared CI runners are noisy, so a failing
//! benchmark is re-run once, warm-ups included; the second result is final.

use std::path::PathBuf;
use std::time::Duration;

use crate::pty::{self, Session, ms};

const WARMUPS: usize = 3;
const RUNS: usize = 30;
const BUDGET_MS: f64 = 100.0;
const FRAME_TIMEOUT: Duration = Duration::from_secs(10);

struct Args {
    bin: PathBuf,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut bin = pty::default_binary();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bin" => bin = it.next().ok_or("--bin needs a path")?.into(),
            other => return Err(format!("bench-startup: unknown argument {other:?}")),
        }
    }
    Ok(Args { bin })
}

pub fn run(args: &[String]) -> i32 {
    let args = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\nusage: cargo xtask bench-startup [--bin <path>]");
            return 2;
        }
    };
    println!(
        "bench-startup: {} ({WARMUPS} warm-ups, {RUNS} runs, budget p95 < {BUDGET_MS} ms)",
        args.bin.display()
    );
    let first = attempt(&args, 1);
    let result = match first {
        Verdict::Fail => {
            println!(
                "bench-startup: over budget; re-running the whole benchmark once (the second result is final)"
            );
            attempt(&args, 2)
        }
        v => v,
    };
    match result {
        Verdict::Pass => 0,
        Verdict::Fail => 1,
        Verdict::Invalid => 2,
    }
}

enum Verdict {
    Pass,
    Fail,
    Invalid,
}

fn attempt(args: &Args, n: usize) -> Verdict {
    let mut times = Vec::with_capacity(RUNS);
    for i in 0..WARMUPS + RUNS {
        let warmup = i < WARMUPS;
        let mut s = match Session::spawn(&args.bin, &[], Duration::from_secs(3600)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bench-startup: {e}");
                return Verdict::Invalid;
            }
        };
        let frame = s.wait_frame(FRAME_TIMEOUT);
        let outcome = s.finish();
        let problems = outcome.problems(None);
        if !problems.is_empty() {
            eprintln!(
                "bench-startup: run {} INVALID: {}\n  evidence: {}",
                i + 1,
                problems.join("; "),
                outcome.evidence()
            );
            return Verdict::Invalid;
        }
        let Some(t) = frame else {
            return Verdict::Invalid;
        };
        if !warmup {
            times.push(ms(t));
        }
    }
    times.sort_by(|a, b| a.total_cmp(b));
    let p50 = percentile(&times, 0.50);
    let p95 = percentile(&times, 0.95);
    let max = times.last().copied().unwrap_or(0.0);
    let pass = p95 < BUDGET_MS;
    let result = if pass { "PASS" } else { "FAIL" };
    println!(
        "os={} attempt={n} runs={} p50_ms={p50:.1} p95_ms={p95:.1} max_ms={max:.1} budget_p95_ms={BUDGET_MS} result={result}",
        std::env::consts::OS,
        times.len()
    );
    println!(
        "  all runs (ms, sorted): {}",
        times
            .iter()
            .map(|t| format!("{t:.1}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    pty::step_summary(&format!(
        "### Startup ({}, attempt {n})\n\n| runs | p50 | p95 | max | budget | result |\n|---|---|---|---|---|---|\n| {} | {p50:.1} ms | {p95:.1} ms | {max:.1} ms | p95 < {BUDGET_MS} ms | **{result}** |\n",
        std::env::consts::OS,
        times.len()
    ));
    if pass { Verdict::Pass } else { Verdict::Fail }
}

/// Nearest-rank percentile of sorted values.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[cfg(test)]
mod tests {
    use super::percentile;

    #[test]
    fn nearest_rank() {
        let v: Vec<f64> = (1..=30).map(f64::from).collect();
        assert_eq!(percentile(&v, 0.95), 29.0, "p95 of 30 is the 29th value");
        assert_eq!(percentile(&v, 0.50), 15.0);
        assert_eq!(percentile(&[5.0], 0.95), 5.0);
    }
}
