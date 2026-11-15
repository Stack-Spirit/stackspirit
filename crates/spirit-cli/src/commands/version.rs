//! `spirit version`.

use serde::Serialize;

use crate::error::CliError;
use crate::output::Output;

#[derive(Debug, Serialize)]
struct VersionInfo {
    name: &'static str,
    version: &'static str,
}

pub(crate) fn run(output: &mut Output<'_>) -> Result<(), CliError> {
    let info = VersionInfo {
        name: "spirit",
        version: env!("CARGO_PKG_VERSION"),
    };
    if output.is_json() {
        return output.json(&info);
    }
    output.text(format!("spirit {}", info.version))
}
