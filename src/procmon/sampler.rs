//! Process enumeration: PID, start time, parent, name, owner and CPU for every process.
//!
//! - Linux reads `/proc` directly: one `/proc/<pid>/stat` read per process per refresh and the
//!   owner once per process. sysinfo 0.39 also reads `/proc/<pid>/status` for every process on
//!   every refresh, which put the refresh over its 2% CPU budget at 600 processes (T117).
//! - macOS uses `proc_listallpids` and `proc_pidinfo` (BSD and task info). sysinfo was dropped
//!   here: it reads KERN_PROCARGS2 (argv *and environment*) for every new process to get its
//!   name, into a buffer it frees without wiping. The SC-007 canary caught it (T120).
//! - Windows uses a Toolhelp snapshot plus limited-information handles. sysinfo was dropped
//!   here too: it reads every process's PEB through `PROCESS_VM_READ` handles to find parents,
//!   which R-10 allows only on demand for detected LLM processes.

use compact_str::CompactString;

use super::snapshot::{ProcessKey, Uid};
use super::worker::ProcSample;

/// Takes one sample of all processes plus total and available RAM.
pub trait Sample {
    fn sample(&mut self) -> (Vec<ProcSample>, u64, u64);
}

#[cfg(target_os = "linux")]
pub use linux::Sampler;
#[cfg(not(target_os = "linux"))]
pub use other::Sampler;

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    use crate::procmon::platform::read_proc;
    use std::time::Instant;

    use super::*;

    /// Fields of `/proc/<pid>/stat` Fallow uses.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Stat {
        pub name: CompactString,
        pub ppid: u32,
        /// utime + stime, clock ticks.
        pub cpu_ticks: u64,
        /// Start time, clock ticks since boot.
        pub start_ticks: u64,
        /// The kernel marks kernel threads with PF_KTHREAD; they have no user process.
        pub kernel_thread: bool,
    }

    /// Parses `/proc/<pid>/stat`. The name is in parentheses and may contain spaces or
    /// parentheses, so the fields are counted from the last `)`.
    pub fn parse_stat(text: &str) -> Option<Stat> {
        let open = text.find('(')?;
        let close = text.rfind(')')?;
        let name = CompactString::from(text.get(open + 1..close)?);
        let f: Vec<&str> = text.get(close + 2..)?.split_whitespace().collect();
        // f[0] is field 3 (state).
        let ppid = f.get(1)?.parse().ok()?;
        let flags: u64 = f.get(6)?.parse().ok()?;
        let utime: u64 = f.get(11)?.parse().ok()?;
        let stime: u64 = f.get(12)?.parse().ok()?;
        let start_ticks = f.get(19)?.parse().ok()?;
        Some(Stat {
            name,
            ppid,
            cpu_ticks: utime + stime,
            start_ticks,
            kernel_thread: flags & 0x0020_0000 != 0,
        })
    }

    pub struct Sampler {
        hz: u64,
        boot_time: u64,
        buf: String,
        last: Option<Instant>,
        /// (pid, start ticks) → (cpu ticks at the last sample, owner).
        known: HashMap<(u32, u64), (u64, Option<Uid>)>,
    }

    impl Default for Sampler {
        fn default() -> Self {
            // SAFETY: sysconf has no preconditions.
            let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            let boot_time = fs::read_to_string("/proc/stat")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())
                })
                .unwrap_or(0);
            Sampler {
                hz: if hz > 0 { hz as u64 } else { 100 },
                boot_time,
                buf: String::with_capacity(512),
                last: None,
                known: HashMap::new(),
            }
        }
    }

    impl Sample for Sampler {
        fn sample(&mut self) -> (Vec<ProcSample>, u64, u64) {
            let now = Instant::now();
            let elapsed = self.last.map(|t| now.duration_since(t).as_secs_f64());
            self.last = Some(now);
            let mut out = Vec::with_capacity(self.known.len() + 16);
            let mut seen = HashMap::with_capacity(self.known.len() + 16);
            if let Ok(dir) = fs::read_dir("/proc") {
                for e in dir.flatten() {
                    let name = e.file_name();
                    let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
                        continue;
                    };
                    self.buf.clear();
                    let read = read_proc(&format!("/proc/{pid}/stat"), &mut self.buf);
                    if read.is_err() {
                        continue;
                    }
                    let Some(st) = parse_stat(&self.buf) else {
                        continue;
                    };
                    if st.kernel_thread {
                        continue;
                    }
                    let id = (pid, st.start_ticks);
                    let (prev, owner) = match self.known.remove(&id) {
                        Some((ticks, owner)) => (Some(ticks), owner),
                        None => {
                            let owner = fs::metadata(format!("/proc/{pid}"))
                                .ok()
                                .map(|m| Uid(CompactString::from(m.uid().to_string())));
                            (None, owner)
                        }
                    };
                    let cpu = match (prev, elapsed) {
                        (Some(p), Some(dt)) if dt > 0.0 => {
                            st.cpu_ticks.saturating_sub(p) as f64 / self.hz as f64 / dt * 100.0
                        }
                        _ => 0.0,
                    } as f32;
                    seen.insert(id, (st.cpu_ticks, owner.clone()));
                    out.push(ProcSample {
                        key: ProcessKey {
                            pid,
                            start_time: self.boot_time + st.start_ticks / self.hz,
                        },
                        parent_pid: (st.ppid != 0).then_some(st.ppid),
                        name: st.name,
                        owner,
                        cpu,
                    });
                }
            }
            self.known = seen;
            let (total, avail) = meminfo(&mut self.buf);
            (out, total, avail)
        }
    }

    fn meminfo(buf: &mut String) -> (u64, u64) {
        if read_proc("/proc/meminfo", buf).is_err() {
            return (0, 0);
        }
        let kb = |key: &str| -> u64 {
            buf.lines()
                .find_map(|l| {
                    l.strip_prefix(key)?
                        .trim()
                        .strip_suffix("kB")?
                        .trim()
                        .parse::<u64>()
                        .ok()
                })
                .unwrap_or(0)
                * 1024
        };
        (kb("MemTotal:"), kb("MemAvailable:"))
    }

    #[cfg(test)]
    mod tests {
        use super::parse_stat;

        #[test]
        fn stat_with_odd_names() {
            let line = "4242 (my (odd) name) S 1 4242 4242 0 -1 4194560 100 0 0 0 7 3 0 0 20 0 1 0 98765 1000 50 0";
            let s = parse_stat(line).unwrap();
            assert_eq!(s.name, "my (odd) name");
            assert_eq!(
                (s.ppid, s.cpu_ticks, s.start_ticks, s.kernel_thread),
                (1, 10, 98765, false)
            );
            let kthread = "2 (kthreadd) S 0 0 0 0 -1 2129984 0 0 0 0 0 0 0 0 20 0 1 0 1 0 0";
            assert!(parse_stat(kthread).unwrap().kernel_thread);
        }
    }
}

