//! Relay control frames, remote frames and the binary data-frame header.
//! Spec: `docs/design/relay-and-packaging.md` §6.3, §6.5 and §6.6.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use vowp_protocol::{
    ClientKind, DATA_PREFIX_BYTES, DataFrameError, ExecChannel, Jwk, MAX_RELAY_FRAME_BYTES, MachineCapabilities,
    MachineState, RelayDataFrame, RelayFrame, RemoteFrame, Scope, Timestamp, codes,
};

fn jwk() -> Jwk {
    Jwk {
        kty: "EC".to_owned(),
        crv: "P-256".to_owned(),
        x: "x-coordinate".to_owned(),
        y: "y-coordinate".to_owned(),
    }
}

fn at() -> Timestamp {
    Timestamp::from_unix_millis(1_764_072_000_123)
}

fn every_frame() -> Vec<RelayFrame> {
    vec![
        RelayFrame::MachineHello {
            v: 1,
            machine_id: "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A".to_owned(),
            account_id: "acct_01JQ9B7M5X6P8S1T3W4Y5Z6A7B".to_owned(),
            key_id: "ed25519:ab".to_owned(),
            public_key: "ab".to_owned(),
            credential: "jwt".to_owned(),
            agent_version: "0.5.1".to_owned(),
            capabilities: MachineCapabilities {
                http: true,
                projects: vec!["vowp".to_owned()],
                ..MachineCapabilities::default()
            },
            sent_at: at(),
        },
        RelayFrame::RelayChallenge {
            v: 1,
            relay_id: "relay-1".to_owned(),
            session_id: "sess-1".to_owned(),
            nonce: "n".to_owned(),
            server_time: at(),
            heartbeat_secs: 20,
            max_frame_bytes: 1_048_576,
            features: vec!["resume".to_owned()],
        },
        RelayFrame::MachineAuth {
            v: 1,
            signature: "00".to_owned(),
        },
        RelayFrame::RelayWelcome {
            v: 1,
            relay_id: "relay-1".to_owned(),
            session_id: "sess-1".to_owned(),
            machine_id: "m".to_owned(),
            account_id: "a".to_owned(),
            heartbeat_secs: 20,
            max_frame_bytes: 1_048_576,
            features: vec![],
        },
        RelayFrame::MachineStatus {
            v: 1,
            state: MachineState::Degraded,
            channels: 3,
            sent_at: at(),
        },
        RelayFrame::ClientHello {
            v: 1,
            machine_id: "m".to_owned(),
            ticket: "t".to_owned(),
            client_id: "cli_1".to_owned(),
            kind: ClientKind::Desktop,
            protocol: "stackspirit-remote/1".to_owned(),
            capabilities: vec![],
            client_key: jwk(),
            client_nonce: "cn".to_owned(),
            sent_at: at(),
        },
        RelayFrame::ClientWelcome {
            v: 1,
            relay_id: "r".to_owned(),
            session_id: "s".to_owned(),
            channel_id: "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A".to_owned(),
            machine_id: "m".to_owned(),
            account_id: "a".to_owned(),
            expires_at: at(),
            features: vec![],
        },
        RelayFrame::ChannelOpen {
            v: 1,
            channel_id: "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A".to_owned(),
            machine_id: "m".to_owned(),
            client_id: "c".to_owned(),
            kind: ClientKind::Web,
            ticket: "t".to_owned(),
            scopes: vec![Scope::HttpRead, Scope::McpRead],
            client_key: jwk(),
            client_nonce: "cn".to_owned(),
        },
        RelayFrame::ChannelAccept {
            v: 1,
            channel_id: "c".to_owned(),
            machine_key: jwk(),
            machine_nonce: "mn".to_owned(),
            machine_key_id: "ed25519:ab".to_owned(),
            machine_signature: "00".to_owned(),
            suite: "s".to_owned(),
        },
        RelayFrame::ChannelReject {
            v: 1,
            channel_id: "c".to_owned(),
            code: codes::SCOPE_DENIED.to_owned(),
            message: "no".to_owned(),
            retryable: false,
        },
        RelayFrame::ChannelClose {
            v: 1,
            channel_id: "c".to_owned(),
            code: "timeout".to_owned(),
            message: "idle".to_owned(),
        },
        RelayFrame::Superseded {
            v: 1,
            reason: "another connection".to_owned(),
        },
        RelayFrame::Ping {
            v: 1,
            nonce: "p".to_owned(),
        },
        RelayFrame::Pong {
            v: 1,
            nonce: "p".to_owned(),
        },
        RelayFrame::Error {
            v: 1,
            code: codes::UNAUTHENTICATED.to_owned(),
            message: "bad".to_owned(),
            retryable: true,
        },
    ]
}

