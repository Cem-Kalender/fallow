//! Fallback paths (T121, FR-019, R-3). Each platform module asks these policy functions what
//! to report when its preferred read fails, so every fallback is forced here on every host.

use fallow::procmon::platform::parse::{
    linux_footprint, macos_footprint, windows_cwd, windows_footprint,
};
use fallow::procmon::snapshot::{CwdState, FootprintMetric};

#[test]
fn linux_falls_back_to_statm_rss() {
    let smaps = "Rss: 12 kB\nPss: 8 kB\n";
    let statm = "500 100 20 1 0 50 0";
    let pss = linux_footprint(Some(smaps), Some(statm), 4096).unwrap();
    assert_eq!((pss.bytes, pss.metric), (8 * 1024, FootprintMetric::Pss));
    // smaps_rollup missing (kernel < 4.14) or unreadable.
    let rss = linux_footprint(None, Some(statm), 4096).unwrap();
    assert_eq!((rss.bytes, rss.metric), (100 * 4096, FootprintMetric::Rss));
    // smaps_rollup present but without a Pss line.
    let rss = linux_footprint(Some("Rss: 12 kB\n"), Some(statm), 4096).unwrap();
    assert_eq!(rss.metric, FootprintMetric::Rss);
    assert!(linux_footprint(None, None, 4096).is_none());
}

#[test]
fn windows_falls_back_to_working_set() {
    let p = windows_footprint(Some(1000), Some(3000)).unwrap();
    assert_eq!(
        (p.bytes, p.metric),
        (1000, FootprintMetric::PrivateWorkingSet)
    );
    // PROCESS_MEMORY_COUNTERS_EX2 rejected (builds before 1809).
    let w = windows_footprint(None, Some(3000)).unwrap();
    assert_eq!((w.bytes, w.metric), (3000, FootprintMetric::WorkingSet));
    assert!(windows_footprint(None, None).is_none());
}

#[test]
fn windows_failed_peb_read_is_unavailable_with_reason() {
    assert_eq!(
        windows_cwd(Err("access denied")),
        CwdState::Unavailable("access denied".into())
    );
    assert_eq!(
        windows_cwd(Ok(r"C:\work\".into())),
        CwdState::Known(r"C:\work".into())
    );
}

#[test]
fn macos_never_reports_a_wrong_metric() {
    let f = macos_footprint(Some(42)).unwrap();
    assert_eq!((f.bytes, f.metric), (42, FootprintMetric::PhysFootprint));
    assert_eq!(macos_footprint(None), Err("proc_pid_rusage failed"));
}

#[test]
fn ui_labels_match_the_metric_used() {
    // contracts/ui-commands.md "Displayed semantics": footprint, private WS, PSS / RSS.
    let labels = [
        (FootprintMetric::PhysFootprint, "footprint"),
        (FootprintMetric::PrivateWorkingSet, "private WS"),
        (FootprintMetric::Pss, "PSS"),
        (FootprintMetric::Rss, "RSS"),
        (FootprintMetric::WorkingSet, "WS"),
    ];
    for (m, l) in labels {
        assert_eq!(m.label(), l);
    }
}
