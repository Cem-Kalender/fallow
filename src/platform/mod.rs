//! What differs per operating system: random values, private folders, process groups and
//! opening the browser. Windows is the target of 006 (clarified 2026-10-29); the Unix code keeps
//! the door open and is not built in CI.

pub mod random;

#[cfg(windows)]
#[path = "job_windows.rs"]
pub mod job;
#[cfg(unix)]
#[path = "job_unix.rs"]
pub mod job;

use std::path::Path;

/// Creates `dir` (and its parents) readable by the current user only (constitution V). On
/// Windows the folder gets a protected access list for the current user, inherited by
/// everything created in it; on Unix, mode 0700.
pub fn private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    imp::restrict(dir)
}

/// The full path of `program` as the shell would find it on `PATH`. On Windows, a name without
/// an extension is tried with `.exe`, `.cmd`, `.bat` and `.com` only, as Windows does: a file
/// without an extension (such as npm's shell shim next to `claude.cmd`) is never a program
/// there. The search path is read, never stored or shown.
pub fn find_program(program: &str) -> Option<std::path::PathBuf> {
    let p = Path::new(program);
    if p.components().count() > 1 {
        return p.is_file().then(|| p.to_path_buf());
    }
    let has_ext = p.extension().is_some();
    let exts: &[&str] = if cfg!(windows) {
        if has_ext {
            &[""]
        } else {
            &[".exe", ".cmd", ".bat", ".com"]
        }
    } else {
        &[""]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in exts {
            let c = dir.join(format!("{program}{ext}"));
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

/// Opens `url` in the engineer's default browser.
pub fn open_browser(url: &str) -> Result<(), String> {
    imp::open_browser(url)
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SetFileSecurityW,
    };

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// `D:P(A;OICI;GA;;;<user>)`: only the current user, inherited by files and folders inside.
    pub fn restrict(dir: &Path) -> Result<(), String> {
        let sid = crate::mcp::pipe::current_user_sid()?;
        let sddl = wide(std::ffi::OsStr::new(&format!("D:P(A;OICI;GA;;;{sid})")));
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; `sd` receives a LocalAlloc'd descriptor freed below.
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
                "cannot build an access list: {}",
                std::io::Error::last_os_error()
            ));
        }
        let path = wide(dir.as_os_str());
        // SAFETY: `path` is NUL-terminated and `sd` is a valid descriptor.
        let set = unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                sd,
            )
        };
        let err = std::io::Error::last_os_error();
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(sd) };
        if set == 0 {
            return Err(format!("cannot restrict {}: {err}", dir.display()));
        }
        Ok(())
    }

    /// Windows' own URL handler: no shell parses the address.
    pub fn open_browser(url: &str) -> Result<(), String> {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open the browser: {e}"))
    }
}

#[cfg(unix)]
mod imp {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    pub fn restrict(dir: &Path) -> Result<(), String> {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("cannot restrict {}: {e}", dir.display()))
    }

    pub fn open_browser(url: &str) -> Result<(), String> {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open the browser: {e}"))
    }
}
