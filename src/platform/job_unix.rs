//! Unix: an agent step is started in its own process group (`process_group(0)`), so the group id
//! equals its PID. Signals go to the whole group.

/// The agent's process group.
#[derive(Debug)]
pub struct Group {
    /// 0 until the agent is adopted.
    pgid: std::sync::atomic::AtomicI32,
}

impl Group {
    pub fn new() -> std::io::Result<Group> {
        Ok(Group {
            pgid: std::sync::atomic::AtomicI32::new(0),
        })
    }

    /// Not sampled on Unix yet: the cap on parallel steps still applies.
    pub fn memory_mb(&self) -> u64 {
        0
    }

    fn pgid(&self) -> Option<i32> {
        Some(self.pgid.load(std::sync::atomic::Ordering::Relaxed)).filter(|p| *p > 1)
    }

    /// Records the agent right after spawn.
    pub fn adopt(&self, pid: u32) -> std::io::Result<()> {
        if let Some(p) = i32::try_from(pid).ok().filter(|p| *p > 1) {
            self.pgid.store(p, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    fn signal(&self, sig: libc::c_int) {
        if let Some(pgid) = self.pgid() {
            // SAFETY: kill(2) with a negative pid signals that process group; no memory is
            // touched.
            unsafe {
                libc::kill(-pgid, sig);
            }
        }
    }

    /// Forced stop: SIGKILL to the group.
    pub fn kill(&self) {
        self.signal(libc::SIGKILL);
    }

    /// Unix has no job membership test; the bridge's identity is checked by descent instead.
    pub fn contains(&self, pid: u32) -> bool {
        self.pgid().is_some_and(|g| {
            // SAFETY: getpgid(2) reads a process attribute; no memory is passed.
            i32::try_from(pid).is_ok_and(|p| unsafe { libc::getpgid(p) } == g)
        })
    }
}
