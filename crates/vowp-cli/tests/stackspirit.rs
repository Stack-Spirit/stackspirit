//! The `stackspirit` binary is the same CLI as `vowp`, with StackSpirit Engine branding,
//! `STACKSPIRIT_HOME` precedence and `stackspirit.toml` discovery. Hermetic: temp dirs only.

use std::path::Path;
use std::process::{Command, Output};

fn run(bin: &str, home_env: &[(&str, &Path)], args: &[&str], cwd: &Path) -> Output {
    std::fs::create_dir_all(cwd.join(".tmux-private")).expect("private tmux dir");
    let mut command = Command::new(bin);
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("VOWP_HOME")
        .env_remove("STACKSPIRIT_HOME")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env("HOME", cwd)
        .env("TMUX_TMPDIR", cwd.join(".tmux-private"));
    for (key, value) in home_env {
        command.env(key, value);
    }
    command.output().expect("run binary")
}

fn git_repo(parent: &Path, name: &str) -> std::path::PathBuf {
    let repo = parent.join(name);
    std::fs::create_dir_all(repo.join(".git")).expect("fake git dir");
    repo
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn stackspirit_binary_matches_vowp_cli() {
    let dir = tempfile::tempdir().expect("tempdir");
    let new = run(env!("CARGO_BIN_EXE_stackspirit"), &[], &["--version"], dir.path());
    let old = run(env!("CARGO_BIN_EXE_vowp"), &[], &["--version"], dir.path());
    assert!(new.status.success());
    assert_eq!(stdout(&new), stdout(&old));
    let help_new = run(env!("CARGO_BIN_EXE_stackspirit"), &[], &["--help"], dir.path());
    let help_old = run(env!("CARGO_BIN_EXE_vowp"), &[], &["--help"], dir.path());
    // Only the usage line may differ: clap shows the invoked binary name.
    assert_eq!(
        stdout(&help_new).replace("Usage: stackspirit", "Usage: vowp"),
        stdout(&help_old)
    );
    assert!(stdout(&help_new).contains("StackSpirit Engine"));
}

#[test]
fn stackspirit_home_beats_vowp_home_and_flag_beats_both() {
    let dir = tempfile::tempdir().expect("tempdir");
    let new_home = dir.path().join("new-home");
    let old_home = dir.path().join("old-home");
    let flag_home = dir.path().join("flag-home");
    for home in [&new_home, &old_home, &flag_home] {
        std::fs::create_dir_all(home).expect("mkdir");
    }
    let bin = env!("CARGO_BIN_EXE_stackspirit");
    let env = [
        ("STACKSPIRIT_HOME", new_home.as_path()),
        ("VOWP_HOME", old_home.as_path()),
    ];

    let repo = git_repo(dir.path(), "repo");
    let out = run(bin, &env, &["init", "--name", "demo"], &repo);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let repo2 = git_repo(dir.path(), "repo2");
    let flag = flag_home.to_string_lossy().into_owned();
    let out = run(bin, &env, &["--home", &flag, "init", "--name", "demo2"], &repo2);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let created_in = |home: &Path| home.join("vowp.db").exists();
    assert!(
        created_in(&new_home),
        "STACKSPIRIT_HOME should be used when both are set"
    );
    assert!(!created_in(&old_home), "VOWP_HOME must lose to STACKSPIRIT_HOME");
    assert!(created_in(&flag_home), "--home must beat both variables");
}

#[test]
fn vowp_home_still_works_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let old_home = dir.path().join("old-home");
    std::fs::create_dir_all(&old_home).expect("mkdir");
    let repo = git_repo(dir.path(), "repo");
    let out = run(
        env!("CARGO_BIN_EXE_vowp"),
        &[("VOWP_HOME", old_home.as_path())],
        &["init", "--name", "demo"],
        &repo,
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(old_home.join("vowp.db").exists());
}

#[test]
fn init_with_stackspirit_toml_updates_it_and_project_is_discovered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("mkdir");
    let project = git_repo(dir.path(), "proj");
    let bin = env!("CARGO_BIN_EXE_stackspirit");
    let env = [("STACKSPIRIT_HOME", home.as_path())];
    let init = run(bin, &env, &["init", "--name", "demo"], &project);
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    std::fs::rename(project.join("vowp.toml"), project.join("stackspirit.toml")).expect("rename");

    let status = run(bin, &env, &["--json", "status"], &project);
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    let again = run(bin, &env, &["init", "--name", "demo", "--force"], &project);
    assert!(again.status.success(), "{}", String::from_utf8_lossy(&again.stderr));
    assert!(
        !project.join("vowp.toml").exists(),
        "--force must update the existing name"
    );
    assert!(project.join("stackspirit.toml").is_file());
}
