//! Running git or the hosting service's tool for publishing (010 research R-11): with a time
//! limit, and never waiting for a prompt or a sign-in window. Only the commands Fallow runs are
//! affected; the engineer's own terminal is not.

use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Reads: protection, a pull request's state.
pub const READ: Duration = Duration::from_secs(20);
/// Writes: a push, opening a pull request.
pub const WRITE: Duration = Duration::from_secs(60);

/// Runs `program args` in `dir` within `limit`; stdout on success, otherwise the last lines of
/// what it said (stderr, else stdout), or that it took too long.
pub fn run(program: &str, args: &[String], dir: &Path, limit: Duration) -> Result<String, String> {
    run_full(program, args, dir, limit).map(|(out, _)| out)
}

/// As `run`, with stderr too on success (some tools report there).
pub fn run_full(
    program: &str,
    args: &[String],
    dir: &Path,
    limit: Duration,
) -> Result<(String, String), String> {
    let mut cmd = crate::platform::command(program);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GCM_INTERACTIVE", "never");
    // SSH without a terminal must not ask for a passphrase; the engineer's own SSH program
    // (GIT_SSH_COMMAND, GIT_SSH such as plink, core.sshCommand, ssh.variant) is used as it is.
    let reaches_remote = args
        .first()
        .is_some_and(|a| matches!(a.as_str(), "push" | "fetch" | "ls-remote" | "pull"));
    if is_git(program)
        && reaches_remote
        && std::env::var_os("GIT_SSH_COMMAND").is_none()
        && std::env::var_os("GIT_SSH").is_none()
        && !config_set(dir, "core.sshCommand")
        && !config_set(dir, "ssh.variant")
    {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let name = Path::new(program)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string());
    // Its own process group: a time-out ends what it started too (git-remote-https, ssh, a
    // credential helper), so nothing finishes a push after it was reported stopped.
    let group = crate::platform::job::Group::new().map_err(|e| format!("{name}: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {name}: {e}"))?;
    let _ = group.adopt(child.id());
    // Both pipes are read on threads, so a chatty program never blocks on a full pipe; a
    // grandchild holding them open cannot hold Fallow past the limit.
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    for (is_out, pipe) in [
        (true, child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>)),
        (false, child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)),
    ] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut b);
            }
            let _ = tx.send((is_out, String::from_utf8_lossy(&b).into_owned()));
        });
    }
    drop(tx);
    let started = Instant::now();
    let too_long = || {
        format!(
            "{name} took longer than {} s and was stopped",
            limit.as_secs()
        )
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= limit => {
                group.kill();
                let _ = child.kill();
                let _ = child.wait();
                return Err(too_long());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                group.kill();
                return Err(format!("{name}: {e}"));
            }
        }
    };
    let (mut out, mut err) = (String::new(), String::new());
    let mut got = 0;
    for _ in 0..2 {
        let left = limit.saturating_sub(started.elapsed()).max(Duration::from_secs(2));
        match rx.recv_timeout(left) {
            Ok((true, s)) => out = s,
            Ok((false, s)) => err = s,
            Err(_) => break,
        }
        got += 1;
    }
    if status.success() && got < 2 {
        // Something it started still holds its output: what it said is not complete.
        return Err(format!("{name} finished, but its output could not be read in time"));
    }
    if status.success() {
        return Ok((out, err));
    }
    let said = if err.trim().is_empty() { &out } else { &err };
    let tail: Vec<&str> = said.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(3)..].join(" ");
    Err(if tail.is_empty() {
        format!("{name} failed ({status})")
    } else {
        format!("{name}: {tail}")
    })
}

fn is_git(program: &str) -> bool {
    Path::new(program)
        .file_stem()
        .is_some_and(|s| s.eq_ignore_ascii_case("git"))
}

/// Whether git's configuration (the repository's or the engineer's) sets `key`.
fn config_set(dir: &Path, key: &str) -> bool {
    crate::platform::command("git")
        .args(["config", "--get", key])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_command_that_takes_too_long_is_stopped() {
        let dir = std::env::temp_dir();
        let t = Instant::now();
        let r = if cfg!(windows) {
            run("ping", &s(&["-n", "30", "127.0.0.1"]), &dir, Duration::from_millis(500))
        } else {
            run("sleep", &s(&["30"]), &dir, Duration::from_millis(500))
        };
        assert!(r.unwrap_err().contains("took longer than"));
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn output_and_failures_are_reported() {
        let dir = std::env::temp_dir();
        let ok = run("git", &s(&["--version"]), &dir, READ).unwrap();
        assert!(ok.starts_with("git version"), "{ok}");
        let e = run("git", &s(&["no-such-command"]), &dir, READ).unwrap_err();
        assert!(e.starts_with("git: "), "{e}");
        let e = run("no-such-program-fallow", &[], &dir, READ).unwrap_err();
        assert!(e.contains("cannot start"), "{e}");
    }
}
