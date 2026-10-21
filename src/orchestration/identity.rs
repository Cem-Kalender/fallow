//! Tokens and peer identity (003 research R-3).
//!
//! A bridge connecting to the channel says which session it belongs to and shows that session's
//! token. Fallow believes it only if the kernel-reported peer process is also part of that session:
//! - Windows: the peer is in the session's Job Object.
//! - Linux and macOS: the peer's POSIX session id is the agent's PID (the agent runs under
//!   `setsid`). Failing that, walking up its parents reaches the agent.
//!
//! The token alone proves nothing: same-user processes can read each other's command lines.

use std::fmt;

/// 32 random bytes handed to one session's bridge. Never shown, logged or written, except into
/// that session's per-run agent configuration (research R-5).
#[derive(Clone, PartialEq, Eq)]
pub struct Token([u8; 32]);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(…)")
    }
}

impl Token {
    pub fn new() -> std::io::Result<Token> {
        let mut b = [0u8; 32];
        fill_random(&mut b)?;
        Ok(Token(b))
    }

    pub fn from_bytes(b: [u8; 32]) -> Token {
        Token(b)
    }

    /// 64 lowercase hex characters.
    pub fn hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push(char::from(b"0123456789abcdef"[usize::from(b >> 4)]));
            s.push(char::from(b"0123456789abcdef"[usize::from(b & 15)]));
        }
        s
    }

    /// Compares with a claimed hex token in constant time (for a well-formed claim).
    pub fn matches(&self, claim: &str) -> bool {
        let c = claim.as_bytes();
        if c.len() != 64 {
            return false;
        }
        let mut diff = 0u8;
        for (i, b) in self.0.iter().enumerate() {
            let (Some(hi), Some(lo)) = (nibble(c[2 * i]), nibble(c[2 * i + 1])) else {
                return false;
            };
            diff |= b ^ (hi << 4 | lo);
        }
        diff == 0
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Random bytes from the operating system.
pub fn fill_random(buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(buf)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{
            BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
        };
        let len = u32::try_from(buf.len()).map_err(std::io::Error::other)?;
        // SAFETY: `buf` is valid for `len` bytes; a null algorithm handle is allowed with
        // BCRYPT_USE_SYSTEM_PREFERRED_RNG.
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                len,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "BCryptGenRandom failed: {status:#x}"
            )))
        }
    }
}

/// Who is on the other end of a channel connection, as the kernel reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub pid: u32,
    /// Unix only; on Windows the pipe's DACL admits only the current user.
    pub uid: Option<u32>,
}

/// A process's parent and start time (seconds and microseconds since the epoch, or the
/// platform's own clock; only compared with values from the same source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcInfo {
    pub ppid: u32,
    pub start: u64,
}

/// Whether `peer` descends from `agent` (`(pid, start)`), walking parents with `info`. Every
/// process on the way must have started no earlier than the agent: an older one with a reused
/// PID breaks the chain. The peer is read again after the walk, so a peer that exited and whose
/// PID was reused during the walk is refused.
pub fn descends(peer: u32, agent: (u32, u64), info: impl Fn(u32) -> Option<ProcInfo>) -> bool {
    let Some(first) = info(peer) else {
        return false;
    };
    let mut pid = peer;
    let mut cur = first;
    let mut found = false;
    for _ in 0..64 {
        if pid == agent.0 {
            found = cur.start == agent.1;
            break;
        }
        if cur.start < agent.1 || cur.ppid == 0 || cur.ppid == pid {
            break;
        }
        pid = cur.ppid;
        match info(pid) {
            Some(i) => cur = i,
            None => break,
        }
    }
    found && info(peer) == Some(first)
}

/// The parent and start time of a live process.
#[cfg(target_os = "linux")]
pub fn proc_info(pid: u32) -> Option<ProcInfo> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The name is in parentheses and may contain anything; fields follow the last ')'.
    let rest = &s[s.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // After the name: state(0) ppid(1) … starttime is field 22 of the line, index 19 here.
    Some(ProcInfo {
        ppid: f.get(1)?.parse().ok()?,
        start: f.get(19)?.parse().ok()?,
    })
}

#[cfg(target_os = "macos")]
pub fn proc_info(pid: u32) -> Option<ProcInfo> {
    let mut bsd: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `bsd` is a proc_bsdinfo of `size` bytes.
    let r = unsafe {
        libc::proc_pidinfo(
            i32::try_from(pid).ok()?,
            libc::PROC_PIDTBSDINFO,
            0,
            (&raw mut bsd).cast(),
            size,
        )
    };
    (r == size).then(|| ProcInfo {
        ppid: bsd.pbi_ppid,
        start: bsd.pbi_start_tvsec * 1_000_000 + bsd.pbi_start_tvusec,
    })
}

/// Whether the peer belongs to the session whose agent is `agent` = (pid, start from
/// `proc_info`). Unix: the same POSIX session (the agent leads it), else descent.
#[cfg(unix)]
pub fn in_session(peer: u32, agent: (u32, u64)) -> bool {
    let (Ok(p), Ok(a)) = (i32::try_from(peer), i32::try_from(agent.0)) else {
        return false;
    };
    // SAFETY: getsid(2) reads a process attribute; no memory is passed.
    let sid = unsafe { libc::getsid(p) };
    if sid == a {
        // The agent still holds its PID (Fallow has not reaped it), so a session id equal to it
        // is the agent's session. The peer must not be older than the agent.
        let started_after = match (proc_info(peer), proc_info(agent.0)) {
            (Some(pi), Some(ai)) => ai.start == agent.1 && pi.start >= agent.1,
            _ => false,
        };
        if started_after {
            return true;
        }
    }
    descends(peer, agent, proc_info)
}

