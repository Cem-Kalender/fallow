//! Windows: each agent is placed in a Job Object created with `KILL_ON_JOB_CLOSE`. Fallow holds
//! the job handle, so when Fallow exits, by any route, the agent and everything it started end
//! too: sessions never outlive Fallow (002 spec Edge Cases, research R-7).

use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
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

    /// Puts the agent into the job right after spawn.
    pub fn adopt(&mut self, pid: u32) -> io::Result<()> {
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

    /// Whether process `pid` is in this session's job (003 research R-3: a bridge that
    /// connects must belong to the session it names).
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

    /// Graceful stop on Windows is closing the pseudoconsole (CTRL_CLOSE_EVENT), done by the
    /// caller; nothing to signal here.
    pub fn terminate(&self) {}

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
