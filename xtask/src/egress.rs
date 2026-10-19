//! `cargo xtask egress` (001 T051, SC-009, FR-035, FR-022a, Constitution VII; 002 T050).
//!
//! Fallow runs a full session twice in a pseudo-terminal: the agent workspace, an agent session
//! with keys typed into it, the editor view, then quit. The first run resolves provider
//! hostnames; the second runs with `--no-dns`. The only traffic allowed is DNS queries (A/AAAA,
//! never PTR) for hostnames in the built-in `providers.toml`, and with `--no-dns` none at all.
//!
//! How each platform sees Fallow's traffic:
//! - Linux: Fallow runs under `strace -f -e trace=network`, so every connect and send of Fallow
//!   and its children is recorded, with DNS question names decoded from the sent bytes.
//! - Windows: Fallow's own TCP and UDP sockets are polled every 100 ms (`GetExtendedTcpTable`,
//!   `GetExtendedUdpTable`), and the DNS-Client operational log is read for queries made by its
//!   PID (the log is switched on for the run; that needs an administrator, as on CI).
//! - macOS: Fallow's sockets are polled with `lsof` every 200 ms; name resolution goes through
//!   mDNSResponder, so `tcpdump -k` on `pktap` attributes DNS packets to Fallow when macOS marks
//!   them with Fallow as the effective process. Names that macOS does not attribute are reported,
//!   not judged.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::fixtures;
use crate::pty::{self, Session};

const PROVIDERS: &str = include_str!("../../assets/defaults/providers.toml");
const LEAVE: &[u8] = b"\x1d";
const CTRL_E: &[u8] = b"\x05";
/// How long each run keeps Fallow busy.
const RUN_FOR: Duration = Duration::from_secs(15);

/// What one run showed.
#[derive(Debug, Default)]
struct Seen {
    /// DNS questions: (name, type).
    queries: BTreeSet<(String, String)>,
    /// Anything that is not an allowed DNS query.
    violations: Vec<String>,
    /// Facts worth printing that are not violations (local sockets, unattributed names).
    notes: Vec<String>,
    /// The capture worked (for example the event log could be read).
    capture_ok: bool,
}

pub fn run(args: &[String]) -> i32 {
    let mut bin = pty::default_binary();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--bin", Some(v)) => bin = PathBuf::from(v),
            _ => {
                eprintln!("usage: cargo xtask egress [--bin <path>]");
                return 2;
            }
        }
    }
    let bin = std::path::absolute(&bin).unwrap_or(bin);
    let hosts = provider_hosts();
    let base = fixtures::temp_dir("egress");
    let result = (|| -> Result<(Seen, Seen), String> {
        let config = setup(&base)?;
        let dns = one_run(&bin, &base, &config, false)?;
        let none = one_run(&bin, &base, &config, true)?;
        Ok((dns, none))
    })();
    let _ = std::fs::remove_dir_all(&base);
    let (mut dns, mut none) = match result {
        Ok(v) => v,
        Err(e) => {
            println!("invalid: {e}");
            println!("os={} egress result=INVALID", std::env::consts::OS);
            return 2;
        }
    };

    // Judge the names: provider hostnames only, A or AAAA only.
    for (name, kind) in &dns.queries {
        let host = name.trim_end_matches('.').to_ascii_lowercase();
        if !hosts.contains(&host) {
            dns.violations
                .push(format!("DNS query for {host} (not a provider hostname)"));
        }
        if kind != "A" && kind != "AAAA" {
            dns.violations
                .push(format!("DNS query of type {kind} for {host}"));
        }
    }
    for (name, kind) in &none.queries {
        none.violations
            .push(format!("DNS query {kind} {name} with --no-dns"));
    }

    let mut invalid = Vec::new();
    if !dns.capture_ok || !none.capture_ok {
        invalid.push("the capture did not work (see notes)".to_string());
    }
    // Linux and Windows see the queries themselves; seeing none means the capture missed them.
    if !cfg!(target_os = "macos") && dns.queries.is_empty() {
        invalid.push("no DNS query seen in the run with DNS on".to_string());
    }
    for (label, s) in [("dns", &dns), ("no-dns", &none)] {
        for (name, kind) in &s.queries {
            println!("{label}: query {kind} {name}");
        }
        for n in &s.notes {
            println!("{label}: note: {n}");
        }
        for v in &s.violations {
            println!("{label}: VIOLATION: {v}");
        }
    }
    let violations = dns.violations.len() + none.violations.len();
    let result = if violations > 0 {
        "FAIL"
    } else if !invalid.is_empty() {
        "INVALID"
    } else {
        "PASS"
    };
    for i in &invalid {
        println!("invalid: {i}");
    }
    println!(
        "os={} egress dns_queries={} violations={violations} result={result}",
        std::env::consts::OS,
        dns.queries.len()
    );
    pty::step_summary(&format!(
        "### Egress (SC-009, {})\n\n| run | DNS queries | violations |\n|---|---|---|\n| DNS on | {} | {} |\n| `--no-dns` | {} | {} |\n\n**{result}**\n",
        std::env::consts::OS,
        dns.queries.len(),
        dns.violations.len(),
        none.queries.len(),
        none.violations.len()
    ));
    match result {
        "PASS" => 0,
        "FAIL" => 1,
        _ => 2,
    }
}

