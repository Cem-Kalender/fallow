//! `cargo xtask unprivileged` (T119, FR-030, Constitution V): run as a non-admin, non-root
//! user, it starts the core detection fixtures as that user, runs `fallow --snapshot-json`
//! and checks that every core field of the user's own processes is present.
//!
//! CI starts this binary as the unprivileged user (`sudo -u nobody` on Linux and macOS, a
//! standard local user on Windows); it refuses to run elevated.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::fixtures;
use crate::pty;

struct Fixtures(Vec<Child>);

impl Drop for Fixtures {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

pub fn run(args: &[String]) -> i32 {
    let mut bin = pty::default_binary();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--bin", Some(v)) => bin = PathBuf::from(v),
            _ => {
                eprintln!("usage: xtask unprivileged [--bin <path>]");
                return 2;
            }
        }
    }
    if fixtures::is_elevated() {
        eprintln!("unprivileged: INVALID: running elevated; start this as a standard user");
        return 2;
    }
    println!(
        "unprivileged: running as a non-elevated user (pid {})",
        std::process::id()
    );
    match check(&bin) {
        Ok(lines) => {
            for l in &lines {
                println!("ok: {l}");
            }
            println!("os={} result=PASS", std::env::consts::OS);
            fixtures::summary(&format!(
                "### Unprivileged run ({})\n\n**PASS**: {} checks as a non-elevated user.\n",
                std::env::consts::OS,
                lines.len()
            ));
            0
        }
        Err(problems) => {
            for p in &problems {
                println!("fail: {p}");
            }
            println!("os={} result=FAIL", std::env::consts::OS);
            fixtures::summary(&format!(
                "### Unprivileged run ({})\n\n**FAIL**\n\n{}\n",
                std::env::consts::OS,
                problems
                    .iter()
                    .map(|p| format!("- {p}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
            1
        }
    }
}

fn spawn(cmd: &mut Command) -> Result<Child, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("fixture: {e}"))
}

fn check(bin: &Path) -> Result<Vec<String>, Vec<String>> {
    let dir = fixtures::temp_dir("unpriv");
    let me = std::env::current_exe().map_err(|e| vec![e.to_string()])?;
    let port_file = dir.join("port");
    let mut fx = Fixtures(Vec::new());
    let one = |r: Result<Child, String>| r.map_err(|e| vec![e]);

    let server_bin = fixtures::copy_self_as(&dir, "ollama");
    fx.0.push(one(spawn(
        Command::new(&server_bin)
            .args(["run", "fixture-model"])
            .env("FALLOW_FIXTURE_PORT_FILE", &port_file),
    ))?);
    let server = fx.0[0].id();
    let port =
        fixtures::read_port(&port_file).ok_or(vec!["server fixture did not listen".to_string()])?;
    fx.0.push(one(spawn(
        Command::new(&me).args(["client-fixture", &port.to_string()]),
    ))?);
    let client = fx.0[1].id();
    let agent_bin = fixtures::copy_self_as(&dir, "claude");
    fx.0.push(one(spawn(&mut Command::new(&agent_bin)))?);
    let agent = fx.0[2].id();
    fx.0.push(one(spawn(Command::new(&me).arg("provider-fixture")))?);
    let provider = fx.0[3].id();
    std::thread::sleep(Duration::from_secs(4));

    let out = Command::new(bin)
        .args(["--snapshot-json", "--no-dns"])
        .output()
        .map_err(|e| vec![format!("cannot run {}: {e}", bin.display())])?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut problems = Vec::new();
    let mut ok = Vec::new();
    if !out.status.success() {
        problems.push(format!(
            "fallow exited with {:?}: {stderr}",
            out.status.code()
        ));
        return Err(problems);
    }
    if stderr.to_lowercase().contains("elevat") {
        problems.push(format!("fallow mentioned elevation: {stderr}"));
    }
    let snap: Value =
        serde_json::from_slice(&out.stdout).map_err(|e| vec![format!("JSON: {e}")])?;
    ok.push("fallow --snapshot-json exited 0 without elevation".into());

    let llm = |pid: u32| {
        snap["llm"]
            .as_array()?
            .iter()
            .find(|l| l["key"]["pid"].as_u64() == Some(pid as u64))
    };
    let record = |pid: u32| {
        snap["records"]
            .as_array()?
            .iter()
            .find(|r| r["key"]["pid"].as_u64() == Some(pid as u64))
    };
    let expect = [
        (
            agent,
            "agent CLI fixture",
            "agent_cli",
            "certain",
            None,
            None,
            None,
        ),
        (
            server,
            "local model server",
            "local_model_server",
            "certain",
            Some("serving"),
            None,
            Some("fixture-model"),
        ),
        (
            client,
            "local model client",
            "",
            "likely",
            Some("llm_connected"),
            None,
            Some("fixture-model"),
        ),
        (
            provider,
            "provider connection",
            "",
            "likely",
            Some("llm_connected"),
            Some("anthropic"),
            None,
        ),
    ];
    for (pid, what, kind, conf, status, provider_id, model) in expect {
        let Some(l) = llm(pid) else {
            problems.push(format!("{what} (pid {pid}) not detected"));
            continue;
        };
        let mut bad = Vec::new();
        if !kind.is_empty() && l["kind"] != kind {
            bad.push(format!("kind {}", l["kind"]));
        }
        if l["confidence"] != conf {
            bad.push(format!("confidence {}", l["confidence"]));
        }
        if let Some(s) = status {
            if l["status"] != s {
                bad.push(format!("status {}", l["status"]));
            }
        }
        if let Some(p) = provider_id {
            if l["provider"] != p {
                bad.push(format!("provider {}", l["provider"]));
            }
        }
        if let Some(m) = model {
            if l["model"] != m {
                bad.push(format!("model {}", l["model"]));
            }
        }
        // Core fields of the user's own process (FR-011): none may be missing because of
        // privileges.
        match record(pid) {
            None => bad.push("no record".into()),
            Some(r) => {
                if r["name"].as_str().is_none_or(str::is_empty) {
                    bad.push("name missing".into());
                }
                if r["key"]["start_time"].as_u64().is_none_or(|t| t == 0) {
                    bad.push("start time (uptime) missing".into());
                }
                if r["footprint"].is_null() {
                    bad.push("footprint missing".into());
                }
                if r["cpu_samples"].as_array().is_none_or(Vec::is_empty) {
                    bad.push("CPU missing".into());
                }
                if r["cwd"]["state"] != "known" {
                    bad.push(format!("cwd {}", r["cwd"]));
                }
            }
        }
        if bad.is_empty() {
            ok.push(format!("{what}: detected, {conf}, all core fields present"));
        } else {
            problems.push(format!("{what} (pid {pid}): {}", bad.join(", ")));
        }
    }
    drop(fx);
    let _ = std::fs::remove_dir_all(&dir);
    if problems.is_empty() {
        Ok(ok)
    } else {
        Err(problems)
    }
}
