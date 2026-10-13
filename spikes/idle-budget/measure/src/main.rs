//! Measures peak RSS of a TUI binary run inside a pseudo-terminal, using the method of the CI
//! benchmark (research.md R-13): 120x40 PTY, RSS read from the OS once per second.
//!
//! RSS source: Linux `/proc/<pid>/status` VmRSS, macOS `proc_pidinfo(PROC_PIDTASKINFO)`
//! `pti_resident_size`, Windows `GetProcessMemoryInfo` `WorkingSetSize`.
//!
//! Usage: idle-measure <binary> [seconds=300] [limit_mb=25] [csv_out]
//! Exits 1 if peak RSS exceeds the limit, 2 on harness errors or an invalid run (no first frame,
//! or no output after warm-up).

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// First-frame marker. contracts/cli.md specifies "Ctrl-P Commands" in the hint bar; only the
/// word is matched because ConPTY may re-encode spaces as cursor movement.
const FRAME_MARKER: &[u8] = b"Commands";

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

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
    let cwd = std::env::current_dir().unwrap_or_else(|e| fail(&format!("cwd: {e}")));
    let stats_path = cwd.join("spike-stats.txt");
    let _ = std::fs::remove_file(&stats_path);
    let mut cmd = CommandBuilder::new(&bin);
    cmd.arg("--stats");
    cmd.arg(&stats_path);
    cmd.cwd(&cwd);
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .unwrap_or_else(|e| fail(&format!("spawn: {e}")));
    drop(pair.slave);
    let pid = child.process_id().unwrap_or_else(|| fail("no child pid"));
    let measured_exe = exe_path(pid).unwrap_or_else(|| "<unreadable>".into());
    println!("harness_pid={} measured_pid={} measured_exe={}", std::process::id(), pid, measured_exe);

    let writer = pair.master.take_writer().unwrap_or_else(|e| fail(&format!("writer: {e}")));
    let writer = Arc::new(Mutex::new(writer));

    // Drain the PTY so the child never blocks on output, answer cursor-position queries
    // (ConPTY on Windows can hold the child until `ESC[6n` is answered), and record that the
    // first frame was drawn plus how much output arrived later, as evidence the spike is live.
    let frame_seen = Arc::new(AtomicBool::new(false));
    let late_bytes = Arc::new(AtomicU64::new(0));
    let mut reader = pair.master.try_clone_reader().unwrap_or_else(|e| fail(&format!("reader: {e}")));
    {
        let (writer, frame_seen, late_bytes) = (writer.clone(), frame_seen.clone(), late_bytes.clone());
        let begin = Instant::now();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut tail: Vec<u8> = Vec::with_capacity(8192 + 64);
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if begin.elapsed() >= Duration::from_secs(30) {
                    late_bytes.fetch_add(n as u64, Ordering::Relaxed);
                }
                tail.extend_from_slice(&buf[..n]);
                if contains(&tail, FRAME_MARKER) {
                    frame_seen.store(true, Ordering::Relaxed);
                }
                if contains(&tail, b"\x1b[6n") {
                    if let Ok(mut w) = writer.lock() {
                        let _ = w.write_all(b"\x1b[1;1R");
                        let _ = w.flush();
                    }
                    tail.clear();
                }
                let keep = tail.len().saturating_sub(32);
                tail.drain(..keep);
            }
        });
    }

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

    let elapsed = start.elapsed().as_secs_f64();

    // Ask the spike to quit so it writes its stats file, then give it up to 5 s.
    if let Ok(mut w) = writer.lock() {
        let _ = w.write_all(b"q");
        let _ = w.flush();
    }
    let quit_by = Instant::now() + Duration::from_secs(5);
    let mut clean_exit = false;
    while Instant::now() < quit_by {
        if let Ok(Some(_)) = child.try_wait() {
            clean_exit = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !clean_exit {
        let _ = child.kill();
    }
    let frame = frame_seen.load(Ordering::Relaxed);
    let late = late_bytes.load(Ordering::Relaxed);

    // Evidence: the stats file is written by the spike itself and carries its own PID.
    let stats = std::fs::read_to_string(&stats_path).unwrap_or_default();
    let field = |k: &str| -> Option<u64> {
        stats.split_whitespace().find_map(|kv| kv.strip_prefix(k)?.strip_prefix('=')?.parse().ok())
    };
    let spike_pid = field("pid");
    let cycles = field("cycles").unwrap_or(0);
    let min_cycles = secs / 2 * 9 / 10; // 2 s refresh; allow 10% for scan time
    let exe_ok = std::path::Path::new(&measured_exe)
        .file_stem()
        .is_some_and(|s| s.to_string_lossy() == "idle-spike");
    let pid_ok = spike_pid == Some(pid as u64);
    let duration_ok = elapsed >= secs as f64;
    println!(
        "evidence: elapsed_s={:.1} clean_exit={} exe_is_spike={} stats_pid_matches_measured={} cycles={} min_cycles={}",
        elapsed, clean_exit, exe_ok, pid_ok, cycles, min_cycles
    );
    println!("spike_stats: {}", stats.trim());

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
    // A run only counts if the spike drew its first frame and kept producing output after
    // warm-up (redraws on every 2 s snapshot); otherwise the process may have been stalled.
    // It also needs proof that the measured PID is the spike and that procmon kept cycling.
    let valid = frame && late > 0 && exe_ok && pid_ok && duration_ok && cycles >= min_cycles;
    let pass = valid && mb(peak) <= limit_mb;
    let result = if !valid { "INVALID" } else if pass { "PASS" } else { "FAIL" };
    let line = format!(
        "os={} duration_s={} samples={} peak_rss_mb={:.2} peak_at_s={} peak_after_30s_mb={:.2} final_rss_mb={:.2} limit_mb={:.0} first_frame={} output_after_30s_bytes={} result={}",
        std::env::consts::OS,
        secs,
        samples.len(),
        mb(peak),
        peak_at,
        mb(after_warmup),
        mb(last),
        limit_mb,
        frame,
        late,
        result
    );
    println!("{line}");
    if let Ok(summary) = std::env::var("GITHUB_STEP_SUMMARY") {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(summary) {
            let _ = writeln!(
                f,
                "### S-3 idle budget: {}\n\n| peak RSS | peak after 30 s | final | limit | first frame | output after 30 s | result |\n|---|---|---|---|---|---|---|\n| {:.2} MB (at {} s) | {:.2} MB | {:.2} MB | {:.0} MB | {} | {} bytes | **{}** |\n",
                std::env::consts::OS,
                mb(peak),
                peak_at,
                mb(after_warmup),
                mb(last),
                limit_mb,
                frame,
                late,
                result
            );
        }
    }
    std::process::exit(if !valid { 2 } else if pass { 0 } else { 1 });
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

/// Executable path of the measured process, to prove it is the spike and not the harness.
#[cfg(target_os = "linux")]
fn exe_path(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| p.display().to_string())
}

#[cfg(target_os = "macos")]
fn exe_path(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { libc::proc_pidpath(pid as libc::c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

#[cfg(windows)]
fn exe_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}
