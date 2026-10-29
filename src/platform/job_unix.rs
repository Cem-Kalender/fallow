//! Unix: an agent step is started in its own process group (`process_group(0)`), so the group id
//! equals its PID. Signals go to the whole group.

/// The agent's process group.
#[derive(Debug)]
pub struct Group {
    pgid: Option<i32>,
}

impl Group {
    pub fn new() -> std::io::Result<Group> {
        Ok(Group { pgid: None })
    }

    /// Records the agent right after spawn.
    pub fn adopt(&mut self, pid: u32) -> std::io::Result<()> {
        self.pgid = i32::try_from(pid).ok().filter(|p| *p > 1);
        Ok(())
    }

    fn signal(&self, sig: libc::c_int) {
        if let Some(pgid) = self.pgid {
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
        self.pgid.is_some_and(|g| {
            // SAFETY: getpgid(2) reads a process attribute; no memory is passed.
            i32::try_from(pid).is_ok_and(|p| unsafe { libc::getpgid(p) } == g)
        })
    }
}