/// The hostnames in the built-in providers file.
fn provider_hosts() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in PROVIDERS.lines() {
        let l = line.trim();
        if !l.starts_with("hostnames") {
            continue;
        }
        for part in l.split('"').skip(1).step_by(2) {
            out.insert(part.to_ascii_lowercase());
        }
    }
    out
}

/// A config directory whose only agent profile is the echo stand-in, and a project folder.
fn setup(base: &Path) -> Result<PathBuf, String> {
    let config = base.join("config");
    std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(base.join("project")).map_err(|e| e.to_string())?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut profiles = String::new();
    for id in ["claude", "codex", "gemini", "aider", "shell"] {
        profiles.push_str(&format!("[[profile]]\nid = \"{id}\"\nenabled = false\n\n"));
    }
    profiles.push_str(&format!(
        "[[profile]]\nid = \"echo\"\nname = \"echo\"\ncommand = {:?}\nargs = [\"echo-agent\"]\n",
        me.display().to_string()
    ));
    std::fs::write(config.join("profiles.toml"), profiles).map_err(|e| e.to_string())?;
    Ok(config)
}

/// One full session; returns what the platform capture saw.
fn one_run(bin: &Path, base: &Path, config: &Path, no_dns: bool) -> Result<Seen, String> {
    let cfg = config.display().to_string();
    let proj = base.join("project").display().to_string();
    let mut args = vec!["--config-dir", cfg.as_str(), proj.as_str()];
    if no_dns {
        args.push("--no-dns");
    }
    let label = if no_dns { "--no-dns" } else { "DNS on" };
    println!("egress: run with {label}");
    let capture = Capture::start(base, no_dns)?;
    let mut s = match &capture.wrapper {
        Some(w) => {
            let w: Vec<&str> = w.iter().map(String::as_str).collect();
            Session::spawn_wrapped(&w, bin, &args, Duration::from_secs(1))?
        }
        None => Session::spawn(bin, &args, Duration::from_secs(1))?,
    };
    let poller = capture.watch_pid(s.pid);
    let started = std::time::Instant::now();
    let steps = (|| {
        if s.wait_frame(Duration::from_secs(20)).is_none() {
            return Err("no first frame".to_string());
        }
        // Give the DNS thread its first pass.
        std::thread::sleep(Duration::from_secs(3));
        s.expect(b"echo-ready");
        s.send(b"n");
        std::thread::sleep(Duration::from_millis(400));
        s.send(b"\r");
        s.seen(Duration::from_secs(30))
            .ok_or("the session never started")?;
        for _ in 0..10 {
            s.send(b"k");
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_millis(700));
        s.send(LEAVE);
        std::thread::sleep(Duration::from_millis(700));
        s.send(CTRL_E);
        std::thread::sleep(Duration::from_millis(500));
        s.send(CTRL_E);
        if let Some(rest) = RUN_FOR.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
        Ok(())
    })();
    s.quit_with_sessions();
    let outcome = s.finish();
    let fallow_pid = outcome.stats.map_or(outcome.pid, |st| st.pid);
    if let Some(p) = poller {
        p.stop.store(true, Ordering::Relaxed);
        let _ = p.handle.join();
    }
    steps?;
    capture.finish(fallow_pid)
}

