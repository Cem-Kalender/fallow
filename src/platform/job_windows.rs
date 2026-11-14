//! Windows: each agent step is placed in a Job Object created with `KILL_ON_JOB_CLOSE`. Fallow
//! holds the job handle, so when Fallow exits, by any route, the agent and everything it started
//! end too; the step is resumed by session when Fallow starts again (006 research R-2).

use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

/// The Job Object holding the agent and its descendants.
#[derive(Debug)]
pub struct Group {
    job: HANDLE,
}

// SAFETY: a job handle is a kernel handle usable from any thread.
unsafe impl Send for Group {}
// SAFETY: the handle is only passed to thread-safe kernel calls.
unsafe impl Sync for Group {}

impl Group {
    pub fn new() -> io::Result<Group> {
        // SAFETY: null attributes and name create an anonymous job.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is valid (no limits).
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` lives for the call and its size is passed.
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            let e = io::Error::last_os_error();
            // SAFETY: `job` is a handle we own.
            unsafe { CloseHandle(job) };
            return Err(e);
        }
        Ok(Group { job })
    }

    /// The most memory the job's processes have committed so far, in MB (the memory budget,
    /// research S-19).
    pub fn memory_mb(&self) -> u64 {
        // SAFETY: an all-zero structure is valid output space.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `info` lives for the call and its size is passed.
        let ok = unsafe {
            QueryInformationJobObject(
                self.job,
                JobObjectExtendedLimitInformation,
                (&raw mut info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return 0;
        }
        (info.PeakJobMemoryUsed / (1024 * 1024)) as u64
    }

    /// Puts the agent into the job right after spawn.
    pub fn adopt(&self, pid: u32) -> io::Result<()> {
        // SAFETY: OpenProcess with a PID; the handle is closed below.
        let h = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both handles are valid.
        let ok = unsafe { AssignProcessToJobObject(self.job, h) };
        let e = io::Error::last_os_error();
        // SAFETY: `h` is ours.
        unsafe { CloseHandle(h) };
        if ok == 0 { Err(e) } else { Ok(()) }
    }

    /// Whether process `pid` is in this step's job (a bridge that connects must belong to the
    /// step it names, 006 research R-5).
    pub fn contains(&self, pid: u32) -> bool {
        use windows_sys::Win32::System::JobObjects::IsProcessInJob;
        use windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
        // SAFETY: OpenProcess with a PID; the handle is closed below.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return false;
        }
        let mut result = 0;
        // SAFETY: both handles are valid; `result` is writable.
        let ok = unsafe { IsProcessInJob(h, self.job, &mut result) };
        // SAFETY: `h` is ours.
        unsafe { CloseHandle(h) };
        ok != 0 && result != 0
    }

    /// 012 (research R-19): nothing to record: the job is created with `KILL_ON_JOB_CLOSE`, so
    /// its processes end with Fallow and nothing of a turn runs on after a crash.
    pub fn record(&self, _path: &std::path::Path) -> bool {
        true
    }

    /// Forced stop: ends every process in the job.
    pub fn kill(&self) {
        // SAFETY: `self.job` is a valid job handle.
        unsafe { TerminateJobObject(self.job, 1) };
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        // SAFETY: closing our handle; KILL_ON_JOB_CLOSE ends the processes in it.
        unsafe { CloseHandle(self.job) };
    }
}

/// 012: see `Group::record`; the job already ended the processes.
pub fn kill_recorded(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

/// 014 (`fallow quit --installer`): waits up to `limit` for process `pid` to exit; returns
/// whether it did (or was gone already).
pub fn wait_exit(pid: u32, limit: std::time::Duration) -> bool {
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{PROCESS_SYNCHRONIZE, WaitForSingleObject};
    // SAFETY: a handle opened for waiting only, closed after the wait.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return true;
        }
        let r = WaitForSingleObject(h, limit.as_millis().min(u32::MAX as u128) as u32);
        CloseHandle(h);
        r == WAIT_OBJECT_0
    }
}

/// 014: how many processes named `name` run besides `me` (the agents' `fallow.exe mcp-bridge`).
pub fn others_named(name: &str, me: u32) -> usize {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let mut n = 0;
    // SAFETY: a process snapshot read into a live, sized entry, then closed.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return 0;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                let exe = String::from_utf16_lossy(&e.szExeFile[..len]);
                if e.th32ProcessID != me && exe.eq_ignore_ascii_case(name) {
                    n += 1;
                }
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    n
}
