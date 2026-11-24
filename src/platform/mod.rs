//! What differs per operating system: random values, private folders, process groups and
//! opening the browser. Windows is the target of 006 (clarified 2026-10-29); the Unix code keeps
//! the door open and is not built in CI.

pub mod display;
pub mod notify;
pub mod picker;

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

/// 015: a child that may outlive the moment (an agent, a gate, a push, an editor, the service)
/// gets none of Fallow's open files but its own standard input, output and error.
///
/// Found under load in 015's tests: on macOS the standard library makes a child's pipes in two
/// steps (`pipe`, then "close on exec"), so another thread starting an agent in between gave
/// the agent a copy of a git pipe. The engine then waited on that git command's output, which
/// stays open until the agent ends, while the agent waited on the engine to answer it: every
/// request stalled. Here the child marks every other descriptor "close on exec" before it
/// starts, so no such copy survives. Unix only; Windows hands a child only what is named.
pub fn no_inherited_files(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Made in the parent: after `fork` the child must not allocate.
        let mut fds = inherited::Fds::new();
        // SAFETY: the closure makes only system calls (`fcntl`, and `getpid` with
        // `proc_pidinfo` or `close_range`), allocates nothing and takes no lock. Descriptors 0
        // to 2 are the child's own, set up before it runs.
        unsafe {
            cmd.pre_exec(move || {
                fds.close_on_exec_from(3);
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

#[cfg(unix)]
mod inherited {
    /// What the child needs to find its open descriptors without allocating.
    pub struct Fds {
        /// The descriptors' upper bound, for the slow way.
        max: libc::c_int,
        /// macOS: room for the kernel's list of the process's descriptors.
        #[cfg(target_os = "macos")]
        list: Vec<libc::proc_fdinfo>,
    }

    impl Fds {
        pub fn new() -> Fds {
            let mut lim = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: `getrlimit` only writes the struct given.
            let max = if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } == 0 {
                lim.rlim_cur.min(65_536) as libc::c_int
            } else {
                4_096
            };
            Fds {
                max,
                #[cfg(target_os = "macos")]
                list: vec![
                    libc::proc_fdinfo {
                        proc_fd: 0,
                        proc_fdtype: 0,
                    };
                    4_096
                ],
            }
        }

        /// Marks every open descriptor from `first` on "close on exec". Runs in the child.
        pub fn close_on_exec_from(&mut self, first: libc::c_int) {
            if self.fast(first) {
                return;
            }
            for fd in first..self.max {
                set_cloexec(fd);
            }
        }

        /// macOS: the kernel lists the open descriptors (one call, not one per possible one;
        /// the limit can be a million).
        #[cfg(target_os = "macos")]
        fn fast(&mut self, first: libc::c_int) -> bool {
            let room = (self.list.len() * std::mem::size_of::<libc::proc_fdinfo>()) as libc::c_int;
            // SAFETY: the buffer is `room` bytes, owned by this closure's copy.
            let got = unsafe {
                libc::proc_pidinfo(
                    libc::getpid(),
                    libc::PROC_PIDLISTFDS,
                    0,
                    self.list.as_mut_ptr().cast(),
                    room,
                )
            };
            if got <= 0 || got >= room {
                return false;
            }
            let n = got as usize / std::mem::size_of::<libc::proc_fdinfo>();
            for f in &self.list[..n] {
                if f.proc_fd >= first {
                    set_cloexec(f.proc_fd);
                }
            }
            true
        }

        /// Linux: one call for the whole range.
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        fn fast(&mut self, first: libc::c_int) -> bool {
            // SAFETY: `close_range` with CLOSE_RANGE_CLOEXEC only sets flags.
            unsafe {
                libc::close_range(
                    first as libc::c_uint,
                    libc::c_uint::MAX,
                    libc::CLOSE_RANGE_CLOEXEC as libc::c_int,
                ) == 0
            }
        }

        #[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
        fn fast(&mut self, _first: libc::c_int) -> bool {
            false
        }
    }

    fn set_cloexec(fd: libc::c_int) {
        // SAFETY: `fcntl` on a descriptor that may not be open only fails.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
                libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
            }
        }
    }
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
/// shim (`claude.cmd`) is replaced by the `.exe` it starts. A batch file runs through `cmd.exe`,
/// which cannot take an argument holding a line break (Rust refuses to start it), and an
/// agent's instructions are such arguments.
pub fn program_to_run(program: &str) -> Option<std::path::PathBuf> {
    find_program(program).map(|found| past_shim(&found))
}

/// The `.exe` an npm shim starts, also while it is missing: an update deletes and writes it
/// again, and the shim in its place would refuse the agent's arguments ("batch file arguments
/// are invalid"). A missing program is waited for and tried again by the agent's start. Any
/// other program, or a shim that names no `.exe`, is kept as it is.
fn past_shim(found: &std::path::Path) -> std::path::PathBuf {
    let ext = found
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(ext.as_deref(), Some("cmd" | "bat")) {
        return found.to_path_buf();
    }
    let shim = std::fs::read_to_string(found).unwrap_or_default();
    match (shim_target(&shim), found.parent()) {
        (Some(t), Some(dir)) => dir.join(t),
        _ => found.to_path_buf(),
    }
}

/// The `.exe` an npm batch shim starts, relative to the shim's folder: the quoted path after
/// `%dp0%\` (or `%~dp0\`) that ends in `.exe` and is given the shim's arguments (`%*`). A
/// node program's shim names `node.exe` only to look for it (`IF EXIST`, `SET`): not that.
pub fn shim_target(shim: &str) -> Option<String> {
    for line in shim.lines() {
        for start in ["\"%dp0%\\", "\"%~dp0\\"] {
            if let Some(i) = line.find(start) {
                let rest = &line[i + start.len()..];
                let end = rest.find('"')?;
                let target = &rest[..end];
                if target.to_ascii_lowercase().ends_with(".exe")
                    && !target.contains('%')
                    && rest[end + 1..].trim_start().starts_with("%*")
                {
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

/// 016 (research M-3): the `Fallow.app` this program runs from (`…/Fallow.app/Contents/MacOS/`,
/// also through a link such as `~/.local/bin/fallow`), or `None` outside a bundle.
#[cfg(target_os = "macos")]
pub fn bundle() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    bundle_of(&exe)
}

/// The `.app` folder `exe` lies in as its bundle's executable.
pub fn bundle_of(exe: &Path) -> Option<std::path::PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

/// 016 (spec edge case "A project in a folder macOS protects", T026): the message for a project
/// folder macOS did not let Fallow open (`home` the engineer's home): the protected folder it lies
/// in, and where to allow it. `None` outside them.
pub fn protected_message(path: &Path, home: &Path) -> Option<String> {
    for (folder, shown) in [
        ("Documents", "~/Documents"),
        ("Desktop", "~/Desktop"),
        ("Downloads", "~/Downloads"),
        ("Library/Mobile Documents", "iCloud Drive"),
    ] {
        if path.starts_with(home.join(folder)) {
            return Some(format!(
                "macOS did not let Fallow open {shown}: allow it in System Settings → Privacy & Security → Files and Folders → Fallow, then add the project again"
            ));
        }
    }
    None
}

/// 016: `err`, said for the engineer when it is macOS refusing a protected folder.
pub fn folder_error(path: &Path, err: &std::io::Error) -> String {
    if cfg!(target_os = "macos") && err.kind() == std::io::ErrorKind::PermissionDenied {
        if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
            if let Some(m) = protected_message(path, Path::new(&home)) {
                return m;
            }
        }
    }
    format!("{}: {err}", path.display())
}

/// 016 (found on the first release): the search path for the app opened from Finder, the Dock
/// or `open`. macOS gives such an app only `/usr/bin:/bin:/usr/sbin:/sbin`, so the agent's
/// program (`claude` in `~/.local/bin`, Homebrew's tools) was "not installed". The user's login
/// shell is asked for its `PATH` (as a terminal has it, about 30 ms), waited for at most 3 s;
/// the usual install folders are added in any case. Read, never stored or shown.
#[cfg(target_os = "macos")]
pub fn app_search_path() -> std::ffi::OsString {
    let shell = std::env::var_os("SHELL")
        .filter(|s| Path::new(s).is_absolute())
        .unwrap_or_else(|| "/bin/zsh".into());
    let from_shell = login_shell_path(&shell).unwrap_or_default();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut usual = Vec::new();
    if let Some(h) = &home {
        usual.push(h.join(".local/bin"));
        usual.push(h.join(".claude/local"));
    }
    usual.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(std::path::PathBuf::from));
    merge_paths(
        &from_shell,
        &std::env::var_os("PATH").unwrap_or_default(),
        &usual,
    )
}

/// `$PATH` as `shell -ilc` has it, between markers (an interactive shell may print other
/// things); `None` when it fails or takes longer than 3 s.
#[cfg(target_os = "macos")]
fn login_shell_path(shell: &std::ffi::OsStr) -> Option<std::ffi::OsString> {
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    let mut child = std::process::Command::new(shell)
        .args([
            "-ilc",
            "printf '\\n__FALLOW_PATH__%s__FALLOW_END__\\n' \"$PATH\"",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < end => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let buf = reader.join().ok()?;
    let start = find(&buf, b"__FALLOW_PATH__")? + b"__FALLOW_PATH__".len();
    let len = find(&buf[start..], b"__FALLOW_END__")?;
    Some(std::ffi::OsString::from_vec(
        buf[start..start + len].to_vec(),
    ))
}

#[cfg(target_os = "macos")]
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The login shell's folders first (the order a terminal finds programs in), then the ones the
/// process had, then the usual install folders; each once, empty entries dropped.
pub fn merge_paths(
    from_shell: &std::ffi::OsStr,
    current: &std::ffi::OsStr,
    usual: &[std::path::PathBuf],
) -> std::ffi::OsString {
    let mut all: Vec<std::path::PathBuf> = Vec::new();
    let dirs = std::env::split_paths(from_shell)
        .chain(std::env::split_paths(current))
        .chain(usual.iter().cloned());
    for d in dirs {
        if !d.as_os_str().is_empty() && !all.contains(&d) {
            all.push(d);
        }
    }
    std::env::join_paths(all).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{past_shim, shim_target};

    #[cfg(unix)]
    #[test]
    fn the_apps_search_path_puts_the_shells_first_and_keeps_each_folder_once() {
        let usual = ["/u/.local/bin", "/opt/homebrew/bin"].map(std::path::PathBuf::from);
        let p = super::merge_paths(
            "/u/.local/bin:/opt/homebrew/bin:/usr/bin".as_ref(),
            "/usr/bin:/bin::/usr/sbin".as_ref(),
            &usual,
        );
        assert_eq!(p, "/u/.local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin");
        // The shell gave nothing: the minimal path, then the usual folders.
        let p = super::merge_paths("".as_ref(), "/usr/bin:/bin".as_ref(), &usual);
        assert_eq!(p, "/usr/bin:/bin:/u/.local/bin:/opt/homebrew/bin");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_login_shell_is_asked_for_its_path() {
        let p = super::login_shell_path("/bin/zsh".as_ref()).expect("zsh answered");
        assert!(std::env::split_paths(&p).any(|d| d == std::path::Path::new("/usr/bin")));
    }

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
        // npm's shim for a node program: node.exe is looked for, not started with the arguments.
        let node = r#"@ECHO off
GOTO start
:find_dp0
SET dp0=%~dp0
EXIT /b
:start
SETLOCAL
CALL :find_dp0

IF EXIST "%dp0%\node.exe" (
  SET "_prog=%dp0%\node.exe"
) ELSE (
  SET "_prog=node"
  SET PATHEXT=%PATHEXT:;.JS;=;%
)

endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\node_modules\@anthropic-ai\claude-code\cli.js" %*
"#;
        assert_eq!(shim_target(node), None);
    }

    #[test]
    fn a_shim_leads_to_its_exe_also_while_an_update_has_removed_it() {
        let dir = std::env::temp_dir().join(format!("fallow-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("claude.cmd");
        std::fs::write(&shim, "@ECHO off\r\n\"%dp0%\\bin\\claude.exe\"   %*\r\n").unwrap();
        // Not there (the update deleted it): still the exe, never the shim.
        assert_eq!(past_shim(&shim), dir.join(r"bin\claude.exe"));
        // A shim naming no exe, and any other program, stay as they are.
        let node = dir.join("tool.cmd");
        std::fs::write(&node, "@node \"%~dp0\\cli.js\" %*\r\n").unwrap();
        assert_eq!(past_shim(&node), node);
        let exe = dir.join("other.exe");
        assert_eq!(past_shim(&exe), exe);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bundle_is_found_only_for_its_own_executable() {
        let b = |p: &str| super::bundle_of(std::path::Path::new(p));
        assert_eq!(
            b("/Users/u/Applications/Fallow.app/Contents/MacOS/fallow"),
            Some(std::path::PathBuf::from("/Users/u/Applications/Fallow.app"))
        );
        assert_eq!(b("/Users/u/src/fallow/target/release/fallow"), None);
        assert_eq!(
            b("/Applications/Fallow.app/Contents/Resources/fallow"),
            None
        );
        assert_eq!(b("/x/Fallow/Contents/MacOS/fallow"), None);
        assert_eq!(b("/fallow"), None);
    }

    #[test]
    fn a_protected_folder_is_named_with_where_to_allow_it() {
        let home = std::path::Path::new("/Users/u");
        let m = super::protected_message(std::path::Path::new("/Users/u/Documents/work/app"), home)
            .unwrap();
        assert!(m.contains("~/Documents"), "{m}");
        assert!(m.contains("Files and Folders → Fallow"), "{m}");
        assert!(
            super::protected_message(
                std::path::Path::new("/Users/u/Library/Mobile Documents/com~apple~CloudDocs/x"),
                home
            )
            .unwrap()
            .contains("iCloud Drive")
        );
        assert_eq!(
            super::protected_message(std::path::Path::new("/Users/u/src/app"), home),
            None
        );
        assert_eq!(
            super::protected_message(std::path::Path::new("/Users/u/DocumentsOld/x"), home),
            None
        );
    }

    /// 015: a descriptor left open without "close on exec" (as a pipe is, for a moment, while
    /// another thread starts a child) does not reach a child started with `no_inherited_files`.
    #[cfg(unix)]
    #[test]
    fn a_child_gets_no_inherited_files() {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `pipe` writes two descriptors into the array.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let listed = |isolated: bool| {
            let mut cmd = std::process::Command::new("sh");
            cmd.args(["-c", "ls /dev/fd"]);
            if isolated {
                super::no_inherited_files(&mut cmd);
            }
            let out = cmd.output().unwrap();
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        // Moved high, so no file the shell or `ls` opens takes its number.
        // SAFETY: `F_DUPFD` copies an open descriptor of this test's (without "close on exec").
        let high = unsafe { libc::fcntl(fds[1], libc::F_DUPFD, 300) };
        assert!(high >= 300);
        let write_end = high.to_string();
        assert!(
            listed(false).contains(&write_end),
            "the check sees inherited files"
        );
        assert!(!listed(true).contains(&write_end));
        // SAFETY: both descriptors are this test's own.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
            libc::close(high);
        }
    }
}
