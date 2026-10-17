//! Windows platform layer (R-1, R-3, R-4, R-10).
//!
//! The command line comes from `ProcessCommandLineInformation`, which returns it alone. The
//! working directory needs a PEB read: only `ProcessParameters.CurrentDirectory.DosPath` and
//! its buffer are copied, never the `Environment` (FR-024). Which processes may be read is the
//! worker's decision (R-10 policy, protected processes excluded).

use std::ffi::c_void;
use std::mem::size_of;
use std::path::PathBuf;
use std::sync::Arc;

use windows_sys::Wdk::System::Threading::NtQueryInformationProcess;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, TCP_TABLE_OWNER_PID_ALL,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::ProcessStatus::GetProcessMemoryInfo;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows_sys::Win32::UI::Shell::CommandLineToArgvW;

use super::parse::{self, RawConn, TcpQuery, TcpRow};
use super::{Platform, PlatformError, Result};
use crate::procmon::snapshot::{Footprint, ProcessKey};

const PROCESS_BASIC_INFORMATION_CLASS: i32 = 0;
const PROCESS_COMMAND_LINE_INFORMATION: i32 = 60;
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
/// x64 offsets: `PEB.ProcessParameters` and `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory`.
const PEB_PROCESS_PARAMETERS: usize = 0x20;
const PARAMS_CURRENT_DIRECTORY: usize = 0x38;

/// PROCESS_MEMORY_COUNTERS_EX2 (Windows 10 1809+).
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
#[derive(Default)]
struct BasicInformation {
    exit_status: i32,
    peb_base_address: usize,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: usize,
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcess and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn open(pid: u32, access: u32) -> Result<Handle> {
    // SAFETY: plain Win32 call.
    let h = unsafe { OpenProcess(access, 0, pid) };
    if h.is_null() {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(Handle(h))
    }
}

/// Splits a command line with the system's own rules (`CommandLineToArgvW`).
pub fn split_command_line(line: &str) -> Vec<Box<str>> {
    if line.trim().is_empty() {
        // CommandLineToArgvW("") returns the current executable's path instead.
        return Vec::new();
    }
    let wide: Vec<u16> = line.encode_utf16().chain(Some(0)).collect();
    let mut n: i32 = 0;
    // SAFETY: `wide` is NUL-terminated; the result is freed with LocalFree below.
    let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &mut n) };
    if argv.is_null() {
        return vec![line.into()];
    }
    let mut out = Vec::with_capacity(n.max(0) as usize);
    for i in 0..n.max(0) as usize {
        // SAFETY: argv holds `n` valid NUL-terminated wide strings.
        unsafe {
            let p = *argv.add(i);
            let len = (0..).take_while(|&j| *p.add(j) != 0).count();
            out.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)).into());
        }
    }
    // SAFETY: allocated by CommandLineToArgvW.
    unsafe { LocalFree(argv.cast()) };
    out
}

#[derive(Debug)]
pub struct OsPlatform {
    cmd: Vec<u64>,
    tcp: Vec<u64>,
    rows: Vec<TcpRow>,
}

impl Default for OsPlatform {
    fn default() -> Self {
        OsPlatform {
            cmd: vec![0; 512],
            tcp: vec![0; 8 * 1024],
            rows: Vec::with_capacity(256),
        }
    }
}

impl OsPlatform {
    fn table(&mut self, af: u32) -> bool {
        loop {
            let mut size = (self.tcp.len() * 8) as u32;
            // SAFETY: `tcp` is writable for `size` bytes.
            let r = unsafe {
                GetExtendedTcpTable(
                    self.tcp.as_mut_ptr().cast(),
                    &mut size,
                    0,
                    af,
                    TCP_TABLE_OWNER_PID_ALL,
                    0,
                )
            };
            if r == 0 {
                return true;
            }
            if r == ERROR_INSUFFICIENT_BUFFER {
                self.tcp.resize((size as usize).div_ceil(8) + 1024, 0);
                continue;
            }
            return false;
        }
    }

    fn table_bytes(&self) -> &[u8] {
        // SAFETY: `tcp` is initialized memory of this many bytes.
        unsafe { std::slice::from_raw_parts(self.tcp.as_ptr().cast::<u8>(), self.tcp.len() * 8) }
    }
}

