//! Fallow-owned reads (research.md R-1, R-2, R-3, R-4). The process environment is never
//! requested on any platform.

#[cfg(target_os = "linux")]
mod imp {
    use crate::Conn;
    use std::collections::HashMap;
    use std::fs;

    pub struct Platform {
        net: String,
    }

    impl Platform {
        pub fn new() -> Self {
            Self { net: String::with_capacity(64 * 1024) }
        }

        pub fn cmdline(&mut self, pid: u32) -> Option<Box<[Box<str>]>> {
            let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            Some(
                raw.split(|b| *b == 0)
                    .filter(|s| !s.is_empty())
                    .map(|s| String::from_utf8_lossy(s).into_owned().into_boxed_str())
                    .collect(),
            )
        }

        pub fn footprint(&mut self, pid: u32) -> Option<u64> {
            if let Ok(s) = fs::read_to_string(format!("/proc/{pid}/smaps_rollup")) {
                for line in s.lines() {
                    if let Some(rest) = line.strip_prefix("Pss:") {
                        let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
                        return Some(kb * 1024);
                    }
                }
            }
            let statm = fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
            let resident: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
            let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
            Some(resident * page)
        }

        pub fn tcp(&mut self, pids: &[u32]) -> Vec<Conn> {
            // inode -> (local port, remote port, established, listening)
            let mut wanted: HashMap<u64, (u16, u16, bool, bool)> = HashMap::new();
            for file in ["/proc/net/tcp", "/proc/net/tcp6"] {
                self.net.clear();
                let Ok(s) = fs::read_to_string(file) else { continue };
                self.net.push_str(&s);
                for line in self.net.lines().skip(1) {
                    let f: Vec<&str> = line.split_whitespace().collect();
                    if f.len() < 10 {
                        continue;
                    }
                    let port = |a: &str| a.rsplit(':').next().and_then(|p| u16::from_str_radix(p, 16).ok());
                    let (Some(lp), Some(rp)) = (port(f[1]), port(f[2])) else { continue };
                    let est = f[3] == "01";
                    let listen = f[3] == "0A";
                    let interesting = (est && rp == 443)
                        || (listen && crate::LOCAL_MODEL_PORTS.contains(&lp))
                        || (est && crate::LOCAL_MODEL_PORTS.contains(&lp));
                    if interesting {
                        if let Ok(inode) = f[9].parse::<u64>() {
                            wanted.insert(inode, (lp, rp, est, listen));
                        }
                    }
                }
            }
            let mut out = Vec::new();
            if wanted.is_empty() {
                return out;
            }
            'pids: for pid in pids {
                let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
                for e in dir.flatten() {
                    let Ok(target) = fs::read_link(e.path()) else { continue };
                    let t = target.to_string_lossy();
                    if let Some(n) = t.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']')) {
                        if let Ok(inode) = n.parse::<u64>() {
                            if let Some((lp, rp, est, listen)) = wanted.remove(&inode) {
                                out.push(Conn {
                                    pid: *pid,
                                    remote_port: rp,
                                    local_port: lp,
                                    established: est,
                                    listening: listen,
                                });
                                if wanted.is_empty() {
                                    break 'pids;
                                }
                            }
                        }
                    }
                }
            }
            out
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use crate::Conn;
    use std::mem::{size_of, zeroed};
    use zeroize::Zeroize;

    // From xnu bsd/sys/proc_info.h. Layout of struct socket_fdinfo:
    //   proc_fileinfo pfi (24 bytes) | socket_info psi
    //   socket_info: vinfo_stat (136) … soi_kind at +232, soi_proto union at +240
    //   tcp_sockinfo: in_sockinfo (80: insi_fport +0, insi_lport +4) then tcpsi_state at +80
    const PROC_PIDFDSOCKETINFO: libc::c_int = 3;
    const SOCKINFO_TCP: i32 = 2;
    const TSI_S_LISTEN: i32 = 1;
    const TSI_S_ESTABLISHED: i32 = 4;
    const OFF_PSI: usize = 24;
    const OFF_SOI_KIND: usize = OFF_PSI + 232;
    const OFF_SOI_PROTO: usize = OFF_PSI + 240;
    const OFF_INSI_FPORT: usize = OFF_SOI_PROTO;
    const OFF_INSI_LPORT: usize = OFF_SOI_PROTO + 4;
    const OFF_TCP_STATE: usize = OFF_SOI_PROTO + 80;

    pub struct Platform {
        args: Vec<u8>,
        fds: Vec<libc::proc_fdinfo>,
        /// Larger than sizeof(struct socket_fdinfo); u64 for alignment.
        sock: Vec<u64>,
    }

    impl Platform {
        pub fn new() -> Self {
            let mut argmax: libc::c_int = 0;
            let mut size = size_of::<libc::c_int>();
            let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
            unsafe {
                libc::sysctl(
                    mib.as_mut_ptr(),
                    2,
                    (&mut argmax as *mut libc::c_int).cast(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                );
            }
            let argmax = if argmax > 0 { argmax as usize } else { 1 << 20 };
            Self { args: vec![0; argmax], fds: Vec::with_capacity(256), sock: vec![0; 128] }
        }

        /// KERN_PROCARGS2 returns argc, exec path, argv **and envp** (research R-2). Only argc
        /// strings are parsed, and the whole buffer is wiped before returning on every path.
        pub fn cmdline(&mut self, pid: u32) -> Option<Box<[Box<str>]>> {
            let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
            let mut size = self.args.len();
            let r = unsafe {
                libc::sysctl(
                    mib.as_mut_ptr(),
                    3,
                    self.args.as_mut_ptr().cast(),
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                )
            };
            let filled = size.min(self.args.len());
            let result = if r == 0 { parse_procargs(&self.args[..filled]) } else { None };
            self.args[..filled].zeroize();
            result
        }

        pub fn footprint(&mut self, pid: u32) -> Option<u64> {
            let mut info: libc::rusage_info_v4 = unsafe { zeroed() };
            let r = unsafe {
                libc::proc_pid_rusage(
                    pid as libc::c_int,
                    libc::RUSAGE_INFO_V4,
                    (&mut info as *mut libc::rusage_info_v4).cast(),
                )
            };
            (r == 0).then_some(info.ri_phys_footprint)
        }

        pub fn tcp(&mut self, pids: &[u32]) -> Vec<Conn> {
            let mut out = Vec::new();
            for &pid in pids {
                let pid = pid as libc::c_int;
                let need = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
                if need <= 0 {
                    continue;
                }
                let n = need as usize / size_of::<libc::proc_fdinfo>() + 16;
                self.fds.clear();
                self.fds.reserve(n);
                let got = unsafe {
                    libc::proc_pidinfo(
                        pid,
                        libc::PROC_PIDLISTFDS,
                        0,
                        self.fds.as_mut_ptr().cast(),
                        (n * size_of::<libc::proc_fdinfo>()) as libc::c_int,
                    )
                };
                if got <= 0 {
                    continue;
                }
                unsafe { self.fds.set_len(got as usize / size_of::<libc::proc_fdinfo>()) };
                for fd in &self.fds {
                    if fd.proc_fdtype != libc::PROX_FDTYPE_SOCKET as u32 {
                        continue;
                    }
                    // libc has no socket_fdinfo, so read the needed fields at their offsets.
                    let r = unsafe {
                        libc::proc_pidfdinfo(
                            pid,
                            fd.proc_fd,
                            PROC_PIDFDSOCKETINFO,
                            self.sock.as_mut_ptr().cast(),
                            (self.sock.len() * 8) as libc::c_int,
                        )
                    };
                    if r < (OFF_TCP_STATE + 4) as libc::c_int {
                        continue;
                    }
                    let bytes = unsafe {
                        std::slice::from_raw_parts(self.sock.as_ptr().cast::<u8>(), self.sock.len() * 8)
                    };
                    let i32_at = |off: usize| i32::from_ne_bytes(bytes[off..off + 4].try_into().unwrap());
                    if i32_at(OFF_SOI_KIND) != SOCKINFO_TCP {
                        continue;
                    }
                    let lp = u16::from_be(i32_at(OFF_INSI_LPORT) as u16);
                    let rp = u16::from_be(i32_at(OFF_INSI_FPORT) as u16);
                    let state = i32_at(OFF_TCP_STATE);
                    let est = state == TSI_S_ESTABLISHED;
                    let listen = state == TSI_S_LISTEN;
                    let interesting = (est && rp == 443)
                        || (listen && crate::LOCAL_MODEL_PORTS.contains(&lp))
                        || (est && crate::LOCAL_MODEL_PORTS.contains(&lp));
                    if interesting {
                        out.push(Conn {
                            pid: pid as u32,
                            remote_port: rp,
                            local_port: lp,
                            established: est,
                            listening: listen,
                        });
                    }
                }
            }
            out
        }
    }

    fn parse_procargs(buf: &[u8]) -> Option<Box<[Box<str>]>> {
        if buf.len() < 4 {
            return None;
        }
        let argc = i32::from_ne_bytes(buf[..4].try_into().ok()?) as usize;
        let mut i = 4;
        while i < buf.len() && buf[i] != 0 {
            i += 1; // exec path
        }
        while i < buf.len() && buf[i] == 0 {
            i += 1; // padding
        }
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            let start = i;
            while i < buf.len() && buf[i] != 0 {
                i += 1;
            }
            if start >= buf.len() {
                break;
            }
            args.push(String::from_utf8_lossy(&buf[start..i]).into_owned().into_boxed_str());
            i += 1;
        }
        Some(args.into_boxed_slice())
    }
}

