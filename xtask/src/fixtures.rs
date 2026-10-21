//! Fixture processes and OS readers shared by the Phase 3a checks.
//!
//! Fixtures are copies of this xtask binary under another name. `main` looks at the name it
//! was started as and plays that part:
//! - `ollama run <model>`: a local model server stand-in listening on 127.0.0.1 (port written
//!   to the file named by `FALLOW_FIXTURE_PORT_FILE`);
//! - `claude`: an idle agent CLI stand-in;
//! - anything else with `idle-child`, `client-fixture <port>` or `provider-fixture`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Plays a fixture part if the binary was started as one. Never returns in that case.
pub fn persona() {
    let exe = std::env::args().next().unwrap_or_default();
    let stem = Path::new(&exe)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match stem.as_str() {
        "ollama" => serve(),
        "claude" => idle(),
        "llama-server" => big(),
        _ => {}
    }
}

/// Fixture subcommands of the plain xtask binary.
pub fn subcommand(task: &str, args: &[String]) -> Option<i32> {
    match task {
        "idle-child" => idle(),
        "echo-agent" => echo_agent(),
        "banner-agent" => banner_agent(args.first().map_or("A", String::as_str)),
        "flood-agent" => flood_agent(args.first().map_or("forever", String::as_str)),
        "canary-agent" => crate::canary_sessions::agent(args.first()?),
        "mcp-agent" => crate::mcp_agent::run(args),
        "client-fixture" => {
            let port: u16 = args.first()?.parse().ok()?;
            let s = TcpStream::connect(("127.0.0.1", port)).ok()?;
            hold(s)
        }
        "provider-fixture" => {
            // A plain TCP connection to a provider's port 443, held open with no payload.
            for _ in 0..10 {
                if let Ok(addrs) = ("api.anthropic.com", 443).to_socket_addrs() {
                    for a in addrs {
                        if let Ok(s) = TcpStream::connect_timeout(&a, Duration::from_secs(5)) {
                            hold(s);
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            Some(3)
        }
        _ => None,
    }
}

fn idle() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn hold(mut s: TcpStream) -> ! {
    let mut buf = [0u8; 64];
    loop {
        // Blocks until the peer closes; never sends anything.
        match s.read(&mut buf) {
            Ok(0) | Err(_) => idle(),
            Ok(_) => {}
        }
    }
}

/// A large local model stand-in: maps `FALLOW_FIXTURE_ALLOC_MB` MB (default 1024), writes
/// pseudo-random data to every byte so no page is shared or compressible, writes the file named
/// by `FALLOW_FIXTURE_READY_FILE`, then idles holding the memory.
fn big() -> ! {
    let mb: usize = std::env::var("FALLOW_FIXTURE_ALLOC_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024);
    let mut mem: Vec<u64> = Vec::with_capacity(mb * 1024 * 1024 / 8);
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ std::process::id() as u64;
    for _ in 0..mem.capacity() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        mem.push(x);
    }
    std::hint::black_box(&mem);
    if let Ok(path) = std::env::var("FALLOW_FIXTURE_READY_FILE") {
        let _ = std::fs::write(path, mem.len().to_string());
    }
    loop {
        std::thread::sleep(Duration::from_secs(3600));
        std::hint::black_box(&mem);
    }
}

fn serve() -> ! {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    if let Ok(path) = std::env::var("FALLOW_FIXTURE_PORT_FILE") {
        let _ = std::fs::write(path, l.local_addr().unwrap().port().to_string());
    }
    let mut held = Vec::new();
    for s in l.incoming().flatten() {
        held.push(s);
    }
    idle()
}

/// A fresh directory for fixture copies.
pub fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fallow-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

/// Copies this binary into `dir` as `name` (plus `.exe` on Windows).
pub fn copy_self_as(dir: &Path, name: &str) -> PathBuf {
    let me = std::env::current_exe().expect("current exe");
    let to = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(&me, &to).expect("copy fixture binary");
    to
}

/// Waits for a fixture's port file.
pub fn read_port(path: &Path) -> Option<u16> {
    for _ in 0..100 {
        if let Ok(t) = std::fs::read_to_string(path) {
            if let Ok(p) = t.trim().parse() {
                return Some(p);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// Random bytes for canaries and offsets.
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            if f.read_exact(&mut out).is_ok() {
                return out;
            }
        }
    }
    // Fallback: mix the clock and addresses (enough for test timing offsets).
    let mut x = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
        ^ (&out as *const _ as u64);
    for b in &mut out {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Number of processes on the system.
#[cfg(target_os = "linux")]
pub fn process_count() -> usize {
    std::fs::read_dir("/proc")
        .map(|d| {
            d.flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                })
                .count()
        })
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
pub fn process_count() -> usize {
    // SAFETY: a null buffer returns the count.
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    n.max(0) as usize
}

#[cfg(windows)]
pub fn process_count() -> usize {
    use windows_sys::Win32::System::ProcessStatus::EnumProcesses;
    let mut pids = vec![0u32; 8192];
    let mut needed = 0u32;
    // SAFETY: `pids` is writable for its byte size.
    let ok = unsafe { EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut needed) };
    if ok == 0 { 0 } else { needed as usize / 4 }
}

/// User plus system CPU time of a process.
#[cfg(target_os = "linux")]
pub fn cpu_time(pid: u32) -> Option<Duration> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Fields after the `(comm)`, which may contain spaces.
    let rest = &s[s.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks: u64 = f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?;
    // SAFETY: sysconf has no preconditions.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as u64;
    Some(Duration::from_nanos(ticks * 1_000_000_000 / hz))
}

#[cfg(target_os = "macos")]
pub fn cpu_time(pid: u32) -> Option<Duration> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    // SAFETY: plain struct of the requested flavor.
    let r = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V4,
            (&mut info as *mut libc::rusage_info_v4).cast(),
        )
    };
    if r != 0 {
        return None;
    }
    // ri_*_time are in Mach absolute time units.
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
    }
    let mut tb = Timebase { numer: 0, denom: 0 };
    // SAFETY: writes the timebase into `tb`.
    unsafe { mach_timebase_info(&mut tb) };
    let units = (info.ri_user_time + info.ri_system_time) as u128;
    let nanos = units * tb.numer.max(1) as u128 / tb.denom.max(1) as u128;
    Some(Duration::from_nanos(nanos as u64))
}

#[cfg(windows)]
pub fn cpu_time(pid: u32) -> Option<Duration> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let zero = || FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut c, mut e, mut k, mut u) = (zero(), zero(), zero(), zero());
    // SAFETY: plain Win32 calls; the handle is closed.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
    }
    let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    Some(Duration::from_nanos((ft(k) + ft(u)) * 100))
}

