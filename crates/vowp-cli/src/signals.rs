//! Graceful shutdown for the supervisor: watch `~/.vowp/stop` and cancel the loop.
//!
//! Spec: `docs/design/crates/vowp-cli.md` §1 (`SIGTERM/SIGINT -> CancelToken`) and
//! `docs/design/cli.md` §2 (`vowp down` writes `~/.vowp/stop`). The workspace declares no
//! signal crate and this binary forbids `unsafe`, so the documented stop file is the
//! cancellation channel; the supervisor stops within one poll interval and leaves the
//! same state a signal would (runners interrupted, runs marked stalled on the next start).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use vowp_protocol::CancelToken;
use vowp_store::NodePaths;

/// How often the watcher looks for the stop file.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Returns the path of the stop file (`<home>/stop`).
pub fn stop_file(paths: &NodePaths) -> PathBuf {
    paths.root().join("stop")
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

/// Watches the stop file and cancels a [`CancelToken`] when it appears.
///
/// The watcher stops when dropped, when [`SignalWatcher::stop`] is called, or after it has
/// observed the stop file. Cloning the token shares the cancellation flag with every
/// runner.
pub struct SignalWatcher {
    cancel: CancelToken,
    stop: Arc<AtomicBool>,
    stop_file: PathBuf,
    handle: Option<JoinHandle<()>>,
}

impl SignalWatcher {
    /// Starts watching `<home>/stop`.
    pub fn install(paths: &NodePaths) -> Self {
        let cancel = CancelToken::default();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_file = stop_file(paths);

        let thread_cancel = cancel.clone();
        let thread_stop = Arc::clone(&stop);
        let thread_file = stop_file.clone();
        let handle = thread::Builder::new()
            .name("vowp-signals".to_owned())
            .spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    if thread_file.is_file() {
                        thread_cancel.cancel();
                        break;
                    }
                    thread::sleep(POLL_INTERVAL);
                }
            })
            .ok();

        Self {
            cancel,
            stop,
            stop_file,
            handle,
        }
    }

    /// Returns the shared cancellation token.
    pub fn cancel_token(&self) -> CancelToken {
        self.cancel.clone()
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
        let watcher = SignalWatcher::install(&paths);
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
        assert!(clear_stop(&paths).expect("clear stop"));
        assert!(!stop_requested(&paths));
    }
}
