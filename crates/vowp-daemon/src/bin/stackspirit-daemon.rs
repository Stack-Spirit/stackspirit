//! The `stackspirit-daemon` binary: the StackSpirit Engine name for the exact same daemon.
//!
//! It runs the identical entry point as `vowp-daemon`, so arguments, the loopback binding and
//! the legacy first stdout line `vowp-daemon: listening on http://…` (a wire contract in v6)
//! are the same.

use std::process::ExitCode;

#[path = "../main.rs"]
mod daemon;

fn main() -> ExitCode {
    daemon::main()
}