#[cfg(target_os = "macos")]
mod other {
    use std::collections::HashMap;
    use std::ffi::CStr;
    use std::mem::{size_of, zeroed};
    use std::time::Instant;

    use super::*;

    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }

    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
        fn mach_host_self() -> libc::mach_port_t;
    }

    pub struct Sampler {
        pids: Vec<libc::c_int>,
        /// Mach absolute time units → nanoseconds.
        numer: u64,
        denom: u64,
        page: u64,
        total_ram: u64,
        last: Option<Instant>,
        /// (pid, start) → cpu time (ns) at the last sample.
        known: HashMap<(u32, u64), u64>,
    }

    impl Default for Sampler {
        fn default() -> Self {
            let mut tb = Timebase { numer: 1, denom: 1 };
            // SAFETY: writes the timebase into `tb`.
            unsafe { mach_timebase_info(&mut tb) };
            let mut total: u64 = 0;
            let mut len = size_of::<u64>();
            // SAFETY: hw.memsize is a u64; its size is passed.
            unsafe {
                libc::sysctlbyname(
                    c"hw.memsize".as_ptr(),
                    (&mut total as *mut u64).cast(),
                    &mut len,
                    std::ptr::null_mut(),
                    0,
                )
            };
            // SAFETY: sysconf has no preconditions.
            let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
            Sampler {
                pids: Vec::new(),
                numer: tb.numer.max(1) as u64,
                denom: tb.denom.max(1) as u64,
                page,
                total_ram: total,
                last: None,
                known: HashMap::new(),
            }
        }
    }

    impl Sampler {
        fn list_pids(&mut self) {
            // SAFETY: a null buffer returns the count.
            let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) }.max(0) as usize;
            self.pids.clear();
            self.pids.resize(n + 64, 0);
            let bytes = (self.pids.len() * size_of::<libc::c_int>()) as libc::c_int;
            // SAFETY: `pids` is writable for `bytes`.
            let got = unsafe { libc::proc_listallpids(self.pids.as_mut_ptr().cast(), bytes) };
            self.pids.truncate(got.max(0) as usize);
        }

        fn available(&self) -> u64 {
            let mut stat: libc::vm_statistics64 = unsafe { zeroed() };
            let mut count = libc::HOST_VM_INFO64_COUNT;
            // SAFETY: `stat` is a vm_statistics64 and `count` its size in natural_t units.
            let r = unsafe {
                libc::host_statistics64(
                    mach_host_self(),
                    libc::HOST_VM_INFO64,
                    (&mut stat as *mut libc::vm_statistics64).cast(),
                    &mut count,
                )
            };
            if r != libc::KERN_SUCCESS {
                return 0;
            }
            // XNU's available memory: active + inactive + free (as sysinfo computed it).
            (stat.active_count as u64 + stat.inactive_count as u64 + stat.free_count as u64)
                * self.page
        }
    }

    impl Sample for Sampler {
        fn sample(&mut self) -> (Vec<ProcSample>, u64, u64) {
            let now = Instant::now();
            let elapsed = self.last.map(|t| now.duration_since(t).as_secs_f64());
            self.last = Some(now);
            self.list_pids();
            let mut out = Vec::with_capacity(self.pids.len());
            let mut seen = HashMap::with_capacity(self.pids.len());
            for &pid in &self.pids {
                if pid <= 0 {
                    continue;
                }
                let mut bsd: libc::proc_bsdinfo = unsafe { zeroed() };
                let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
                // SAFETY: `bsd` is a proc_bsdinfo of `size` bytes.
                let r = unsafe {
                    libc::proc_pidinfo(
                        pid,
                        libc::PROC_PIDTBSDINFO,
                        0,
                        (&mut bsd as *mut libc::proc_bsdinfo).cast(),
                        size,
                    )
                };
                if r != size {
                    continue;
                }
                let name_ptr = if bsd.pbi_name[0] != 0 {
                    bsd.pbi_name.as_ptr()
                } else {
                    bsd.pbi_comm.as_ptr()
                };
                // SAFETY: both fields are NUL-terminated within their arrays.
                let name = unsafe { CStr::from_ptr(name_ptr) }.to_string_lossy();
                let start = bsd.pbi_start_tvsec;
                let mut task: libc::proc_taskinfo = unsafe { zeroed() };
                let tsize = size_of::<libc::proc_taskinfo>() as libc::c_int;
                // SAFETY: `task` is a proc_taskinfo of `tsize` bytes. Fails for other users'
                // processes without privileges; their CPU then reads 0.
                let tr = unsafe {
                    libc::proc_pidinfo(
                        pid,
                        libc::PROC_PIDTASKINFO,
                        0,
                        (&mut task as *mut libc::proc_taskinfo).cast(),
                        tsize,
                    )
                };
                let cpu_ns = if tr == tsize {
                    (task.pti_total_user + task.pti_total_system) * self.numer / self.denom
                } else {
                    0
                };
                let id = (pid as u32, start);
                let cpu = match (self.known.get(&id), elapsed) {
                    (Some(&p), Some(dt)) if dt > 0.0 => {
                        cpu_ns.saturating_sub(p) as f64 / 1e9 / dt * 100.0
                    }
                    _ => 0.0,
                } as f32;
                seen.insert(id, cpu_ns);
                out.push(ProcSample {
                    key: ProcessKey {
                        pid: pid as u32,
                        start_time: start,
                    },
                    parent_pid: (bsd.pbi_ppid != 0).then_some(bsd.pbi_ppid),
                    name: CompactString::from(name.as_ref()),
                    owner: Some(Uid(CompactString::from(bsd.pbi_uid.to_string()))),
                    cpu,
                });
            }
            self.known = seen;
            (out, self.total_ram, self.available())
        }
    }
}

