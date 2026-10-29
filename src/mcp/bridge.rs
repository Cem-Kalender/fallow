//! `fallow mcp-bridge`: the stdio MCP server an agent step starts (006 research R-5,
//! contracts/mcp-tools.md).
//!
//! It connects to the Fallow service over the local channel, sends the hello
//! `{"kind": "step", "token": "…"}`, then copies lines between its stdio and the channel
//! unchanged. All protocol logic lives in the service. It reads no configuration and no
//! environment variable.

use std::ffi::OsString;
use std::io::{BufRead, Read, Write};
use std::process::ExitCode;
use std::sync::mpsc;

use super::pipe;

pub const USAGE: &str = "\
Usage: fallow mcp-bridge --fallow-pid <PID> --step <HEX>

Internal: agent steps start this to reach Fallow's step tools. Fallow writes these arguments
itself, into the step's MCP configuration.
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub fallow_pid: u32,
    pub token: String,
}

/// Parses the arguments after `mcp-bridge`.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Args, String> {
    let mut pid = None;
    let mut token = None;
    let mut it = args.into_iter().map(|a| a.to_string_lossy().into_owned());
    while let Some(a) = it.next() {
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| it.next())
                .filter(|v| !v.is_empty())
                .ok_or(format!("{name} needs a value"))
        };
        match name.as_str() {
            "--fallow-pid" => {
                pid = Some(
                    value()?
                        .parse::<u32>()
                        .map_err(|_| "--fallow-pid: not a number")?,
                )
            }
            "--step" => {
                let t = value()?;
                if t.len() != 64
                    || !t
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err("--step: expected 64 lowercase hex characters".into());
                }
                token = Some(t);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(Args {
        fallow_pid: pid.ok_or("--fallow-pid is required")?,
        token: token.ok_or("--step is required")?,
    })
}

enum Ended {
    /// The agent closed our stdin.
    Agent,
    /// Fallow closed the channel; `true` if it ever sent anything.
    Fallow(bool),
}

/// Runs the bridge. Exit codes: 0 when either side closes, 3 when Fallow cannot be reached or
/// refuses the connection.
pub fn run(a: &Args) -> ExitCode {
    let endpoint = match pipe::service_endpoint() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("fallow mcp-bridge: {e}");
            return ExitCode::from(3);
        }
    };
    let (mut from_fallow, mut to_fallow) = match pipe::connect(&endpoint, Some(a.fallow_pid)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("fallow mcp-bridge: {e}");
            return ExitCode::from(3);
        }
    };
    let hello = serde_json::json!({ "kind": "step", "token": a.token }).to_string();
    if pipe::write_line(&mut *to_fallow, &hello).is_err() {
        eprintln!("fallow mcp-bridge: Fallow closed the connection");
        return ExitCode::from(3);
    }
    let (tx, rx) = mpsc::channel();
    let tx2 = tx.clone();
    // Agent → Fallow, one line at a time.
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut lock = stdin.lock();
        let mut line = Vec::new();
        loop {
            line.clear();
            match lock.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if !line.ends_with(b"\n") {
                        line.push(b'\n');
                    }
                    if to_fallow
                        .write_all(&line)
                        .and_then(|()| to_fallow.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = tx.send(Ended::Agent);
    });
    // Fallow → agent.
    std::thread::spawn(move || {
        let stdout = std::io::stdout();
        let mut buf = [0u8; 16 * 1024];
        let mut any = false;
        loop {
            match from_fallow.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    any = true;
                    let mut out = stdout.lock();
                    if out.write_all(&buf[..n]).and_then(|()| out.flush()).is_err() {
                        break;
                    }
                }
            }
        }
        let _ = tx2.send(Ended::Fallow(any));
    });
    match rx.recv() {
        Ok(Ended::Fallow(false)) => {
            eprintln!("fallow mcp-bridge: Fallow refused the connection");
            ExitCode::from(3)
        }
        _ => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Args, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn arguments() {
        let t = "0a".repeat(32);
        let a = p(&["--fallow-pid=42", "--step", &t]).unwrap();
        assert_eq!((a.fallow_pid, a.token.as_str()), (42, t.as_str()));
        assert!(p(&["--fallow-pid", "1"]).is_err());
        assert!(p(&["--fallow-pid", "1", "--step", "XY"]).is_err());
        assert!(p(&["--bogus"]).is_err());
        assert!(p(&["--fallow-pid", "nope", "--step", &t]).is_err());
    }
}
