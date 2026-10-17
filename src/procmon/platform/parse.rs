//! Pure parsers and decision rules for the platform layer. Nothing here makes a system call,
//! so every platform's parsing is tested on every host (T030, T121).

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::procmon::snapshot::{ConnState, CwdState, Footprint, FootprintMetric};

/// One TCP socket as the OS reports it. `owner` is a PID (macOS, Windows) or a socket inode
/// (Linux, resolved to a PID later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpRow {
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: ConnState,
    pub owner: u64,
}

/// A selected connection with its owning PID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawConn {
    pub pid: u32,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: ConnState,
}

/// What a TCP table query is looking for (data-model.md, "Connection").
pub struct TcpQuery<'a> {
    /// PIDs whose connections may be reported (the visible processes).
    pub candidates: &'a HashSet<u32>,
    /// PIDs of detected local model servers.
    pub servers: &'a HashSet<u32>,
    /// Whether an address belongs to the provider set.
    pub is_provider: &'a dyn Fn(IpAddr) -> bool,
}

/// True for loopback, or for a connection to one of this machine's own addresses (both ends
/// on the same address).
pub fn is_local(local: SocketAddr, remote: SocketAddr) -> bool {
    remote.ip().is_loopback() || remote.ip() == local.ip() || is_mapped_loopback(remote.ip())
}

fn is_mapped_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
        IpAddr::V4(_) => false,
    }
}

/// Keeps only the connections that can matter, for rows whose owner is already a PID:
/// - established to port 443 of a provider address;
/// - listening sockets of local model servers;
/// - established loopback/local connections to or from those listening ports.
pub fn select(rows: &[(u32, TcpRow)], q: &TcpQuery<'_>, out: &mut Vec<RawConn>) {
    let server_ports: HashSet<u16> = rows
        .iter()
        .filter(|(pid, r)| r.state == ConnState::Listen && q.servers.contains(pid))
        .map(|(_, r)| r.local.port())
        .collect();
    for &(pid, r) in rows {
        if !q.candidates.contains(&pid) {
            continue;
        }
        let keep = match r.state {
            ConnState::Listen => q.servers.contains(&pid),
            ConnState::Established => {
                (r.remote.port() == 443 && (q.is_provider)(r.remote.ip()))
                    || (is_local(r.local, r.remote)
                        && (server_ports.contains(&r.remote.port())
                            || server_ports.contains(&r.local.port())))
            }
            ConnState::Other => false,
        };
        if keep {
            out.push(RawConn {
                pid,
                local: r.local,
                remote: r.remote,
                state: r.state,
            });
        }
    }
}

// ---- Linux -----------------------------------------------------------------------------------

