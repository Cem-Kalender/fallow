//! `cargo xtask bench-memory --idle` (T024) and `--loaded` (T083) (FR-040, SC-005, research
//! R-13).
//!
//! `--idle`: starts Fallow with no project, waits 30 s, samples RSS every second for 60 s and
//! fails if the peak is 50 MB or more.
//!
//! `--loaded`: opens the reference large project (`cargo xtask gen-fixture`), runs one project
//! search, opens five files of different languages through scripted keys (each found with a
//! project search for its unique token), copies once (so arboard's cost is measured), then
//! waits 30 s and samples for 60 s. RSS is also sampled during the script; the peak over the
//! whole run must stay under 150 MB.
//!
//! The run must also be valid (pty.rs): first frame, measured PID is Fallow, clean exit, and the
//! liveness counter chosen with `--require` (ticks until procmon exists, then cycles). A loaded
//! run additionally needs Fallow's own counters to show 5 opened files, 6 searches and 1 copy.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::pty::{self, Require, Session};
use crate::rss;

const SETTLE: Duration = Duration::from_secs(30);
const SAMPLES: u64 = 60;
const IDLE_BUDGET_MB: f64 = 50.0;
const LOADED_BUDGET_MB: f64 = 150.0;

struct Args {
    bin: PathBuf,
    csv: PathBuf,
    require: Require,
    settle: Duration,
    samples: u64,
    /// CT-1 (T054): the run counts only if at least this many LLM groups are detected.
    min_llm_groups: usize,
    /// `--loaded`: the fixture to open.
    loaded: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut idle = false;
    let mut a = Args {
        bin: pty::default_binary(),
        csv: PathBuf::from(format!("bench-memory-idle-{}.csv", std::env::consts::OS)),
        require: Require::Cycles {
            refresh: std::time::Duration::from_millis(2000),
        },
        settle: SETTLE,
        samples: SAMPLES,
        min_llm_groups: 0,
        loaded: None,
    };
    let mut loaded = false;
    let mut fixture = crate::gen_fixture::default_dir();
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
            "--require-llm-groups" => {
                a.min_llm_groups = value("--require-llm-groups")?
                    .parse()
                    .map_err(|_| "--require-llm-groups needs a number")?
            }
            "--samples" => {
                a.samples = value("--samples")?
                    .parse()
                    .map_err(|_| "--samples needs a number")?
            }
            "--loaded" => loaded = true,
            "--fixture" => fixture = value("--fixture")?.into(),
            other => return Err(format!("bench-memory: unknown argument {other:?}")),
        }
    }
    if idle == loaded {
        return Err("bench-memory: say which profile: --idle or --loaded".into());
    }
    if loaded {
        if !fixture.join(".fallow-fixture").is_file() {
            return Err(format!(
                "bench-memory --loaded: no fixture at {} (run cargo xtask gen-fixture)",
                fixture.display()
            ));
        }
        a.loaded = Some(std::path::absolute(&fixture).map_err(|e| e.to_string())?);
        if a.csv.to_string_lossy().contains("-idle-") {
            a.csv = PathBuf::from(format!("bench-memory-loaded-{}.csv", std::env::consts::OS));
        }
    }
    Ok(a)
}

