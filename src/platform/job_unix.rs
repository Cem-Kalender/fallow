//! Unix: an agent step is started in its own process group (`process_group(0)`), so the group id
//! equals its PID. Signals go to the whole group.
//!
//! In its own group an agent no longer gets the terminal's Ctrl-C with Fallow, so Fallow stops
//! the live groups itself when it is interrupted, terminated or hung up (seventh review, 1):
//! they end with Fallow, as Windows' job objects do.

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

/// The live agents' groups, for the signal handler (atomics only: async-signal-safe).
static LIVE: [AtomicI32; 256] = [const { AtomicI32::new(0) }; 256];

/// No slot.
const NONE: usize = usize::MAX;

extern "C" fn on_exit_signal(sig: libc::c_int) {
    for slot in &LIVE {
        let p = slot.load(Ordering::SeqCst);
        if p > 1 {
            // SAFETY: kill(2) is async-signal-safe; it signals that process group.
            unsafe {
                libc::kill(-p, libc::SIGKILL);
            }
        }
    }
    // SAFETY: signal(2) and raise(3) are async-signal-safe: Fallow then ends as the signal says.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// Installs the handler once, never over a signal set to be ignored (`nohup`).
fn install_handlers() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            // Read first: a signal ignored (`nohup`) is never handled, not even for a moment.
            // SAFETY: a zeroed sigaction is valid to be filled in; only `old` is written.
            let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
            // SAFETY: a null new action only reads the current one into `old`.
            if unsafe { libc::sigaction(sig, std::ptr::null(), &mut old) } != 0
                || old.sa_sigaction == libc::SIG_IGN
            {
                continue;
            }
            let handler = on_exit_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            // SAFETY: the handler only uses async-signal-safe calls and atomics.
            unsafe { libc::signal(sig, handler) };
        }
    });
}

/// The agent's process group.
#[derive(Debug)]
pub struct Group {
    /// 0 until the agent is adopted.
    pgid: AtomicI32,
    /// Its slot in `LIVE`, or `NONE`.
    slot: AtomicUsize,
}

impl Drop for Group {
    fn drop(&mut self) {
        let i = self.slot.swap(NONE, Ordering::SeqCst);
        if i != NONE {
            LIVE[i].store(0, Ordering::SeqCst);
        }
    }
}

impl Group {
    pub fn new() -> std::io::Result<Group> {
        Ok(Group {
            pgid: AtomicI32::new(0),
            slot: AtomicUsize::new(NONE),
        })
    }

    /// Not sampled on Unix yet: the cap on parallel steps still applies.
    pub fn memory_mb(&self) -> u64 {
        0
    }

    fn pgid(&self) -> Option<i32> {
        Some(self.pgid.load(Ordering::Relaxed)).filter(|p| *p > 1)
    }

    /// Records the agent right after spawn.
    pub fn adopt(&self, pid: u32) -> std::io::Result<()> {
        if let Some(p) = i32::try_from(pid).ok().filter(|p| *p > 1) {
            self.pgid.store(p, Ordering::Relaxed);
            install_handlers();
            let old = self.slot.swap(NONE, Ordering::SeqCst);
            if old != NONE {
                LIVE[old].store(0, Ordering::SeqCst);
            }
            if let Some(i) = LIVE.iter().position(|s| {
                s.compare_exchange(0, p, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            }) {
                self.slot.store(i, Ordering::SeqCst);
            }
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

    /// 012 (research R-19): writes the group's id and its leader's start time to `path`, so a
    /// Fallow that crashed can stop it on its next start. False until the agent is adopted.
    pub fn record(&self, path: &std::path::Path) -> bool {
        let Some(pgid) = self.pgid() else {
            return false;
        };
        let start = start_time(pgid).unwrap_or_default();
        std::fs::write(path, format!("{pgid} {start}")).is_ok()
    }

    /// Unix has no job membership test: a process belongs when it is in the agent's group.
    pub fn contains(&self, pid: u32) -> bool {
        self.pgid().is_some_and(|g| {
            // SAFETY: getpgid(2) reads a process attribute; no memory is passed.
            i32::try_from(pid).is_ok_and(|p| unsafe { libc::getpgid(p) } == g)
        })
    }
}

/// A process's start time as `ps` reads it, to tell it from a later process with its id.
fn start_time(pid: i32) -> Option<String> {
    let out = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// Whether any process is still in group `pgid`.
fn group_alive(pgid: i32) -> bool {
    // SAFETY: kill(2) with signal 0 only checks; no memory is touched.
    unsafe { libc::kill(-pgid, 0) == 0 }
}

/// 012 (research R-19): stops the process group recorded in `path` by `Group::record`, and
/// waits (up to five seconds) until nothing of it runs, so the snapshot after it is final.
///
/// A reused id is never killed. While its leader runs, the leader must be the same process
/// (same start time). Once the leader has ended, the group's id cannot be given to a new
/// process or group while any member is left (POSIX), so the members left are the turn's own:
/// a child the agent started that outlived it (fourth review, 1).
pub fn kill_recorded(path: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let mut it = text.splitn(2, ' ');
    let pgid: i32 = it.next().and_then(|p| p.trim().parse().ok()).unwrap_or(0);
    let recorded = it.next().unwrap_or_default().trim().to_string();
    let ours = pgid > 1
        && match start_time(pgid) {
            Some(now) => !recorded.is_empty() && now == recorded,
            None => group_alive(pgid),
        };
    if ours {
        // SAFETY: kill(2) with a negative pid signals that process group; no memory is touched.
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
        for _ in 0..100 {
            if !group_alive(pgid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    let _ = std::fs::remove_file(path);
}
