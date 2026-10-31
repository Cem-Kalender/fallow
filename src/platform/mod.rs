//! What differs per operating system: random values, private folders, process groups and
//! opening the browser. Windows is the target of 006 (clarified 2026-10-29); the Unix code keeps
//! the door open and is not built in CI.

pub mod notify;

/// The machine's physical memory in MB, if it can be read.
#[cfg(windows)]
pub fn total_memory_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: an all-zero MEMORYSTATUSEX with its length set is valid input.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `m` lives for the call.
    let ok = unsafe { GlobalMemoryStatusEx(&mut m) } != 0;
    ok.then_some(m.ullTotalPhys / (1024 * 1024))
}

/// The machine's physical memory in MB, if it can be read.
#[cfg(not(windows))]
pub fn total_memory_mb() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = text
        .lines()
        .find(|l| l.starts_with("MemTotal:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(kb / 1024)
}
pub mod random;

/// A command for a console program that opens no window of its own. The service has no
/// visible console, so each console child would otherwise flash a window (found on a real run).
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

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

/// The program to start for `program`: as [`find_program`] finds it, except that npm's batch
/// shim (`claude.cmd`) is replaced by the `.exe` it starts, when it names one that exists. A
/// batch file runs through `cmd.exe`, which cannot take an argument holding a line break (Rust
/// refuses to start it), and an agent's instructions are such arguments.
pub fn program_to_run(program: &str) -> Option<std::path::PathBuf> {
    let found = find_program(program)?;
    let ext = found
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(ext.as_deref(), Some("cmd" | "bat")) {
        return Some(found);
    }
    let shim = std::fs::read_to_string(&found).unwrap_or_default();
    let dir = found.parent()?;
    Some(
        shim_target(&shim)
            .map(|t| dir.join(t))
            .filter(|t| t.is_file())
            .unwrap_or(found),
    )
}

/// The `.exe` an npm batch shim starts, relative to the shim's folder: the quoted path after
/// `%dp0%\` (or `%~dp0\`) that ends in `.exe`.
pub fn shim_target(shim: &str) -> Option<String> {
    for line in shim.lines() {
        for start in ["\"%dp0%\\", "\"%~dp0\\"] {
            if let Some(i) = line.find(start) {
                let rest = &line[i + start.len()..];
                let target = &rest[..rest.find('"')?];
                if target.to_ascii_lowercase().ends_with(".exe") && !target.contains('%') {
                    return Some(target.to_string());
                }
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

#[cfg(test)]
mod tests {
    use super::shim_target;

    #[test]
    fn npm_shims_name_the_exe_they_start() {
        let npm = concat!(
            "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\n",
            "SETLOCAL\r\nCALL :find_dp0\r\n",
            r#""%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe"   %*"#,
            "\r\n"
        );
        assert_eq!(
            shim_target(npm).as_deref(),
            Some(r"node_modules\@anthropic-ai\claude-code\bin\claude.exe")
        );
        assert_eq!(
            shim_target(r#"@"%~dp0\tool.exe" %*"#).as_deref(),
            Some("tool.exe")
        );
        // A shim that starts node with a script, or anything else, is kept as it is.
        assert_eq!(
            shim_target(r#""%_prog%" "%dp0%\node_modules\x\cli.js" %*"#),
            None
        );
        assert_eq!(shim_target("echo hello"), None);
    }
}
