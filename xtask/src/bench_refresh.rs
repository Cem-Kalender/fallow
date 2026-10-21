//! `cargo xtask bench-refresh` (T117, plan Performance Goals, Constitution VI).
//!
//! Raises the process count to about 600 with idle children and measures Fallow's own CPU time
//! (from the OS, over 120 s after a 30 s warm-up, default 2 s refresh) twice, each time with a
//! fresh Fallow:
//! 1. **without** large processes;
//! 2. **with** at least five LLM processes (`llama-server` stand-ins) that each map and touch
//!    ≥ 1 GB, so footprint reads cost what they cost on a real machine (research R-3).
//!
//! The 2% of one core budget applies to run 2. Run 1 is reported for comparison.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::fixtures;
use crate::pty::{self, Require, Session};

const BUDGET_PCT: f64 = 2.0;

struct Children(Vec<Child>);

impl Drop for Children {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

struct Opts {
    bin: PathBuf,
    target: usize,
    warmup: u64,
    secs: u64,
    large: usize,
    large_mb: usize,
}

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        bin: pty::default_binary(),
        target: 600,
        warmup: 30,
        secs: 120,
        large: 5,
        large_mb: 1024,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut num = |name: &str| {
            it.next()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or(format!("{name} needs a number"))
        };
        match a.as_str() {
            "--bin" => o.bin = it.next().ok_or("--bin needs a path")?.into(),
            "--target" => o.target = num("--target")? as usize,
            "--warmup-secs" => o.warmup = num("--warmup-secs")?,
            "--secs" => o.secs = num("--secs")?,
            "--large" => o.large = num("--large")? as usize,
            "--large-mb" => o.large_mb = num("--large-mb")? as usize,
            other => return Err(format!("bench-refresh: unknown argument {other:?}")),
        }
    }
    Ok(o)
}

pub fn run(args: &[String]) -> i32 {
    let o = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let me = std::env::current_exe().expect("current exe");
    let mut idle = Children(Vec::new());
    let before = fixtures::process_count();
    // Leave room for the large processes so both runs see about the same total.
    for _ in before..o.target.saturating_sub(o.large) {
        match spawn(Command::new(&me).arg("idle-child")) {
            Ok(c) => idle.0.push(c),
            Err(e) => {
                eprintln!("bench-refresh: spawning fixture {}: {e}", idle.0.len());
                break;
            }
        }
    }
    std::thread::sleep(Duration::from_secs(2));

    // Run 1: without large processes. Pad with idle children up to the target.
    let mut pad = Children(Vec::new());
    for _ in 0..o.large {
        if let Ok(c) = spawn(Command::new(&me).arg("idle-child")) {
            pad.0.push(c);
        }
    }
    std::thread::sleep(Duration::from_secs(1));
    let n1 = fixtures::process_count();
    println!("bench-refresh: run 1 without large processes: processes={n1} (before={before})");
    fixtures::flush();
    let r1 = measure(&o, n1, "without");
    drop(pad);

    // Run 2: the same total, with the large LLM processes in place of the padding.
    let dir = fixtures::temp_dir("refresh");
    let big_bin = fixtures::copy_self_as(&dir, "llama-server");
    let mut big = Children(Vec::new());
    for i in 0..o.large {
        let ready = dir.join(format!("ready-{i}"));
        match spawn(
            Command::new(&big_bin)
                .args(["-m", "stand-in.gguf"])
                .env("FALLOW_FIXTURE_ALLOC_MB", o.large_mb.to_string())
                .env("FALLOW_FIXTURE_READY_FILE", &ready),
        ) {
            Ok(c) => big.0.push(c),
            Err(e) => eprintln!("bench-refresh: large fixture {i}: {e}"),
        }
    }
    let deadline = Instant::now() + Duration::from_secs(180);
    while (0..o.large).any(|i| !dir.join(format!("ready-{i}")).exists())
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(200));
    }
    let n2 = fixtures::process_count();
    let (detected, min_fp) = large_detected(&o.bin, &big);
    println!(
        "bench-refresh: run 2 with large processes: processes={n2} large={} each={} MB detected_as_llm={detected} smallest_footprint_mb={:.0}",
        big.0.len(),
        o.large_mb,
        min_fp as f64 / 1_048_576.0
    );
    fixtures::flush();
    let mut r2 = measure(&o, n2, "with");
    // Windows may trim an idle fixture's working set even with a minimum set (see
    // `fixtures::keep_working_set`); the footprint read costs the same either way, so half the
    // allocation is enough there. Elsewhere it must be near the full size.
    let need = if cfg!(windows) {
        (o.large_mb as u64) * 1_048_576 / 2
    } else {
        (o.large_mb as u64) * 1_048_576 * 9 / 10
    };
    let enough = detected >= o.large && min_fp >= need;
    if r2.code != 2 && !enough {
        println!(
            "invalid: {detected} of {} large processes detected as LLM processes; the smallest footprint is {:.0} MB, at least {} MB needed",
            o.large,
            min_fp as f64 / 1_048_576.0,
            need / 1_048_576
        );
        r2.code = 2;
        r2.result = "INVALID";
    }
    drop(big);
    let _ = std::fs::remove_dir_all(&dir);

    let os = std::env::consts::OS;
    println!(
        "os={os} without_large_pct={:.3} with_large_pct={:.3} large={}x{}MB budget_pct={BUDGET_PCT} result={}",
        r1.pct, r2.pct, o.large, o.large_mb, r2.result
    );
    fixtures::summary(&format!(
        "### Refresh CPU ({os})\n\n| run | processes | Fallow CPU (avg, % of one core) | result |\n|---|---|---|---|\n| without large processes | {n1} | {:.2}% | {} (reported) |\n| with {} LLM processes × {} MB | {n2} | {:.2}% | **{}** (budget ≤ {BUDGET_PCT}%) |\n",
        r1.pct, r1.result, o.large, o.large_mb, r2.pct, r2.result
    ));
    if r1.code == 2 { 2 } else { r2.code }
}

fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// How many of the large fixtures Fallow detects as LLM processes, and the smallest footprint
/// among them.
fn large_detected(bin: &Path, big: &Children) -> (usize, u64) {
    let Ok(out) = Command::new(bin)
        .args(["--snapshot-json", "--no-dns"])
        .output()
    else {
        return (0, 0);
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return (0, 0);
    };
    let pids: Vec<u64> = big.0.iter().map(|c| c.id() as u64).collect();
    let detected = v["llm"].as_array().map_or(0, |l| {
        l.iter()
            .filter(|x| pids.contains(&x["key"]["pid"].as_u64().unwrap_or(0)))
            .count()
    });
    let min_fp = v["records"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter(|x| pids.contains(&x["key"]["pid"].as_u64().unwrap_or(0)))
                .map(|x| x["footprint"]["bytes"].as_u64().unwrap_or(0))
                .min()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    (detected, min_fp)
}

struct RunResult {
    pct: f64,
    result: &'static str,
    code: i32,
}

fn measure(o: &Opts, processes: usize, label: &str) -> RunResult {
    let invalid = |why: String| {
        println!("invalid ({label}): {why}");
        RunResult {
            pct: f64::NAN,
            result: "INVALID",
            code: 2,
        }
    };
    let mut s = match Session::spawn(&o.bin, &[], Duration::from_secs(o.warmup)) {
        Ok(s) => s,
        Err(e) => return invalid(e),
    };
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(o.warmup));
    let Some(t0) = fixtures::cpu_time(s.pid) else {
        return invalid(format!("cannot read CPU time of pid {}", s.pid));
    };
    let m0 = Instant::now();
    std::thread::sleep(Duration::from_secs(o.secs));
    if s.exited() {
        return invalid("fallow exited early".into());
    }
    let t1 = fixtures::cpu_time(s.pid).unwrap_or(t0);
    let wall = m0.elapsed();
    let outcome = s.finish();
    let pct = (t1 - t0).as_secs_f64() / wall.as_secs_f64() * 100.0;
    let problems = outcome.problems(Some(Require::Cycles {
        refresh: Duration::from_secs(2),
    }));
    let valid = problems.is_empty() && start.elapsed() >= Duration::from_secs(o.warmup + o.secs);
    println!("evidence ({label}): {}", outcome.evidence());
    for p in &problems {
        println!("invalid ({label}): {p}");
    }
    let pass = valid && pct <= BUDGET_PCT;
    let result = if !valid {
        "INVALID"
    } else if pass {
        "PASS"
    } else {
        "FAIL"
    };
    println!(
        "run={label} processes={processes} sample_s={:.1} cpu_s={:.3} avg_cpu_pct={pct:.3} result={result}",
        wall.as_secs_f64(),
        (t1 - t0).as_secs_f64()
    );
    fixtures::flush();
    RunResult {
        pct,
        result,
        code: if !valid {
            2
        } else if pass {
            0
        } else {
            1
        },
    }
}