#[cfg(windows)]
mod imp {
    use crate::Conn;
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::ProcessStatus::GetProcessMemoryInfo;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    const PROCESS_COMMAND_LINE_INFORMATION: i32 = 60;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    const MIB_TCP_STATE_LISTEN: u32 = 2;
    const MIB_TCP_STATE_ESTAB: u32 = 5;

    /// PROCESS_MEMORY_COUNTERS_EX2 (Windows 10 1809+), defined here to avoid depending on
    /// whether the bindings expose it.
    #[repr(C)]
    #[derive(Default)]
    struct Pmc2 {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
        private_working_set_size: usize,
        shared_commit_usage: u64,
    }

    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
    fn open(pid: u32) -> Option<Handle> {
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        (!h.is_null()).then_some(Handle(h))
    }

    pub struct Platform {
        cmd: Vec<u64>,
        tcp: Vec<u32>,
    }

    impl Platform {
        pub fn new() -> Self {
            Self { cmd: vec![0; 512], tcp: vec![0; 16 * 1024] }
        }

        /// ProcessCommandLineInformation needs only PROCESS_QUERY_LIMITED_INFORMATION and
        /// returns the command line alone. There is no PEB read and no environment.
        pub fn cmdline(&mut self, pid: u32) -> Option<Box<[Box<str>]>> {
            let h = open(pid)?;
            loop {
                let mut ret: u32 = 0;
                let len = (self.cmd.len() * 8) as u32;
                let st = unsafe {
                    NtQueryInformationProcess(
                        h.0,
                        PROCESS_COMMAND_LINE_INFORMATION,
                        self.cmd.as_mut_ptr().cast::<c_void>(),
                        len,
                        &mut ret,
                    )
                };
                if st == STATUS_INFO_LENGTH_MISMATCH && ret > len {
                    self.cmd.resize((ret as usize).div_ceil(8), 0);
                    continue;
                }
                if st != 0 {
                    return None;
                }
                let us = unsafe { &*(self.cmd.as_ptr() as *const UnicodeString) };
                if us.buffer.is_null() {
                    return Some(Box::new([]));
                }
                let wide = unsafe { std::slice::from_raw_parts(us.buffer, us.length as usize / 2) };
                let s = String::from_utf16_lossy(wide);
                return Some(s.split_whitespace().map(|a| a.trim_matches('"').into()).collect());
            }
        }

