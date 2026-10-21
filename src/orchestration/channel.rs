//! The local channel (003 research R-2, R-3): a Unix domain socket in Fallow's run directory, or
//! a named pipe on Windows. Neither is a network socket (FR-002), and only the current user can
//! reach either one.
//!
//! The accept loop runs on its own thread. Each connection gets a reader thread and a writer
//! thread:
//! - The first line must be the hello `{"session": n, "token": "…"}`. It goes to the main loop
//!   with the kernel-reported peer, and the main loop decides (`Hub::hello`).
//! - After that, the MCP lifecycle is answered on the reader thread (`rpc::Server`). Tool calls
//!   and cancellations go to the main loop as `AppEvent::Hub`.
//! - Replies come back through the connection's `Sender<Outgoing>` to the writer thread.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::time::Duration;

use serde_json::Value;

use super::identity::Peer;
use super::rpc::{self, Action};
use crate::app::events::AppEvent;

pub type ConnId = u64;

/// The longest line accepted from a bridge. Tool arguments are at most 8 KiB of text plus JSON
/// escaping; anything longer is refused without being kept.
pub const MAX_LINE: usize = 256 * 1024;

/// What a connection tells the main loop.
#[derive(Debug)]
pub enum Incoming {
    Hello {
        conn: ConnId,
        session: u32,
        token: String,
        peer: Peer,
        verdict: SyncSender<Result<(), String>>,
        out: Sender<Outgoing>,
    },
    /// The client finished `initialize`.
    Client {
        conn: ConnId,
        name: String,
        version: String,
    },
    Call {
        conn: ConnId,
        id: Value,
        name: String,
        arguments: Value,
        progress: Option<Value>,
    },
    Cancelled {
        conn: ConnId,
        id: Value,
    },
    Closed {
        conn: ConnId,
    },
}

/// What the main loop sends to a connection.
#[derive(Debug, Clone, PartialEq)]
pub enum Outgoing {
    Line(String),
    Close,
}

/// The open channel. Dropping it stops the accept loop and removes the socket file.
pub struct Channel {
    endpoint: String,
    stop: Arc<AtomicBool>,
    #[cfg(unix)]
    path: std::path::PathBuf,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl Channel {
    /// What the bridge connects to: a socket path or a pipe name.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

/// Serves one accepted connection: the hello, then MCP lines.
fn serve<R, W>(reader: R, writer: W, peer: Peer, tools: Value, events: Sender<AppEvent>)
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let conn = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
    let (out_tx, out_rx) = mpsc::channel::<Outgoing>();
    let _ = std::thread::Builder::new()
        .name(format!("orch-write-{conn}"))
        .spawn(move || write_loop(writer, out_rx));
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let send = |i: Incoming| events.send(AppEvent::Hub(i)).is_ok();
    // The hello.
    match read_line(&mut reader, &mut line) {
        Ok(true) => {}
        _ => {
            let _ = out_tx.send(Outgoing::Close);
            return;
        }
    }
    let hello: Option<(u32, String)> = serde_json::from_slice::<Value>(&line).ok().and_then(|v| {
        Some((
            u32::try_from(v.get("session")?.as_u64()?).ok()?,
            v.get("token")?.as_str()?.to_string(),
        ))
    });
    let Some((session, token)) = hello else {
        let _ = out_tx.send(Outgoing::Close);
        return;
    };
    let (vtx, vrx) = mpsc::sync_channel(1);
    if !send(Incoming::Hello {
        conn,
        session,
        token,
        peer,
        verdict: vtx,
        out: out_tx.clone(),
    }) {
        let _ = out_tx.send(Outgoing::Close);
        return;
    }
    match vrx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        _ => {
            let _ = out_tx.send(Outgoing::Close);
            return;
        }
    }
    let mut server = rpc::Server::new(tools);
    let mut announced = false;
    while let Ok(true) = read_line(&mut reader, &mut line) {
        if line.len() > MAX_LINE {
            let _ = out_tx.send(Outgoing::Line(rpc::error_line(
                &Value::Null,
                rpc::INVALID_REQUEST,
                "message too large",
            )));
            continue;
        }
        let text = String::from_utf8_lossy(&line);
        match server.handle(&text) {
            Action::Reply(l) => {
                if out_tx.send(Outgoing::Line(l)).is_err() {
                    break;
                }
            }
            Action::Call {
                id,
                name,
                arguments,
                progress,
            } => {
                if !send(Incoming::Call {
                    conn,
                    id,
                    name,
                    arguments,
                    progress,
                }) {
                    break;
                }
            }
            Action::Cancelled(id) => {
                let _ = send(Incoming::Cancelled { conn, id });
            }
            Action::None => {}
        }
        if !announced {
            if let Some((name, version)) = server.client() {
                announced = true;
                let _ = send(Incoming::Client {
                    conn,
                    name,
                    version,
                });
            }
        }
    }
    let _ = send(Incoming::Closed { conn });
    let _ = out_tx.send(Outgoing::Close);
}

