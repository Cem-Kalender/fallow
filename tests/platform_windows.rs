//! Windows reader tests (T030): TCP table parsing runs on every host; command-line splitting
//! uses the system's `CommandLineToArgvW`, so it runs on Windows.

use std::net::SocketAddr;

use fallow::procmon::platform::parse::{parse_tcp_table_v4, parse_tcp_table_v6};
use fallow::procmon::snapshot::ConnState;

fn port_dword(p: u16) -> [u8; 4] {
    let b = p.to_be_bytes();
    [b[0], b[1], 0, 0]
}

#[test]
fn tcp_table_v4() {
    let mut b = 2u32.to_ne_bytes().to_vec();
    for (state, l, lp, r, rp, pid) in [
        (
            5u32,
            [10, 0, 0, 2],
            50000u16,
            [160, 79, 104, 10],
            443u16,
            4242u32,
        ),
        (2, [127, 0, 0, 1], 11434, [0, 0, 0, 0], 0, 77),
    ] {
        b.extend_from_slice(&state.to_ne_bytes());
        b.extend_from_slice(&l);
        b.extend_from_slice(&port_dword(lp));
        b.extend_from_slice(&r);
        b.extend_from_slice(&port_dword(rp));
        b.extend_from_slice(&pid.to_ne_bytes());
    }
    let mut rows = Vec::new();
    parse_tcp_table_v4(&b, &mut rows).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].local,
        "10.0.0.2:50000".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        rows[0].remote,
        "160.79.104.10:443".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        (rows[0].state, rows[0].owner),
        (ConnState::Established, 4242)
    );
    assert_eq!((rows[1].state, rows[1].owner), (ConnState::Listen, 77));
    // A count larger than the buffer stops cleanly.
    let mut short = 3u32.to_ne_bytes().to_vec();
    short.extend_from_slice(&b[4..]);
    let mut rows = Vec::new();
    assert!(parse_tcp_table_v4(&short, &mut rows).is_none());
    assert_eq!(rows.len(), 2);
}

#[test]
fn tcp_table_v6() {
    let mut b = 1u32.to_ne_bytes().to_vec();
    let local: std::net::Ipv6Addr = "::1".parse().unwrap();
    let remote: std::net::Ipv6Addr = "2607:6bc0::10".parse().unwrap();
    b.extend_from_slice(&local.octets());
    b.extend_from_slice(&0u32.to_ne_bytes()); // scope
    b.extend_from_slice(&port_dword(50000));
    b.extend_from_slice(&remote.octets());
    b.extend_from_slice(&0u32.to_ne_bytes());
    b.extend_from_slice(&port_dword(443));
    b.extend_from_slice(&5u32.to_ne_bytes()); // ESTAB
    b.extend_from_slice(&9001u32.to_ne_bytes());
    let mut rows = Vec::new();
    parse_tcp_table_v6(&b, &mut rows).unwrap();
    assert_eq!(rows[0].local, "[::1]:50000".parse::<SocketAddr>().unwrap());
    assert_eq!(
        rows[0].remote,
        "[2607:6bc0::10]:443".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        (rows[0].state, rows[0].owner),
        (ConnState::Established, 9001)
    );
}

#[cfg(windows)]
#[test]
fn command_line_splitting_follows_windows_rules() {
    use fallow::procmon::platform::split_command_line;
    let s = |l: &str| {
        split_command_line(l)
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        s(r#""C:\Program Files\nodejs\node.exe" C:\x\cli.js --resume"#),
        [
            r"C:\Program Files\nodejs\node.exe",
            r"C:\x\cli.js",
            "--resume"
        ]
    );
    assert_eq!(s(r#"app.exe "a b" c\"d"#), ["app.exe", "a b", "c\"d"]);
    assert_eq!(
        s(r#"app.exe "trailing\\" x"#),
        ["app.exe", "trailing\\", "x"]
    );
    assert!(s("").is_empty());
}