struct Poller {
    stop: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

/// The platform's way of seeing Fallow's traffic during one run.
struct Capture {
    dir: PathBuf,
    no_dns: bool,
    /// Linux: run Fallow under strace.
    wrapper: Option<Vec<String>>,
    /// Socket rows seen by the poller (Windows, macOS).
    sockets: Arc<std::sync::Mutex<BTreeSet<String>>>,
    /// macOS: the tcpdump child.
    tcpdump: Option<std::process::Child>,
}

impl Capture {
    fn start(base: &Path, no_dns: bool) -> Result<Capture, String> {
        let dir = base.join(if no_dns { "cap-nodns" } else { "cap-dns" });
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut c = Capture {
            dir,
            no_dns,
            wrapper: None,
            sockets: Arc::default(),
            tcpdump: None,
        };
        if cfg!(target_os = "linux") {
            let out = c.dir.join("strace.txt");
            if std::process::Command::new("strace")
                .arg("-V")
                .output()
                .is_err()
            {
                return Err("strace is not installed (apt-get install strace)".into());
            }
            c.wrapper = Some(vec![
                "strace".into(),
                "-f".into(),
                "-qq".into(),
                "-e".into(),
                "trace=network".into(),
                "-s".into(),
                "4096".into(),
                "-xx".into(),
                "-o".into(),
                out.display().to_string(),
            ]);
        }
        if cfg!(windows) {
            // The DNS-Client log records queries with the caller's PID. Switching it on needs an
            // administrator; without it the names can't be checked and the run is INVALID.
            let _ = std::process::Command::new("wevtutil")
                .args(["sl", "Microsoft-Windows-DNS-Client/Operational", "/e:true"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        if cfg!(target_os = "macos") {
            let out =
                std::fs::File::create(c.dir.join("tcpdump.txt")).map_err(|e| e.to_string())?;
            let child = std::process::Command::new("sudo")
                .args(["tcpdump", "-i", "pktap,all", "-k", "-n", "-l", "-tt"])
                .stdout(out)
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| format!("tcpdump: {e}"))?;
            c.tcpdump = Some(child);
            std::thread::sleep(Duration::from_secs(2));
        }
        Ok(c)
    }

    /// Windows and macOS: poll the PID's sockets until stopped.
    fn watch_pid(&self, pid: u32) -> Option<Poller> {
        if cfg!(target_os = "linux") {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let sockets = self.sockets.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                for row in sockets_of(pid) {
                    if let Ok(mut s) = sockets.lock() {
                        s.insert(row);
                    }
                }
                std::thread::sleep(Duration::from_millis(if cfg!(windows) { 100 } else { 200 }));
            }
        });
        Some(Poller { stop, handle })
    }