/// Parses `/proc/net/tcp` or `/proc/net/tcp6`, keeping Established and Listen rows. `owner`
/// is the socket inode.
pub fn parse_proc_net_tcp(text: &str, out: &mut Vec<TcpRow>) {
    for line in text.lines().skip(1) {
        let mut f = line.split_whitespace();
        let (Some(_sl), Some(local), Some(remote), Some(st)) =
            (f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        let state = match st {
            "01" => ConnState::Established,
            "0A" => ConnState::Listen,
            _ => continue,
        };
        // tx_queue:rx_queue, tr:tm->when, retrnsmt, uid, timeout, inode
        let Some(inode) = f.nth(5).and_then(|i| i.parse::<u64>().ok()) else {
            continue;
        };
        let (Some(local), Some(remote)) = (proc_net_addr(local), proc_net_addr(remote)) else {
            continue;
        };
        out.push(TcpRow {
            local,
            remote,
            state,
            owner: inode,
        });
    }
}

/// `0100007F:1F90` (IPv4) or 32 hex digits plus port (IPv6). The kernel prints each 32-bit word
/// of the network-order address as a native integer, so the bytes come back with `to_ne_bytes`.
fn proc_net_addr(s: &str) -> Option<SocketAddr> {
    let (addr, port) = s.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let word = |h: &str| u32::from_str_radix(h, 16).ok().map(u32::to_ne_bytes);
    let ip = match addr.len() {
        8 => IpAddr::V4(Ipv4Addr::from(word(addr)?)),
        32 => {
            let mut b = [0u8; 16];
            for i in 0..4 {
                b[i * 4..i * 4 + 4].copy_from_slice(&word(addr.get(i * 8..i * 8 + 8)?)?);
            }
            IpAddr::V6(Ipv6Addr::from(b))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// The `Pss:` line of `/proc/<pid>/smaps_rollup`, in bytes.
pub fn parse_smaps_rollup_pss(text: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let kb = l.strip_prefix("Pss:")?.trim().strip_suffix("kB")?.trim();
        kb.parse::<u64>().ok().map(|k| k * 1024)
    })
}

/// Resident pages from `/proc/<pid>/statm` (second field).
pub fn parse_statm_resident(text: &str) -> Option<u64> {
    text.split_whitespace().nth(1)?.parse().ok()
}

/// Splits `/proc/<pid>/cmdline`. Arguments are NUL-terminated; empty arguments in the middle
/// are kept, the final terminator is not an argument.
pub fn split_nul_cmdline(raw: &[u8]) -> Vec<Box<str>> {
    if raw.is_empty() {
        return Vec::new(); // kernel threads
    }
    let body = raw.strip_suffix(&[0]).unwrap_or(raw);
    body.split(|b| *b == 0)
        .map(|a| String::from_utf8_lossy(a).into())
        .collect()
}

/// Linux footprint policy (R-3): PSS from `smaps_rollup`, otherwise resident pages × page size
/// marked `Rss`.
pub fn linux_footprint(
    smaps_rollup: Option<&str>,
    statm: Option<&str>,
    page: u64,
) -> Option<Footprint> {
    if let Some(bytes) = smaps_rollup.and_then(parse_smaps_rollup_pss) {
        return Some(Footprint {
            bytes,
            metric: FootprintMetric::Pss,
        });
    }
    let pages = statm.and_then(parse_statm_resident)?;
    Some(Footprint {
        bytes: pages * page,
        metric: FootprintMetric::Rss,
    })
}

// ---- macOS -----------------------------------------------------------------------------------

/// Parses a KERN_PROCARGS2 buffer: `argc`, the exec path, NUL padding, then argv, then envp and
/// the Apple strings, which are never looked at (R-2).
///
/// The exec path is padded with NULs to an 8-byte boundary (xnu `exec_extract_strings`). Only
/// that padding is skipped: any further NUL is an empty argument. If the padding rule were
/// wrong, the result would be cut short; it can never run past argv into the environment.
pub fn parse_procargs2(buf: &[u8]) -> Option<Vec<Box<str>>> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    if argc < 0 {
        return None;
    }
    let strings = &buf[4..];
    let path_end = strings.iter().position(|b| *b == 0)?;
    let aligned = (path_end + 1).div_ceil(8) * 8;
    let mut i = path_end + 1;
    while i < aligned.min(strings.len()) && strings[i] == 0 {
        i += 1;
    }
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        if i >= strings.len() {
            break; // truncated buffer
        }
        let len = strings[i..]
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(strings.len() - i);
        args.push(String::from_utf8_lossy(&strings[i..i + len]).into());
        i += len + 1;
    }
    Some(args)
}

/// Offsets in xnu's `struct socket_fdinfo` (bsd/sys/proc_info.h), validated in spike S-3:
/// `proc_fileinfo` (24 bytes), then `socket_info` with `soi_kind` at +232 and the protocol union
/// at +240. In `tcp_sockinfo`, `in_sockinfo` comes first (80 bytes: fport +0, lport +4,
/// vflag +24, faddr +32, laddr +48), then `tcpsi_state`.
pub mod socket_fdinfo {
    pub const SOI_KIND: usize = 24 + 232;
    pub const PROTO: usize = 24 + 240;
    pub const FPORT: usize = PROTO;
    pub const LPORT: usize = PROTO + 4;
    pub const VFLAG: usize = PROTO + 24;
    pub const FADDR: usize = PROTO + 32;
    pub const LADDR: usize = PROTO + 48;
    pub const TCP_STATE: usize = PROTO + 80;
    pub const MIN_LEN: usize = TCP_STATE + 4;
    pub const SOCKINFO_TCP: i32 = 2;
    pub const INI_IPV4: u8 = 0x1;
    pub const INI_IPV6: u8 = 0x2;
    pub const TSI_S_LISTEN: i32 = 1;
    pub const TSI_S_ESTABLISHED: i32 = 4;
}