        pub fn footprint(&mut self, pid: u32) -> Option<u64> {
            let h = open(pid)?;
            let mut c = Pmc2 { cb: size_of::<Pmc2>() as u32, ..Default::default() };
            let ok = unsafe { GetProcessMemoryInfo(h.0, (&mut c as *mut Pmc2).cast(), c.cb) };
            (ok != 0).then_some(c.private_working_set_size as u64)
        }

        fn table(&mut self, af: u32) -> bool {
            loop {
                let mut size = (self.tcp.len() * 4) as u32;
                let r = unsafe {
                    GetExtendedTcpTable(self.tcp.as_mut_ptr().cast(), &mut size, 0, af, TCP_TABLE_OWNER_PID_ALL, 0)
                };
                if r == 0 {
                    return true;
                }
                if r == ERROR_INSUFFICIENT_BUFFER {
                    self.tcp.resize((size as usize).div_ceil(4) + 1024, 0);
                    continue;
                }
                return false;
            }
        }

        pub fn tcp(&mut self, _pids: &[u32]) -> Vec<Conn> {
            let mut out = Vec::new();
            let keep = |est: bool, listen: bool, lp: u16, rp: u16| {
                (est && rp == 443)
                    || (listen && crate::LOCAL_MODEL_PORTS.contains(&lp))
                    || (est && crate::LOCAL_MODEL_PORTS.contains(&lp))
            };
            if self.table(AF_INET as u32) {
                let n = self.tcp[0] as usize;
                let rows = unsafe {
                    std::slice::from_raw_parts(self.tcp.as_ptr().add(1) as *const MIB_TCPROW_OWNER_PID, n)
                };
                for r in rows {
                    let lp = u16::from_be(r.dwLocalPort as u16);
                    let rp = u16::from_be(r.dwRemotePort as u16);
                    let est = r.dwState == MIB_TCP_STATE_ESTAB;
                    let listen = r.dwState == MIB_TCP_STATE_LISTEN;
                    if keep(est, listen, lp, rp) {
                        out.push(Conn { pid: r.dwOwningPid, remote_port: rp, local_port: lp, established: est, listening: listen });
                    }
                }
            }
            if self.table(AF_INET6 as u32) {
                let n = self.tcp[0] as usize;
                let rows = unsafe {
                    std::slice::from_raw_parts(self.tcp.as_ptr().add(1) as *const MIB_TCP6ROW_OWNER_PID, n)
                };
                for r in rows {
                    let lp = u16::from_be(r.dwLocalPort as u16);
                    let rp = u16::from_be(r.dwRemotePort as u16);
                    let est = r.dwState == MIB_TCP_STATE_ESTAB;
                    let listen = r.dwState == MIB_TCP_STATE_LISTEN;
                    if keep(est, listen, lp, rp) {
                        out.push(Conn { pid: r.dwOwningPid, remote_port: rp, local_port: lp, established: est, listening: listen });
                    }
                }
            }
            out
        }
    }
}

pub use imp::Platform;
