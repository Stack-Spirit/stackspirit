//! Signal watcher integration tests: the documented `~/.vowp/stop` path still works.
//!
//! Every test uses a temporary node home; no real home directory, tmux server or harness
//! is touched. The SIGTERM/SIGINT path is exercised by spawning the real binary in
//! `tests/up.rs`.

use std::time::{Duration, Instant};

use vowp_cli::signals::{SignalWatcher, clear_stop, heartbeat_file, mark_stopped, stop_file, stop_requested};
use vowp_protocol::{CancelToken, Timestamp};
use vowp_store::NodePaths;

/// Polls `predicate` for up to two seconds.
fn eventually(mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn signals_stop_file_still_works() {
    let home = tempfile::tempdir().expect("temp home");
    let paths = NodePaths::new(home.path());
    let watcher = SignalWatcher::install(&paths).expect("install watcher");
    let token: CancelToken = watcher.cancel_token();
    assert!(!token.is_cancelled(), "no stop request yet");
    assert!(!stop_requested(&paths));

    std::fs::write(stop_file(&paths), "now").expect("write stop file");
    assert!(
        eventually(|| token.is_cancelled()),
        "the stop file cancels the same token the supervisor runs with"
    );
    assert!(!watcher.cancelled_by_signal(), "the stop file is not a signal");

    assert!(clear_stop(&paths).expect("clear stop"), "the file existed");
    assert!(!stop_requested(&paths));
    assert!(
        !clear_stop(&paths).expect("clear missing stop"),
        "clearing a missing stop file reports false"
    );
}

#[test]
fn stopped_heartbeat_records_the_marker_atomically() {
    let home = tempfile::tempdir().expect("temp home");
    let paths = NodePaths::new(home.path());
    mark_stopped(&paths, Timestamp::from_unix_secs(1_700_000_000)).expect("mark stopped");

    let text = std::fs::read_to_string(heartbeat_file(&paths)).expect("heartbeat");
    assert!(text.starts_with("1700000000\nstopped\n"), "{text:?}");
    assert!(
        !paths.root().join(".heartbeat.stopped.tmp").exists(),
        "the temporary heartbeat is renamed into place"
    );
}
