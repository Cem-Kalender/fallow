//! macOS platform layer (R-1 to R-4).
//!
//! The command line comes from KERN_PROCARGS2, which also returns the target's environment
//! (R-2). The reader owns one buffer, parses exactly `argc` strings and nothing after them,
//! and zeroes the buffer before every return (Constitution VII; gated by the canary, T120).

use std::ffi::CStr;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::sync::Arc;

use zeroize::Zeroize;

use super::parse::{self, RawConn, TcpQuery, TcpRow};
use super::{Platform, PlatformError, Result};
use crate::procmon::snapshot::{Footprint, ProcessKey};

const PROC_PIDFDSOCKETINFO: libc::c_int = 3;

#[derive(Debug)]
pub struct OsPlatform {
    /// KERN_PROCARGS2 buffer, `kern.argmax` bytes. All zeros between calls.
    args: Vec<u8>,
    fds: Vec<libc::proc_fdinfo>,
    /// Larger than `struct socket_fdinfo`; u64 for alignment.
    sock: Vec<u64>,
    rows: Vec<(u32, TcpRow)>,
}

impl Default for OsPlatform {
    fn default() -> Self {
        let mut argmax: libc::c_int = 0;
        let mut size = size_of::<libc::c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: writes one c_int into `argmax`, whose size is passed.
        let r = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut argmax as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let argmax = if r == 0 && argmax > 0 {
            argmax as usize
        } else {
            1 << 20
        };
        OsPlatform {
            args: vec![0; argmax],
            fds: Vec::with_capacity(256),
            sock: vec![0; 128],
            rows: Vec::with_capacity(64),
        }
    }
}

impl OsPlatform {
    /// Test hook (T030): true when the KERN_PROCARGS2 buffer holds no data.
    #[doc(hidden)]
    pub fn args_buffer_is_zeroed(&self) -> bool {
        self.args.iter().all(|b| *b == 0)
    }

    fn list_sockets(&mut self, pid: u32) {
        let ipid = pid as libc::c_int;
        // SAFETY: a null buffer asks only for the needed size.
        let need =
            unsafe { libc::proc_pidinfo(ipid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
        if need <= 0 {
            return;
        }
        let n = need as usize / size_of::<libc::proc_fdinfo>() + 16;
        self.fds.clear();
        self.fds.reserve(n);
        // SAFETY: the buffer has capacity for `n` entries and that size is passed.
        let got = unsafe {
            libc::proc_pidinfo(
                ipid,
                libc::PROC_PIDLISTFDS,
                0,
                self.fds.as_mut_ptr().cast(),
                (n * size_of::<libc::proc_fdinfo>()) as libc::c_int,
            )
        };
        if got <= 0 {
            return;
        }
        let count = (got as usize / size_of::<libc::proc_fdinfo>()).min(n);
        // SAFETY: the kernel initialized `count` entries.
        unsafe { self.fds.set_len(count) };
        for i in 0..self.fds.len() {
            let fd = self.fds[i];
            if fd.proc_fdtype != libc::PROX_FDTYPE_SOCKET as u32 {
                continue;
            }
            let len = self.sock.len() * 8;
            // SAFETY: `sock` is writable for `len` bytes.
            let r = unsafe {
                libc::proc_pidfdinfo(
                    ipid,
                    fd.proc_fd,
                    PROC_PIDFDSOCKETINFO,
                    self.sock.as_mut_ptr().cast(),
                    len as libc::c_int,
                )
            };
            if r <= 0 {
                continue;
            }
            // SAFETY: `sock` is `len` initialized bytes.
            let bytes = unsafe { std::slice::from_raw_parts(self.sock.as_ptr().cast::<u8>(), len) };
            if let Some(row) = parse::parse_socket_fdinfo(&bytes[..(r as usize).min(len)], pid) {
                self.rows.push((pid, row));
            }
        }
    }
}

impl Platform for OsPlatform {
    fn cmdline(&mut self, key: ProcessKey) -> Result<Arc<[Box<str>]>> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, key.pid as libc::c_int];
        let mut size = self.args.len();
        // SAFETY: `args` is writable for `size` bytes.
        let r = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                self.args.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let filled = size.min(self.args.len());
        let result = if r == 0 {
            parse::parse_procargs2(&self.args[..filled])
                .map(Into::into)
                .ok_or_else(|| PlatformError::Other("malformed KERN_PROCARGS2".into()))
        } else {
            Err(std::io::Error::last_os_error().into())
        };
        // Wipe before returning, on every path. After an error the kernel may have written an
        // unknown amount, so the whole buffer is cleared then.
        if r == 0 {
            self.args[..filled].zeroize();
        } else {
            self.args.zeroize();
        }
        result
    }

    fn cwd(&mut self, key: ProcessKey) -> Result<PathBuf> {
        // SAFETY: zeroed plain-old-data struct, size passed.
        let mut info: libc::proc_vnodepathinfo = unsafe { zeroed() };
        let size = size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
        let r = unsafe {
            libc::proc_pidinfo(
                key.pid as libc::c_int,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&mut info as *mut libc::proc_vnodepathinfo).cast(),
                size,
            )
        };
        if r != size {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: vip_path is a NUL-terminated C string within MAXPATHLEN bytes.
        let path = unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
        let path = path.to_string_lossy();
        if path.is_empty() {
            return Err(PlatformError::Other("no working directory".into()));
        }
        Ok(PathBuf::from(path.into_owned()))
    }

    fn footprint(&mut self, key: ProcessKey) -> Result<Footprint> {
        // SAFETY: zeroed plain-old-data struct of the requested flavor.
        let mut info: libc::rusage_info_v4 = unsafe { zeroed() };
        let r = unsafe {
            libc::proc_pid_rusage(
                key.pid as libc::c_int,
                libc::RUSAGE_INFO_V4,
                (&mut info as *mut libc::rusage_info_v4).cast(),
            )
        };
        parse::macos_footprint((r == 0).then_some(info.ri_phys_footprint))
            .map_err(|why| PlatformError::Other(why.into()))
    }

    fn tcp_table(&mut self, q: &TcpQuery<'_>, out: &mut Vec<RawConn>) -> Result<()> {
        self.rows.clear();
        for &pid in q.candidates {
            self.list_sockets(pid);
        }
        let rows = std::mem::take(&mut self.rows);
        parse::select(&rows, q, out);
        self.rows = rows;
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
