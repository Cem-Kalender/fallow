//! `cargo xtask bench-search` (T084, SC-010, research R-13).
//!
//! On the reference large project (`cargo xtask gen-fixture`), measures the time from `Enter` in
//! the find bar's "whole project" scope to the first result row on screen (the results header
//! `Results:`, which Fallow draws only once a result exists). 10 runs, each in a fresh Fallow;
//! fails if the p95 is 1 s or more. A run that never shows a result, or whose Fallow does not
//! pass the pty.rs identity checks, makes the benchmark INVALID.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::pty::{self, Session};

const RUNS: usize = 10;
const BUDGET: Duration = Duration::from_secs(1);
const MARKER: &[u8] = b"Results:";

struct Args {
    bin: PathBuf,
    fixture: PathBuf,
    runs: usize,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        bin: pty::default_binary(),
        fixture: crate::gen_fixture::default_dir(),
        runs: RUNS,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |n: &str| it.next().cloned().ok_or(format!("{n} needs a value"));
        match arg.as_str() {
            "--bin" => a.bin = value("--bin")?.into(),
            "--fixture" => a.fixture = value("--fixture")?.into(),
            "--runs" => a.runs = value("--runs")?.parse().map_err(|_| "--runs: number")?,
            other => return Err(format!("bench-search: unknown argument {other:?}")),
        }
    }
    if !a.fixture.join(".fallow-fixture").is_file() {
        return Err(format!(
            "bench-search: no fixture at {} (run cargo xtask gen-fixture)",
            a.fixture.display()
        ));
    }
    a.fixture = std::path::absolute(&a.fixture).map_err(|e| e.to_string())?;
    Ok(a)
}

/// One run: the time from Enter to the first result row.
fn once(a: &Args) -> Result<Duration, String> {
    let fixture = a.fixture.display().to_string();
    let mut s = Session::spawn(&a.bin, &[&fixture], Duration::from_secs(1))?;
    let result = (|| {
        if s.wait_frame(Duration::from_secs(10)).is_none() {
            return Err("no first frame".to_string());
        }
        // Fallow starts on the agent workspace (002); Ctrl-E switches to the editor view of the
        // project, with focus in Files. Tab moves to the editor, where Ctrl-F opens the find
        // bar. No file is open, so it opens in "whole project".
        s.send(b"\x05");
        std::thread::sleep(Duration::from_millis(300));
        s.send(b"\t");
        std::thread::sleep(Duration::from_millis(200));
        s.send(b"\x06");
        std::thread::sleep(Duration::from_millis(300));
        s.send(crate::gen_fixture::NEEDLE.as_bytes());
        std::thread::sleep(Duration::from_millis(500));
        s.expect(MARKER);
        let t0 = Instant::now();
        s.send(b"\r");
        match s.seen(Duration::from_secs(30)) {
            Some(t1) => Ok(t1.saturating_duration_since(t0)),
            None => Err("no result row within 30 s".to_string()),
        }
    })();
    // Esc cancels a search that is still running, so Ctrl-Q quits promptly. Esc directly
    // followed by Ctrl-Q would read as Alt+Ctrl-Q on Unix terminals, hence the pause.
    s.send(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));
    let outcome = s.finish();
    let problems = outcome.problems(None);
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let searches = pty::stat_field(&outcome.stats_line, "searches").unwrap_or(0);
    if searches != 1 {
        return Err(format!("fallow ran {searches} searches, expected 1"));
    }
    result
}

pub fn run(args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\nusage: cargo xtask bench-search [--fixture <dir>] [--bin <path>]");
            return 2;
        }
    };
    println!(
        "bench-search: {} on {} ({} runs, budget p95 < {} ms)",
        a.bin.display(),
        a.fixture.display(),
        a.runs,
        BUDGET.as_millis()
    );
    let mut times = Vec::with_capacity(a.runs);
    for i in 1..=a.runs {
        match once(&a) {
            Ok(d) => {
                println!("run {i}: first result after {:.1} ms", pty::ms(d));
                times.push(d);
            }
            Err(e) => {
                println!("run {i}: INVALID: {e}");
                pty::step_summary(&format!(
                    "### Search time ({})\n\nrun {i} INVALID: {e}\n",
                    std::env::consts::OS
                ));
                return 2;
            }
        }
    }
    times.sort();
    // Nearest-rank p95.
    let rank = ((times.len() as f64) * 0.95).ceil() as usize;
    let p95 = times[rank.clamp(1, times.len()) - 1];
    let median = times[times.len() / 2];
    let pass = p95 < BUDGET;
    let result = if pass { "PASS" } else { "FAIL" };
    println!(
        "os={} runs={} median_ms={:.1} p95_ms={:.1} max_ms={:.1} budget_ms={} result={result}",
        std::env::consts::OS,
        times.len(),
        pty::ms(median),
        pty::ms(p95),
        pty::ms(*times.last().unwrap_or(&Duration::ZERO)),
        BUDGET.as_millis()
    );
    pty::step_summary(&format!(
        "### Search time ({})\n\n| median | p95 | budget | result |\n|---|---|---|---|\n| {:.1} ms | {:.1} ms | < {} ms | **{result}** |\n",
        std::env::consts::OS,
        pty::ms(median),
        pty::ms(p95),
        BUDGET.as_millis()
    ));
    if pass { 0 } else { 1 }
}