/// True when this process runs elevated (root, or an elevated Windows token).
pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: no preconditions.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{
            GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        // SAFETY: our own token, closed before returning.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut len = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut e as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            );
            CloseHandle(token);
            ok != 0 && e.TokenIsElevated != 0
        }
    }
}

/// Searches a file for `needle` in chunks, with overlap so matches across chunk edges count.
pub fn file_contains(path: &Path, needle: &[u8]) -> std::io::Result<bool> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 8 << 20];
    let keep = needle.len().saturating_sub(1);
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(false);
        }
        carry.extend_from_slice(&buf[..n]);
        if carry.windows(needle.len()).any(|w| w == needle) {
            return Ok(true);
        }
        let cut = carry.len().saturating_sub(keep);
        carry.drain(..cut);
    }
}

pub fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

pub fn summary(markdown: &str) {
    crate::pty::step_summary(markdown);
}

/// Flushes stdout so CI logs interleave correctly.
pub fn flush() {
    let _ = std::io::stdout().flush();
}

/// A stand-in agent for `bench-sessions` (002 T026): prints `echo-ready`, then answers every
/// byte it reads by redrawing row 5 with `OOOOOOOO` and `XXXXXXXX` in turn. Every cell of that
/// row changes on each key, so the redraw reaches the outer terminal as one run of bytes the
/// harness can match.
fn echo_agent() -> Option<i32> {
    use std::io::{Read, Write};
    raw_stdin();
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x1b[2J\x1b[Hecho-ready\r\n");
    let _ = out.flush();
    let mut stdin = std::io::stdin();
    let mut b = [0u8; 64];
    let mut n = 0u64;
    loop {
        let got = match stdin.read(&mut b) {
            Ok(0) | Err(_) => return Some(0),
            Ok(k) => k,
        };
        for _ in 0..got {
            let row: &[u8] = if n % 2 == 0 { b"OOOOOOOO" } else { b"XXXXXXXX" };
            n += 1;
            let _ = out.write_all(b"\x1b[5;1H");
            let _ = out.write_all(row);
            let _ = out.flush();
        }
    }
}

/// Byte-at-a-time input without echo, like an agent's TUI.
#[cfg(unix)]
fn raw_stdin() {
    // SAFETY: tcgetattr/tcsetattr on stdin with a zero-initialised termios that tcgetattr fills.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(0, &mut t) == 0 {
            libc::cfmakeraw(&mut t);
            libc::tcsetattr(0, libc::TCSANOW, &t);
        }
    }
}

