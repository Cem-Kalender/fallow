//! `cargo xtask canary` (T120, SC-007, FR-024, Constitution VII), macOS.
//!
//! A monitored child carries a random 32-byte canary in an environment variable and a command
//! line matching a signature, so Fallow reads its KERN_PROCARGS2 buffer (which includes the
//! environment). Fallow's memory is then dumped with lldb while `--snapshot-hold` keeps all its
//! state alive, and the core, Fallow's output, and its config and cache directories are
//! searched for the canary in UTF-8 and UTF-16LE. Any hit fails.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::fixtures;
use crate::pty;

pub fn run(args: &[String]) -> i32 {
    let mut bin = pty::default_binary();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--bin", Some(v)) => bin = PathBuf::from(v),
            _ => {
                eprintln!("usage: cargo xtask canary [--bin <path>]");
                return 2;
            }
        }
    }
    if !cfg!(target_os = "macos") {
        eprintln!(
            "canary: only macOS reads the environment along with the command line (R-2); not run here"
        );
        return 2;
    }
    let bin = std::path::absolute(&bin).unwrap_or(bin);
    let canary = fixtures::hex(&fixtures::random_bytes(32));
    let dir = fixtures::temp_dir("canary");
    let agent_bin = fixtures::copy_self_as(&dir, "claude");
    let mut agent = match Command::new(&agent_bin)
        .env("FALLOW_CANARY_SECRET", &canary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("canary: fixture: {e}");
            return 2;
        }
    };
    std::thread::sleep(Duration::from_secs(1));
    let verdict = inspect(&bin, &dir, &agent_bin, agent.id(), &canary);
    let _ = agent.kill();
    let _ = agent.wait();
    let _ = Command::new("sudo").args(["rm", "-rf"]).arg(&dir).status();
    verdict
}

fn inspect(bin: &Path, dir: &Path, agent_bin: &Path, agent_pid: u32, canary: &str) -> i32 {
    let mut fallow = match Command::new(bin)
        .args(["--snapshot-hold", "40", "--no-dns"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("canary: cannot start fallow: {e}");
            return 2;
        }
    };
    let pid = fallow.id();
    let mut stdout = BufReader::new(fallow.stdout.take().unwrap());
    let mut json = String::new();
    let _ = stdout.read_line(&mut json);
    let detected = json.contains(&format!("{{\"pid\":{agent_pid},"));
    let agent_path = agent_bin.to_string_lossy().into_owned();

    let core = dir.join("fallow.core");
    let mut dumped = false;
    // A modified-memory core sometimes comes out partial; retry, then fall back to a full one.
    // A dump counts only if it holds the fixture's argv, i.e. Fallow's heap.
    for style in [
        Some("modified-memory"),
        Some("modified-memory"),
        Some("modified-memory"),
        Some("full"),
    ] {
        let _ = Command::new("sudo").args(["rm", "-f"]).arg(&core).status();
        let mut cmd = format!("process save-core {}", core.display());
        if let Some(s) = style {
            cmd = format!("process save-core -s {s} {}", core.display());
        }
        let st = Command::new("sudo")
            .args([
                "lldb",
                "-p",
                &pid.to_string(),
                "--batch",
                "-o",
                &cmd,
                "-o",
                "detach",
            ])
            .stdout(Stdio::null())
            .status();
        let _ = Command::new("sudo")
            .args(["chmod", "a+r"])
            .arg(&core)
            .status();
        if st.is_ok_and(|s| s.success())
            && core.metadata().is_ok_and(|m| m.len() > 0)
            && fixtures::file_contains(&core, agent_path.as_bytes()).unwrap_or(false)
        {
            println!(
                "canary: core dump with {} style: {} bytes",
                style.unwrap_or("default"),
                core.metadata().unwrap().len()
            );
            dumped = true;
            break;
        }
    }
    let _ = fallow.kill();
    let mut rest = String::new();
    let _ = stdout.read_to_string(&mut rest);
    let mut stderr = String::new();
    if let Some(mut e) = fallow.stderr.take() {
        let _ = e.read_to_string(&mut stderr);
    }
    let _ = fallow.wait();

    // Validity: the fixture was detected (so its KERN_PROCARGS2 was read), and the dump holds
    // Fallow's heap (it contains the fixture's argv, which Fallow keeps).
    let mut invalid = Vec::new();
    if !detected {
        invalid.push(format!("fixture pid {agent_pid} not detected"));
    }
    if !dumped {
        invalid.push("no core dump".into());
    } else if !fixtures::file_contains(&core, agent_path.as_bytes()).unwrap_or(false) {
        invalid.push("core does not contain the fixture's argv (heap not captured)".into());
    }

    let utf8 = canary.as_bytes().to_vec();
    let utf16 = fixtures::utf16le(canary);
    let mut hits = Vec::new();
    for (what, needle) in [("UTF-8", &utf8), ("UTF-16LE", &utf16)] {
        if dumped && fixtures::file_contains(&core, needle).unwrap_or(false) {
            hits.push(format!("core dump ({what})"));
        }
        let output = format!("{json}{rest}{stderr}");
        if output
            .as_bytes()
            .windows(needle.len())
            .any(|w| w == needle.as_slice())
        {
            hits.push(format!("stdout/stderr ({what})"));
        }
        let home = std::env::var("HOME").unwrap_or_default();
        for d in [
            "Library/Application Support/fallow",
            "Library/Caches/fallow",
            "Library/Logs/fallow",
        ] {
            for f in files(&Path::new(&home).join(d)) {
                if fixtures::file_contains(&f, needle).unwrap_or(false) {
                    hits.push(format!("{} ({what})", f.display()));
                }
            }
        }
    }
    let result = if !invalid.is_empty() {
        "INVALID"
    } else if hits.is_empty() {
        "PASS"
    } else {
        "FAIL"
    };
    for i in &invalid {
        println!("invalid: {i}");
    }
    for h in &hits {
        println!("canary found in: {h}");
    }
    println!(
        "os={} fixture_detected={detected} core_dumped={dumped} hits={} result={result}",
        std::env::consts::OS,
        hits.len()
    );
    fixtures::summary(&format!(
        "### Canary (SC-007, {})\n\n| fixture detected | core dumped | canary hits | result |\n|---|---|---|---|\n| {detected} | {dumped} | {} | **{result}** |\n",
        std::env::consts::OS,
        hits.len()
    ));
    if !invalid.is_empty() {
        2
    } else if hits.is_empty() {
        0
    } else {
        1
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(files(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}