    fn finish(mut self, fallow_pid: u32) -> Result<Seen, String> {
        let mut seen = Seen {
            capture_ok: true,
            ..Seen::default()
        };
        let sockets: Vec<String> = self
            .sockets
            .lock()
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        for row in sockets {
            // On Windows and macOS the system resolver runs in another process, so Fallow
            // itself has no reason to own an internet socket. A UDP socket with DNS on is
            // reported (it would be the resolver running in-process); anything else fails.
            if row.starts_with("UDP") && !self.no_dns {
                seen.notes.push(format!("own socket: {row}"));
            } else {
                seen.violations.push(format!("own socket: {row}"));
            }
        }
        if cfg!(target_os = "linux") {
            let text = std::fs::read_to_string(self.dir.join("strace.txt"))
                .map_err(|e| format!("strace output: {e}"))?;
            judge_strace(&text, &mut seen);
        }
        if cfg!(windows) {
            dns_client_events(fallow_pid, &mut seen);
        }
        if let Some(mut child) = self.tcpdump.take() {
            std::thread::sleep(Duration::from_secs(1));
            let _ = std::process::Command::new("sudo")
                .args(["kill", "-INT", &child.id().to_string()])
                .status();
            let _ = child.wait();
            let text = std::fs::read_to_string(self.dir.join("tcpdump.txt")).unwrap_or_default();
            judge_tcpdump(&text, &mut seen);
        }
        Ok(seen)
    }
}

// ----- Linux: strace -----

/// Judges `strace -f -e trace=network -xx` output. Internet traffic may only go to port 53, and
/// every sent DNS question is recorded.
///
/// glibc's `getaddrinfo` "connects" a UDP socket to each address it found, with port 0, to pick
/// a source address for sorting (RFC 6724); no packet is sent. Such a connect is allowed, but
/// only on a UDP socket and only if nothing is ever sent on it.
fn judge_strace(text: &str, seen: &mut Seen) {
    #[derive(Clone, Copy)]
    struct Sock {
        dgram: bool,
        port: Option<u16>,
    }
    let mut socks: std::collections::HashMap<(String, String), Sock> = Default::default();
    let mut local = BTreeSet::new();
    let mut probes = 0usize;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let tid = words.next().unwrap_or("").to_string();
        let rest = words.collect::<Vec<_>>().join(" ");
        let call = rest.split('(').next().unwrap_or("");
        let fd = rest
            .split_once('(')
            .and_then(|(_, a)| a.split([',', ')']).next())
            .unwrap_or("")
            .trim()
            .to_string();
        let inet = line.contains("sa_family=AF_INET");
        match call {
            "socket" if line.contains("(AF_INET") => {
                if let Some(ret) = line.rsplit("= ").next().map(str::trim) {
                    socks.insert(
                        (tid.clone(), ret.to_string()),
                        Sock {
                            dgram: line.contains("SOCK_DGRAM"),
                            port: None,
                        },
                    );
                }
            }
            "connect" if inet => {
                let port = port_of(line);
                let sock = socks.get(&(tid.clone(), fd.clone())).copied();
                match port {
                    Some(53) => {}
                    Some(0) if sock.is_some_and(|s| s.dgram) => probes += 1,
                    Some(p) => seen
                        .violations
                        .push(format!("connect to port {p}: {}", short(line))),
                    None => seen
                        .violations
                        .push(format!("connect without a port: {}", short(line))),
                }
                if let Some(s) = socks.get_mut(&(tid.clone(), fd.clone())) {
                    s.port = port;
                }
            }
            "connect" if line.contains("sa_family=AF_UNIX") => {
                if let Some(path) = line.split("sun_path=\"").nth(1) {
                    local.insert(decode_hex(path.split('"').next().unwrap_or("")));
                }
            }
            "sendto" | "sendmsg" | "sendmmsg" | "send" => {
                if inet {
                    match port_of(line) {
                        Some(53) => {}
                        p => seen.violations.push(format!(
                            "{call} to port {}: {}",
                            p.map_or("?".into(), |p| p.to_string()),
                            short(line)
                        )),
                    }
                } else if let Some(s) = socks.get(&(tid.clone(), fd.clone())) {
                    // A send on a connected internet socket: it goes where the socket points.
                    if s.port != Some(53) {
                        seen.violations.push(format!(
                            "{call} on a socket connected to port {}: {}",
                            s.port.map_or("?".into(), |p| p.to_string()),
                            short(line)
                        ));
                    }
                }
                for chunk in line.split('"').skip(1).step_by(2) {
                    let bytes = decode_hex_bytes(chunk);
                    if let Some(q) = dns_question(&bytes) {
                        seen.queries.insert(q);
                    }
                }
            }
            _ => {}
        }
    }
    if probes > 0 {
        seen.notes.push(format!(
            "{probes} source-address probes by getaddrinfo (UDP connect to port 0, nothing sent)"
        ));
    }
    for l in local {
        seen.notes
            .push(format!("local socket: {}", String::from_utf8_lossy(&l)));
    }
}