#[test]
fn every_relay_frame_round_trips_with_its_dotted_type_tag() {
    let frames = every_frame();
    assert_eq!(frames.len(), 15, "one fixture per RelayFrame variant");
    for frame in frames {
        let text = frame.to_json().expect("serialise");
        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["type"], frame.type_name(), "{text}");
        assert_eq!(value["v"], 1);
        let back = RelayFrame::from_json(&text).expect("parse");
        assert_eq!(back, frame, "{text}");
    }
}

#[test]
fn relay_frames_ignore_unknown_fields_and_default_optional_ones() {
    let frame = RelayFrame::from_json(
        r#"{"type":"relay.challenge","v":1,"relay_id":"r","session_id":"s","nonce":"n",
            "server_time":"2026-11-25T12:00:00.000Z","heartbeat_secs":20,"max_frame_bytes":1048576,
            "future_field":{"anything":true}}"#,
    )
    .expect("unknown fields are ignored and features defaults");
    assert!(matches!(frame, RelayFrame::RelayChallenge { features, .. } if features.is_empty()));

    let hello = RelayFrame::from_json(
        r#"{"type":"machine.hello","v":1,"machine_id":"m","account_id":"a","key_id":"k","public_key":"p",
            "credential":"c","agent_version":"1","sent_at":"2026-11-25T12:00:00.000Z"}"#,
    )
    .expect("capabilities default");
    assert!(
        matches!(hello, RelayFrame::MachineHello { capabilities, .. } if capabilities == MachineCapabilities::default())
    );
    let reject = RelayFrame::from_json(r#"{"type":"channel.reject","v":1,"channel_id":"c","code":"x","message":"y"}"#)
        .expect("retryable defaults to false");
    assert!(matches!(reject, RelayFrame::ChannelReject { retryable: false, .. }));
}

