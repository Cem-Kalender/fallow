//! 016 (research M-16, M-17, T039, T041): Fallow's processes on macOS: its own, and WebKit's
//! (`com.apple.WebKit.*`, XPC services whose parent is launchd) found by their *responsible*
//! process, which is Fallow when it was started as an app; with their physical footprint (Activity
//! Monitor's "Memory").

use std::collections::HashSet;

unsafe extern "C" {
    fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
}

/// The process macOS holds responsible for `pid` (an app for its XPC services).
pub fn responsible(pid: i32) -> i32 {
    // SAFETY: a plain query by pid.
    unsafe { responsibility_get_pid_responsible_for_pid(pid) }
}

/// Every process id on the machine.
pub fn all_pids() -> Vec<i32> {
    let mut buf = vec![0i32; 16384];
    // SAFETY: the buffer holds as many pids as its byte size says.
    let n = unsafe { libc::proc_listallpids(buf.as_mut_ptr().cast(), (buf.len() * 4) as i32) };
    buf.truncate(n.max(0) as usize);
    buf.retain(|p| *p > 0);
    buf
}

/// A process's name.
pub fn name(pid: i32) -> String {
    let mut b = [0u8; 256];
    // SAFETY: the buffer and its length.
    let n = unsafe { libc::proc_name(pid, b.as_mut_ptr().cast(), b.len() as u32) };
    String::from_utf8_lossy(&b[..n.max(0) as usize]).to_string()
}

/// A process's physical footprint in MB (0 when it is gone).
pub fn footprint_mb(pid: i32) -> f64 {
    // SAFETY: `rusage_info_v4` is what flavor 4 writes.
    unsafe {
        let mut r: libc::rusage_info_v4 = std::mem::zeroed();
        if libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, (&raw mut r).cast()) != 0 {
            return 0.0;
        }
        r.ri_phys_footprint as f64 / 1_048_576.0
    }
}

/// The WebKit processes serving `fallow`'s window: those `fallow` is responsible for (started as
/// an app). Started from a terminal, the terminal is responsible for both, so then the WebKit
/// processes of that responsible process that were not there `before` count.
pub fn webkit_of(fallow: i32, before: &HashSet<i32>) -> Vec<(i32, String)> {
    let r = responsible(fallow);
    all_pids()
        .into_iter()
        .filter(|p| *p != fallow)
        .filter(|p| {
            let rp = responsible(*p);
            rp == fallow || (r != fallow && rp == r && !before.contains(p))
        })
        .map(|p| (p, name(p)))
        .filter(|(_, n)| n.starts_with("com.apple.WebKit"))
        .collect()
}

/// The WebKit processes there now (to tell new ones apart).
pub fn webkit_now() -> HashSet<i32> {
    all_pids()
        .into_iter()
        .filter(|p| name(*p).starts_with("com.apple.WebKit"))
        .collect()
}

/// Fallow's memory: its own footprint and its WebKit processes', summed.
pub fn total_mb(fallow: i32, before: &HashSet<i32>) -> f64 {
    footprint_mb(fallow)
        + webkit_of(fallow, before)
            .iter()
            .map(|(p, _)| footprint_mb(*p))
            .sum::<f64>()
}
