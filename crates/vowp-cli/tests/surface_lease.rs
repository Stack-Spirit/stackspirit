//! `vowp session lease` / `session takeover` / `surface rename` ownership output.
//! Spec: `docs/design/v0.5.1/session-ownership.md` §2.4 and §2.6. Hermetic: a temporary home.

mod common;

/// Creates a session and returns its id.
fn create(env: &common::TestEnv) -> String {
    let (code, stdout, stderr) = env.run(&[
        "session",
        "new",
        "--title",
        "ownership",
        "--executor",
        "opencode:deepseek/deepseek-flash#max",
    ]);
    assert_eq!(code, 0, "{stderr}");
    stdout.trim().to_owned()
}

#[test]
fn session_lease_accepts_desktop_ui_and_prints_mode_state_epoch() {
    let env = common::TestEnv::new();
    let id = create(&env);

    let (code, stdout, stderr) = env.run(&[
        "session",
        "lease",
        &id,
        "--acquire",
        "desktop-ui",
        "--holder",
        "StackSpirit",
        "--label",
        "Desk UI",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let token = stdout.trim().to_owned();
    assert_eq!(token.len(), 52, "--acquire prints the one-time token");

    let (code, status, stderr) = env.run_json(&["session", "lease", &id]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(status["held"], true);
    assert_eq!(status["kind"], "desktop_ui");
    assert_eq!(status["holder"], "StackSpirit");
    assert_eq!(status["mode"], "interactive");
    assert_eq!(status["state"], "active");
    assert_eq!(status["activity"], "idle");
    assert_eq!(status["epoch"], 1);
    assert_eq!(status["label"], "Desk UI");
    assert!(status["token"].is_null(), "a plain status never carries the token");
    assert!(!status.to_string().contains(&token), "the token is not echoed");

    let (code, human, stderr) = env.run(&["session", "lease", &id]);
    assert_eq!(code, 0, "{stderr}");
    assert!(human.contains("(desktop-ui)"), "{human}");
    assert!(
        human.contains("mode interactive state active activity idle epoch 1"),
        "{human}"
    );
    assert!(!human.contains(&token), "{human}");
}

#[test]
fn session_takeover_increments_the_epoch_and_names_the_new_owner() {
    let env = common::TestEnv::new();
    let id = create(&env);
    let (code, _, stderr) = env.run(&["session", "lease", &id, "--acquire", "chat", "--holder", "laptop"]);
    assert_eq!(code, 0, "{stderr}");

    let takeover = |holder: &str, label: Option<&str>| {
        let mut args = vec![
            "session",
            "takeover",
            &id,
            "--kind",
            "desktop-ui",
            "--holder",
            holder,
            "--node",
            "local",
        ];
        if let Some(label) = label {
            args.extend(["--label", label]);
        }
        env.run(&args)
    };
    let (code, _, stderr) = takeover("StackSpirit", Some("Desk UI"));
    assert_eq!(code, 0, "{stderr}");
    let (_, status, _) = env.run_json(&["session", "lease", &id]);
    assert_eq!(status["epoch"], 2);
    assert_eq!(status["label"], "Desk UI");
    assert_eq!(status["kind"], "desktop_ui");

    // The plumbing command never asks: it fences whoever holds the session, even mid-turn,
    // and each takeover is one more epoch. The caller (chat, a UI, MCP) does the asking.
    let session = vowp_protocol::SessionId::parse(&id).expect("session id");
    env.store()
        .set_session_state(&session, vowp_protocol::SessionState::Running, None)
        .expect("running");
    let (_, running, _) = env.run_json(&["session", "lease", &id]);
    assert_eq!(
        running["activity"], "running",
        "the owner view says the turn is running"
    );
    let (code, _, stderr) = takeover("phone", None);
    assert_eq!(code, 0, "{stderr}");
    let (_, status, _) = env.run_json(&["session", "lease", &id]);
    assert_eq!(status["holder"], "phone");
    assert_eq!(status["epoch"], 3);
}

#[test]
fn unknown_surface_kind_lists_desktop_ui() {
    let env = common::TestEnv::new();
    let id = create(&env);
    let (code, _, stderr) = env.run(&["session", "lease", &id, "--acquire", "toaster"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("desktop-ui"), "{stderr}");
}
