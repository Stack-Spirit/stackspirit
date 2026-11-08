//! Shared private-tmux harness for integration tests.
//!
//! Every test runs against its own `tmux -L vowp-test-<hex>` server, and the
//! guard kills that server on drop. The default server is never touched.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use vowp_tmux::Tmux;

static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A private tmux server plus its guard.
pub struct PrivateTmux {
    /// Manager bound to the private socket.
    pub tmux: Tmux,
    /// The private socket name.
    pub socket: String,
}

impl PrivateTmux {
    /// Start a guard for a fresh private socket, or return `None` with a
    /// printed notice when tmux is not installed.
    pub fn new() -> Option<Self> {
        let socket = socket_name();
        let tmux = Tmux::new(Some(socket.clone()));
        if !tmux.has_tmux() {
            eprintln!("SKIP: tmux not installed");
            return None;
        }
        Some(Self { tmux, socket })
    }

    /// Run a raw tmux command against the private server, bypassing the safe
    /// manager (used to install foreign/untagged fixtures).
    pub fn raw(&self, args: &[&str]) -> std::process::Output {
        Command::new("tmux")
            .arg("-L")
            .arg(&self.socket)
            .args(args)
            .env_remove("TMUX")
            .output()
            .expect("run raw tmux")
    }
}

impl Drop for PrivateTmux {
    fn drop(&mut self) {
        let _ = self.tmux.kill_server_for_tests();
    }
}

fn socket_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let pid = u64::from(std::process::id());
    let counter = SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = nanos ^ (pid << 32) ^ counter.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("vowp-test-{:08x}", mixed as u32)
}