/// `htons(N)` in a strace line.
fn port_of(line: &str) -> Option<u16> {
    line.split("htons(").nth(1)?.split(')').next()?.parse().ok()
}

fn short(line: &str) -> String {
    line.chars().take(200).collect()
}

/// `\x12\x34...` → bytes.
fn decode_hex_bytes(s: &str) -> Vec<u8> {
    s.split("\\x")
        .filter(|h| h.len() >= 2)
        .filter_map(|h| u8::from_str_radix(&h[..2], 16).ok())
        .collect()
}

fn decode_hex(s: &str) -> Vec<u8> {
    decode_hex_bytes(s)
}

/// The question of a DNS query message: (name, type).
fn dns_question(b: &[u8]) -> Option<(String, String)> {
    if b.len() < 17 || b[2] & 0x80 != 0 {
        return None; // too short, or a response
    }
    let qdcount = u16::from_be_bytes([b[4], b[5]]);
    if qdcount != 1 {
        return None;
    }
    let mut i = 12;
    let mut labels = Vec::new();
    loop {
        let len = *b.get(i)? as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len > 63 {
            return None;
        }
        let label = b.get(i + 1..i + 1 + len)?;
        if !label
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
        {
            return None;
        }
        labels.push(String::from_utf8_lossy(label).into_owned());
        i += 1 + len;
    }
    let qtype = u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]);
    let kind = match qtype {
        1 => "A".to_string(),
        28 => "AAAA".to_string(),
        12 => "PTR".to_string(),
        n => format!("type{n}"),
    };
    Some((labels.join("."), kind))
}

// ----- Windows: socket tables and the DNS-Client log -----

#[cfg(windows)]
mod win {
    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        pub fn GetExtendedTcpTable(
            table: *mut core::ffi::c_void,
            size: *mut u32,
            order: i32,
            af: u32,
            class: i32,
            reserved: u32,
        ) -> u32;
        pub fn GetExtendedUdpTable(
            table: *mut core::ffi::c_void,
            size: *mut u32,
            order: i32,
            af: u32,
            class: i32,
            reserved: u32,
        ) -> u32;
    }
    pub const AF_INET: u32 = 2;
    pub const AF_INET6: u32 = 23;
    pub const TCP_TABLE_OWNER_PID_ALL: i32 = 5;
    pub const UDP_TABLE_OWNER_PID: i32 = 1;

    /// The raw table: entry count, then fixed-size rows.
    pub fn table(tcp: bool, af: u32) -> Vec<u8> {
        let mut size = 0u32;
        let call = |buf: *mut core::ffi::c_void, size: &mut u32| unsafe {
            // SAFETY: `buf` is null (size query) or points to `size` writable bytes.
            if tcp {
                GetExtendedTcpTable(buf, size, 0, af, TCP_TABLE_OWNER_PID_ALL, 0)
            } else {
                GetExtendedUdpTable(buf, size, 0, af, UDP_TABLE_OWNER_PID, 0)
            }
        };
        for _ in 0..4 {
            let mut buf = vec![0u8; size as usize + 4096];
            let mut n = buf.len() as u32;
            if call(buf.as_mut_ptr().cast(), &mut n) == 0 {
                buf.truncate(n as usize);
                return buf;
            }
            size = n;
        }
        Vec::new()
    }
}

