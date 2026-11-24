//! The local channel (006 research R-1, R-5): a Windows named pipe, or a Unix domain socket.
//! Neither is a network socket (constitution V), and only the current user can reach either.
//!
//! The service listens on one fixed name per user, which also makes it the only service. The
//! command line and agent steps' MCP bridges connect to it; the service reads the first line of
//! each connection to tell which is which (`service::pipe_conn`).

use std::io::{BufRead, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::identity::Peer;

/// The longest line accepted on the channel. Anything longer is refused without being kept.
pub const MAX_LINE: usize = 256 * 1024;

/// One accepted connection.
pub struct Conn {
    pub peer: Peer,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
}

/// The reading and writing ends of a connection.
pub type Ends = (Box<dyn Read + Send>, Box<dyn Write + Send>);

/// Called on its own thread for every accepted connection.
pub type Handler = Arc<dyn Fn(Conn) + Send + Sync>;

/// The open listener. Dropping it stops accepting.
pub struct Listener {
    endpoint: String,
    stop: Arc<AtomicBool>,
    #[cfg(unix)]
    path: std::path::PathBuf,
}

impl std::fmt::Debug for Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Listener")
            .field("endpoint", &self.endpoint)
            .finish()
    }
}

impl Listener {
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

/// Reads one `\n`-terminated line into `buf` (without the newline). A line longer than
/// `MAX_LINE` is read to its end but only its first `MAX_LINE + 1` bytes are kept, so the caller
/// sees it is too long. `Ok(false)` at end of input.
pub fn read_line(r: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<bool> {
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

/// Writes one line and flushes.
pub fn write_line(w: &mut dyn Write, line: &str) -> std::io::Result<()> {
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::time::Duration;

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

    /// The current user's SID as a string (`S-1-5-21-…`).
    pub fn current_user_sid() -> Result<String, String> {
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
        Ok(sid)
    }

    /// The service's pipe name for the current user.
    pub fn service_endpoint() -> Result<String, String> {
        Ok(format!(r"\\.\pipe\fallow-{}", current_user_sid()?))
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

    /// `D:P(A;;GA;;;<current user SID>)`: only this user, nothing inherited.
    fn descriptor() -> Result<Descriptor, String> {
        let sddl = wide(&format!("D:P(A;;GA;;;{})", current_user_sid()?));
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

    fn instance(name: &[u16], sd: &Descriptor, first: bool) -> Result<Pipe, std::io::Error> {
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
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Pipe(Arc::new(Owned(h))))
        }
    }

    /// Listens on `endpoint`. The first instance is created with `FIRST_PIPE_INSTANCE`, so this
    /// fails when another process already listens there (one service per user).
    pub fn listen(endpoint: &str, handler: Handler) -> Result<Listener, String> {
        let name = wide(endpoint);
        let sd = descriptor()?;
        let first = instance(&name, &sd, true).map_err(|e| {
            if e.raw_os_error() == Some(5) || e.raw_os_error() == Some(231) {
                "another Fallow service is already running".to_string()
            } else {
                format!("cannot open the channel: {e}")
            }
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("pipe-accept".into())
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
                    let handler = Arc::clone(&handler);
                    let conn = Conn {
                        peer: Peer { pid, uid: None },
                        reader: Box::new(pipe.clone()),
                        writer: Box::new(pipe),
                    };
                    let _ = std::thread::Builder::new()
                        .name("pipe-conn".into())
                        .spawn(move || handler(conn));
                }
            })
            .map_err(|e| format!("cannot start the channel: {e}"))?;
        Ok(Listener {
            endpoint: endpoint.to_string(),
            stop,
        })
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Wakes the accept loop: connecting completes its pending ConnectNamedPipe.
            let _ = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.endpoint);
        }
    }

    /// Opens the pipe without allowing impersonation. With `server_pid`, checks that the server
    /// is that process (another same-user process could have created an instance of the name).
    pub fn connect(endpoint: &str, server_pid: Option<u32>) -> Result<Ends, String> {
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
            return Err(format!("cannot reach the Fallow service: {e}"));
        };
        let pipe = Pipe(Arc::new(Owned(h)));
        if let Some(expected) = server_pid {
            let mut server = 0u32;
            // SAFETY: a valid pipe handle and a writable u32.
            let ok = unsafe { GetNamedPipeServerProcessId(h, &mut server) };
            if ok == 0 || server != expected {
                return Err(format!(
                    "the pipe is not served by Fallow process {expected}"
                ));
            }
        }
        Ok((Box::new(pipe.clone()), Box::new(pipe)))
    }
}

#[cfg(unix)]
mod imp {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::*;

    /// The service's socket for the current user, in Fallow's private data folder.
    pub fn service_endpoint() -> Result<String, String> {
        let dir = crate::config::data_dir().ok_or("no data folder")?;
        crate::platform::private_dir(&dir)?;
        Ok(dir.join("service.sock").to_string_lossy().into_owned())
    }

    pub fn current_user_sid() -> Result<String, String> {
        Ok(super::super::identity::my_uid().to_string())
    }

    pub fn listen(endpoint: &str, handler: Handler) -> Result<Listener, String> {
        if UnixStream::connect(endpoint).is_ok() {
            return Err("another Fallow service is already running".into());
        }
        let _ = std::fs::remove_file(endpoint);
        let listener =
            UnixListener::bind(endpoint).map_err(|e| format!("cannot open the channel: {e}"))?;
        let _ = std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600));
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("pipe-accept".into())
            .spawn(move || {
                for s in listener.incoming() {
                    if stop2.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(s) = s else { continue };
                    let Ok(peer) = super::super::identity::peer_of(&s) else {
                        continue;
                    };
                    let Ok(w) = s.try_clone() else { continue };
                    let handler = Arc::clone(&handler);
                    let conn = Conn {
                        peer,
                        reader: Box::new(s),
                        writer: Box::new(w),
                    };
                    let _ = std::thread::Builder::new()
                        .name("pipe-conn".into())
                        .spawn(move || handler(conn));
                }
            })
            .map_err(|e| format!("cannot start the channel: {e}"))?;
        Ok(Listener {
            endpoint: endpoint.to_string(),
            stop,
            path: endpoint.into(),
        })
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = UnixStream::connect(&self.path);
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub fn connect(endpoint: &str, _server_pid: Option<u32>) -> Result<Ends, String> {
        let s = UnixStream::connect(endpoint)
            .map_err(|e| format!("cannot reach the Fallow service: {e}"))?;
        let w = s.try_clone().map_err(|e| e.to_string())?;
        Ok((Box::new(s), Box::new(w)))
    }
}

pub use imp::{connect, current_user_sid, listen, service_endpoint};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

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
