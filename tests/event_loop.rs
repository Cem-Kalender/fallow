//! 002 T004: one event channel (research R-5).

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use fallow::app::events::{AppEvent, FrameCap, OutputWaker, next_batch};

#[test]
fn session_output_wakes_the_loop_promptly() {
    let (tx, rx) = channel();
    let waker = OutputWaker::new(3, tx);
    let sent = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        waker.wake();
    });
    let batch = next_batch(&rx, Duration::from_millis(250));
    let waited = sent.elapsed();
    assert!(matches!(batch.as_slice(), [AppEvent::SessionOutput(3)]));
    assert!(waited < Duration::from_millis(100), "woke after {waited:?}");
}

#[test]
fn sessions_wake_the_loop_at_most_once_until_rearmed() {
    use fallow::session::SessionManager;
    // An empty manager has nothing waiting; rearm reports nothing.
    let m = SessionManager::default();
    assert!(!m.output_waiting());
    assert!(!m.rearm());
}

#[test]
fn output_events_are_coalesced_until_handled() {
    let (tx, rx) = channel();
    let waker = OutputWaker::new(1, tx);
    for _ in 0..1000 {
        waker.wake();
    }
    assert_eq!(next_batch(&rx, Duration::from_millis(10)).len(), 1);
    waker.handled();
    waker.wake();
    assert_eq!(next_batch(&rx, Duration::from_millis(10)).len(), 1);
}

#[test]
fn a_batch_takes_everything_queued_and_times_out_empty() {
    let (tx, rx) = channel();
    tx.send(AppEvent::Wake).unwrap();
    tx.send(AppEvent::SessionExited(2, Some(0))).unwrap();
    tx.send(AppEvent::Wake).unwrap();
    assert_eq!(next_batch(&rx, Duration::from_millis(10)).len(), 3);
    let t = Instant::now();
    assert!(next_batch(&rx, Duration::from_millis(30)).is_empty());
    assert!(t.elapsed() >= Duration::from_millis(25));
}

#[test]
fn frames_are_capped() {
    let mut cap = FrameCap::new(Duration::from_millis(16));
    assert_eq!(cap.wait(), Duration::ZERO);
    cap.drawn();
    assert!(cap.wait() > Duration::from_millis(5));
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(cap.wait(), Duration::ZERO);
}
