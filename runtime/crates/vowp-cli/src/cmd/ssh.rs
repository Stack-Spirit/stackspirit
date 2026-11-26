//! `stackspirit ssh <machine> [-- <ssh-args>...]`: OpenSSH through the relay.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.6 and §6.7. The command resolves the machine
//! through the cloud API and starts the system OpenSSH client with
//! `ProxyCommand=stackspirit relay proxy --machine <id> --stdio`. The proxy mints its own
//! short-lived `ssh` ticket and carries the encrypted TCP stream; this command never stores,
//! prints or forwards an SSH private key and never passes a token on a command line.

use std::process::Command;

use crate::cli::SshArgs;
use crate::cloud::{Client, list_machines, resolve_machine};
use crate::{CliError, Global, IoStreams};

/// Runs `stackspirit ssh`.
pub fn run(global: &Global, _io: &mut dyn IoStreams, args: &SshArgs) -> Result<i32, CliError> {
    let home = crate::ctx::node_paths(global)?.root().to_path_buf();
    let mut client = Client::load(&home)?;
    let machines = list_machines(&mut client)?;
    let machine = resolve_machine(&machines, &args.machine)?;
    if machine.state == "revoked" {
        return Err(CliError::Guard(format!(
            "machine {} was revoked by its owner",
            machine.id
        )));
    }
    validate_component(&machine.id, "machine id")?;
    let ssh_user = machine
        .ssh_user
        .clone()
        .filter(|user| !user.trim().is_empty())
        .unwrap_or_else(|| vowp_protocol::RelayConfig::default().ssh_user);
    validate_component(&ssh_user, "SSH user")?;

    let executable = std::env::current_exe().map_err(CliError::Io)?;
    let proxy = format!(
        "{} --home {} relay proxy --machine {} --stdio",
        shell_quote(&executable.display().to_string()),
        shell_quote(&home.display().to_string()),
        shell_quote(&machine.id),
    );
    let destination = format!("{ssh_user}@stackspirit-{}", machine.id);
    let mut command = Command::new("ssh");
    command
        .arg("-o")
        .arg(format!("ProxyCommand={proxy}"))
        .arg("-o")
        .arg(format!("HostKeyAlias=stackspirit-{}", machine.id))
        .arg(destination);
    command.args(&args.ssh_args);
    let status = command.status().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CliError::NotFound(
                "the OpenSSH client `ssh` is not installed; install it to use `stackspirit ssh`".to_owned(),
            )
        } else {
            CliError::Io(error)
        }
    })?;
    Ok(status.code().unwrap_or(1))
}

/// Refuses a value that could be parsed as an OpenSSH option or a second destination.
fn validate_component(value: &str, what: &str) -> Result<(), CliError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(CliError::Invalid(format!(
            "the {what} is not a safe OpenSSH value: {value:?}"
        )));
    }
    Ok(())
}

/// Quotes one string for the shell OpenSSH uses to run `ProxyCommand`.
fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '=' | '@'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_leaves_plain_paths_and_quotes_spaces() {
        assert_eq!(shell_quote("/usr/bin/stackspirit"), "/usr/bin/stackspirit");
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn validate_component_rejects_option_like_values() {
        assert!(validate_component("mch_01JQ8Z", "machine id").is_ok());
        assert!(validate_component("stackspirit", "SSH user").is_ok());
        assert!(validate_component("-oProxyCommand=x", "SSH user").is_err());
        assert!(validate_component("a b", "SSH user").is_err());
        assert!(validate_component("", "SSH user").is_err());
    }
}
