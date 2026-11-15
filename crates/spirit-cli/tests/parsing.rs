//! Command parsing and `--help` coverage for every documented command.

mod common;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use spirit_cli::Cli;
use spirit_cli::args::{Command, EngineCommand, HostCommand};

use common::FINGERPRINT;

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).unwrap_or_else(|error| panic!("{args:?}: {error}"))
}

#[test]
fn parses_every_documented_command() {
    let cli = parse(&[
        "spirit",
        "host",
        "add",
        "--label",
        "vps",
        "--ssh",
        "dev@example.com",
        "--port",
        "2222",
        "--fingerprint",
        FINGERPRINT,
    ]);
    assert!(matches!(
        cli.command,
        Command::Host(HostCommand::Add(ref args))
            if args.label == "vps" && args.port == Some(2222)
    ));

    assert!(matches!(
        parse(&["spirit", "host", "ls"]).command,
        Command::Host(HostCommand::Ls)
    ));
    assert!(matches!(
        parse(&["spirit", "host", "show", "host-id"]).command,
        Command::Host(HostCommand::Show(ref args)) if args.host.as_deref() == Some("host-id")
    ));
    assert!(matches!(
        parse(&["spirit", "host", "remove", "host-id"]).command,
        Command::Host(HostCommand::Remove(_))
    ));
    assert!(matches!(
        parse(&[
            "spirit",
            "host",
            "trust",
            "host-id",
            "--fingerprint",
            FINGERPRINT
        ])
        .command,
        Command::Host(HostCommand::Trust(_))
    ));
    assert!(matches!(
        parse(&["spirit", "host", "probe", "host-id"]).command,
        Command::Host(HostCommand::Probe(_))
    ));
    assert!(matches!(
        parse(&["spirit", "host", "connect", "host-id"]).command,
        Command::Host(HostCommand::Connect(_))
    ));
    assert!(matches!(
        parse(&["spirit", "host", "disconnect"]).command,
        Command::Host(HostCommand::Disconnect(_))
    ));
}

#[test]
fn parses_read_commands_and_global_flags_after_the_subcommand() {
    let cli = parse(&[
        "spirit",
        "fleet",
        "--project",
        "vowp",
        "--json",
        "--host",
        "abc",
    ]);
    assert!(cli.json);
    assert_eq!(cli.host.as_deref(), Some("abc"));
    assert!(matches!(
        cli.command,
        Command::Fleet(ref args) if args.project.as_deref() == Some("vowp")
    ));

    let cli = parse(&[
        "spirit",
        "--fixtures",
        "--fixture",
        "multi-project",
        "gates",
        "--json",
    ]);
    assert!(cli.fixtures);
    assert_eq!(cli.fixture.as_deref(), Some("multi-project"));
    assert!(matches!(cli.command, Command::Gates));

    assert!(matches!(
        parse(&["spirit", "capacity", "--json"]).command,
        Command::Capacity
    ));
    assert!(matches!(
        parse(&["spirit", "engine", "status", "--json"]).command,
        Command::Engine(EngineCommand::Status)
    ));
    assert!(matches!(
        parse(&["spirit", "version", "--json"]).command,
        Command::Version
    ));
}

#[test]
fn rejects_an_unknown_command_with_exit_code_two() {
    let error = Cli::try_parse_from(["spirit", "unknown"]).expect_err("must fail");
    assert_eq!(error.kind(), ErrorKind::InvalidSubcommand);
    assert_eq!(error.exit_code(), 2);
}

#[test]
fn top_level_help_lists_every_command() {
    let help = Cli::command().render_help().to_string();
    for command in ["host", "fleet", "capacity", "gates", "engine", "version"] {
        assert!(help.contains(command), "help is missing {command}: {help}");
    }
}

#[test]
fn every_command_and_subcommand_has_help() {
    let help_args: &[&[&str]] = &[
        &["spirit", "--help"],
        &["spirit", "host", "--help"],
        &["spirit", "host", "add", "--help"],
        &["spirit", "host", "ls", "--help"],
        &["spirit", "host", "show", "--help"],
        &["spirit", "host", "remove", "--help"],
        &["spirit", "host", "trust", "--help"],
        &["spirit", "host", "probe", "--help"],
        &["spirit", "host", "connect", "--help"],
        &["spirit", "host", "disconnect", "--help"],
        &["spirit", "fleet", "--help"],
        &["spirit", "capacity", "--help"],
        &["spirit", "gates", "--help"],
        &["spirit", "engine", "--help"],
        &["spirit", "engine", "status", "--help"],
        &["spirit", "version", "--help"],
    ];
    for args in help_args {
        let error = Cli::try_parse_from(*args).expect_err("help must be a clap error");
        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelp,
            "unexpected help error for {args:?}: {error}"
        );
        assert!(
            !error.to_string().trim().is_empty(),
            "empty help for {args:?}"
        );
    }
}

#[test]
fn help_covers_every_host_subcommand() {
    let host = Cli::command()
        .find_subcommand("host")
        .expect("host command")
        .clone()
        .render_help()
        .to_string();
    for command in [
        "add",
        "ls",
        "show",
        "remove",
        "trust",
        "probe",
        "connect",
        "disconnect",
    ] {
        assert!(
            host.contains(command),
            "host help is missing {command}: {host}"
        );
    }
}