#[cfg(windows)]
mod other {
    use std::collections::HashMap;
    use std::mem::size_of;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{
        CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use super::*;

    const STILL_ACTIVE: u32 = 259;
    /// Seconds between 1601-01-01 (FILETIME) and 1970-01-01.
    const EPOCH_DIFF: u64 = 11_644_473_600;

    /// An open limited-information handle, kept across refreshes (a fresh OpenProcess per
    /// process per refresh would cost more).
    struct Tracked {
        handle: HANDLE,
        start: u64,
        owner: Option<Uid>,
        cpu_100ns: u64,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            // SAFETY: opened by OpenProcess, closed once.
            unsafe { CloseHandle(self.handle) };
        }
    }

    #[derive(Default)]
    pub struct Sampler {
        tracked: HashMap<u32, Tracked>,
        last: Option<Instant>,
    }

    fn ft(f: FILETIME) -> u64 {
        ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
    }

    /// Creation time (seconds since the Unix epoch) and CPU time (100 ns units).
    fn times(h: HANDLE) -> Option<(u64, u64)> {
        let z = || FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut c, mut e, mut k, mut u) = (z(), z(), z(), z());
        // SAFETY: plain call on a valid handle.
        if unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) } == 0 {
            return None;
        }
        Some((
            (ft(c) / 10_000_000).saturating_sub(EPOCH_DIFF),
            ft(k) + ft(u),
        ))
    }

    fn alive(h: HANDLE) -> bool {
        let mut code = 0u32;
        // SAFETY: plain call on a valid handle.
        unsafe { GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE }
    }

    /// The owner's SID as a string (S-1-5-…), read once per process.
    fn owner(h: HANDLE) -> Option<Uid> {
        // SAFETY: our token query on a valid process handle; buffers sized by the first call;
        // the string is freed with LocalFree.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(h, TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(token);
            if ok == 0 {
                return None;
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 || s.is_null() {
                return None;
            }
            let n = (0..).take_while(|&i| *s.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
            LocalFree(s.cast());
            Some(Uid(CompactString::from(text)))
        }
    }

    fn memory() -> (u64, u64) {
        let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `m` is a MEMORYSTATUSEX with its length set.
        if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 {
            return (0, 0);
        }
        (m.ullTotalPhys, m.ullAvailPhys)
    }

    impl Sample for Sampler {
        fn sample(&mut self) -> (Vec<ProcSample>, u64, u64) {
            let now = Instant::now();
            let elapsed = self.last.map(|t| now.duration_since(t).as_secs_f64());
            self.last = Some(now);
            let mut out = Vec::with_capacity(self.tracked.len() + 16);
            let mut next: HashMap<u32, Tracked> = HashMap::with_capacity(self.tracked.len() + 16);
            // SAFETY: Toolhelp snapshot, iterated with a correctly sized entry, then closed.
            unsafe {
                let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
                if snap == INVALID_HANDLE_VALUE {
                    let (t, a) = memory();
                    return (out, t, a);
                }
                let mut e: PROCESSENTRY32W = std::mem::zeroed();
                e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
                let mut more = Process32FirstW(snap, &mut e) != 0;
                while more {
                    let pid = e.th32ProcessID;
                    let ppid = e.th32ParentProcessID;
                    let n = e
                        .szExeFile
                        .iter()
                        .position(|c| *c == 0)
                        .unwrap_or(e.szExeFile.len());
                    let name = String::from_utf16_lossy(&e.szExeFile[..n]);
                    more = Process32NextW(snap, &mut e) != 0;
                    if pid == 0 {
                        continue;
                    }
                    // Reuse the handle if it still refers to a live process.
                    let mut t = match self.tracked.remove(&pid) {
                        Some(t) if alive(t.handle) => t,
                        _ => {
                            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                            if h.is_null() {
                                continue; // protected or another session: no start time, dropped
                            }
                            let Some((start, cpu)) = times(h) else {
                                CloseHandle(h);
                                continue;
                            };
                            Tracked {
                                handle: h,
                                start,
                                owner: owner(h),
                                cpu_100ns: cpu,
                            }
                        }
                    };
                    let (_, cpu_now) = times(t.handle).unwrap_or((t.start, t.cpu_100ns));
                    let cpu = match elapsed {
                        Some(dt) if dt > 0.0 => {
                            cpu_now.saturating_sub(t.cpu_100ns) as f64 / 1e7 / dt * 100.0
                        }
                        _ => 0.0,
                    } as f32;
                    t.cpu_100ns = cpu_now;
                    out.push(ProcSample {
                        key: ProcessKey {
                            pid,
                            start_time: t.start,
                        },
                        parent_pid: (ppid != 0).then_some(ppid),
                        name: CompactString::from(name),
                        owner: t.owner.clone(),
                        cpu,
                    });
                    next.insert(pid, t);
                }
                CloseHandle(snap);
            }
            // Handles of exited processes close when the old map drops.
            self.tracked = next;
            let (total, avail) = memory();
            (out, total, avail)
        }
    }
}
