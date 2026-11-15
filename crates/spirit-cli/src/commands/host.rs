//! Host management commands over `spirit-core`'s host store, scanner, probe and
//! connection manager.

use std::time::Duration;

use serde::Serialize;
use spirit_core::connection::ConnectionManager;
use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::host::{
    HostDraft, HostId, HostKeyPin, HostProbe, HostProbeInput, HostProfile, HostStore,
    NewHostProfile, probe_host, verify_pin_against_scan,
};
use spirit_core::ssh::SshTarget;
use spirit_core::ssh::scan_host_keys;
use spirit_core::time::Timestamp;
use spirit_core::view::format::{short_id, truncate_chars};

use crate::args::{AddHostArgs, Cli, HostCommand, HostRefArgs, TrustArgs};
use crate::context::CliEnv;
use crate::error::CliError;
use crate::known_hosts;
use crate::output::{Output, safe_text};

const KEY_SCAN_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) async fn dispatch(
    command: &HostCommand,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    match command {
        HostCommand::Add(args) => add(args, env, output),
        HostCommand::Ls => list(env, output),
        HostCommand::Show(args) => show(args, cli, env, output),
        HostCommand::Remove(args) => remove(args, cli, env, output),
        HostCommand::Trust(args) => trust(args, cli, env, output).await,
        HostCommand::Probe(args) => probe(args, cli, env, output).await,
        HostCommand::Connect(args) => connect(args, cli, env, output).await,
        HostCommand::Disconnect(args) => disconnect(args, cli, env, output),
    }
}

fn add(args: &AddHostArgs, env: &CliEnv, output: &mut Output<'_>) -> Result<(), CliError> {
    env.paths().ensure_root()?;
    let target = parse_ssh_target(&args.ssh, args.port)?;
    let draft = HostDraft {
        label: args.label.clone(),
        ssh: target,
        identity_file: args.identity_file.clone(),
        vowp_home: args.vowp_home.clone(),
        default_project: args.default_project.clone(),
    };
    let host_key = pin(&args.algorithm, &args.fingerprint);
    let profile = env.host_store().add(NewHostProfile { draft, host_key })?;

    if output.is_json() {
        return output.json(&profile);
    }
    output.text(format!(
        "added host {} ({}) at {}",
        short_id(profile.id.as_str()),
        safe_text(&profile.label, 64),
        profile.destination()
    ))?;
    output.note(format!(
        "note: run \"spirit host trust {}\" to verify the fingerprint and install the app-private known_hosts entry",
        short_id(profile.id.as_str())
    ))
}

fn list(env: &CliEnv, output: &mut Output<'_>) -> Result<(), CliError> {
    let hosts = env.host_store().list()?;
    if output.is_json() {
        return output.json(&hosts);
    }
    if hosts.is_empty() {
        return output.text("no hosts saved");
    }
    output.text(format!(
        "{:<8}  {:<24}  {:<32}  {}",
        "ID", "LABEL", "SSH", "PIN"
    ))?;
    for host in hosts {
        output.text(format!(
            "{:<8}  {:<24}  {:<32}  {}",
            short_id(host.id.as_str()),
            safe_text(&host.label, 24),
            safe_text(&host.destination(), 32),
            truncate_chars(&host.host_key.sha256, 24)
        ))?;
    }
    Ok(())
}

fn show(
    args: &HostRefArgs,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let store = env.host_store();
    let host = resolve(store.as_ref(), args.host.as_deref(), cli.host.as_deref())?;
    if output.is_json() {
        return output.json(&host);
    }
    show_profile(&host, output)
}

fn show_profile(host: &HostProfile, output: &mut Output<'_>) -> Result<(), CliError> {
    output.text(format!("id: {}", host.id.as_str()))?;
    output.text(format!("label: {}", safe_text(&host.label, 64)))?;
    output.text(format!("ssh: {}", host.destination()))?;
    output.text(format!(
        "identity file: {}",
        host.identity_file.as_ref().map_or_else(
            || "none".to_owned(),
            |path| safe_text(&path.display().to_string(), 300)
        )
    ))?;
    output.text(format!(
        "vowp home: {}",
        host.vowp_home
            .as_deref()
            .map_or_else(|| "default".to_owned(), |home| safe_text(home, 300))
    ))?;
    output.text(format!(
        "default project: {}",
        host.default_project
            .as_deref()
            .map_or_else(|| "none".to_owned(), |project| safe_text(project, 64))
    ))?;
    output.text(format!(
        "host key: {} {}",
        safe_text(&host.host_key.algorithm, 64),
        safe_text(&host.host_key.sha256, 200)
    ))?;
    output.text(format!("pinned at: {}", host.host_key.pinned_at))?;
    output.text(format!("created at: {}", host.created_at))?;
    output.text(format!("updated at: {}", host.updated_at))?;
    output.text(format!(
        "last connected: {}",
        host.last_connected_at
            .map_or_else(|| "never".to_owned(), |at| at.to_string())
    ))
}