#[test]
fn relay_frames_reject_unknown_types_versions_and_missing_fields() {
    assert!(
        RelayFrame::from_json(r#"{"type":"machine_hello","v":1}"#).is_err(),
        "snake tag is not the wire tag"
    );
    assert!(
        RelayFrame::from_json(r#"{"type":"teleport","v":1}"#).is_err(),
        "unknown type"
    );
    assert!(
        RelayFrame::from_json(r#"{"type":"ping","v":2,"nonce":"n"}"#).is_err(),
        "unsupported v"
    );
    assert!(
        RelayFrame::from_json(r#"{"type":"ping","v":1}"#).is_err(),
        "missing nonce"
    );
    assert!(
        RelayFrame::from_json(r#"{"type":"ping","nonce":"n"}"#).is_err(),
        "missing v"
    );
    assert!(RelayFrame::from_json("not json").is_err());
}

#[test]
fn a_control_frame_over_the_cap_is_refused_before_parsing() {
    let big = format!(r#"{{"type":"ping","v":1,"nonce":"{}"}}"#, "a".repeat(70_000));
    let error = RelayFrame::from_json(&big).expect_err("over 65536 bytes");
    assert!(error.to_string().contains("65536"), "{error}");
}

#[test]
fn scopes_use_their_dotted_wire_names() {
    for scope in Scope::ALL {
        let text = serde_json::to_string(&scope).expect("scope");
        assert_eq!(text, format!("\"{}\"", scope.as_str()));
        assert_eq!(Scope::parse(scope.as_str()), Some(scope));
    }
    assert!(serde_json::from_str::<Scope>("\"http_read\"").is_err());
    assert_eq!(Scope::parse("root"), None);
    assert_eq!(serde_json::to_string(&ClientKind::Mcp).unwrap(), "\"mcp\"");
    assert_eq!(serde_json::to_string(&MachineState::Revoked).unwrap(), "\"revoked\"");
}

#[test]
fn canonical_jwk_is_rfc_7638_sorted_and_compact() {
    assert_eq!(
        jwk().canonical(),
        r#"{"crv":"P-256","kty":"EC","x":"x-coordinate","y":"y-coordinate"}"#
    );
    let hostile = Jwk {
        x: "a\",\"kty\":\"x".to_owned(),
        ..jwk()
    };
    let parsed: Value = serde_json::from_str(&hostile.canonical()).expect("still valid JSON");
    assert_eq!(parsed["kty"], "EC", "a hostile member cannot inject a key");
}

fn remote_frames() -> Vec<RemoteFrame> {
    let mut headers = BTreeMap::new();
    headers.insert("Accept".to_owned(), "application/json".to_owned());
    vec![
        RemoteFrame::Hello {
            v: 1,
            client: "c".to_owned(),
            machine: "m".to_owned(),
            scopes: vec![Scope::HttpRead],
            capabilities: vec![],
        },
        RemoteFrame::HelloAck {
            v: 1,
            machine: "m".to_owned(),
            scopes: vec![Scope::HttpRead],
            capabilities: vec!["http".to_owned()],
        },
        RemoteFrame::HttpRequest {
            stream: 1,
            method: "GET".to_owned(),
            path: "/api/v1/health".to_owned(),
            headers: headers.clone(),
            body: String::new(),
        },
        RemoteFrame::HttpResponse {
            stream: 1,
            status: 200,
            headers,
        },
        RemoteFrame::HttpBody {
            stream: 1,
            data: "e30=".to_owned(),
            end: true,
        },
        RemoteFrame::HttpCancel { stream: 1 },
        RemoteFrame::McpRequest {
            stream: 2,
            project: Some("vowp".to_owned()),
            session: None,
            request: json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
        },
        RemoteFrame::McpResponse {
            stream: 2,
            response: json!({"jsonrpc":"2.0","id":1,"result":{}}),
        },
        RemoteFrame::McpCancel { stream: 2 },
        RemoteFrame::TcpOpen {
            stream: 3,
            target: "ssh".to_owned(),
        },
        RemoteFrame::TcpData {
            stream: 3,
            data: "AA==".to_owned(),
        },
        RemoteFrame::TcpClose { stream: 3 },
        RemoteFrame::ExecStart {
            stream: 4,
            argv: vec!["/bin/true".to_owned()],
            cwd: None,
            pty: true,
            cols: 80,
            rows: 24,
            env: BTreeMap::new(),
        },
        RemoteFrame::ExecData {
            stream: 4,
            data: "AA==".to_owned(),
            channel: ExecChannel::Stderr,
        },
        RemoteFrame::ExecResize {
            stream: 4,
            cols: 100,
            rows: 30,
        },
        RemoteFrame::ExecExit {
            stream: 4,
            code: 1,
            signal: Some("TERM".to_owned()),
        },
        RemoteFrame::StreamError {
            stream: 5,
            code: codes::NOT_IMPLEMENTED.to_owned(),
            message: "later".to_owned(),
            retryable: false,
        },
        RemoteFrame::Ping { nonce: "p".to_owned() },
        RemoteFrame::Pong { nonce: "p".to_owned() },
        RemoteFrame::Close {
            code: "bye".to_owned(),
            message: "done".to_owned(),
        },
    ]
}

#[test]
fn every_remote_frame_round_trips_with_a_snake_case_tag() {
    let frames = remote_frames();
    assert_eq!(frames.len(), 20, "one fixture per RemoteFrame variant");
    for frame in frames {
        let bytes = frame.to_json().expect("serialise");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let tag = value["type"].as_str().expect("type tag");
        assert!(tag.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{tag}");
        assert_eq!(RemoteFrame::from_json(&bytes).expect("parse"), frame);
    }
}

#[test]
fn remote_frames_default_optionals_ignore_unknowns_and_reject_bad_input() {
    let request = RemoteFrame::from_json(
        br#"{"type":"http_request","stream":1,"method":"GET","path":"/api/v1/health","extra":1}"#,
    )
    .expect("headers and body default");
    assert!(matches!(request, RemoteFrame::HttpRequest { headers, body, .. } if headers.is_empty() && body.is_empty()));
    let mcp = RemoteFrame::from_json(br#"{"type":"mcp_request","stream":1,"request":{}}"#).expect("optionals default");
    assert!(matches!(
        mcp,
        RemoteFrame::McpRequest {
            project: None,
            session: None,
            ..
        }
    ));
    assert!(
        RemoteFrame::from_json(br#"{"type":"http_request","stream":1}"#).is_err(),
        "missing method"
    );
    assert!(RemoteFrame::from_json(br#"{"type":"nope"}"#).is_err(), "unknown type");
    assert_eq!(RemoteFrame::Ping { nonce: "x".to_owned() }.stream(), None);
    assert_eq!(RemoteFrame::HttpCancel { stream: 9 }.stream(), Some(9));
    assert!(!vowp_protocol::valid_stream_id(0));
    assert!(vowp_protocol::valid_stream_id(2_147_483_647));
    assert!(!vowp_protocol::valid_stream_id(2_147_483_648));
}

#[test]
fn an_oversized_remote_frame_is_refused_both_ways() {
    let frame = RemoteFrame::TcpData {
        stream: 1,
        data: "A".repeat(800_000),
    };
    assert!(frame.to_json().is_err(), "sender never sends an over-cap frame");
    let bytes = vec![b' '; 800_000];
    assert!(
        RemoteFrame::from_json(&bytes).is_err(),
        "reader refuses an over-cap frame"
    );
}

#[test]
fn data_frame_header_layout_is_exact() {
    let channel = RelayDataFrame::channel_bytes("01JQ8Z6K4W5N7R9T2V3X4Y5Z6A").expect("ulid");
    assert_eq!(RelayDataFrame::channel_string(channel), "01JQ8Z6K4W5N7R9T2V3X4Y5Z6A");
    let frame = RelayDataFrame {
        channel_id: channel,
        sequence: 0x0102_0304_0506_0708,
        ciphertext: vec![9, 9, 9],
    };
    let bytes = frame.to_bytes().expect("encode");
    assert_eq!(&bytes[0..4], b"SSR1");
    assert_eq!(bytes[4], 1, "version");
    assert_eq!(bytes[5], 1, "type data");
    assert_eq!(&bytes[6..8], &[0, 0], "flags");
    assert_eq!(&bytes[8..24], &channel);
    assert_eq!(&bytes[24..32], &[1, 2, 3, 4, 5, 6, 7, 8], "big-endian sequence");
    assert_eq!(&bytes[32..36], &[0, 0, 0, 3], "big-endian length");
    assert_eq!(&bytes[36..], &[9, 9, 9]);
    assert_eq!(&bytes[..32], &RelayDataFrame::aad(channel, frame.sequence));
    assert_eq!(RelayDataFrame::from_bytes(&bytes).expect("decode"), frame);
}

#[test]
fn data_frames_reject_every_malformation() {
    let good = RelayDataFrame {
        channel_id: [7; 16],
        sequence: 1,
        ciphertext: vec![1; 20],
    }
    .to_bytes()
    .expect("encode");
    let mutate = |index: usize, value: u8| {
        let mut bytes = good.clone();
        bytes[index] = value;
        RelayDataFrame::from_bytes(&bytes)
    };
    assert_eq!(
        RelayDataFrame::from_bytes(&good[..DATA_PREFIX_BYTES - 1]),
        Err(DataFrameError::Truncated)
    );
    assert_eq!(mutate(0, b'X'), Err(DataFrameError::BadMagic));
    assert_eq!(mutate(4, 2), Err(DataFrameError::BadVersion(2)));
    assert_eq!(
        mutate(5, 2),
        Err(DataFrameError::BadType(2)),
        "rekey is reserved and rejected"
    );
    assert_eq!(mutate(5, 0), Err(DataFrameError::BadType(0)));
    assert_eq!(mutate(6, 1), Err(DataFrameError::BadFlags));
    assert_eq!(mutate(7, 1), Err(DataFrameError::BadFlags));
    assert_eq!(
        mutate(35, 21),
        Err(DataFrameError::BadLength),
        "length longer than the body"
    );
    let mut extra = good.clone();
    extra.push(0);
    assert_eq!(
        RelayDataFrame::from_bytes(&extra),
        Err(DataFrameError::BadLength),
        "trailing bytes"
    );
    let huge = vec![0u8; MAX_RELAY_FRAME_BYTES + 1];
    assert_eq!(RelayDataFrame::from_bytes(&huge), Err(DataFrameError::TooLarge));
    let oversized = RelayDataFrame {
        channel_id: [0; 16],
        sequence: 1,
        ciphertext: vec![0; MAX_RELAY_FRAME_BYTES],
    };
    assert_eq!(
        oversized.to_bytes(),
        Err(DataFrameError::TooLarge),
        "no partial frame is produced"
    );
}

#[test]
fn channel_id_strings_must_be_ulids() {
    assert!(RelayDataFrame::channel_bytes("not-a-ulid").is_none());
    assert!(RelayDataFrame::channel_bytes("").is_none());
}
