//! The `vowp` command-line binary.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        Some("--version" | "-V") => {
            println!("vowp {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(command) => {
            eprintln!("vowp: unknown command `{command}`");
            eprintln!("usage: vowp --version");
            ExitCode::from(2)
        }
        None => {
            eprintln!("usage: vowp --version");
            ExitCode::from(2)
        }
    }
}