#[cfg(windows)]
fn sockets_of(pid: u32) -> Vec<String> {
    let u32_at = |b: &[u8], o: usize| -> u32 {
        b.get(o..o + 4)
            .map_or(0, |x| u32::from_ne_bytes([x[0], x[1], x[2], x[3]]))
    };
    let port = |raw: u32| u16::from_be((raw & 0xffff) as u16);
    let mut out = Vec::new();
    // (tcp, address family, row size, pid offset, remote address/port offsets)
    for (tcp, af, row, pid_at) in [
        (true, win::AF_INET, 24, 20),
        (true, win::AF_INET6, 56, 52),
        (false, win::AF_INET, 12, 8),
        (false, win::AF_INET6, 28, 24),
    ] {
        let t = win::table(tcp, af);
        let n = u32_at(&t, 0) as usize;
        for i in 0..n {
            let r = 4 + i * row;
            if u32_at(&t, r + pid_at) != pid {
                continue;
            }
            let desc = match (tcp, af == win::AF_INET) {
                (true, true) => {
                    let ip = u32_at(&t, r + 12).to_ne_bytes();
                    format!(
                        "TCP {}.{}.{}.{}:{}",
                        ip[0],
                        ip[1],
                        ip[2],
                        ip[3],
                        port(u32_at(&t, r + 16))
                    )
                }
                (true, false) => format!("TCP6 remote port {}", port(u32_at(&t, r + 44))),
                (false, true) => format!("UDP local port {}", port(u32_at(&t, r + 4))),
                (false, false) => format!("UDP6 local port {}", port(u32_at(&t, r + 20))),
            };
            out.push(desc);
        }
    }
    out
}

