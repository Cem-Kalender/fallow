//! Process enumeration: PID, start time, parent, name, owner and CPU for every process.
//!
//! - Linux reads `/proc` directly: one `/proc/<pid>/stat` read per process per refresh and the
//!   owner once per process. sysinfo 0.39 also reads `/proc/<pid>/status` for every process on
//!   every refresh, which put the refresh over its 2% CPU budget at 600 processes (T117).
//! - macOS and Windows use sysinfo with only the fields R-1 allows: `cmd`, `cwd`, `environ`,
//!   `exe` and `memory` are never requested.

use compact_str::CompactString;

use super::snapshot::{ProcessKey, Uid};
use super::worker::ProcSample;

/// Takes one sample of all processes plus total and available RAM.
pub trait Sample {
    fn sample(&mut self) -> (Vec<ProcSample>, u64, u64);
}

#[cfg(target_os = "linux")]
pub use linux::Sampler;
#[cfg(not(target_os = "linux"))]
pub use other::Sampler;

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    use crate::procmon::platform::read_proc;
    use std::time::Instant;

    use super::*;

    /// Fields of `/proc/<pid>/stat` Fallow uses.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Stat {
        pub name: CompactString,
        pub ppid: u32,
        /// utime + stime, clock ticks.
        pub cpu_ticks: u64,
        /// Start time, clock ticks since boot.
        pub start_ticks: u64,
        /// The kernel marks kernel threads with PF_KTHREAD; they have no user process.
        pub kernel_thread: bool,
    }

    /// Parses `/proc/<pid>/stat`. The name is in parentheses and may contain spaces or
    /// parentheses, so the fields are counted from the last `)`.
    pub fn parse_stat(text: &str) -> Option<Stat> {
        let open = text.find('(')?;
        let close = text.rfind(')')?;
        let name = CompactString::from(text.get(open + 1..close)?);
        let f: Vec<&str> = text.get(close + 2..)?.split_whitespace().collect();
        // f[0] is field 3 (state).
        let ppid = f.get(1)?.parse().ok()?;
        let flags: u64 = f.get(6)?.parse().ok()?;
        let utime: u64 = f.get(11)?.parse().ok()?;
        let stime: u64 = f.get(12)?.parse().ok()?;
        let start_ticks = f.get(19)?.parse().ok()?;
        Some(Stat {
            name,
            ppid,
            cpu_ticks: utime + stime,
            start_ticks,
            kernel_thread: flags & 0x0020_0000 != 0,
        })
    }

    pub struct Sampler {
        hz: u64,
        boot_time: u64,
        buf: String,
        last: Option<Instant>,
        /// (pid, start ticks) → (cpu ticks at the last sample, owner).
        known: HashMap<(u32, u64), (u64, Option<Uid>)>,
    }

    impl Default for Sampler {
        fn default() -> Self {
            // SAFETY: sysconf has no preconditions.
            let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
            let boot_time = fs::read_to_string("/proc/stat")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())
                })
                .unwrap_or(0);
            Sampler {
                hz: if hz > 0 { hz as u64 } else { 100 },
                boot_time,
                buf: String::with_capacity(512),
                last: None,
                known: HashMap::new(),
            }
        }
    }

    impl Sample for Sampler {
        fn sample(&mut self) -> (Vec<ProcSample>, u64, u64) {
            let now = Instant::now();
            let elapsed = self.last.map(|t| now.duration_since(t).as_secs_f64());
            self.last = Some(now);
            let mut out = Vec::with_capacity(self.known.len() + 16);
            let mut seen = HashMap::with_capacity(self.known.len() + 16);
            if let Ok(dir) = fs::read_dir("/proc") {
                for e in dir.flatten() {
                    let name = e.file_name();
                    let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
                        continue;
                    };
                    self.buf.clear();
                    let read = read_proc(&format!("/proc/{pid}/stat"), &mut self.buf);
                    if read.is_err() {
                        continue;
                    }
                    let Some(st) = parse_stat(&self.buf) else {
                        continue;
                    };
                    if st.kernel_thread {
                        continue;
                    }
                    let id = (pid, st.start_ticks);
                    let (prev, owner) = match self.known.remove(&id) {
                        Some((ticks, owner)) => (Some(ticks), owner),
                        None => {
                            let owner = fs::metadata(format!("/proc/{pid}"))
                                .ok()
                                .map(|m| Uid(CompactString::from(m.uid().to_string())));
                            (None, owner)
                        }
                    };
                    let cpu = match (prev, elapsed) {
                        (Some(p), Some(dt)) if dt > 0.0 => {
                            st.cpu_ticks.saturating_sub(p) as f64 / self.hz as f64 / dt * 100.0
                        }
                        _ => 0.0,
                    } as f32;
                    seen.insert(id, (st.cpu_ticks, owner.clone()));
                    out.push(ProcSample {
                        key: ProcessKey {
                            pid,
                            start_time: self.boot_time + st.start_ticks / self.hz,
                        },
                        parent_pid: (st.ppid != 0).then_some(st.ppid),
                        name: st.name,
                        owner,
                        cpu,
                    });
                }
            }
            self.known = seen;
            let (total, avail) = meminfo(&mut self.buf);
            (out, total, avail)
        }
    }

    fn meminfo(buf: &mut String) -> (u64, u64) {
        if read_proc("/proc/meminfo", buf).is_err() {
            return (0, 0);
        }
        let kb = |key: &str| -> u64 {
            buf.lines()
                .find_map(|l| {
                    l.strip_prefix(key)?
                        .trim()
                        .strip_suffix("kB")?
                        .trim()
                        .parse::<u64>()
                        .ok()
                })
                .unwrap_or(0)
                * 1024
        };
        (kb("MemTotal:"), kb("MemAvailable:"))
    }

    #[cfg(test)]
    mod tests {
        use super::parse_stat;

        #[test]
        fn stat_with_odd_names() {
            let line = "4242 (my (odd) name) S 1 4242 4242 0 -1 4194560 100 0 0 0 7 3 0 0 20 0 1 0 98765 1000 50 0";
            let s = parse_stat(line).unwrap();
            assert_eq!(s.name, "my (odd) name");
            assert_eq!(
                (s.ppid, s.cpu_ticks, s.start_ticks, s.kernel_thread),
                (1, 10, 98765, false)
            );
            let kthread = "2 (kthreadd) S 0 0 0 0 -1 2129984 0 0 0 0 0 0 0 0 20 0 1 0 1 0 0";
            assert!(parse_stat(kthread).unwrap().kernel_thread);
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod other {
    use sysinfo::{MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    use super::*;

    pub struct Sampler {
        sys: System,
    }

    impl Default for Sampler {
        fn default() -> Self {
            Sampler { sys: System::new() }
        }
    }

    /// Only the fields R-1 allows.
    fn refresh_kind() -> ProcessRefreshKind {
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_user(UpdateKind::OnlyIfNotSet)
            .without_tasks()
    }

    impl Sample for Sampler {
        fn sample(&mut self) -> (Vec<ProcSample>, u64, u64) {
            let sys = &mut self.sys;
            sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
            sys.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
            let procs = sys
                .processes()
                .iter()
                .filter(|(_, p)| p.thread_kind().is_none())
                .map(|(pid, p)| ProcSample {
                    key: ProcessKey {
                        pid: pid.as_u32(),
                        start_time: p.start_time(),
                    },
                    parent_pid: p.parent().map(|pp| pp.as_u32()),
                    name: CompactString::from(p.name().to_string_lossy().as_ref()),
                    owner: p.user_id().map(|u| Uid(CompactString::from(u.to_string()))),
                    cpu: p.cpu_usage(),
                })
                .collect();
            (procs, sys.total_memory(), sys.available_memory())
        }
    }
}
