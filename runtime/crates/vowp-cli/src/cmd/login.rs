//! `stackspirit login`, `logout` and `whoami`: the cloud account session.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.4 and §6.7. `login` uses the device
//! authorisation grant by default and can accept a pre-issued token on stdin with
//! `--token-stdin`; a token is never a flag value. The session is written to
//! `<home>/cloud-session.json` at mode 0600 (see [`crate::cloud::credentials`]) and no command
//! prints an access token, a refresh token or a device code.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::cli::{LoginArgs, LogoutArgs, WhoamiArgs};
use crate::cloud::{
    self, Account, Api, Client, Session, account_from_body, poll_device_token, start_device_flow, wait_for_poll,
};
use crate::output;
use crate::{CliError, Global, IoStreams};

/// What `login` and `whoami --json` print. It never carries a token.
#[derive(Debug, Serialize)]
struct LoginReport {
    api_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
}

impl LoginReport {
    fn from_session(session: &Session) -> Self {
        Self {
            api_url: session.api_url.clone(),
            account_id: session.account_id.clone(),
            email: session.email.clone(),
            expires_at: session.expires_at.clone(),
        }
    }
}

/// Runs `stackspirit login`.
pub fn run_login(global: &Global, io: &mut dyn IoStreams, args: &LoginArgs) -> Result<i32, CliError> {
    let home = crate::ctx::node_paths(global)?.root().to_path_buf();
    let api = Api::new(&args.api_url)?;
    let tokens = if args.token_stdin {
        let token = crate::read_stdin(io)?.trim().to_owned();
        if token.is_empty() {
            return Err(CliError::Invalid("the stdin token is empty".to_owned()));
        }
        let profile = api.get("/v1/me", Some(&token))?;
        if !profile.success() {
            let message = format!("the API rejected the token: {}", profile.describe());
            return Err(if matches!(profile.status, 401 | 403) {
                CliError::Auth(message)
            } else {
                CliError::Remote(message)
            });
        }
        let account = account_from_body(&profile.body);
        cloud::Tokens {
            access_token: token,
            refresh_token: None,
            expires_at: None,
            account_id: account.id,
            email: account.email,
        }
    } else {
        device_login(&api, io, global.json)?
    };

    let mut account = Account {
        id: tokens.account_id.clone(),
        email: tokens.email.clone(),
    };
    if (account.id.is_none() || account.email.is_none())
        && let Ok(profile) = api.get("/v1/me", Some(&tokens.access_token))
        && profile.success()
    {
        let fetched = account_from_body(&profile.body);
        account.id = account.id.or(fetched.id);
        account.email = account.email.or(fetched.email);
    }
    let session = Session {
        api_url: api.base().to_owned(),
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_at: tokens.expires_at,
        account_id: account.id,
        email: account.email,
    };
    session.save(&home)?;
    let report = LoginReport::from_session(&session);
    if global.json {
        output::print_json(io, &report)?;
    } else {
        print_signed_in(io, &report)?;
    }
    Ok(0)
}

/// Runs the interactive device grant and returns the issued tokens.
fn device_login(api: &Api, io: &mut dyn IoStreams, json: bool) -> Result<cloud::Tokens, CliError> {
    let start = start_device_flow(api)?;
    if json {
        writeln!(
            io.err(),
            "open {} and enter code {}",
            start.verification_uri,
            start.user_code
        )?;
    } else {
        writeln!(io.out(), "open: {}", start.verification_uri)?;
        writeln!(io.out(), "code: {}", start.user_code)?;
        writeln!(io.out(), "waiting for approval...")?;
    }
    let deadline = Instant::now() + Duration::from_secs(start.expires_in.max(1));
    let mut interval = start.interval;
    loop {
        if Instant::now() >= deadline {
            return Err(CliError::Auth(
                "the device login code expired; run `stackspirit login` again".to_owned(),
            ));
        }
        match poll_device_token(api, &start.device_code)? {
            cloud::DevicePoll::Tokens(tokens) => return Ok(tokens),
            cloud::DevicePoll::Pending => wait_for_poll(interval),
            cloud::DevicePoll::SlowDown => {
                interval = interval.saturating_add(5).min(60);
                wait_for_poll(interval);
            }
        }
    }
}

fn print_signed_in(io: &mut dyn IoStreams, report: &LoginReport) -> Result<(), CliError> {
    let out = io.out();
    match (&report.account_id, &report.email) {
        (Some(id), Some(email)) => writeln!(out, "signed in as {email} ({id})")?,
        (Some(id), None) => writeln!(out, "signed in as {id}")?,
        (None, Some(email)) => writeln!(out, "signed in as {email}")?,
        (None, None) => writeln!(out, "signed in")?,
    }
    writeln!(out, "api: {}", report.api_url)?;
    Ok(())
}

/// Runs `stackspirit logout`.
pub fn run_logout(global: &Global, io: &mut dyn IoStreams, _args: &LogoutArgs) -> Result<i32, CliError> {
    let home = crate::ctx::node_paths(global)?.root().to_path_buf();
    let removed = Session::remove(&home)?;
    if global.json {
        output::print_json(io, &serde_json::json!({ "signed_out": removed }))?;
    } else if removed {
        writeln!(io.out(), "signed out of the StackSpirit cloud")?;
    } else {
        writeln!(io.out(), "not signed in to the StackSpirit cloud")?;
    }
    Ok(0)
}

/// Runs `stackspirit whoami`.
pub fn run_whoami(global: &Global, io: &mut dyn IoStreams, _args: &WhoamiArgs) -> Result<i32, CliError> {
    let home = crate::ctx::node_paths(global)?.root().to_path_buf();
    let mut client = Client::load(&home)?;
    let profile = client.get("/v1/me")?;
    let fetched = account_from_body(&profile);
    let session = client.session();
    let report = LoginReport {
        api_url: client.api_url().to_owned(),
        account_id: fetched.id.clone().or_else(|| session.account_id.clone()),
        email: fetched.email.clone().or_else(|| session.email.clone()),
        expires_at: session.expires_at.clone(),
    };
    if global.json {
        output::print_json(io, &report)?;
    } else {
        let out = io.out();
        match (&report.account_id, &report.email) {
            (Some(id), Some(email)) => writeln!(out, "account: {email} ({id})")?,
            (Some(id), None) => writeln!(out, "account: {id}")?,
            (None, Some(email)) => writeln!(out, "account: {email}")?,
            (None, None) => writeln!(out, "account: unknown")?,
        }
        writeln!(out, "api:     {}", report.api_url)?;
    }
    Ok(0)
}
