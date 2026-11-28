//! Hermetic tests for the CLI & MCP tools registry. Uses temp directories only.

use spirit_core::tools::{
    CliSpec, McpSpec, McpTransport, ToolCategory, ToolConnectionState, ToolConnections, ToolEntry,
    ToolFilter, ToolRegistry, ToolTarget, builtin_tools,
};

fn registry() -> (tempfile::TempDir, ToolRegistry) {
    let dir = tempfile::tempdir().unwrap();
    let reg = ToolRegistry::new(dir.path().join("tools"));
    (dir, reg)
}

fn custom_mcp(id: &str) -> ToolEntry {
    ToolEntry {
        id: id.to_owned(),
        name: "My Server".to_owned(),
        category: ToolCategory::Custom,
        description: "internal".to_owned(),
        cli: None,
        mcp: Some(McpSpec {
            transport: McpTransport::Http,
            command: String::new(),
            args: vec![],
            url: "https://mcp.example.com/mcp".to_owned(),
        }),
        builtin: false,
    }
}

#[test]
fn builtins_cover_the_required_presets_and_validate() {
    let tools = builtin_tools();
    assert_eq!(tools.len(), 19);
    for id in [
        "github",
        "vercel",
        "cloudflare",
        "stripe",
        "supabase",
        "linear",
        "slack",
        "gmail",
        "google-calendar",
        "notion",
        "sentry",
        "postgres",
        "playwright",
        "context7",
        "docker",
        "aws",
        "gcloud",
        "flyio",
        "hetzner",
    ] {
        assert!(tools.iter().any(|t| t.id == id), "missing {id}");
    }
    for tool in &tools {
        tool.validate()
            .unwrap_or_else(|e| panic!("{}: {}", tool.id, e.message));
        assert!(tool.builtin);
    }
    let github = tools.iter().find(|t| t.id == "github").unwrap();
    assert!(github.cli.is_some() && github.mcp.is_some());
}

#[test]
fn search_and_category_filter() {
    let (_d, reg) = registry();
    let hits = reg
        .search(&ToolFilter {
            query: "GH".into(),
            category: None,
        })
        .unwrap();
    assert!(hits.iter().any(|t| t.id == "github"));
    let dbs = reg
        .search(&ToolFilter {
            query: String::new(),
            category: Some(ToolCategory::Databases),
        })
        .unwrap();
    let ids: Vec<_> = dbs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["supabase", "postgres"]);
    let none = reg
        .search(&ToolFilter {
            query: "zzzz".into(),
            category: None,
        })
        .unwrap();
    assert!(none.is_empty());
    let both = reg
        .search(&ToolFilter {
            query: "deploy".into(),
            category: Some(ToolCategory::CodeIssues),
        })
        .unwrap();
    assert!(both.is_empty());
}

#[test]
fn custom_entry_round_trips_and_appears_in_custom_category() {
    let (_d, reg) = registry();
    let mut entry = custom_mcp("my-server");
    entry.cli = Some(CliSpec {
        binary: "mytool".into(),
        install_cmd: "brew install \"mytool\"".into(),
        auth_cmd: String::new(),
    });
    reg.save_custom(entry.clone()).unwrap();
    let file = std::fs::read_to_string(reg.dir().join("my-server.toml")).unwrap();
    assert!(file.contains("builtin = false"));
    let custom = reg
        .search(&ToolFilter {
            query: String::new(),
            category: Some(ToolCategory::Custom),
        })
        .unwrap();
    assert_eq!(custom, vec![entry]);
    assert!(reg.delete_custom("my-server").unwrap());
    assert!(!reg.delete_custom("my-server").unwrap());
}

#[test]
fn stdio_custom_entry_round_trips_args() {
    let (_d, reg) = registry();
    let mut entry = custom_mcp("stdio-one");
    entry.mcp = Some(McpSpec {
        transport: McpTransport::Stdio,
        command: "npx".into(),
        args: vec!["-y".into(), "some-mcp".into()],
        url: String::new(),
    });
    reg.save_custom(entry.clone()).unwrap();
    assert_eq!(reg.list().unwrap().tools.last().unwrap(), &entry);
}

#[test]
fn secrets_are_rejected_and_never_written() {
    let (_d, reg) = registry();
    let mut entry = custom_mcp("leaky");
    entry.mcp.as_mut().unwrap().url = "https://user:pw@mcp.example.com/mcp".into();
    assert!(reg.save_custom(entry).is_err());

    let mut entry = custom_mcp("leaky");
    entry.mcp.as_mut().unwrap().url = "https://mcp.example.com/mcp?token=abc".into();
    assert!(reg.save_custom(entry).is_err());

    let mut entry = custom_mcp("leaky");
    entry.mcp = Some(McpSpec {
        transport: McpTransport::Stdio,
        command: "srv".into(),
        args: vec!["--api-key=abc".into()],
        url: String::new(),
    });
    assert!(reg.save_custom(entry).is_err());
    assert!(!reg.dir().join("leaky.toml").exists());
}

#[test]
fn unknown_credential_field_is_skipped_with_a_safe_warning() {
    let (_d, reg) = registry();
    std::fs::create_dir_all(reg.dir()).unwrap();
    std::fs::write(
        reg.dir().join("bad.toml"),
        "id = \"bad\"\nname = \"Bad\"\ncategory = \"custom\"\nmcp_transport = \"http\"\nmcp_url = \"https://x.example.com\"\napi_key = \"hunter2\"\n",
    )
    .unwrap();
    let view = reg.list().unwrap();
    assert_eq!(view.tools.len(), 19);
    assert_eq!(view.warnings.len(), 1);
    assert!(!view.warnings[0].contains("hunter2"));
}

#[test]
fn builtin_ids_cannot_be_shadowed_or_deleted() {
    let (_d, reg) = registry();
    assert!(reg.save_custom(custom_mcp("github")).is_err());
    assert!(reg.delete_custom("github").is_err());
    let mut builtin_flag = custom_mcp("flagged");
    builtin_flag.builtin = true;
    assert!(reg.save_custom(builtin_flag).is_err());
    assert!(reg.save_custom(custom_mcp("Bad Id")).is_err());
    let mut wrong_cat = custom_mcp("wrongcat");
    wrong_cat.category = ToolCategory::Databases;
    assert!(reg.save_custom(wrong_cat).is_err());
}

#[test]
fn target_defaults_to_both_and_status_is_independent() {
    let mut board = ToolConnections::new();
    assert_eq!(board.target("github"), ToolTarget::Both);
    let s = board
        .record_connect(
            "github",
            Some(ToolConnectionState::Connected),
            Some(ToolConnectionState::NeedsAttention),
        )
        .unwrap();
    assert_eq!(s.mac, ToolConnectionState::Connected);
    assert_eq!(s.home, ToolConnectionState::NeedsAttention);

    board.set_target("vercel", ToolTarget::Local).unwrap();
    let s = board
        .record_connect(
            "vercel",
            Some(ToolConnectionState::Connected),
            Some(ToolConnectionState::Connected),
        )
        .unwrap();
    assert_eq!(s.mac, ToolConnectionState::Connected);
    assert_eq!(s.home, ToolConnectionState::NotConnected);

    board.set_target("stripe", ToolTarget::Home).unwrap();
    let s = board
        .record_connect(
            "stripe",
            Some(ToolConnectionState::Connected),
            Some(ToolConnectionState::Connected),
        )
        .unwrap();
    assert_eq!(s.mac, ToolConnectionState::NotConnected);
    assert_eq!(s.home, ToolConnectionState::Connected);

    board.disconnect("github");
    assert_eq!(board.status("github"), Default::default());
}
