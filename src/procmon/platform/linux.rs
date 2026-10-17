//! Linux platform layer (R-1, R-3, R-4). Everything comes from `/proc`.
//!
//! Never opens `/proc/<pid>/environ` (FR-024).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use super::parse::{self, RawConn, TcpQuery, TcpRow};
use super::{Platform, PlatformError, Result};
use crate::procmon::snapshot::{ConnState, Footprint, ProcessKey};

#[derive(Debug)]
pub struct OsPlatform {
    page: u64,
    /// Reused for `/proc/net/tcp{,6}` and `smaps_rollup`.
    text: String,
    rows: Vec<TcpRow>,
    /// Socket inode → owning PID, learned from `/proc/<pid>/fd` scans and kept while the socket
    /// exists.
    inode_owner: HashMap<u64, u32>,
    /// Interesting inodes no visible process owns (another user's sockets): not searched again
    /// while they exist.
    unowned: HashSet<u64>,
}

impl Default for OsPlatform {
    fn default() -> Self {
        // SAFETY: sysconf has no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        OsPlatform {
            page: if page > 0 { page as u64 } else { 4096 },
            text: String::with_capacity(64 * 1024),
            rows: Vec::with_capacity(256),
            inode_owner: HashMap::new(),
            unowned: HashSet::new(),
        }
    }
}

impl OsPlatform {
    fn read_text(&mut self, path: &str) -> std::io::Result<()> {
        self.text.clear();
        fs::File::open(path)?
            .read_to_string(&mut self.text)
            .map(|_| ())
    }

    /// Records every socket inode held by `pid`, returning them.
    fn scan_fds(&mut self, pid: u32, found: &mut Vec<u64>) {
        let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fd")) else {
            return;
        };
        for entry in dir.flatten() {
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            let t = target.as_os_str().as_encoded_bytes();
            if let Some(n) = t
                .strip_prefix(b"socket:[")
                .and_then(|s| s.strip_suffix(b"]"))
            {
                if let Some(inode) = std::str::from_utf8(n).ok().and_then(|n| n.parse().ok()) {
                    self.inode_owner.insert(inode, pid);
                    found.push(inode);
                }
            }
        }
    }
}

impl Platform for OsPlatform {
    fn cmdline(&mut self, key: ProcessKey) -> Result<Arc<[Box<str>]>> {
        let raw = fs::read(format!("/proc/{}/cmdline", key.pid))?;
        Ok(parse::split_nul_cmdline(&raw).into())
    }

    fn cwd(&mut self, key: ProcessKey) -> Result<PathBuf> {
        Ok(fs::read_link(format!("/proc/{}/cwd", key.pid))?)
    }

    fn footprint(&mut self, key: ProcessKey) -> Result<Footprint> {
        let smaps = self
            .read_text(&format!("/proc/{}/smaps_rollup", key.pid))
            .is_ok();
        let pss = if smaps {
            parse::parse_smaps_rollup_pss(&self.text)
        } else {
            None
        };
        if let Some(bytes) = pss {
            return Ok(Footprint {
                bytes,
                metric: crate::procmon::snapshot::FootprintMetric::Pss,
            });
        }
        self.read_text(&format!("/proc/{}/statm", key.pid))?;
        parse::linux_footprint(None, Some(&self.text), self.page)
            .ok_or_else(|| PlatformError::Other("unreadable statm".into()))
    }

    fn tcp_table(&mut self, q: &TcpQuery<'_>, out: &mut Vec<RawConn>) -> Result<()> {
        self.rows.clear();
        for file in ["/proc/net/tcp", "/proc/net/tcp6"] {
            if self.read_text(file).is_ok() {
                let text = std::mem::take(&mut self.text);
                parse::parse_proc_net_tcp(&text, &mut self.rows);
                self.text = text;
            }
        }
        let live: HashSet<u64> = self.rows.iter().map(|r| r.owner).collect();
        self.inode_owner.retain(|inode, _| live.contains(inode));
        self.unowned.retain(|inode| live.contains(inode));

        // Servers are few: learn their sockets directly, which gives their listening ports.
        let mut scratch = Vec::new();
        for &pid in q.servers {
            self.scan_fds(pid, &mut scratch);
        }
        let server_ports: HashSet<u16> = self
            .rows
            .iter()
            .filter(|r| r.state == ConnState::Listen)
            .filter(|r| {
                self.inode_owner
                    .get(&r.owner)
                    .is_some_and(|p| q.servers.contains(p))
            })
            .map(|r| r.local.port())
            .collect();

        // Established rows that could matter; only their inodes need an owner.
        let wanted = |r: &TcpRow| match r.state {
            ConnState::Listen => server_ports.contains(&r.local.port()),
            ConnState::Established => {
                (r.remote.port() == 443 && (q.is_provider)(r.remote.ip()))
                    || (parse::is_local(r.local, r.remote)
                        && (server_ports.contains(&r.remote.port())
                            || server_ports.contains(&r.local.port())))
            }
            ConnState::Other => false,
        };
        let mut pending: HashSet<u64> = self
            .rows
            .iter()
            .filter(|r| wanted(r))
            .map(|r| r.owner)
            .filter(|i| !self.inode_owner.contains_key(i) && !self.unowned.contains(i))
            .collect();
        if !pending.is_empty() {
            for &pid in q.candidates {
                scratch.clear();
                self.scan_fds(pid, &mut scratch);
                for i in &scratch {
                    pending.remove(i);
                }
                if pending.is_empty() {
                    break;
                }
            }
            self.unowned.extend(pending);
        }

        let with_pid: Vec<(u32, TcpRow)> = self
            .rows
            .iter()
            .filter(|r| wanted(r))
            .filter_map(|r| self.inode_owner.get(&r.owner).map(|&pid| (pid, *r)))
            .collect();
        parse::select(&with_pid, q, out);
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