fn read<T: Copy + Default>(h: &Handle, addr: usize) -> Result<T> {
    let mut v = T::default();
    let mut got = 0usize;
    // SAFETY: reads size_of::<T>() bytes into `v`.
    let ok = unsafe {
        ReadProcessMemory(
            h.0,
            addr as *const c_void,
            (&mut v as *mut T).cast(),
            size_of::<T>(),
            &mut got,
        )
    };
    if ok == 0 || got != size_of::<T>() {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(v)
}

impl Platform for OsPlatform {
    fn cmdline(&mut self, key: ProcessKey) -> Result<Arc<[Box<str>]>> {
        let h = open(key.pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        loop {
            let mut ret: u32 = 0;
            let len = (self.cmd.len() * 8) as u32;
            // SAFETY: `cmd` is writable for `len` bytes.
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
                return Err(PlatformError::Other(format!(
                    "NtQueryInformationProcess {st:#x}"
                )));
            }
            // SAFETY: the buffer starts with a UNICODE_STRING whose Buffer points into it.
            let us = unsafe { *(self.cmd.as_ptr() as *const UnicodeString) };
            if us.buffer == 0 {
                return Ok(Arc::from(Vec::new()));
            }
            let wide = unsafe {
                std::slice::from_raw_parts(us.buffer as *const u16, us.length as usize / 2)
            };
            return Ok(split_command_line(&String::from_utf16_lossy(wide)).into());
        }
    }

    fn cwd(&mut self, key: ProcessKey) -> Result<PathBuf> {
        let h = open(key.pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ)?;
        let mut info = BasicInformation::default();
        let mut ret = 0u32;
        // SAFETY: `info` is a correctly sized PROCESS_BASIC_INFORMATION.
        let st = unsafe {
            NtQueryInformationProcess(
                h.0,
                PROCESS_BASIC_INFORMATION_CLASS,
                (&mut info as *mut BasicInformation).cast(),
                size_of::<BasicInformation>() as u32,
                &mut ret,
            )
        };
        if st != 0 || info.peb_base_address == 0 {
            return Err(PlatformError::Other(format!("no PEB ({st:#x})")));
        }
        let params: usize = read(&h, info.peb_base_address + PEB_PROCESS_PARAMETERS)?;
        let dos: UnicodeString = read(&h, params + PARAMS_CURRENT_DIRECTORY)?;
        if dos.buffer == 0 || dos.length == 0 {
            return Err(PlatformError::Other("empty working directory".into()));
        }
        let mut wide = vec![0u16; dos.length as usize / 2];
        let mut got = 0usize;
        // SAFETY: `wide` is writable for `length` bytes.
        let ok = unsafe {
            ReadProcessMemory(
                h.0,
                dos.buffer as *const c_void,
                wide.as_mut_ptr().cast(),
                dos.length as usize,
                &mut got,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        match parse::windows_cwd(Ok(String::from_utf16_lossy(&wide[..got / 2]))) {
            crate::procmon::snapshot::CwdState::Known(p) => Ok(p),
            _ => Err(PlatformError::Other("unreadable working directory".into())),
        }
    }

    fn footprint(&mut self, key: ProcessKey) -> Result<Footprint> {
        let h = open(key.pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let mut c = Pmc2 {
            cb: size_of::<Pmc2>() as u32,
            ..Default::default()
        };
        // SAFETY: `c` is a PROCESS_MEMORY_COUNTERS_EX2 and its size is passed.
        let ex2 = unsafe { GetProcessMemoryInfo(h.0, (&mut c as *mut Pmc2).cast(), c.cb) } != 0;
        let (private, basic) = if ex2 {
            (
                Some(c.private_working_set_size as u64),
                Some(c.working_set_size as u64),
            )
        } else {
            // Before 1809 the EX2 size is rejected; the basic counters still work.
            let mut b = Pmc2 {
                cb: size_of::<windows_sys::Win32::System::ProcessStatus::PROCESS_MEMORY_COUNTERS>()
                    as u32,
                ..Default::default()
            };
            let ok = unsafe { GetProcessMemoryInfo(h.0, (&mut b as *mut Pmc2).cast(), b.cb) } != 0;
            (None, ok.then_some(b.working_set_size as u64))
        };
        parse::windows_footprint(private, basic)
            .ok_or_else(|| std::io::Error::last_os_error().into())
    }

    fn tcp_table(&mut self, q: &TcpQuery<'_>, out: &mut Vec<RawConn>) -> Result<()> {
        self.rows.clear();
        let mut rows = std::mem::take(&mut self.rows);
        if self.table(AF_INET as u32) {
            parse::parse_tcp_table_v4(self.table_bytes(), &mut rows);
        }
        if self.table(AF_INET6 as u32) {
            parse::parse_tcp_table_v6(self.table_bytes(), &mut rows);
        }
        let with_pid: Vec<(u32, TcpRow)> = rows.iter().map(|r| (r.owner as u32, *r)).collect();
        parse::select(&with_pid, q, out);
        self.rows = rows;
        Ok(())
    }

    fn suspend(&mut self, _key: ProcessKey) -> Result<()> {
        Err(PlatformError::Unsupported)
    }

    fn resume(&mut self, _key: ProcessKey) -> Result<()> {
        Err(PlatformError::Unsupported)
    }

    fn kill(&mut self, _key: ProcessKey, _force: bool) -> Result<()> {
        Err(PlatformError::Unsupported)
    }
}