#[cfg(windows)]
fn dns_client_events(pid: u32, seen: &mut Seen) {
    let query = format!("*[System[Execution[@ProcessID='{pid}']]]");
    let out = std::process::Command::new("wevtutil")
        .args([
            "qe",
            "Microsoft-Windows-DNS-Client/Operational",
            &format!("/q:{query}"),
            "/f:xml",
        ])
        .output();
    let Ok(out) = out else {
        seen.capture_ok = false;
        seen.notes.push("wevtutil could not run".into());
        return;
    };
    if !out.status.success() {
        seen.capture_ok = false;
        seen.notes.push(format!(
            "DNS-Client log unreadable: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
        return;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let enabled = std::process::Command::new("wevtutil")
        .args(["gl", "Microsoft-Windows-DNS-Client/Operational"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("enabled: true"))
        .unwrap_or(false);
    if !enabled {
        seen.capture_ok = false;
        seen.notes.push(
            "the DNS-Client operational log is off and could not be switched on (needs an administrator)"
                .into(),
        );
    }
    for event in text.split("<Event ").skip(1) {
        let field = |name: &str| {
            event
                .split(&format!("<Data Name='{name}'>"))
                .nth(1)
                .and_then(|r| r.split('<').next())
                .map(str::to_string)
        };
        let (Some(name), Some(kind)) = (field("QueryName"), field("QueryType")) else {
            continue;
        };
        let kind = match kind.as_str() {
            "1" => "A".to_string(),
            "28" => "AAAA".to_string(),
            "12" => "PTR".to_string(),
            "0" => continue, // "any address" bookkeeping events, not a query on the wire
            n => format!("type{n}"),
        };
        seen.queries.insert((name, kind));
    }
}

#[cfg(not(windows))]
fn dns_client_events(_pid: u32, _seen: &mut Seen) {}

// ----- macOS: lsof and tcpdump -k -----

#[cfg(target_os = "macos")]
fn sockets_of(pid: u32) -> Vec<String> {
    let Ok(out) = std::process::Command::new("lsof")
        .args(["-n", "-P", "-a", "-p", &pid.to_string(), "-i"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .skip(1)
        .map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            // TYPE NODE NAME: e.g. "IPv4 ... TCP 10.0.0.1:5000->1.2.3.4:443 (ESTABLISHED)"
            let proto = cols.get(7).copied().unwrap_or("?");
            let name = cols.get(8..).map(|c| c.join(" ")).unwrap_or_default();
            format!("{proto} {name}")
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn sockets_of(_pid: u32) -> Vec<String> {
    Vec::new()
}

/// Judges `tcpdump -k` lines whose process or effective process is Fallow.
fn judge_tcpdump(text: &str, seen: &mut Seen) {
    let mut unattributed = 0usize;
    for line in text.lines() {
        let ours = line.contains("proc fallow:") || line.contains("eproc fallow:");
        let dns = line.contains(".53:") || line.contains(".53 ");
        if !ours {
            if dns && line.contains("proc mDNSResponder") && line.contains('?') {
                unattributed += 1;
            }
            continue;
        }
        if !dns {
            seen.violations.push(format!("packet: {}", short(line)));
            continue;
        }
        // "... 1234+ A? api.anthropic.com. (35)"
        for kind in ["AAAA?", "A?", "PTR?"] {
            if let Some(rest) = line.split(&format!(" {kind} ")).nth(1) {
                let name = rest.split_whitespace().next().unwrap_or("");
                seen.queries.insert((
                    name.trim_end_matches('.').to_string(),
                    kind.trim_end_matches('?').to_string(),
                ));
                break;
            }
        }
    }
    if unattributed > 0 {
        seen.notes.push(format!(
            "{unattributed} DNS queries by mDNSResponder not attributed to any process (names not judged)"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_dns_question() {
        // id 0x1234, flags 0x0100, qd=1, "api.anthropic.com" A IN
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in ["api", "anthropic", "com"] {
            q.push(l.len() as u8);
            q.extend_from_slice(l.as_bytes());
        }
        q.extend_from_slice(&[0, 0, 1, 0, 1]);
        assert_eq!(
            dns_question(&q),
            Some(("api.anthropic.com".into(), "A".into()))
        );
        let hex: String = q.iter().map(|b| format!("\\x{b:02x}")).collect();
        assert_eq!(decode_hex_bytes(&hex), q);
    }

    #[test]
    fn strace_lines_are_judged() {
        let text = concat!(
            "100 connect(5, {sa_family=AF_INET, sin_port=htons(53), sin_addr=inet_addr(\"127.0.0.53\")}, 16) = 0\n",
            "100 connect(6, {sa_family=AF_INET, sin_port=htons(443), sin_addr=inet_addr(\"1.2.3.4\")}, 16) = -1\n",
            "100 connect(7, {sa_family=AF_UNIX, sun_path=\"\\x2f\\x72\\x75\\x6e\"}, 110) = -1\n",
        );
        let mut s = Seen::default();
        judge_strace(text, &mut s);
        assert_eq!(s.violations.len(), 1, "{:?}", s.violations);
        assert!(s.violations[0].contains("port 443"));
        assert_eq!(s.notes, vec!["local socket: /run".to_string()]);
    }

    #[test]
    fn source_address_probes_are_allowed_only_without_sends() {
        let probe = concat!(
            "7 socket(AF_INET, SOCK_DGRAM|SOCK_CLOEXEC, IPPROTO_IP) = 6
",
            "7 connect(6, {sa_family=AF_INET, sin_port=htons(0), sin_addr=inet_addr(\"1.2.3.4\")}, 16) = 0
",
        );
        let mut s = Seen::default();
        judge_strace(probe, &mut s);
        assert!(s.violations.is_empty(), "{:?}", s.violations);
        assert!(s.notes[0].contains("source-address probes"));

        let sent = format!(
            "{probe}7 sendto(6, \"\x01\", 1, 0, NULL, 0) = 1
"
        );
        let mut s = Seen::default();
        judge_strace(&sent, &mut s);
        assert_eq!(s.violations.len(), 1, "{:?}", s.violations);

        let tcp = "7 socket(AF_INET, SOCK_STREAM, IPPROTO_TCP) = 6
7 connect(6, {sa_family=AF_INET, sin_port=htons(0), sin_addr=inet_addr(\"1.2.3.4\")}, 16) = 0
";
        let mut s = Seen::default();
        judge_strace(tcp, &mut s);
        assert_eq!(s.violations.len(), 1, "a TCP connect is never a probe");
    }

    #[test]
    fn provider_hosts_are_read() {
        let h = provider_hosts();
        assert!(h.contains("api.anthropic.com"));
        assert!(h.contains("openrouter.ai"));
    }
}
