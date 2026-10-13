//! Measures peak RSS of a TUI binary run inside a pseudo-terminal, using the method of the CI
//! benchmark (research.md R-13): 120x40 PTY, RSS read from the OS once per second.
//!
//! RSS source: Linux `/proc/<pid>/status` VmRSS, macOS `proc_pidinfo(PROC_PIDTASKINFO)`
//! `pti_resident_size`, Windows `GetProcessMemoryInfo` `WorkingSetSize`.
//!
//! Usage: idle-measure <binary> [seconds=300] [limit_mb=25] [csv_out]
//! Exits 1 if peak RSS exceeds the limit, 2 on harness errors.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(bin) = args.get(1) else {
        eprintln!("usage: idle-measure <binary> [seconds=300] [limit_mb=25] [csv_out]");
        std::process::exit(2);
    };
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300);
    let limit_mb: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(25.0);
    let csv_path = args.get(4).cloned();

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .unwrap_or_else(|e| fail(&format!("openpty: {e}")));
    // portable-pty starts children in the home directory, so pass an absolute path and cwd.
    let bin = std::path::absolute(bin).unwrap_or_else(|e| fail(&format!("binary {bin}: {e}")));
    if !bin.exists() {
        fail(&format!("binary {} does not exist", bin.display()));
    }
    let mut cmd = CommandBuilder::new(&bin);
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .unwrap_or_else(|e| fail(&format!("spawn: {e}")));
    drop(pair.slave);
    let pid = child.process_id().unwrap_or_else(|| fail("no child pid"));

    // Drain the PTY so the child never blocks on output.
    let mut reader = pair.master.try_clone_reader().unwrap_or_else(|e| fail(&format!("reader: {e}")));
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while matches!(reader.read(&mut buf), Ok(n) if n > 0) {}
    });
    let mut writer = pair.master.take_writer().unwrap_or_else(|e| fail(&format!("writer: {e}")));

    let start = Instant::now();
    let mut samples: Vec<(u64, u64)> = Vec::with_capacity(secs as usize + 1);
    let mut peak: u64 = 0;
    let mut peak_at: u64 = 0;
    for s in 0..=secs {
        let target = start + Duration::from_secs(s);
        if let Some(d) = target.checked_duration_since(Instant::now()) {
            std::thread::sleep(d);
        }
        if let Ok(Some(status)) = child.try_wait() {
            fail(&format!("child exited early at {s}s: {status:?}"));
        }
        let Some(rss) = rss_bytes(pid) else { fail(&format!("cannot read RSS of pid {pid}")) };
        samples.push((s, rss));
        if rss > peak {
            peak = rss;
            peak_at = s;
        }
    }

    let _ = writer.write_all(b"q");
    let _ = writer.flush();
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();

    if let Some(path) = csv_path {
        let mut out = String::from("second,rss_bytes\n");
        for (s, r) in &samples {
            out.push_str(&format!("{s},{r}\n"));
        }
        if let Err(e) = std::fs::write(&path, out) {
            eprintln!("warning: could not write {path}: {e}");
        }
    }

    let mb = |b: u64| b as f64 / 1_048_576.0;
    let after_warmup = samples.iter().filter(|(s, _)| *s >= 30).map(|(_, r)| *r).max().unwrap_or(0);
    let last = samples.last().map(|(_, r)| *r).unwrap_or(0);
    let pass = mb(peak) <= limit_mb;
    let line = format!(
        "os={} duration_s={} samples={} peak_rss_mb={:.2} peak_at_s={} peak_after_30s_mb={:.2} final_rss_mb={:.2} limit_mb={:.0} result={}",
        std::env::consts::OS,
        secs,
        samples.len(),
        mb(peak),
        peak_at,
        mb(after_warmup),
        mb(last),
        limit_mb,
        if pass { "PASS" } else { "FAIL" }
    );
    println!("{line}");
    if let Ok(summary) = std::env::var("GITHUB_STEP_SUMMARY") {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(summary) {
            let _ = writeln!(
                f,
                "### S-3 idle budget: {}\n\n| peak RSS | peak after 30 s | final | limit | result |\n|---|---|---|---|---|\n| {:.2} MB (at {} s) | {:.2} MB | {:.2} MB | {:.0} MB | **{}** |\n",
                std::env::consts::OS,
                mb(peak),
                peak_at,
                mb(after_warmup),
                mb(last),
                limit_mb,
                if pass { "PASS" } else { "FAIL" }
            );
        }
    }
    std::process::exit(if pass { 0 } else { 1 });
}

fn fail(msg: &str) -> ! {
    eprintln!("idle-measure: {msg}");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn rss_bytes(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(target_os = "macos")]
fn rss_bytes(pid: u32) -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let r = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    (r == size).then_some(info.pti_resident_size)
}

#[cfg(windows)]
fn rss_bytes(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = GetProcessMemoryInfo(h, &mut c, c.cb);
        CloseHandle(h);
        (ok != 0).then_some(c.WorkingSetSize as u64)
    }
}