#[cfg(windows)]
fn raw_stdin() {
    use windows_sys::Win32::System::Console::{
        ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT, GetConsoleMode, GetStdHandle,
        STD_INPUT_HANDLE, SetConsoleMode,
    };
    // SAFETY: console mode calls on this process's standard input handle.
    unsafe {
        let h = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(h, &mut mode) != 0 {
            SetConsoleMode(
                h,
                mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT),
            );
        }
    }
}

/// `banner-agent <c>` (002 T036): fills its screen with the letter `c`, then waits for input.
/// Switching between two banner sessions changes every cell of the pane, so the new letter
/// reaches the outer terminal as long runs the harness can match.
fn banner_agent(c: &str) -> Option<i32> {
    use std::io::{Read, Write};
    raw_stdin();
    let ch = c.chars().next().unwrap_or('A');
    let row: String = std::iter::repeat_n(ch, 300).collect();
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x1b[2J\x1b[H");
    for _ in 0..80 {
        let _ = out.write_all(row.as_bytes());
        let _ = out.write_all(b"\r\n");
    }
    let _ = out.flush();
    let mut b = [0u8; 64];
    loop {
        match std::io::stdin().read(&mut b) {
            Ok(0) | Err(_) => return Some(0),
            Ok(_) => {}
        }
    }
}

/// `flood-agent <lines|forever|rate:N>` (002 T036, T037): prints 200-column lines in changing
/// colors: a fixed number as fast as possible then waits for input, forever as fast as
/// possible, or forever at N lines per second (the SC-006 load, research R-9).
fn flood_agent(arg: &str) -> Option<i32> {
    use std::io::{Read, Write};
    raw_stdin();
    let limit: Option<u64> = arg.parse().ok();
    let rate: Option<u64> = arg.strip_prefix("rate:").and_then(|r| r.parse().ok());
    let started = std::time::Instant::now();
    let mut out = std::io::BufWriter::with_capacity(64 * 1024, std::io::stdout());
    let body: String = "lorem ipsum dolor sit amet consectetur adipiscing elit "
        .chars()
        .cycle()
        .take(190)
        .collect();
    let mut n = 0u64;
    loop {
        if limit.is_some_and(|l| n >= l) {
            break;
        }
        let color = 31 + (n % 7);
        let line = format!("\x1b[{color}m{n:>8} {body}\x1b[0m\r\n");
        if out.write_all(line.as_bytes()).is_err() {
            return Some(0);
        }
        n += 1;
        if n % 64 == 0 && out.flush().is_err() {
            return Some(0);
        }
        if let Some(r) = rate {
            // Keep to N lines per second: sleep until line n is due.
            let due = std::time::Duration::from_secs_f64(n as f64 / r.max(1) as f64);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                let _ = out.flush();
                std::thread::sleep(wait);
            }
        }
    }
    let _ = out.flush();
    drop(out);
    let mut b = [0u8; 64];
    loop {
        match std::io::stdin().read(&mut b) {
            Ok(0) | Err(_) => return Some(0),
            Ok(_) => {}
        }
    }
}
