//! `cargo xtask record-transcript [--secs N] [--out FILE] -- <program> [args…]` (002 T016).
//!
//! Runs an agent in a 10 × 40 pseudo-terminal for N seconds (default 15), answering the
//! cursor-position query ConPTY sends, and writes everything it printed to FILE (default
//! `target/transcripts/<program>.bin`). Typing is not forwarded: this captures the first screen.
//! The file is NOT added to the repository automatically. Review and scrub it (account names,
//! paths) before copying it to `tests/fixtures/transcripts/` with a `.golden` screen.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

pub fn run(args: &[String]) -> i32 {
    let mut secs = 15u64;
    let mut out: Option<PathBuf> = None;
    let mut it = args.iter();
    let mut cmd: Vec<String> = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--secs" => secs = it.next().and_then(|s| s.parse().ok()).unwrap_or(secs),
            "--out" => out = it.next().map(PathBuf::from),
            "--" => {
                cmd = it.by_ref().cloned().collect();
                break;
            }
            other => {
                eprintln!("record-transcript: unknown argument {other:?}");
                return 2;
            }
        }
    }
    let Some(program) = cmd.first() else {
        eprintln!(
            "usage: cargo xtask record-transcript [--secs N] [--out FILE] -- <program> [args…]"
        );
        return 2;
    };
    let out = out.unwrap_or_else(|| {
        let name = std::path::Path::new(program)
            .file_stem()
            .map_or_else(|| "agent".into(), |s| s.to_string_lossy().into_owned());
        PathBuf::from("target")
            .join("transcripts")
            .join(format!("{name}.bin"))
    });
    let pair = match native_pty_system().openpty(PtySize {
        rows: 10,
        cols: 40,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("record-transcript: {e}");
            return 2;
        }
    };
    let mut b = CommandBuilder::new(program);
    b.args(&cmd[1..]);
    b.env("TERM", "xterm-256color");
    let mut child = match pair.slave.spawn_command(b) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("record-transcript: cannot start {program}: {e}");
            return 2;
        }
    };
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("reader");
    let mut writer = pair.master.take_writer().expect("writer");
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut captured = Vec::new();
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                let _ = writer.write_all(b"\x1b[1;1R");
            }
            captured.extend_from_slice(&chunk);
        }
    }
    let _ = child.kill();
    if let Some(dir) = out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(&out, &captured) {
        Ok(()) => {
            println!(
                "record-transcript: {} bytes in {} (review and scrub before committing)",
                captured.len(),
                out.display()
            );
            0
        }
        Err(e) => {
            eprintln!("record-transcript: {}: {e}", out.display());
            2
        }
    }
}