pub fn run(args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "{e}\nusage: cargo xtask bench-memory --idle|--loaded [--fixture <dir>] [--bin <path>] [--csv <path>] [--require ticks|cycles]"
            );
            return 2;
        }
    };
    let (profile, budget) = match a.loaded {
        Some(_) => ("loaded", LOADED_BUDGET_MB),
        None => ("idle", IDLE_BUDGET_MB),
    };
    println!(
        "bench-memory --{profile}: {} (settle {} s, {} samples at 1 s, budget peak < {budget} MB, liveness {:?}{})",
        a.bin.display(),
        a.settle.as_secs(),
        a.samples,
        a.require,
        a.loaded
            .as_ref()
            .map_or(String::new(), |f| format!(", fixture {}", f.display()))
    );
    let fixture_arg = a.loaded.as_ref().map(|f| f.display().to_string());
    let args: Vec<&str> = fixture_arg.iter().map(String::as_str).collect();
    let mut s = match Session::spawn(&a.bin, &args, a.settle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bench-memory: {e}");
            return 2;
        }
    };
    let pid = s.pid;
    let start = Instant::now();
    // (second since start, RSS bytes)
    let mut samples: Vec<(u64, u64)> = Vec::with_capacity(a.samples as usize + 64);
    let mut problems = Vec::new();
    if a.loaded.is_some() {
        if let Err(e) = script(&mut s, start, &mut samples) {
            problems.push(e);
        }
    }
    let base = start.elapsed().as_secs();
    for i in 1..=a.samples {
        let second = base + a.settle.as_secs() + i;
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
    problems.extend(outcome.problems(Some(a.require)));
    if a.loaded.is_some() {
        // Fallow's own counters prove the script did what it says (T083).
        let field = |k| pty::stat_field(&outcome.stats_line, k).unwrap_or(0);
        let (buffers, searches, copies) = (field("buffers"), field("searches"), field("copies"));
        println!("loaded evidence: buffers={buffers} searches={searches} copies={copies}");
        if buffers < OPEN_FILES {
            problems.push(format!("{buffers} files opened, need {OPEN_FILES}"));
        }
        if searches < OPEN_FILES + 1 {
            problems.push(format!(
                "{searches} project searches, need {}",
                OPEN_FILES + 1
            ));
        }
        if copies < 1 {
            problems.push("no copy was made".into());
        }
    }
    if a.min_llm_groups > 0 {
        // What Fallow saw on this machine right after sampling (CT-1 validity and record).
        let (groups, llm, procs) = llm_groups(&a.bin);
        println!("llm_groups={groups} llm_processes={llm} processes={procs}");
        if groups < a.min_llm_groups {
            problems.push(format!(
                "{groups} LLM groups detected, need at least {}",
                a.min_llm_groups
            ));
        }
    }
    let valid = problems.is_empty();
    let pass = valid && mb(peak) < budget;
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
        "os={} profile={profile} samples={} peak_rss_mb={:.2} peak_at_s={peak_at} final_rss_mb={:.2} budget_mb={budget} csv={} result={result}",
        std::env::consts::OS,
        samples.len(),
        mb(peak),
        mb(last),
        a.csv.display()
    );
    let title = if a.loaded.is_some() {
        "Loaded memory"
    } else {
        "Idle memory"
    };
    pty::step_summary(&format!(
        "### {title} ({})\n\n| peak RSS | at | final | budget | liveness | result |\n|---|---|---|---|---|---|\n| {:.2} MB | {peak_at} s | {:.2} MB | < {budget} MB | {} | **{result}** |\n",
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

const OPEN_FILES: u64 = 5;
const CTRL_A: &[u8] = b"\x01";
const CTRL_C: &[u8] = b"\x03";
const CTRL_F: &[u8] = b"\x06";
const ESC: &[u8] = b"\x1b";

fn sample(s: &Session, start: Instant, samples: &mut Vec<(u64, u64)>) {
    if let Some(b) = rss::rss_bytes(s.pid) {
        samples.push((start.elapsed().as_secs(), b));
    }
}

/// Waits `d` while sampling RSS about every second.
fn wait(s: &Session, d: Duration, start: Instant, samples: &mut Vec<(u64, u64)>) {
    let until = Instant::now() + d;
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        std::thread::sleep(Duration::from_secs(1).min(left));
        sample(s, start, samples);
    }
}

/// A project search for `query` from the find bar; waits for the first result row.
fn search(
    s: &mut Session,
    query: &str,
    toggle_scope: bool,
    start: Instant,
    samples: &mut Vec<(u64, u64)>,
) -> Result<(), String> {
    s.send(CTRL_F);
    wait(s, Duration::from_millis(400), start, samples);
    if toggle_scope {
        s.send(b"\t");
    }
    s.send(query.as_bytes());
    wait(s, Duration::from_millis(400), start, samples);
    s.expect(b"Results:");
    s.send(b"\r");
    if s.seen(Duration::from_secs(60)).is_none() {
        return Err(format!("search for {query:?} showed no result within 60 s"));
    }
    Ok(())
}

/// The loaded-profile script (T083).
fn script(s: &mut Session, start: Instant, samples: &mut Vec<(u64, u64)>) -> Result<(), String> {
    if s.wait_frame(Duration::from_secs(10)).is_none() {
        return Err("no first frame".into());
    }
    // Fallow starts on the agent workspace (002); Ctrl-E switches to the editor view of the
    // project (focus in Files), and Tab moves to the editor, where Ctrl-F works. One project
    // search over the whole fixture: no file is open yet, so the find bar starts in "whole
    // project". The search is left to finish, then the bar is closed.
    s.send(b"\x05");
    wait(s, Duration::from_millis(300), start, samples);
    s.send(b"\t");
    wait(s, Duration::from_millis(300), start, samples);
    search(s, crate::gen_fixture::NEEDLE, false, start, samples)?;
    wait(s, Duration::from_secs(30), start, samples);
    // Esc directly followed by another key reads as Alt+key on Unix terminals, so every Esc is
    // followed by a pause.
    s.send(ESC);
    wait(s, Duration::from_millis(500), start, samples);
    s.send(ESC);
    wait(s, Duration::from_millis(500), start, samples);
    // Five files of different languages, each found by its unique token and opened with Enter
    // on its result. From the second one on, a file is open, so Ctrl-F starts in "this file"
    // and Tab switches to "whole project".
    for (i, lang) in crate::gen_fixture::OPEN.iter().enumerate() {
        search(
            s,
            &crate::gen_fixture::open_token(lang),
            i > 0,
            start,
            samples,
        )?;
        wait(s, Duration::from_millis(500), start, samples);
        s.send(b"\r");
        wait(s, Duration::from_secs(2), start, samples);
    }
    // One copy, which goes through the platform clipboard as well as OSC 52.
    s.send(CTRL_A);
    s.send(CTRL_C);
    wait(s, Duration::from_secs(2), start, samples);
    Ok(())
}

/// Groups and LLM processes in `fallow --snapshot-json`, plus the system's process count.
fn llm_groups(bin: &std::path::Path) -> (usize, usize, usize) {
    let out = std::process::Command::new(bin)
        .arg("--snapshot-json")
        .output();
    let v: serde_json::Value = out
        .ok()
        .and_then(|o| serde_json::from_slice(&o.stdout).ok())
        .unwrap_or_default();
    let n = |k: &str| v[k].as_array().map_or(0, Vec::len);
    (n("groups"), n("llm"), crate::fixtures::process_count())
}
