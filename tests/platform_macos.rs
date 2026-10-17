//! macOS reader tests (T030): the KERN_PROCARGS2 parser and the `socket_fdinfo` reader run on
//! every host; the buffer-zeroing hook runs on macOS against the real reader.

use std::net::SocketAddr;

use fallow::procmon::platform::parse::{parse_procargs2, parse_socket_fdinfo, socket_fdinfo as o};
use fallow::procmon::snapshot::ConnState;
use proptest::prelude::*;

/// argc, exec path padded to 8 bytes, argv, then an environment that must never be read.
fn procargs(path: &str, argv: &[&str], env: &[&str]) -> Vec<u8> {
    let mut b = (argv.len() as i32).to_ne_bytes().to_vec();
    b.extend_from_slice(path.as_bytes());
    b.push(0);
    while (b.len() - 4) % 8 != 0 {
        b.push(0);
    }
    for a in argv.iter().chain(env) {
        b.extend_from_slice(a.as_bytes());
        b.push(0);
    }
    b
}

fn strings(v: Vec<Box<str>>) -> Vec<String> {
    v.into_iter().map(String::from).collect()
}

#[test]
fn procargs_stops_at_argc() {
    let b = procargs(
        "/usr/local/bin/node",
        &["node", "/x/cli.js"],
        &["API_KEY=secret"],
    );
    assert_eq!(strings(parse_procargs2(&b).unwrap()), ["node", "/x/cli.js"]);
}

#[test]
fn procargs_leading_empty_arguments() {
    let b = procargs("/bin/x", &["", "", "run"], &["TOKEN=secret"]);
    assert_eq!(strings(parse_procargs2(&b).unwrap()), ["", "", "run"]);
}

#[test]
fn procargs_truncated_buffer() {
    let b = procargs("/bin/x", &["alpha", "beta"], &[]);
    let cut = &b[..b.len() - 3];
    assert_eq!(strings(parse_procargs2(cut).unwrap()), ["alpha", "be"]);
    assert!(parse_procargs2(&b[..2]).is_none());
    assert!(parse_procargs2(&(-1i32).to_ne_bytes()).is_none());
}

proptest! {
    #[test]
    fn procargs_never_returns_environment(
        path in "/[a-z]{1,20}",
        argv in prop::collection::vec("[a-zA-Z0-9 ./=-]{0,10}", 0..6),
        env in prop::collection::vec("[A-Z]{1,6}=CANARY[a-z0-9]{1,8}", 0..4),
    ) {
        let argv_ref: Vec<&str> = argv.iter().map(String::as_str).collect();
        let env_ref: Vec<&str> = env.iter().map(String::as_str).collect();
        let parsed = parse_procargs2(&procargs(&path, &argv_ref, &env_ref)).unwrap();
        prop_assert_eq!(strings(parsed.clone()), argv.clone());
        prop_assert!(parsed.iter().all(|a| !a.contains("CANARY")));
    }

    #[test]
    fn procargs_any_bytes_never_panic(b in prop::collection::vec(any::<u8>(), 0..64)) {
        let _ = parse_procargs2(&b);
    }
}

/// A `socket_fdinfo` image with the fields Fallow reads.
fn fdinfo(v6: bool, local: SocketAddr, remote: SocketAddr, state: i32) -> Vec<u8> {
    let mut b = vec![0u8; o::MIN_LEN + 64];
    b[o::SOI_KIND..o::SOI_KIND + 4].copy_from_slice(&o::SOCKINFO_TCP.to_ne_bytes());
    b[o::FPORT..o::FPORT + 2].copy_from_slice(&remote.port().to_be_bytes());
    b[o::LPORT..o::LPORT + 2].copy_from_slice(&local.port().to_be_bytes());
    b[o::VFLAG] = if v6 { o::INI_IPV6 } else { o::INI_IPV4 };
    let put = |b: &mut Vec<u8>, off: usize, a: SocketAddr| match a {
        SocketAddr::V4(a) => b[off + 12..off + 16].copy_from_slice(&a.ip().octets()),
        SocketAddr::V6(a) => b[off..off + 16].copy_from_slice(&a.ip().octets()),
    };
    put(&mut b, o::FADDR, remote);
    put(&mut b, o::LADDR, local);
    b[o::TCP_STATE..o::TCP_STATE + 4].copy_from_slice(&state.to_ne_bytes());
    b
}

#[test]
fn socket_fdinfo_v4_and_v6() {
    let (l, r) = (
        "10.0.0.2:50000".parse().unwrap(),
        "160.79.104.10:443".parse().unwrap(),
    );
    let row = parse_socket_fdinfo(&fdinfo(false, l, r, o::TSI_S_ESTABLISHED), 42).unwrap();
    assert_eq!(
        (row.local, row.remote, row.state, row.owner),
        (l, r, ConnState::Established, 42)
    );
    let (l6, r6) = ("[::1]:11434".parse().unwrap(), "[::]:0".parse().unwrap());
    let row = parse_socket_fdinfo(&fdinfo(true, l6, r6, o::TSI_S_LISTEN), 7).unwrap();
    assert_eq!((row.local, row.state), (l6, ConnState::Listen));
    let mut udp = fdinfo(false, l, r, 4);
    udp[o::SOI_KIND] = 1;
    assert!(
        parse_socket_fdinfo(&udp, 1).is_none(),
        "non-TCP sockets are skipped"
    );
    assert!(
        parse_socket_fdinfo(&udp[..10], 1).is_none(),
        "short buffers are skipped"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn procargs_buffer_is_zeroed_after_every_call() {
    use fallow::procmon::platform::{OsPlatform, Platform};
    use fallow::procmon::snapshot::ProcessKey;
    let mut p = OsPlatform::default();
    assert!(p.args_buffer_is_zeroed());
    let me = ProcessKey {
        pid: std::process::id(),
        start_time: 0,
    };
    let argv = p.cmdline(me).expect("own command line");
    assert!(!argv.is_empty());
    assert!(p.args_buffer_is_zeroed(), "after a successful read");
    for bad in [0, u32::MAX >> 1] {
        let _ = p.cmdline(ProcessKey {
            pid: bad,
            start_time: 0,
        });
        assert!(p.args_buffer_is_zeroed(), "after an error (pid {bad})");
    }
}
