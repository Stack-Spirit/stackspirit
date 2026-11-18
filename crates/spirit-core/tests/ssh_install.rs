//! One-time-password SSH key install: secret transport hygiene, askpass file
//! permissions and cleanup, the `authorized_keys` merge, pinned-host-key refusal and
//! honest verification.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tempfile::TempDir;

use spirit_core::error::{SpiritError, SpiritErrorCode};
use spirit_core::ssh::command::{CommandOutput, CommandSpec};
use spirit_core::ssh::password_install::{
    PasswordAuthOutcome, PasswordAuthUnsupported, PasswordInstallRequest,
    install_public_key_with_password, merge_authorized_keys,
};
use spirit_core::ssh::{CommandRunner, SshOptions, SshTarget};
use spirit_core::{HostKeyPin, SecretString, Timestamp};

const PASSWORD: &str = "one-time-password-9x!";
const LOCAL_KEY_BODY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIFakeLocalKeyForStackSpiritTests";
const HOST_KEY_BODY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIFakeHostKeyForStackSpiritTests1";
const LOCAL_FINGERPRINT: &str = "SHA256:localLocalLocalLocalLocalLocalLocalLocalLocal";
const HOST_FINGERPRINT: &str = "SHA256:hostHostHostHostHostHostHostHostHostHostHost";
const DROP_IN: &str = "/etc/ssh/sshd_config.d/99-stackspirit-password-auth.conf";

fn public_key_line() -> String {
    format!("ssh-ed25519 {LOCAL_KEY_BODY} owner@example")
}

fn pinned_host_key() -> HostKeyPin {
    HostKeyPin {
        algorithm: "ssh-ed25519".to_owned(),
        sha256: HOST_FINGERPRINT.to_owned(),
        pinned_at: Timestamp::parse("2026-11-18T00:00:00.000Z").unwrap(),
    }
}

fn target() -> SshTarget {
    SshTarget::new("owner", "192.0.2.10", 2222).unwrap()
}

fn key_pair(work: &TempDir) -> PathBuf {
    let private = work.path().join("id_ed25519");
    fs::write(&private, "not a real private key").unwrap();
    let mut public = private.as_os_str().to_owned();
    public.push(".pub");
    fs::write(PathBuf::from(public), format!("{}\n", public_key_line())).unwrap();
    private
}

fn request(work: &TempDir) -> PasswordInstallRequest {
    PasswordInstallRequest {
        ssh: target(),
        host_key: pinned_host_key(),
        password: SecretString::new(PASSWORD),
        private_key_path: key_pair(work),
        disable_password_auth: false,
    }
}

fn options(work: &TempDir) -> SshOptions {
    SshOptions::new(work.path().join("known_hosts"))
}

struct Route {
    needles: Vec<String>,
    output: CommandOutput,
}

#[cfg_attr(not(unix), allow(dead_code))]
struct AskpassSnapshot {
    dir_mode: u32,
    helper_mode: u32,
    secret_mode: u32,
    secret: Vec<u8>,
    helper_text: String,
}

struct RouteRunner {
    routes: Vec<Route>,
    default: CommandOutput,
    calls: Mutex<Vec<CommandSpec>>,
    askpass: Mutex<Vec<AskpassSnapshot>>,
}