/// Reads one `\n`-terminated line into `buf` (without the newline). A line longer than
/// `MAX_LINE` is read to its end but only its first `MAX_LINE + 1` bytes are kept, so the caller
/// sees it is too long. `Ok(false)` at end of input.
fn read_line(r: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<bool> {
    buf.clear();
    let mut any = false;
    loop {
        let chunk = r.fill_buf()?;
        if chunk.is_empty() {
            return Ok(any);
        }
        any = true;
        let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i, true),
            None => (chunk.len(), false),
        };
        let room = (MAX_LINE + 1).saturating_sub(buf.len());
        buf.extend_from_slice(&chunk[..take.min(room)]);
        r.consume(if done { take + 1 } else { take });
        if done {
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return Ok(true);
        }
    }
}

fn write_loop<W: Write>(mut w: W, rx: Receiver<Outgoing>) {
    while let Ok(o) = rx.recv() {
        match o {
            Outgoing::Line(l) => {
                if w.write_all(l.as_bytes()).is_err()
                    || w.write_all(b"\n").is_err()
                    || w.flush().is_err()
                {
                    break;
                }
            }
            Outgoing::Close => break,
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;

    use super::*;
    use crate::orchestration::identity;

    /// `sun_path` is 104 bytes on macOS and 108 on Linux, including the terminating NUL.
    const SUN_PATH: usize = if cfg!(target_os = "macos") { 104 } else { 108 };

    /// Creates `dir` with mode 0700 if needed and checks it is a real directory of ours with
    /// exactly that mode, so nobody else can reach the socket in it.
    pub fn private_dir(dir: &Path) -> Result<(), String> {
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("cannot create {}: {e}", dir.display())),
        }
        let m = std::fs::symlink_metadata(dir)
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        if !m.file_type().is_dir() {
            return Err(format!("{} is not a plain folder", dir.display()));
        }
        if m.uid() != identity::my_uid() {
            return Err(format!("{} belongs to another user", dir.display()));
        }
        if m.mode() & 0o777 != 0o700 {
            return Err(format!(
                "{} has mode {:o}; expected 700",
                dir.display(),
                m.mode() & 0o777
            ));
        }
        Ok(())
    }

    pub fn open(run_dir: &Path, tools: Value, events: Sender<AppEvent>) -> Result<Channel, String> {
        private_dir(run_dir)?;
        // Sockets left by Fallow processes that ended without cleaning up (panic = abort).
        if let Ok(rd) = std::fs::read_dir(run_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "sock") {
                    if let Err(err) = UnixStream::connect(&p) {
                        if err.kind() == std::io::ErrorKind::ConnectionRefused {
                            let _ = std::fs::remove_file(&p);
                        }
                    }
                }
            }
        }
        let path = run_dir.join(format!("{}.sock", std::process::id()));
        if path.as_os_str().len() >= SUN_PATH {
            return Err(format!(
                "the socket path {} is longer than the system allows",
                path.display()
            ));
        }
        let _ = std::fs::remove_file(&path);
        let listener =
            UnixListener::bind(&path).map_err(|e| format!("cannot open the channel: {e}"))?;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("orch-accept".into())
            .spawn(move || {
                for s in listener.incoming() {
                    if stop2.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(s) = s else { continue };
                    let Ok(peer) = identity::peer_of(&s) else {
                        continue;
                    };
                    let Ok(w) = s.try_clone() else { continue };
                    let tools = tools.clone();
                    let events = events.clone();
                    let _ = std::thread::Builder::new()
                        .name("orch-conn".into())
                        .spawn(move || serve(s, w, peer, tools, events));
                }
            })
            .map_err(|e| format!("cannot start the channel: {e}"))?;
        Ok(Channel {
            endpoint: path.to_string_lossy().into_owned(),
            stop,
            path,
        })
    }

    impl Drop for Channel {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Wakes the accept loop so it sees the flag.
            let _ = UnixStream::connect(&self.path);
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// The bridge side: connect to the socket.
    pub fn connect(endpoint: &str, _fallow_pid: u32) -> Result<(UnixStream, UnixStream), String> {
        let s = UnixStream::connect(endpoint)
            .map_err(|e| format!("cannot reach Fallow at {endpoint}: {e}"))?;
        let w = s.try_clone().map_err(|e| e.to_string())?;
        Ok((s, w))
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_BROKEN_PIPE, ERROR_HANDLE_EOF, ERROR_IO_PENDING, ERROR_PIPE_BUSY,
        ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
        PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
    };
    use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId,
        GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, OpenProcessToken,
    };

    use super::*;
    use crate::orchestration::identity;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// An owned kernel handle.
    struct Owned(HANDLE);
    // SAFETY: kernel handles are usable from any thread.
    unsafe impl Send for Owned {}
    // SAFETY: only passed to thread-safe kernel calls.
    unsafe impl Sync for Owned {}
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: our handle.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// One end of a pipe opened for overlapped I/O. A pipe opened for blocking I/O runs one
    /// operation at a time, so a reader thread waiting for the next line would hold up every
    /// reply; with overlapped I/O each operation waits on its own event.
    #[derive(Clone)]
    pub struct Pipe(Arc<Owned>);

    impl Pipe {
        fn handle(&self) -> HANDLE {
            self.0.0
        }

        /// Runs one overlapped operation to completion.
        fn wait(&self, start: impl FnOnce(*mut OVERLAPPED) -> i32) -> std::io::Result<u32> {
            // SAFETY: a manual-reset, unnamed event, closed by `Owned`.
            let ev = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if ev.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let ev = Owned(ev);
            // SAFETY: an all-zero OVERLAPPED is valid; the event is set below.
            let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
            ov.hEvent = ev.0;
            let ok = start(&mut ov);
            // SAFETY: reads the thread's last error.
            if ok == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
                return Err(std::io::Error::last_os_error());
            }
            let mut n = 0u32;
            // SAFETY: `ov` outlives the operation: GetOverlappedResult waits for it to finish.
            if unsafe { GetOverlappedResult(self.handle(), &ov, &mut n, 1) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(n)
        }
    }

    impl Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
            let h = self.handle();
            let ptr = buf.as_mut_ptr();
            // SAFETY: `buf` is valid for `len` bytes until the operation completes in `wait`.
            match self.wait(|ov| unsafe { ReadFile(h, ptr, len, std::ptr::null_mut(), ov) }) {
                Ok(n) => Ok(n as usize),
                Err(e)
                    if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32)
                        || e.raw_os_error() == Some(ERROR_HANDLE_EOF as i32) =>
                {
                    Ok(0)
                }
                Err(e) => Err(e),
            }
        }
    }

    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
            let h = self.handle();
            let ptr = buf.as_ptr();
            // SAFETY: `buf` is valid for `len` bytes until the operation completes in `wait`.
            self.wait(|ov| unsafe { WriteFile(h, ptr, len, std::ptr::null_mut(), ov) })
                .map(|n| n as usize)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// `D:P(A;;GA;;;<current user SID>)`: only this user, nothing inherited.
    fn owner_only_sddl() -> Result<String, String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo-handle of this process; `token` receives a handle we close.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(format!(
                "OpenProcessToken: {}",
                std::io::Error::last_os_error()
            ));
        }
        let token = Owned(token);
        let mut len = 0u32;
        // SAFETY: a size query with a null buffer.
        unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len) };
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: `buf` holds `len` bytes, 8-byte aligned.
        let ok = unsafe {
            GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
        };
        if ok == 0 {
            return Err(format!(
                "GetTokenInformation: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: the buffer now holds a TOKEN_USER.
        let user = unsafe { &*(buf.as_ptr().cast::<TOKEN_USER>()) };
        let mut sid_str: windows_sys::core::PWSTR = std::ptr::null_mut();
        // SAFETY: the SID is valid while `buf` lives; the string is freed with LocalFree.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_str) } == 0 {
            return Err(format!(
                "ConvertSidToStringSidW: {}",
                std::io::Error::last_os_error()
            ));
        }
        // SAFETY: a NUL-terminated wide string from the system.
        let sid = unsafe {
            let mut n = 0;
            while *sid_str.add(n) != 0 {
                n += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(sid_str, n))
        };
        // SAFETY: allocated by ConvertSidToStringSidW.
        unsafe { LocalFree(sid_str.cast()) };
        Ok(format!("D:P(A;;GA;;;{sid})"))
    }

    /// A security descriptor freed on drop.
    struct Descriptor(PSECURITY_DESCRIPTOR);
    // SAFETY: an immutable descriptor in process memory.
    unsafe impl Send for Descriptor {}
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe { LocalFree(self.0) };
        }
    }

    fn descriptor() -> Result<Descriptor, String> {
        let sddl = wide(&owner_only_sddl()?);
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; `sd` receives a LocalAlloc'd descriptor.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "cannot build the pipe's access list: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Descriptor(sd))
    }

    fn instance(name: &[u16], sd: &Descriptor, first: bool) -> Result<Pipe, String> {
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let open_mode = PIPE_ACCESS_DUPLEX
            | FILE_FLAG_OVERLAPPED
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: `name` is NUL-terminated and `sa` lives for the call.
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                &sa,
            )
        };
        if h == INVALID_HANDLE_VALUE {
            Err(format!(
                "cannot open the channel: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(Pipe(Arc::new(Owned(h))))
        }
    }

    pub fn open(
        _run_dir: &Path,
        tools: Value,
        events: Sender<AppEvent>,
    ) -> Result<Channel, String> {
        let mut rnd = [0u8; 8];
        identity::fill_random(&mut rnd).map_err(|e| e.to_string())?;
        let hex: String = rnd.iter().map(|b| format!("{b:02x}")).collect();
        let endpoint = format!(r"\\.\pipe\fallow-{}-{hex}", std::process::id());
        let name = wide(&endpoint);
        let sd = descriptor()?;
        let first = instance(&name, &sd, true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("orch-accept".into())
            .spawn(move || {
                let mut next = Some(first);
                loop {
                    let pipe = match next.take() {
                        Some(p) => p,
                        None => match instance(&name, &sd, false) {
                            Ok(p) => p,
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(200));
                                continue;
                            }
                        },
                    };
                    let h = pipe.handle();
                    // SAFETY: a pipe handle opened for overlapped I/O; `wait` finishes it.
                    let connected = match pipe.wait(|ov| unsafe { ConnectNamedPipe(h, ov) }) {
                        Ok(_) => true,
                        Err(e) => e.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32),
                    };
                    if stop2.load(Ordering::Relaxed) {
                        break;
                    }
                    if !connected {
                        continue;
                    }
                    let mut pid = 0u32;
                    // SAFETY: a connected pipe handle and a writable u32.
                    if unsafe { GetNamedPipeClientProcessId(h, &mut pid) } == 0 {
                        continue;
                    }
                    let tools = tools.clone();
                    let events = events.clone();
                    let peer = Peer { pid, uid: None };
                    let w = pipe.clone();
                    let _ = std::thread::Builder::new()
                        .name("orch-conn".into())
                        .spawn(move || serve(pipe, w, peer, tools, events));
                }
            })
            .map_err(|e| format!("cannot start the channel: {e}"))?;
        Ok(Channel { endpoint, stop })
    }

    impl Drop for Channel {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Wakes the accept loop: connecting completes its pending ConnectNamedPipe.
            let _ = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.endpoint);
        }
    }

    /// The bridge side: open the pipe without allowing impersonation, and check that the
    /// server is the Fallow process named on the command line (another same-user process could
    /// have created an instance of the same name).
    pub fn connect(endpoint: &str, fallow_pid: u32) -> Result<(Pipe, Pipe), String> {
        let name = wide(endpoint);
        let mut tries = 0;
        let h = loop {
            // SAFETY: `name` is NUL-terminated; the handle is owned by `Pipe` below.
            let h = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    std::ptr::null_mut(),
                )
            };
            if h != INVALID_HANDLE_VALUE {
                break h;
            }
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && tries < 50 {
                tries += 1;
                // SAFETY: `name` is NUL-terminated.
                unsafe { WaitNamedPipeW(name.as_ptr(), 100) };
                continue;
            }
            return Err(format!("cannot reach Fallow at {endpoint}: {e}"));
        };
        let pipe = Pipe(Arc::new(Owned(h)));
        let mut server = 0u32;
        // SAFETY: a valid pipe handle and a writable u32.
        let ok = unsafe { GetNamedPipeServerProcessId(h, &mut server) };
        if ok == 0 || server != fallow_pid {
            return Err(format!(
                "{endpoint} is not served by Fallow process {fallow_pid}"
            ));
        }
        Ok((pipe.clone(), pipe))
    }
}

pub use imp::{connect, open};

#[cfg(unix)]
pub use imp::private_dir;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_split_and_bounded() {
        let data = b"one\r\ntwo\nthree".to_vec();
        let mut r = BufReader::new(&data[..]);
        let mut b = Vec::new();
        assert!(read_line(&mut r, &mut b).unwrap());
        assert_eq!(b, b"one");
        assert!(read_line(&mut r, &mut b).unwrap());
        assert_eq!(b, b"two");
        assert!(read_line(&mut r, &mut b).unwrap());
        assert_eq!(b, b"three");
        assert!(!read_line(&mut r, &mut b).unwrap());
        let mut long = vec![b'x'; MAX_LINE * 2];
        long.extend_from_slice(b"\nok\n");
        let mut r = BufReader::new(&long[..]);
        assert!(read_line(&mut r, &mut b).unwrap());
        assert_eq!(
            b.len(),
            MAX_LINE + 1,
            "kept only enough to know it is too long"
        );
        assert!(read_line(&mut r, &mut b).unwrap());
        assert_eq!(b, b"ok");
    }
}
