//! 016 (FR-032, research M-14, T004): Fallow's data folder on macOS leaves `~/Library/Caches`,
//! which macOS may empty, for `~/Library/Application Support/Fallow`, moved once; not while a
//! Fallow from before 016 still runs on the old folder.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use fallow::config::{Os, data_dir_for, move_old_data};

fn s(v: &str) -> Option<OsString> {
    Some(OsString::from(v))
}

fn scratch(name: &str) -> PathBuf {
    // Short: a socket's path must stay under 104 bytes.
    let base = if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let d = base.join(format!("f016-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_data_folder_per_platform() {
    assert_eq!(
        data_dir_for(Os::MacOs, s("/Users/u"), None, None),
        Some(Path::new("/Users/u/Library/Application Support/Fallow").to_path_buf())
    );
    assert_eq!(data_dir_for(Os::MacOs, None, None, None), None);
    assert_eq!(data_dir_for(Os::MacOs, s(""), None, None), None);
    // Windows and Linux unchanged.
    assert_eq!(
        data_dir_for(Os::Linux, s("/home/u"), s("/xdg/cache"), None),
        Some(Path::new("/xdg/cache").join("fallow"))
    );
    assert_eq!(
        data_dir_for(Os::Linux, s("/home/u"), None, None),
        Some(Path::new("/home/u/.cache").join("fallow"))
    );
    assert_eq!(
        data_dir_for(Os::Windows, None, None, s(r"C:\Users\u\AppData\Local")),
        Some(Path::new(r"C:\Users\u\AppData\Local").join("fallow"))
    );
}

#[test]
fn the_socket_fits_for_a_long_user_name() {
    // A Unix socket's path is at most 103 bytes on macOS; user names are at most 32 characters.
    let home = format!("/Users/{}", "u".repeat(32));
    let sock = data_dir_for(Os::MacOs, s(&home), None, None)
        .unwrap()
        .join("service.sock");
    assert!(sock.as_os_str().len() < 104, "{}", sock.display());
}

#[test]
fn the_old_folder_moves_once() {
    let root = scratch("move");
    let old = root.join("Library/Caches/fallow");
    let new = root.join("Library/Application Support/Fallow");
    std::fs::create_dir_all(old.join("runs/1")).unwrap();
    std::fs::write(old.join("runs/1/state.json"), "{}").unwrap();
    let said = move_old_data(&old, &new).unwrap().unwrap();
    assert!(said.contains("moved Fallow's data from"), "{said}");
    assert!(!old.exists());
    assert_eq!(
        std::fs::read_to_string(new.join("runs/1/state.json")).unwrap(),
        "{}"
    );
    // A second start finds the new folder and moves nothing.
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("other"), "x").unwrap();
    assert_eq!(move_old_data(&old, &new).unwrap(), None);
    assert!(old.join("other").exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_folder_with_nothing_of_fallows_in_it_still_gets_the_data() {
    // 006's settings file in `~/Library/Application Support/fallow` (the same folder on a
    // case-insensitive volume) or a test's socket do not count as data.
    let root = scratch("leftover");
    let old = root.join("old");
    let new = root.join("new");
    std::fs::create_dir_all(old.join("runs")).unwrap();
    std::fs::write(old.join("service.sock"), "").unwrap();
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("settings.toml"), "# 006").unwrap();
    std::fs::write(new.join("service.sock-test-1-0"), "").unwrap();
    assert!(move_old_data(&old, &new).unwrap().is_some());
    assert!(new.join("runs").is_dir());
    assert!(new.join("settings.toml").exists());
    assert!(!old.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_move_that_failed_midway_is_finished_at_the_next_start() {
    let root = scratch("resume");
    let old = root.join("old");
    let new = root.join("new");
    std::fs::create_dir_all(old.join("conversations")).unwrap();
    // The first start moved `runs`, then failed; its mark is left in the new folder.
    std::fs::create_dir_all(new.join("runs")).unwrap();
    std::fs::write(new.join(".moving-from-caches"), "old").unwrap();
    assert!(move_old_data(&old, &new).unwrap().is_some());
    assert!(new.join("runs").is_dir() && new.join("conversations").is_dir());
    assert!(!old.exists());
    assert!(!new.join(".moving-from-caches").exists());
    // Without the mark, a new folder holding data is left alone.
    std::fs::create_dir_all(old.join("x")).unwrap();
    assert_eq!(move_old_data(&old, &new).unwrap(), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn nothing_moves_without_an_old_folder() {
    let root = scratch("none");
    let new = root.join("new");
    assert_eq!(move_old_data(&root.join("missing"), &new).unwrap(), None);
    assert!(!new.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_failed_move_is_an_error() {
    let root = scratch("fail");
    let old = root.join("old");
    std::fs::create_dir_all(old.join("runs")).unwrap();
    // A file in the way of the new folder's parent.
    std::fs::write(root.join("file"), "").unwrap();
    let e = move_old_data(&old, &root.join("file/new")).unwrap_err();
    assert!(e.contains("cannot move Fallow's data"), "{e}");
    assert!(old.join("runs").is_dir());
    let _ = std::fs::remove_dir_all(&root);
}

/// `prepare_data` reads `HOME`: these run one at a time, with a `HOME` of their own.
#[cfg(target_os = "macos")]
mod with_home {
    use super::*;
    use std::sync::Mutex;

    static HOME: Mutex<()> = Mutex::new(());

    fn with_home<T>(home: &Path, f: impl FnOnce() -> T) -> T {
        let _g = HOME.lock().unwrap_or_else(|e| e.into_inner());
        let was = std::env::var_os("HOME");
        // SAFETY: the tests of this binary that read the environment hold `HOME`'s lock.
        unsafe { std::env::set_var("HOME", home) };
        let r = f();
        // SAFETY: as above.
        unsafe {
            match was {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
        }
        r
    }

    #[test]
    fn nothing_moves_while_an_older_fallow_answers_on_the_old_socket() {
        use std::io::{BufRead, Write};
        let home = scratch("older");
        let old = home.join("Library/Caches/fallow");
        std::fs::create_dir_all(old.join("runs")).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(old.join("service.sock")).unwrap();
        let server = std::thread::spawn(move || {
            if let Ok((s, _)) = listener.accept() {
                let mut line = String::new();
                let mut r = std::io::BufReader::new(s.try_clone().unwrap());
                let _ = r.read_line(&mut line);
                let _ = (&s).write_all(b"{\"ok\":true,\"value\":[]}\n");
            }
        });
        let new = home.join("Library/Application Support/Fallow");
        let r = with_home(&home, || fallow::service::prepare_data(&new));
        assert_eq!(r, Err(fallow::service::OLDER_RUNNING.to_string()));
        assert!(old.join("runs").is_dir());
        assert!(!new.exists());
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_older_fallow_that_is_slow_to_answer_still_holds_the_move() {
        let home = scratch("slow");
        let old = home.join("Library/Caches/fallow");
        std::fs::create_dir_all(old.join("runs")).unwrap();
        // Listens, never answers.
        let _listener = std::os::unix::net::UnixListener::bind(old.join("service.sock")).unwrap();
        let new = home.join("Library/Application Support/Fallow");
        let r = with_home(&home, || fallow::service::prepare_data(&new));
        assert_eq!(r, Err(fallow::service::OLDER_RUNNING.to_string()));
        assert!(old.join("runs").is_dir());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn fallow_moves_the_data_before_it_opens_the_services_log() {
        // `fallow`'s order (review): the log is opened in the new folder only after the move, so
        // it never makes the new folder look used.
        let home = scratch("launch");
        let old = home.join("Library/Caches/fallow");
        std::fs::create_dir_all(old.join("runs/1")).unwrap();
        std::fs::write(old.join("runs/1/state.json"), "{}").unwrap();
        let new = home.join("Library/Application Support/Fallow");
        // `fallow` asked the service first, which makes the new (empty) folder for its socket.
        std::fs::create_dir_all(&new).unwrap();
        let log = with_home(&home, || fallow::service::service_log(&new)).unwrap();
        drop(log);
        assert!(new.join("service.log").exists());
        assert!(new.join("runs/1/state.json").exists());
        assert!(!old.exists());
        // The service's own start then moves nothing.
        with_home(&home, || fallow::service::prepare_data(&new)).unwrap();
        assert!(new.join("runs/1/state.json").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn fallow_stop_stops_an_older_fallow_and_waits_for_it() {
        use std::io::BufRead;
        let home = scratch("stopold");
        let old = home.join("Library/Caches/fallow");
        std::fs::create_dir_all(&old).unwrap();
        let sock = old.join("service.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let sock2 = sock.clone();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut line = String::new();
            let _ = std::io::BufReader::new(s).read_line(&mut line);
            // Ends a moment later, as a service stopping does.
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(listener);
            let _ = std::fs::remove_file(&sock2);
            line
        });
        assert!(with_home(&home, fallow::service::stop_older));
        // What it waits for: nothing listens there any more (the file's removal comes a moment
        // after, as with a real service, so its absence is not checked: that raced on CI).
        assert!(
            std::os::unix::net::UnixStream::connect(&sock).is_err(),
            "returned before the older service ended"
        );
        assert!(server.join().unwrap().contains("\"stop\""));
        // Nobody left: nothing to stop.
        assert!(!with_home(&home, fallow::service::stop_older));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_stale_socket_does_not_hold_the_move() {
        let home = scratch("stale");
        let old = home.join("Library/Caches/fallow");
        std::fs::create_dir_all(old.join("runs")).unwrap();
        // A socket file nobody listens on.
        drop(std::os::unix::net::UnixListener::bind(old.join("service.sock")).unwrap());
        let new = home.join("Library/Application Support/Fallow");
        with_home(&home, || fallow::service::prepare_data(&new)).unwrap();
        assert!(new.join("runs").is_dir());
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn another_data_folder_is_never_moved_into() {
        // `--data-dir`: the engineer's own folder is not the platform's; nothing moves.
        let home = scratch("override");
        let old = home.join("Library/Caches/fallow");
        std::fs::create_dir_all(old.join("runs")).unwrap();
        let other = home.join("elsewhere");
        with_home(&home, || fallow::service::prepare_data(&other)).unwrap();
        assert!(old.join("runs").is_dir());
        assert!(!other.exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A pre-016 service on the old socket: answers `quit` with `running`, stops on `confirmed`.
    fn older_service(
        old: &Path,
        running: serde_json::Value,
    ) -> std::thread::JoinHandle<Vec<String>> {
        use std::io::{BufRead, Write};
        std::fs::create_dir_all(old).unwrap();
        let sock = old.join("service.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            for conn in listener.incoming() {
                let Ok(s) = conn else { break };
                let mut line = String::new();
                let _ = std::io::BufReader::new(s.try_clone().unwrap()).read_line(&mut line);
                seen.push(line.clone());
                if line.contains("\"confirmed\":true") {
                    let _ = (&s).write_all(b"{\"ok\":true,\"pid\":1}\n");
                    drop(listener);
                    let _ = std::fs::remove_file(&sock);
                    break;
                }
                let answer = serde_json::json!({ "ok": true, "running": running });
                let _ = (&s).write_all(format!("{answer}\n").as_bytes());
                // A caller that only checks the socket connects and goes: keep listening.
            }
            seen
        })
    }

    #[test]
    fn an_older_fallow_with_nothing_running_quits_without_a_question() {
        let home = scratch("quitold");
        let old = home.join("Library/Caches/fallow");
        let server = older_service(&old, serde_json::json!([]));
        let r = with_home(&home, || {
            fallow::service::quit_older(|_| panic!("nothing runs: no question"))
        });
        assert_eq!(r, Ok(true));
        let seen = server.join().unwrap();
        assert!(
            seen.iter().any(|l| l.contains("\"confirmed\":true")),
            "{seen:?}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_older_fallow_with_work_quits_only_on_the_engineers_word() {
        let home = scratch("keepold");
        let old = home.join("Library/Caches/fallow");
        let running =
            serde_json::json!([{ "kind": "loop", "name": "001-calc", "project": "work" }]);
        let _server = older_service(&old, running);
        let asked = std::sync::Mutex::new(Vec::new());
        let r = with_home(&home, || {
            fallow::service::quit_older(|names| {
                asked.lock().unwrap().extend(names.iter().cloned());
                false
            })
        });
        assert_eq!(r, Ok(false));
        assert_eq!(
            *asked.lock().unwrap(),
            vec!["Loop 001-calc (work)".to_string()]
        );
        // Still running: nothing moves.
        assert!(fallow::service::listening(&old.join("service.sock")));
        let _ = std::fs::remove_dir_all(&home);
    }
}