/// Reads a TCP socket from a `socket_fdinfo` byte image. `None` for non-TCP sockets.
pub fn parse_socket_fdinfo(b: &[u8], pid: u32) -> Option<TcpRow> {
    use socket_fdinfo as o;
    if b.len() < o::MIN_LEN {
        return None;
    }
    let i32_at = |off: usize| i32::from_ne_bytes(b[off..off + 4].try_into().unwrap());
    if i32_at(o::SOI_KIND) != o::SOCKINFO_TCP {
        return None;
    }
    // Ports are in_port_t (network order) stored in an int.
    let port = |off: usize| u16::from_be_bytes([b[off], b[off + 1]]);
    let addr = |off: usize| -> Option<IpAddr> {
        let a: [u8; 16] = b[off..off + 16].try_into().ok()?;
        match b[o::VFLAG] {
            v if v & o::INI_IPV6 != 0 => Some(IpAddr::V6(Ipv6Addr::from(a))),
            v if v & o::INI_IPV4 != 0 => {
                Some(IpAddr::V4(Ipv4Addr::new(a[12], a[13], a[14], a[15])))
            }
            _ => None,
        }
    };
    let state = match i32_at(o::TCP_STATE) {
        o::TSI_S_ESTABLISHED => ConnState::Established,
        o::TSI_S_LISTEN => ConnState::Listen,
        _ => ConnState::Other,
    };
    Some(TcpRow {
        local: SocketAddr::new(addr(o::LADDR)?, port(o::LPORT)),
        remote: SocketAddr::new(addr(o::FADDR)?, port(o::FPORT)),
        state,
        owner: pid as u64,
    })
}

/// macOS footprint policy (R-3): `ri_phys_footprint`, or no figure with a reason. Never a
/// different metric.
pub fn macos_footprint(phys_footprint: Option<u64>) -> Result<Footprint, &'static str> {
    phys_footprint
        .map(|bytes| Footprint {
            bytes,
            metric: FootprintMetric::PhysFootprint,
        })
        .ok_or("proc_pid_rusage failed")
}

// ---- Windows ---------------------------------------------------------------------------------

const MIB_TCP_STATE_LISTEN: u32 = 2;
const MIB_TCP_STATE_ESTAB: u32 = 5;

fn win_state(s: u32) -> ConnState {
    match s {
        MIB_TCP_STATE_ESTAB => ConnState::Established,
        MIB_TCP_STATE_LISTEN => ConnState::Listen,
        _ => ConnState::Other,
    }
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// A port stored in network order in the low 16 bits of a DWORD.
fn win_port(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(off)?, *b.get(off + 1)?]))
}

/// Parses a `MIB_TCPTABLE_OWNER_PID` (AF_INET) buffer: `dwNumEntries`, then 24-byte rows of
/// state, local addr, local port, remote addr, remote port, owning PID.
pub fn parse_tcp_table_v4(b: &[u8], out: &mut Vec<TcpRow>) -> Option<()> {
    let n = u32_at(b, 0)? as usize;
    for i in 0..n {
        let r = 4 + i * 24;
        let addr = |off: usize| -> Option<Ipv4Addr> {
            let a: [u8; 4] = b.get(r + off..r + off + 4)?.try_into().ok()?;
            Some(Ipv4Addr::from(a))
        };
        out.push(TcpRow {
            state: win_state(u32_at(b, r)?),
            local: SocketAddr::new(IpAddr::V4(addr(4)?), win_port(b, r + 8)?),
            remote: SocketAddr::new(IpAddr::V4(addr(12)?), win_port(b, r + 16)?),
            owner: u32_at(b, r + 20)? as u64,
        });
    }
    Some(())
}

/// Parses a `MIB_TCP6TABLE_OWNER_PID` (AF_INET6) buffer: `dwNumEntries`, then 56-byte rows of
/// local addr[16], local scope, local port, remote addr[16], remote scope, remote port, state,
/// owning PID.
pub fn parse_tcp_table_v6(b: &[u8], out: &mut Vec<TcpRow>) -> Option<()> {
    let n = u32_at(b, 0)? as usize;
    for i in 0..n {
        let r = 4 + i * 56;
        let addr = |off: usize| -> Option<Ipv6Addr> {
            let a: [u8; 16] = b.get(r + off..r + off + 16)?.try_into().ok()?;
            Some(Ipv6Addr::from(a))
        };
        out.push(TcpRow {
            local: SocketAddr::new(IpAddr::V6(addr(0)?), win_port(b, r + 20)?),
            remote: SocketAddr::new(IpAddr::V6(addr(24)?), win_port(b, r + 44)?),
            state: win_state(u32_at(b, r + 48)?),
            owner: u32_at(b, r + 52)? as u64,
        });
    }
    Some(())
}