fn remove(
    args: &HostRefArgs,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let store = env.host_store();
    let host = resolve(store.as_ref(), args.host.as_deref(), cli.host.as_deref())?;
    store.remove(&host.id)?;

    #[derive(Serialize)]
    struct RemovedHost {
        removed: bool,
        id: HostId,
        label: String,
    }
    let removed = RemovedHost {
        removed: true,
        id: host.id.clone(),
        label: host.label.clone(),
    };
    if output.is_json() {
        return output.json(&removed);
    }
    output.text(format!(
        "removed host {} ({})",
        short_id(host.id.as_str()),
        safe_text(&host.label, 64)
    ))
}

async fn trust(
    args: &TrustArgs,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let store = env.host_store();
    let host = resolve(
        store.as_ref(),
        args.target.host.as_deref(),
        cli.host.as_deref(),
    )?;
    env.paths().ensure_root()?;

    let host_key = pin(&args.algorithm, &args.fingerprint);
    let scan = scan_host_keys(env.runner().as_ref(), &host.ssh, KEY_SCAN_TIMEOUT).await?;
    verify_pin_against_scan(&host_key, &scan.keys)?;
    known_hosts::install(
        &env.paths().known_hosts_file(),
        &host.ssh,
        &scan.keys,
        &host_key,
    )?;
    let updated = store.repin(&host.id, host_key)?;

    if output.is_json() {
        return output.json(&updated);
    }
    output.text(format!(
        "trusted {} ({}) with {}",
        short_id(updated.id.as_str()),
        updated.destination(),
        safe_text(&updated.host_key.sha256, 200)
    ))?;
    output.note("note: the app-private known_hosts entry is installed; the key file was not read")
}

async fn probe(
    args: &HostRefArgs,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let store = env.host_store();
    let host = resolve(store.as_ref(), args.host.as_deref(), cli.host.as_deref())?;
    let input = HostProbeInput {
        target: host.ssh.clone(),
        options: env.ssh_options(host.identity_file.clone()),
        vowp_home: host.vowp_home.clone(),
        pin: Some(host.host_key.clone()),
    };
    let result = probe_host(env.runner(), &input).await;

    if output.is_json() {
        output.json(&result)?;
    } else {
        show_probe(&result, output)?;
    }
    if result.save_allowed() {
        return Ok(());
    }
    Err(
        CliError::remote("host probe did not pass every required stage").with_hint(
            "Check the failing stage; run `spirit host trust` after verifying the fingerprint.",
        ),
    )
}

fn show_probe(probe: &HostProbe, output: &mut Output<'_>) -> Result<(), CliError> {
    output.text(format!(
        "reachable: {}  auth: {}  host key: {}  vowp: {}  latency: {} ms",
        probe.reachable,
        probe.auth_ok,
        probe.host_key_ok,
        probe.vowp_version.as_deref().unwrap_or("unknown"),
        probe.latency_ms
    ))?;
    for stage in &probe.stages {
        output.text(format!(
            "{:<5} {:<10} {}",
            if stage.ok { "[ok]" } else { "[fail]" },
            safe_text(&stage.stage, 20),
            safe_text(&stage.detail, 240)
        ))?;
    }
    Ok(())
}

async fn connect(
    args: &HostRefArgs,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    let store = env.host_store();
    let host = resolve(store.as_ref(), args.host.as_deref(), cli.host.as_deref())?;
    ensure_pinned(&host)?;

    let manager = ConnectionManager::default();
    let client = env.client_for(&host);
    let snapshot = manager.connect(client.as_ref(), host.id.clone()).await?;
    if let Err(error) = store.mark_connected(&host.id, Timestamp::now()) {
        output.warning(format!(
            "connected, but the local last-connected time was not saved: {}",
            error.message
        ))?;
    }

    if output.is_json() {
        return output.json(&snapshot);
    }
    output.text(format!(
        "connected to {} ({})",
        safe_text(&host.label, 64),
        host.destination()
    ))?;
    output.text(format!(
        "state: {}  latency p50: {} ms  since: {}",
        snapshot.state,
        snapshot
            .latency
            .as_ref()
            .map_or(0, |latency| latency.p50_ms),
        snapshot
            .since
            .map_or_else(|| "unknown".to_owned(), |since| since.to_string())
    ))?;
    output.note("note: this CLI connection is one-shot; later commands open their own connection")
}

fn disconnect(
    args: &HostRefArgs,
    cli: &Cli,
    env: &CliEnv,
    output: &mut Output<'_>,
) -> Result<(), CliError> {
    if args.host.is_some() || cli.host.is_some() {
        let store = env.host_store();
        resolve(store.as_ref(), args.host.as_deref(), cli.host.as_deref())?;
    }
    let snapshot = ConnectionManager::default().disconnect();
    if output.is_json() {
        return output.json(&snapshot);
    }
    output.text(format!("state: {}", snapshot.state))?;
    output.note("note: the CLI holds no background connection; every command is one-shot")
}