impl RouteRunner {
    fn new(routes: Vec<Route>, default: CommandOutput) -> Self {
        Self {
            routes,
            default,
            calls: Mutex::new(Vec::new()),
            askpass: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<CommandSpec> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn askpass_snapshots(&self) -> Vec<AskpassSnapshot> {
        std::mem::take(
            &mut *self
                .askpass
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

#[async_trait]
impl CommandRunner for RouteRunner {
    async fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, SpiritError> {
        if let Some(helper) = spec
            .env
            .iter()
            .find(|(key, _)| key == "SSH_ASKPASS")
            .map(|(_, value)| value.clone())
        {
            let helper_path = PathBuf::from(&helper);
            let directory = helper_path.parent().expect("helper has a parent");
            let secret_path = directory.join("password");
            self.askpass
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(AskpassSnapshot {
                    dir_mode: mode(directory),
                    helper_mode: mode(&helper_path),
                    secret_mode: mode(&secret_path),
                    secret: fs::read(&secret_path)
                        .expect("the askpass secret exists during the hop"),
                    helper_text: fs::read_to_string(&helper_path).expect("helper is readable"),
                });
        }
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(spec.clone());
        let stdin = spec
            .stdin
            .as_deref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default();
        let haystack = format!("{} {} {}", spec.program, spec.args.join(" "), stdin);
        let route = self
            .routes
            .iter()
            .find(|route| route.needles.iter().all(|needle| haystack.contains(needle)));
        Ok(route.map_or_else(|| self.default.clone(), |route| route.output.clone()))
    }
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn mode(_path: &Path) -> u32 {
    0
}

fn route(needles: &[&str], output: CommandOutput) -> Route {
    Route {
        needles: needles.iter().map(|needle| (*needle).to_owned()).collect(),
        output,
    }
}

fn unexpected() -> CommandOutput {
    CommandOutput::fail(127, "unexpected command for the fake ssh harness")
}

fn happy_routes() -> Vec<Route> {
    vec![
        route(
            &["ssh-keygen", LOCAL_KEY_BODY],
            CommandOutput::ok(format!("256 {LOCAL_FINGERPRINT} owner@example (ED25519)\n")),
        ),
        route(
            &["ssh-keygen", HOST_KEY_BODY],
            CommandOutput::ok(format!("256 {HOST_FINGERPRINT} (ED25519)\n")),
        ),
        route(
            &["ssh-keyscan"],
            CommandOutput::ok(format!("192.0.2.10 ssh-ed25519 {HOST_KEY_BODY}\n")),
        ),
        route(&["grep -qxF"], CommandOutput::ok("")),
        route(&["owner@192.0.2.10 'true'"], CommandOutput::ok("")),
    ]
}

fn fake(routes: Vec<Route>) -> Arc<RouteRunner> {
    Arc::new(RouteRunner::new(routes, unexpected()))
}

fn remote_call<'a>(calls: &'a [CommandSpec], needle: &str) -> Option<&'a CommandSpec> {
    calls
        .iter()
        .find(|call| call.args.join(" ").contains(needle))
}

#[tokio::test]
async fn password_install_keeps_the_secret_out_of_argv_env_and_disk_after_return() {
    let work = tempfile::tempdir().unwrap();
    let runner = fake(happy_routes());
    let request = request(&work);
    let outcome = install_public_key_with_password(runner.clone(), &request, &options(&work))
        .await
        .unwrap();

    assert!(outcome.installed);
    assert_eq!(outcome.fingerprint, LOCAL_FINGERPRINT);
    assert!(!outcome.password_login_disabled);
    assert_eq!(outcome.password_auth, PasswordAuthOutcome::NotRequested);

    let calls = runner.calls();
    for call in &calls {
        assert!(
            !call.args.iter().any(|arg| arg.contains(PASSWORD)),
            "the password reached argv of {}",
            call.program
        );
        assert!(
            !call.env.iter().any(|(_, value)| value.contains(PASSWORD)),
            "the password reached the environment of {}",
            call.program
        );
        assert!(
            call.stdin
                .as_deref()
                .is_none_or(|bytes| { !String::from_utf8_lossy(bytes).contains(PASSWORD) }),
            "the password reached stdin of {}",
            call.program
        );
    }

    let ssh_calls: Vec<&CommandSpec> = calls.iter().filter(|call| call.program == "ssh").collect();
    let bootstrap: Vec<&&CommandSpec> = ssh_calls
        .iter()
        .filter(|call| call.env.iter().any(|(key, _)| key == "SSH_ASKPASS"))
        .collect();
    assert_eq!(bootstrap.len(), 1, "exactly one password hop is allowed");
    let bootstrap = bootstrap[0];
    assert!(bootstrap.args.iter().any(|arg| arg == "BatchMode=no"));
    assert!(
        bootstrap
            .args
            .iter()
            .any(|arg| arg == "StrictHostKeyChecking=yes")
    );
    assert!(
        bootstrap
            .args
            .iter()
            .any(|arg| { arg == "PreferredAuthentications=password,keyboard-interactive" })
    );
    assert!(
        bootstrap
            .args
            .iter()
            .any(|arg| arg == "PubkeyAuthentication=no")
    );
    assert!(
        bootstrap
            .env
            .iter()
            .any(|(key, value)| key == "SSH_ASKPASS_REQUIRE" && value == "force")
    );

    let verify = remote_call(&calls, "owner@192.0.2.10 'true'").expect("the verify hop ran");
    assert!(verify.args.iter().any(|arg| arg == "BatchMode=yes"));
    assert!(
        verify
            .args
            .iter()
            .any(|arg| arg == "PreferredAuthentications=publickey")
    );
    assert!(verify.args.iter().any(|arg| arg == "IdentitiesOnly=yes"));
    let identity = verify
        .args
        .iter()
        .position(|arg| arg == "-i")
        .expect("the verify hop names a key");
    assert_eq!(
        verify.args[identity + 1],
        request.private_key_path.display().to_string()
    );
    assert!(
        !calls
            .iter()
            .any(|call| call.args.iter().any(|arg| arg.contains("accept-new")))
    );

    let snapshots = runner.askpass_snapshots();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].secret, PASSWORD.as_bytes());
    assert!(
        !snapshots[0].helper_text.contains(PASSWORD),
        "the helper may know only the secret path"
    );
    assert!(
        snapshots[0].helper_text.contains("/password'"),
        "the helper receives only the secret file path: {}",
        snapshots[0].helper_text
    );
    #[cfg(unix)]
    {
        assert_eq!(snapshots[0].dir_mode, 0o700);
        assert_eq!(snapshots[0].helper_mode, 0o700);
        assert_eq!(snapshots[0].secret_mode, 0o600);
    }

    let askpass_path = bootstrap
        .env
        .iter()
        .find(|(key, _)| key == "SSH_ASKPASS")
        .map(|(_, value)| value.as_str())
        .unwrap();
    let askpass_dir = Path::new(askpass_path).parent().unwrap();
    assert!(
        !askpass_dir.exists(),
        "the askpass temp directory must be deleted on return"
    );

    let known_hosts = fs::read_to_string(work.path().join("known_hosts")).unwrap();
    assert_eq!(
        known_hosts,
        format!("[192.0.2.10]:2222 ssh-ed25519 {HOST_KEY_BODY}\n"),
        "only the confirmed key is pinned"
    );

    let install = remote_call(&calls, "grep -qxF").expect("the key install hop ran");
    assert_eq!(
        String::from_utf8(install.stdin.clone().unwrap()).unwrap(),
        format!("{}\n", public_key_line()),
        "the hop receives the exact key line and nothing else"
    );
    let script = install.args.last().unwrap();
    assert!(script.contains("chmod 700 \"$HOME/.ssh\""));
    assert!(script.contains("chmod 600 -- \"$file\""));
    assert!(script.contains("grep -qxF -- \"$key\" \"$file\""));
    assert!(script.contains(">> \"$file\""));
}

#[test]
fn authorized_keys_merge_is_idempotent_and_preserves_every_existing_line() {
    let key = public_key_line();
    let existing = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7existing existing@host\n# keep me\n\n";

    let first = merge_authorized_keys(existing, &key).unwrap();
    assert!(first.changed);
    assert_eq!(first.content, format!("{existing}{key}\n"));
    assert!(first.content.starts_with(existing));

    let second = merge_authorized_keys(&first.content, &key).unwrap();
    assert!(!second.changed, "a retry must not add a duplicate");
    assert_eq!(second.content, first.content);

    let crlf = format!("{key}\r\n");
    assert!(!merge_authorized_keys(&crlf, &key).unwrap().changed);

    let no_final_newline = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7existing existing@host";
    assert_eq!(
        merge_authorized_keys(no_final_newline, &key)
            .unwrap()
            .content,
        format!("{no_final_newline}\n{key}\n")
    );
}

#[test]
fn authorized_keys_merge_rejects_malformed_key_lines() {
    for bad in [
        "",
        "not-a-key",
        "ssh-ed25519",
        "ssh-ed25519 not*base64 owner@example",
        "from=\"10.0.0.1\" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeLocalKeyForStackSpiritTests",
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeLocalKeyForStackSpiritTests\nssh-rsa AAAA",
        " ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeLocalKeyForStackSpiritTests",
        "ssh-dss AAAAB3NzaC1kc3MAAACBAP",
    ] {
        let error = merge_authorized_keys("", bad).unwrap_err();
        assert_eq!(
            error.code,
            SpiritErrorCode::InvalidInput,
            "{bad:?} must be refused before connecting"
        );
    }
    let key = public_key_line();
    assert!(merge_authorized_keys("", &key).unwrap().changed);
}

#[tokio::test]
async fn wrong_password_is_a_typed_error_that_leaves_no_temp_files() {
    let work = tempfile::tempdir().unwrap();
    let mut routes: Vec<Route> = happy_routes()
        .into_iter()
        .filter(|route| !route.needles.iter().any(|needle| needle == "grep -qxF"))
        .collect();
    routes.push(route(
        &["grep -qxF"],
        CommandOutput::fail(
            255,
            "owner@192.0.2.10: Permission denied (publickey,password).\n",
        ),
    ));
    let runner = fake(routes);
    let request = request(&work);
    let error = install_public_key_with_password(runner.clone(), &request, &options(&work))
        .await
        .unwrap_err();

    assert_eq!(error.code, SpiritErrorCode::SshAuth);
    assert!(!error.message.contains(PASSWORD));
    assert!(
        error
            .hint
            .as_deref()
            .unwrap_or_default()
            .contains("one-time password")
    );

    let calls = runner.calls();
    let askpass = calls
        .iter()
        .find_map(|call| {
            call.env
                .iter()
                .find(|(key, _)| key == "SSH_ASKPASS")
                .map(|(_, value)| value.clone())
        })
        .expect("the password hop ran and used the helper");
    let askpass_dir = Path::new(&askpass).parent().unwrap();
    assert!(
        !askpass_dir.exists(),
        "a failed first hop must still delete the askpass temp directory"
    );
}

#[tokio::test]
async fn a_pin_mismatch_is_refused_before_the_password_hop() {
    let work = tempfile::tempdir().unwrap();
    let runner = fake(vec![
        route(
            &["ssh-keygen", LOCAL_KEY_BODY],
            CommandOutput::ok(format!("256 {LOCAL_FINGERPRINT} owner@example (ED25519)\n")),
        ),
        route(
            &["ssh-keygen", HOST_KEY_BODY],
            CommandOutput::ok(
                "256 SHA256:differentDifferentDifferentDifferentDifferent (ED25519)\n",
            ),
        ),
        route(
            &["ssh-keyscan"],
            CommandOutput::ok(format!("192.0.2.10 ssh-ed25519 {HOST_KEY_BODY}\n")),
        ),
    ]);
    let request = request(&work);
    let error = install_public_key_with_password(runner.clone(), &request, &options(&work))
        .await
        .unwrap_err();

    assert_eq!(error.code, SpiritErrorCode::HostKeyMismatch);
    assert!(!error.message.contains(PASSWORD));
    let calls = runner.calls();
    assert!(
        !calls
            .iter()
            .any(|call| call.env.iter().any(|(key, _)| key == "SSH_ASKPASS")),
        "no password may be prepared when the pinned key does not match"
    );
    assert!(
        calls.iter().all(|call| call.program != "ssh"),
        "no SSH login may run when the pinned key does not match"
    );
}

#[tokio::test]
async fn a_failing_verify_hop_is_not_reported_as_installed() {
    let work = tempfile::tempdir().unwrap();
    let mut routes: Vec<Route> = happy_routes()
        .into_iter()
        .filter(|route| {
            !route
                .needles
                .iter()
                .any(|needle| needle == "owner@192.0.2.10 'true'")
        })
        .collect();
    routes.push(route(
        &["owner@192.0.2.10 'true'"],
        CommandOutput::fail(255, "owner@192.0.2.10: Permission denied (publickey).\n"),
    ));
    let runner = fake(routes);
    let request = request(&work);
    let error = install_public_key_with_password(runner.clone(), &request, &options(&work))
        .await
        .unwrap_err();

    assert_eq!(error.code, SpiritErrorCode::SshAuth);
    assert!(
        error.message.contains("not reported as installed"),
        "the honest failure text is required: {}",
        error.message
    );
    assert!(!error.message.contains(PASSWORD));
    assert!(
        remote_call(&runner.calls(), "grep -qxF").is_some(),
        "the first hop really succeeded before the verify hop failed"
    );
}

#[tokio::test]
async fn password_disable_without_passwordless_sudo_is_a_typed_not_supported_outcome() {
    let work = tempfile::tempdir().unwrap();
    let mut routes = happy_routes();
    routes.push(route(
        &["'sudo' '-n' 'true'"],
        CommandOutput::fail(1, "sudo: a password is required\n"),
    ));
    let runner = fake(routes);
    let mut request = request(&work);
    request.disable_password_auth = true;
    let outcome = install_public_key_with_password(runner.clone(), &request, &options(&work))
        .await
        .unwrap();

    assert!(outcome.installed);
    assert!(!outcome.password_login_disabled);
    assert_eq!(
        outcome.password_auth,
        PasswordAuthOutcome::NotSupported {
            reason: PasswordAuthUnsupported::NoPasswordlessSudo
        }
    );
    for call in runner.calls() {
        if call.args.iter().any(|arg| arg == "sudo") {
            assert!(
                call.args.iter().any(|arg| arg == "-n"),
                "sudo must never be allowed to prompt"
            );
        }
    }
}

#[tokio::test]
async fn password_disable_applies_a_validated_effective_drop_in() {
    let work = tempfile::tempdir().unwrap();
    let runner = fake(passwords_disabled_routes(
        "passwordauthentication no\npubkeyauthentication yes\n",
    ));
    let mut request = request(&work);
    request.disable_password_auth = true;
    let outcome = install_public_key_with_password(runner.clone(), &request, &options(&work))
        .await
        .unwrap();

    assert!(outcome.installed);
    assert!(outcome.password_login_disabled);
    assert_eq!(outcome.password_auth, PasswordAuthOutcome::Disabled);
    let calls = runner.calls();
    assert!(
        remote_call(&calls, "tee").is_some(),
        "the drop-in was written"
    );
    assert!(
        remote_call(&calls, "'sshd' '-t'").is_some(),
        "the drop-in was validated before reloading"
    );
    assert!(
        remote_call(&calls, "'systemctl' 'reload' 'sshd'").is_some(),
        "sshd was reloaded"
    );
}

#[tokio::test]
async fn an_ignored_drop_in_is_reverted_and_not_reported_as_disabled() {
    let work = tempfile::tempdir().unwrap();
    let runner = fake(passwords_disabled_routes(
        "passwordauthentication yes\npubkeyauthentication yes\n",
    ));
    let mut request = request(&work);
    request.disable_password_auth = true;
    let outcome = install_public_key_with_password(runner.clone(), &request, &options(&work))
        .await
        .unwrap();

    assert!(outcome.installed);
    assert!(!outcome.password_login_disabled);
    assert_eq!(
        outcome.password_auth,
        PasswordAuthOutcome::NotSupported {
            reason: PasswordAuthUnsupported::DropInIgnored
        }
    );
    assert!(
        remote_call(&runner.calls(), "'rm' '-f'").is_some(),
        "the ineffective drop-in must be removed again"
    );
}

fn passwords_disabled_routes(sshd_t_output: &'static str) -> Vec<Route> {
    let mut routes = happy_routes();
    routes.extend([
        route(&["'sudo' '-n' 'true'"], CommandOutput::ok("")),
        route(
            &["'test' '-d' '/etc/ssh/sshd_config.d'"],
            CommandOutput::ok(""),
        ),
        route(&["'tee'", DROP_IN], CommandOutput::ok("")),
        route(&["'sshd' '-t'"], CommandOutput::ok("")),
        route(&["'sshd' '-T'"], CommandOutput::ok(sshd_t_output)),
        route(&["'systemctl' 'reload' 'sshd'"], CommandOutput::ok("")),
        route(&["'systemctl' 'reload' 'ssh'"], CommandOutput::ok("")),
        route(&["'rm' '-f'", DROP_IN], CommandOutput::ok("")),
    ]);
    routes
}
