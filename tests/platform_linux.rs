//! Linux reader parsers against fixtures (T030): `/proc/net/tcp{,6}`, `smaps_rollup`, cmdline.
//! Pure functions, so these run on every host. The `/proc/net` encoding is little-endian.

use std::net::SocketAddr;

use fallow::procmon::platform::parse::{
    TcpRow, parse_proc_net_tcp, parse_smaps_rollup_pss, parse_statm_resident, split_nul_cmdline,
};
use fallow::procmon::snapshot::ConnState;
use proptest::prelude::*;

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

#[cfg(target_endian = "little")]
#[test]
fn proc_net_tcp_fixture() {
    let mut rows = Vec::new();
    parse_proc_net_tcp(include_str!("fixtures/proc_net_tcp.txt"), &mut rows);
    let want = [
        TcpRow {
            local: a("127.0.0.1:11434"),
            remote: a("0.0.0.0:0"),
            state: ConnState::Listen,
            owner: 41001,
        },
        TcpRow {
            local: a("10.0.0.2:50000"),
            remote: a("160.79.104.10:443"),
            state: ConnState::Established,
            owner: 41002,
        },
        TcpRow {
            local: a("127.0.0.1:40000"),
            remote: a("127.0.0.1:11434"),
            state: ConnState::Established,
            owner: 41003,
        },
        TcpRow {
            local: a("127.0.0.1:11434"),
            remote: a("127.0.0.1:40000"),
            state: ConnState::Established,
            owner: 41004,
        },
        // Row 4 (TIME_WAIT) is skipped.
        TcpRow {
            local: a("0.0.0.0:22"),
            remote: a("0.0.0.0:0"),
            state: ConnState::Listen,
            owner: 18200,
        },
    ];
    assert_eq!(rows, want);
}

#[cfg(target_endian = "little")]
#[test]
fn proc_net_tcp6_fixture() {
    let mut rows = Vec::new();
    parse_proc_net_tcp(include_str!("fixtures/proc_net_tcp6.txt"), &mut rows);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].local, a("[::1]:11434"));
    assert_eq!(rows[0].state, ConnState::Listen);
    assert_eq!(rows[1].local, a("[::ffff:10.0.0.2]:50002"));
    assert_eq!(rows[1].remote, a("[::ffff:160.79.104.10]:443"));
    assert_eq!(rows[1].owner, 52002);
}

#[test]
fn garbage_lines_are_skipped() {
    let mut rows = Vec::new();
    parse_proc_net_tcp(
        "header\n   0: zz:qq 00000000:0000 0A\n\n1: 0100007F",
        &mut rows,
    );
    assert!(rows.is_empty());
}

#[test]
fn smaps_rollup_pss() {
    assert_eq!(
        parse_smaps_rollup_pss(include_str!("fixtures/smaps_rollup.txt")),
        Some(428_085 * 1024)
    );
    assert_eq!(
        parse_smaps_rollup_pss("Rss: 10 kB\n"),
        None,
        "Pss_* lines are not Pss"
    );
    assert_eq!(
        parse_smaps_rollup_pss("Pss_Anon: 5 kB\nPss: 7 kB\n"),
        Some(7 * 1024)
    );
}

#[test]
fn statm_resident_pages() {
    assert_eq!(parse_statm_resident("1000 250 30 4 0 200 0\n"), Some(250));
    assert_eq!(parse_statm_resident(""), None);
}

#[test]
fn cmdline_nul_splitting() {
    assert_eq!(
        split_nul_cmdline(b"node\0/x/cli.js\0--resume\0"),
        ["node", "/x/cli.js", "--resume"].map(Box::from)
    );
    assert_eq!(
        split_nul_cmdline(b"a\0\0b\0"),
        ["a", "", "b"].map(Box::from),
        "empty middle arg kept"
    );
    assert_eq!(
        split_nul_cmdline(b"setproctitle style"),
        ["setproctitle style"].map(Box::from)
    );
    assert!(
        split_nul_cmdline(b"").is_empty(),
        "kernel threads have no cmdline"
    );
}

proptest! {
    #[test]
    fn cmdline_round_trip(args in prop::collection::vec("[^\0]{0,12}", 1..8)) {
        let mut raw = Vec::new();
        for a in &args {
            raw.extend_from_slice(a.as_bytes());
            raw.push(0);
        }
        let back: Vec<String> = split_nul_cmdline(&raw).iter().map(|s| s.to_string()).collect();
        prop_assert_eq!(back, args);
    }

    #[test]
    fn proc_net_v4_round_trip(ip in any::<[u8; 4]>(), port in any::<u16>(), rport in any::<u16>()) {
        let word = format!("{:08X}", u32::from_ne_bytes(ip));
        let line = format!(
            "h\n   0: {word}:{port:04X} {word}:{rport:04X} 01 00000000:00000000 00:00000000 00000000 1000 0 777 1\n"
        );
        let mut rows = Vec::new();
        parse_proc_net_tcp(&line, &mut rows);
        prop_assert_eq!(rows.len(), 1);
        prop_assert_eq!(rows[0].local, SocketAddr::from((ip, port)));
        prop_assert_eq!(rows[0].remote.port(), rport);
        prop_assert_eq!(rows[0].owner, 777);
    }
}
