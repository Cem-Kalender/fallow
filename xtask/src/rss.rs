//! Readers for another process's RSS and executable path (research R-13: the constitution's
//! budgets are in RSS). Ported from the S-3 spike harness.
//!
//! RSS source: Linux `/proc/<pid>/status` VmRSS, macOS `proc_pidinfo(PROC_PIDTASKINFO)`
//! `pti_resident_size`, Windows `GetProcessMemoryInfo` `WorkingSetSize`.

#[cfg(target_os = "linux")]
pub fn rss_bytes(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(target_os = "macos")]
pub fn rss_bytes(pid: u32) -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a properly sized, writable proc_taskinfo.
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
pub fn rss_bytes(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: plain Win32 calls with a zeroed, correctly sized counters struct; the handle is
    // closed on every path.
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

/// Executable path of a process, to prove the measured PID is Fallow and not the harness
/// (on Unix the child is a fork of the harness until `exec`).
#[cfg(target_os = "linux")]
pub fn exe_path(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.display().to_string())
}

#[cfg(target_os = "macos")]
pub fn exe_path(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; 4096];
    // SAFETY: `buf` is writable for `buf.len()` bytes.
    let n = unsafe {
        libc::proc_pidpath(
            pid as libc::c_int,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
        )
    };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

#[cfg(windows)]
pub fn exe_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    // SAFETY: `buf` is writable for `len` UTF-16 units; the handle is closed on every path.
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
