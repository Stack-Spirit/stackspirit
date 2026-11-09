//! Graceful shutdown for the supervisor: watch `~/.vowp/stop` and SIGTERM/SIGINT.
//!
//! Spec: `docs/design/crates/vowp-cli.md` §1 (`SIGTERM/SIGINT -> CancelToken`) and
//! `docs/design/architecture.md` §2 (`signal-hook` sets the supervisor's cancellation
//! token). The stop file and the two signals feed the same [`CancelToken`], so `vowp
//! down`, `systemctl --user stop` and Ctrl-C all cancel runner pools the same way.
//!
//! A second signal exits immediately with the conventional status: 130 for SIGINT and
//! 143 for SIGTERM. That gives an operator a way out when a runner refuses to stop.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::flag;
use vowp_protocol::{CancelToken, Timestamp};
use vowp_store::NodePaths;

/// How often the watcher looks for the stop file and checks the signal flag.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Exit status for a second SIGINT (`128 + SIGINT`).
pub const SIGINT_EXIT: i32 = 130;

/// Exit status for a second SIGTERM (`128 + SIGTERM`).
pub const SIGTERM_EXIT: i32 = 143;

/// Returns the path of the stop file (`<home>/stop`).
pub fn stop_file(paths: &NodePaths) -> PathBuf {
    paths.root().join("stop")
}

/// Returns the path of the heartbeat file (`<home>/heartbeat`).
pub fn heartbeat_file(paths: &NodePaths) -> PathBuf {
    paths.root().join("heartbeat")
}

/// Removes a stale stop file, returning whether one existed.
pub fn clear_stop(paths: &NodePaths) -> std::io::Result<bool> {
    let path = stop_file(paths);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Returns whether a shutdown was requested through the stop file.
pub fn stop_requested(paths: &NodePaths) -> bool {
    stop_file(paths).is_file()
}

/// Rewrites the heartbeat with a trailing `stopped` marker after a clean shutdown.
///
/// The first line remains the Unix timestamp, so the watchdog's `head -n 1` contract
/// still sees a normal heartbeat. The second line tells an operator and tests that the
/// supervisor exited on purpose rather than crashing. A later `vowp up` overwrites the
/// file with numeric heartbeats on its first tick.
pub fn mark_stopped(paths: &NodePaths, at: Timestamp) -> std::io::Result<()> {
    let path = heartbeat_file(paths);
    let temp = paths.root().join(".heartbeat.stopped.tmp");
    std::fs::write(&temp, format!("{}\nstopped\n", at.unix_secs()))?;
    std::fs::rename(&temp, &path)
}

/// Watches the stop file and SIGTERM/SIGINT, cancelling a [`CancelToken`] on the first
/// request.
///
/// The watcher stops when dropped, when [`SignalWatcher::stop`] is called, or after it has
/// observed a shutdown request. Cloning the token shares the cancellation flag with every
/// runner. Signal handlers stay installed for the life of the process; a second signal
/// exits immediately even after the watcher thread has finished.
pub struct SignalWatcher {
    cancel: CancelToken,
    signalled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    stop_file: PathBuf,
    handle: Option<JoinHandle<()>>,
}

impl SignalWatcher {
    /// Starts watching `<home>/stop`, SIGTERM and SIGINT.
    ///
    /// Returns an I/O error when the process cannot register the signal handlers; the
    /// supervisor must not run without them.
    pub fn install(paths: &NodePaths) -> std::io::Result<Self> {
        let cancel = CancelToken::default();
        let stop = Arc::new(AtomicBool::new(false));
        let signalled = Arc::new(AtomicBool::new(false));
        let stop_file = stop_file(paths);

        // The conditional-shutdown action must be registered before the flag that arms it:
        // on the first signal it sees `false`, on the second it exits without cleanup.
        flag::register_conditional_shutdown(SIGTERM, SIGTERM_EXIT, Arc::clone(&signalled))?;
        flag::register_conditional_shutdown(SIGINT, SIGINT_EXIT, Arc::clone(&signalled))?;
        flag::register(SIGTERM, Arc::clone(&signalled))?;
        flag::register(SIGINT, Arc::clone(&signalled))?;

        let thread_cancel = cancel.clone();
        let thread_signalled = Arc::clone(&signalled);
        let thread_stop = Arc::clone(&stop);
        let thread_file = stop_file.clone();
        let handle = thread::Builder::new().name("vowp-signals".to_owned()).spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                if thread_signalled.load(Ordering::SeqCst) || thread_file.is_file() {
                    thread_cancel.cancel();
                    break;
                }
                thread::sleep(POLL_INTERVAL);
            }
        })?;

        Ok(Self {
            cancel,
            signalled,
            stop,
            stop_file,
            handle: Some(handle),
        })
    }

    /// Returns the shared cancellation token.
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Returns whether a signal (rather than the stop file) requested shutdown.
    pub fn cancelled_by_signal(&self) -> bool {
        self.signalled.load(Ordering::SeqCst)
    }

    /// Returns the stop file path this watcher observes.
    pub fn stop_file(&self) -> &PathBuf {
        &self.stop_file
    }

    /// Stops the watcher thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    /// Signals the watcher to finish and joins it.
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Cancels the token directly (used when `down --force` is not needed).
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl Drop for SignalWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn stop_file_cancels_watcher() {
        let home = tempfile::tempdir().expect("temp home");
        let paths = NodePaths::new(home.path());
        let watcher = SignalWatcher::install(&paths).expect("install watcher");
        assert!(!watcher.cancel_token().is_cancelled());

        std::fs::write(stop_file(&paths), "stop").expect("write stop file");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !watcher.cancel_token().is_cancelled() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            watcher.cancel_token().is_cancelled(),
            "the stop file must cancel the supervisor token"
        );
        assert!(!watcher.cancelled_by_signal());
        assert!(clear_stop(&paths).expect("clear stop"));
        assert!(!stop_requested(&paths));
    }

    #[test]
    fn stopped_heartbeat_keeps_the_timestamp_on_the_first_line() {
        let home = tempfile::tempdir().expect("temp home");
        let paths = NodePaths::new(home.path());
        mark_stopped(&paths, Timestamp::from_unix_secs(1_700_000_000)).expect("mark stopped");
        let text = std::fs::read_to_string(heartbeat_file(&paths)).expect("heartbeat");
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("1700000000"));
        assert_eq!(lines.next(), Some("stopped"));
    }
}