/// Windows footprint policy (R-3): private working set from `PROCESS_MEMORY_COUNTERS_EX2`
/// (Windows 10 1809+), otherwise the working set, labelled as such.
pub fn windows_footprint(
    private_working_set: Option<u64>,
    working_set: Option<u64>,
) -> Option<Footprint> {
    match (private_working_set, working_set) {
        (Some(bytes), _) => Some(Footprint {
            bytes,
            metric: FootprintMetric::PrivateWorkingSet,
        }),
        (None, Some(bytes)) => Some(Footprint {
            bytes,
            metric: FootprintMetric::WorkingSet,
        }),
        (None, None) => None,
    }
}

/// Windows working-directory result (R-10): a failed or skipped PEB read is `Unavailable`
/// with its reason.
pub fn windows_cwd(read: Result<String, &str>) -> CwdState {
    match read {
        Ok(path) => CwdState::Known(path.trim_end_matches('\\').to_string().into()),
        Err(reason) => CwdState::Unavailable(reason.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_rules() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let row = |l: &str, r: &str, state| TcpRow {
            local: a(l),
            remote: a(r),
            state,
            owner: 0,
        };
        let rows = vec![
            (
                1,
                row("10.0.0.2:5000", "160.79.104.10:443", ConnState::Established),
            ), // provider
            (
                1,
                row("10.0.0.2:5001", "8.8.8.8:443", ConnState::Established),
            ), // not provider
            (
                1,
                row("10.0.0.2:5002", "160.79.104.10:80", ConnState::Established),
            ), // wrong port
            (2, row("127.0.0.1:11434", "0.0.0.0:0", ConnState::Listen)), // server
            (
                3,
                row("127.0.0.1:40000", "127.0.0.1:11434", ConnState::Established),
            ), // client
            (
                2,
                row("127.0.0.1:11434", "127.0.0.1:40000", ConnState::Established),
            ), // serving
            (3, row("0.0.0.0:22", "0.0.0.0:0", ConnState::Listen)),      // not a server
            (
                9,
                row("10.0.0.2:5003", "160.79.104.10:443", ConnState::Established),
            ), // not visible
        ];
        let candidates: HashSet<u32> = [1, 2, 3].into();
        let servers: HashSet<u32> = [2].into();
        let is_provider = |ip: IpAddr| ip.to_string().starts_with("160.79.10");
        let q = TcpQuery {
            candidates: &candidates,
            servers: &servers,
            is_provider: &is_provider,
        };
        let mut out = Vec::new();
        select(&rows, &q, &mut out);
        let kept: Vec<(u32, u16, u16)> = out
            .iter()
            .map(|c| (c.pid, c.local.port(), c.remote.port()))
            .collect();
        assert_eq!(
            kept,
            [
                (1, 5000, 443),
                (2, 11434, 0),
                (3, 40000, 11434),
                (2, 11434, 40000)
            ]
        );
    }

    #[test]
    fn procargs_padding_and_empty_args() {
        // argc=3, path "/bin/x" + NUL padded to 8, argv "a", "", "c", then envp.
        let mut b = 3i32.to_ne_bytes().to_vec();
        b.extend_from_slice(b"/bin/x\0\0a\0\0c\0SECRET=1\0");
        assert_eq!(
            parse_procargs2(&b).unwrap(),
            vec!["a".into(), "".into(), "c".into()] as Vec<Box<str>>
        );
        // A leading empty argument right after the padding is kept as an argument.
        let mut b = 2i32.to_ne_bytes().to_vec();
        b.extend_from_slice(b"/bin/x\0\0\0b\0SECRET=1\0");
        assert_eq!(
            parse_procargs2(&b).unwrap(),
            vec!["".into(), "b".into()] as Vec<Box<str>>
        );
    }
}
