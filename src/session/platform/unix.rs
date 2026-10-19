//! Unix: portable-pty starts the agent with `setsid`, so it leads its own process group and
//! the group id equals its PID. Signals go to the whole group.

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

    /// Graceful stop: SIGTERM to the group.
    pub fn terminate(&self) {
        self.signal(libc::SIGTERM);
    }

    /// Forced stop: SIGKILL to the group.
    pub fn kill(&self) {
        self.signal(libc::SIGKILL);
    }
}