/// Resolves a host by exact id, exact label, or unique id prefix.
pub(crate) fn resolve(
    store: &dyn HostStore,
    local: Option<&str>,
    global: Option<&str>,
) -> Result<HostProfile, CliError> {
    let value = match (clean(local), clean(global)) {
        (Some(left), Some(right)) if left != right => {
            return Err(CliError::usage(
                "specify the host once, either as a positional value or with --host",
            ));
        }
        (Some(value), _) | (None, Some(value)) => Some(value),
        (None, None) => None,
    };

    let hosts = store.list()?;
    let Some(value) = value else {
        return match hosts.len() {
            0 => Err(CliError::host_not_found("no host is saved")
                .with_hint("Add one with `spirit host add`.")),
            1 => Ok(hosts[0].clone()),
            _ => Err(CliError::conflict("more than one host is saved")
                .with_hint("Pass a host id, a unique id prefix, or a label.")),
        };
    };

    if let Some(host) = hosts.iter().find(|host| host.id.as_str() == value) {
        return Ok(host.clone());
    }
    let label_matches: Vec<&HostProfile> = hosts
        .iter()
        .filter(|host| host.label.eq_ignore_ascii_case(value))
        .collect();
    match label_matches.len() {
        1 => return Ok(label_matches[0].clone()),
        0 => {}
        _ => {
            return Err(
                CliError::conflict(format!("more than one host has the label {value:?}"))
                    .with_hint("Use the host id instead."),
            );
        }
    }
    let prefix_matches: Vec<&HostProfile> = hosts
        .iter()
        .filter(|host| host.id.as_str().starts_with(value))
        .collect();
    match prefix_matches.len() {
        1 => Ok(prefix_matches[0].clone()),
        0 => Err(
            CliError::host_not_found(format!("host {value:?} not found"))
                .with_hint("Run `spirit host ls` to see every saved host id and label."),
        ),
        _ => Err(CliError::conflict(format!(
            "host prefix {value:?} matches more than one saved host"
        ))
        .with_hint("Use more id characters or the exact label.")),
    }
}

/// Rejects a profile whose pinned fingerprint is missing or malformed.
pub(crate) fn ensure_pinned(host: &HostProfile) -> Result<(), CliError> {
    host.host_key.validate().map_err(|error| {
        CliError::new(SpiritError::new(
            SpiritErrorCode::HostKeyMismatch,
            format!(
                "the host profile has no valid pinned fingerprint: {}",
                error.message
            ),
        ))
        .with_hint("Run `spirit host trust` with a freshly verified fingerprint.")
    })
}

fn clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn pin(algorithm: &str, fingerprint: &str) -> HostKeyPin {
    HostKeyPin {
        algorithm: algorithm.trim().to_owned(),
        sha256: fingerprint.trim().to_owned(),
        pinned_at: Timestamp::now(),
    }
}

fn parse_ssh_target(raw: &str, explicit_port: Option<u16>) -> Result<SshTarget, CliError> {
    let raw = raw.trim();
    let (user, host_port) = raw
        .split_once('@')
        .ok_or_else(|| CliError::usage("--ssh must be USER@HOST or USER@HOST:PORT"))?;
    if user.is_empty() || host_port.is_empty() || user.contains('@') || host_port.contains('@') {
        return Err(CliError::usage(
            "--ssh must contain exactly one non-empty USER@HOST",
        ));
    }
    let (host, embedded_port) = split_host_port(host_port)?;
    let port = match (explicit_port, embedded_port) {
        (Some(_), Some(_)) => {
            return Err(CliError::usage(
                "specify the SSH port once, with --port or the :PORT suffix",
            ));
        }
        (Some(port), None) | (None, Some(port)) => port,
        (None, None) => 22,
    };
    if port == 0 {
        return Err(CliError::usage("--port must be 1-65535"));
    }
    SshTarget::new(user, host, port).map_err(CliError::from)
}

fn split_host_port(input: &str) -> Result<(String, Option<u16>), CliError> {
    if let Some(rest) = input.strip_prefix('[') {
        let Some(close) = rest.find(']') else {
            return Err(CliError::usage("--ssh has an unterminated IPv6 address"));
        };
        let host = &rest[..close];
        let tail = &rest[close + 1..];
        if host.is_empty() {
            return Err(CliError::usage("--ssh has an empty host"));
        }
        if tail.is_empty() {
            return Ok((host.to_owned(), None));
        }
        let Some(port) = tail.strip_prefix(':') else {
            return Err(CliError::usage(
                "--ssh must put the port after the closing bracket",
            ));
        };
        return Ok((host.to_owned(), Some(parse_port(port)?)));
    }

    if input.matches(':').count() == 1 {
        if let Some((host, port)) = input.rsplit_once(':') {
            if host.is_empty() {
                return Err(CliError::usage("--ssh has an empty host"));
            }
            return Ok((host.to_owned(), Some(parse_port(port)?)));
        }
    }
    Ok((input.to_owned(), None))
}

fn parse_port(raw: &str) -> Result<u16, CliError> {
    let port: u16 = raw
        .parse()
        .map_err(|_| CliError::usage(format!("invalid SSH port {raw:?}")))?;
    if port == 0 {
        return Err(CliError::usage("--port must be 1-65535"));
    }
    Ok(port)
}
