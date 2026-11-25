//! The `stackspirit` binary: the StackSpirit name for the exact same CLI as `vowp`.
//!
//! It runs the identical library entrypoint, so commands, flags and JSON output match `vowp`.

#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut io = vowp_cli::RealIo::new();
    let code = vowp_cli::run(std::env::args().collect(), &mut io);
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