/// The peer of a Unix socket connection.
#[cfg(unix)]
pub fn peer_of(stream: &std::os::unix::net::UnixStream) -> std::io::Result<Peer> {
    use std::os::fd::AsRawFd;
    let fd = stream.as_raw_fd();
    #[cfg(target_os = "linux")]
    {
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `cred` and `len` describe a writable ucred.
        let r = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&raw mut cred).cast(),
                &mut len,
            )
        };
        if r != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Peer {
            pid: u32::try_from(cred.pid).unwrap_or(0),
            uid: Some(cred.uid),
        })
    }
    #[cfg(target_os = "macos")]
    {
        let mut uid: libc::uid_t = 0;
        let mut gid: libc::gid_t = 0;
        // SAFETY: both out-pointers are valid.
        if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut pid: libc::pid_t = 0;
        let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
        // SAFETY: `pid` and `len` describe a writable pid_t.
        let r = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&raw mut pid).cast(),
                &mut len,
            )
        };
        if r != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Peer {
            pid: u32::try_from(pid).unwrap_or(0),
            uid: Some(uid),
        })
    }
}

/// The current user's id.
#[cfg(unix)]
pub fn my_uid() -> u32 {
    // SAFETY: getuid(2) cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn hex_and_compare() {
        let mut b = [0u8; 32];
        b[0] = 0xab;
        b[31] = 0x0f;
        let t = Token::from_bytes(b);
        let h = t.hex();
        assert_eq!(h.len(), 64);
        assert!(h.starts_with("ab00"));
        assert!(h.ends_with("0f"));
        assert!(t.matches(&h));
        assert!(!t.matches(&h.to_uppercase()), "lowercase only");
        assert!(!t.matches(&h[..63]));
        let mut wrong = h.clone().into_bytes();
        wrong[10] = b'1';
        assert!(!t.matches(std::str::from_utf8(&wrong).unwrap()));
        assert_eq!(format!("{t:?}"), "Token(…)", "never printed");
    }

    #[test]
    fn random_tokens_differ() {
        assert_ne!(Token::new().unwrap(), Token::new().unwrap());
    }

    fn table(rows: &[(u32, u32, u64)]) -> HashMap<u32, ProcInfo> {
        rows.iter()
            .map(|&(pid, ppid, start)| (pid, ProcInfo { ppid, start }))
            .collect()
    }

    #[test]
    fn descent_through_wrappers() {
        // agent 100 (start 50) → node 200 → npx 300 → bridge 400
        let t = table(&[
            (1, 0, 0),
            (100, 1, 50),
            (200, 100, 60),
            (300, 200, 70),
            (400, 300, 80),
        ]);
        assert!(descends(400, (100, 50), |p| t.get(&p).copied()));
        assert!(descends(100, (100, 50), |p| t.get(&p).copied()));
        // Another session's agent.
        assert!(!descends(400, (999, 50), |p| t.get(&p).copied()));
    }

    #[test]
    fn reused_pids_break_the_chain() {
        // 300's real parent exited; PID 200 now belongs to an older, unrelated process that
        // happens to be a child of the agent's PID number from an earlier life.
        let t = table(&[(100, 1, 50), (200, 100, 10), (300, 200, 70), (400, 300, 80)]);
        assert!(!descends(400, (100, 50), |p| t.get(&p).copied()));
        // The agent's PID now names a different, newer process.
        let t = table(&[(100, 1, 90), (400, 100, 95)]);
        assert!(!descends(400, (100, 50), |p| t.get(&p).copied()));
        // An orphan reparented to init never reaches the agent.
        let t = table(&[(1, 0, 0), (100, 1, 50), (400, 1, 80)]);
        assert!(!descends(400, (100, 50), |p| t.get(&p).copied()));
        // A cycle ends.
        let t = table(&[(400, 500, 80), (500, 400, 80)]);
        assert!(!descends(400, (100, 50), |p| t.get(&p).copied()));
    }

    #[test]
    fn a_peer_replaced_during_the_walk_is_refused() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let info = |p: u32| {
            calls.set(calls.get() + 1);
            match p {
                100 => Some(ProcInfo { ppid: 1, start: 50 }),
                // The second read of the peer sees a different process.
                400 if calls.get() > 2 => Some(ProcInfo {
                    ppid: 100,
                    start: 99,
                }),
                400 => Some(ProcInfo {
                    ppid: 100,
                    start: 80,
                }),
                _ => None,
            }
        };
        assert!(!descends(400, (100, 50), info));
    }

    #[cfg(unix)]
    #[test]
    fn this_process_is_in_its_own_session_tree() {
        let me = std::process::id();
        let info = proc_info(me).expect("own process info");
        assert!(descends(me, (me, info.start), proc_info));
        assert!(!descends(me, (me, info.start + 1), proc_info));
    }
}
