//! The committed cross-language vector for the relay end-to-end channel and machine auth.
//! Spec: `docs/design/relay-and-packaging.md` §6.5 and §9.2.
//!
//! `fixtures/relay-vectors.json` was also reproduced with an independent implementation (Python
//! `cryptography`) before it was committed. The TypeScript test in the Cloudflare slice reads the
//! same file. Every key in it is a clearly marked test key.

use serde_json::Value;
use vowp_agent::relay::auth::{machine_auth_canonical, sign_machine_auth, verify_machine_auth};
use vowp_agent::relay::crypto::{
    AcceptInput, ChannelCipher, ChannelKeys, EphemeralKey, Role, TranscriptInput, client_finish, jwk_thumbprint,
    machine_accept_with, transcript,
};
use vowp_agent::relay::{RelayError, RelayKey};
use vowp_protocol::{Jwk, RelayDataFrame};

fn vectors() -> Value {
    let text = include_str!("fixtures/relay-vectors.json");
    serde_json::from_str(text).expect("fixture is JSON")
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_else(|| panic!("fixture field {key}"))
}

fn bytes(value: &Value, key: &str) -> Vec<u8> {
    hex::decode(text(value, key)).unwrap_or_else(|_| panic!("fixture field {key} is hex"))
}

fn jwk(value: &Value, key: &str) -> Jwk {
    serde_json::from_value(value[key].clone()).expect("jwk")
}

struct Fixture {
    v: Value,
    client_key: Jwk,
    machine_key: Jwk,
}

fn fixture() -> Fixture {
    let v = vectors();
    assert_eq!(v["test_only"], true, "the fixture must declare itself test-only");
    let client_key = jwk(&v, "client_key");
    let machine_key = jwk(&v, "machine_key");
    Fixture {
        v,
        client_key,
        machine_key,
    }
}

impl Fixture {
    fn transcript(&self) -> Vec<u8> {
        transcript(&TranscriptInput {
            channel_id: text(&self.v, "channel_id"),
            client_key: &self.client_key,
            machine_key: &self.machine_key,
            client_nonce: text(&self.v, "client_nonce"),
            machine_nonce: text(&self.v, "machine_nonce"),
            machine_id: text(&self.v, "machine_id"),
            account_id: text(&self.v, "account_id"),
            ticket_jti: text(&self.v, "ticket_jti"),
        })
    }

    fn keys(&self) -> ChannelKeys {
        let client = EphemeralKey::from_bytes(&bytes(&self.v, "client_private_key")).expect("client key");
        let shared = client.agree(&self.machine_key).expect("ecdh");
        ChannelKeys::derive(&shared, &self.transcript()).expect("hkdf")
    }

    fn cipher(&self, role: Role) -> ChannelCipher {
        ChannelCipher::new(&self.keys(), text(&self.v, "channel_id"), role).expect("cipher")
    }

    fn frame(&self, index: usize) -> Value {
        self.v["frames"][index].clone()
    }
}

#[test]
fn relay_crypto_matches_the_cross_language_vector() {
    let f = fixture();
    let client = EphemeralKey::from_bytes(&bytes(&f.v, "client_private_key")).expect("client key");
    let machine = EphemeralKey::from_bytes(&bytes(&f.v, "machine_private_key")).expect("machine key");
    assert_eq!(client.jwk(), f.client_key, "client public key");
    assert_eq!(machine.jwk(), f.machine_key, "machine public key");
    assert_eq!(jwk_thumbprint(&f.client_key), text(&f.v, "client_thumbprint"));

    let t = f.transcript();
    assert_eq!(String::from_utf8(t.clone()).expect("utf8"), text(&f.v, "transcript"));
    assert!(!t.ends_with(b"\n"), "no trailing newline");
    let shared = client.agree(&f.machine_key).expect("client ecdh");
    assert_eq!(
        shared.to_vec(),
        machine.agree(&f.client_key).expect("machine ecdh").to_vec()
    );
    assert_eq!(hex::encode(shared), text(&f.v, "shared_secret"));

    let keys = ChannelKeys::derive(&shared, &t).expect("hkdf");
    assert_eq!(hex::encode(keys.c2m), text(&f.v, "c2m_key"));
    assert_eq!(hex::encode(keys.m2c), text(&f.v, "m2c_key"));
    assert_eq!(hex::encode(keys.c2m_iv), text(&f.v, "c2m_iv"));
    assert_eq!(hex::encode(keys.m2c_iv), text(&f.v, "m2c_iv"));

    let identity = RelayKey::from_secret(bytes(&f.v, "identity_secret").try_into().expect("32 bytes"));
    assert_eq!(identity.public_key_hex(), text(&f.v, "identity_public"));
    assert_eq!(identity.sign_hex(&t), text(&f.v, "machine_signature"));

    // Both directions, byte for byte: nonce, AAD, ciphertext and the whole binary frame.
    let mut client_cipher = f.cipher(Role::Client);
    let mut machine_cipher = f.cipher(Role::Machine);
    for (index, sealer) in [(0usize, &mut client_cipher), (1usize, &mut machine_cipher)] {
        let frame = f.frame(index);
        let sealed = sealer.seal(text(&frame, "plaintext").as_bytes()).expect("seal");
        assert_eq!(sealed.sequence, 1);
        assert_eq!(
            hex::encode(RelayDataFrame::aad(sealed.channel_id, 1)),
            text(&frame, "aad")
        );
        assert_eq!(hex::encode(&sealed.ciphertext), text(&frame, "ciphertext"));
        assert_eq!(hex::encode(sealed.to_bytes().expect("encode")), text(&frame, "frame"));
        let iv = if index == 0 { keys.c2m_iv } else { keys.m2c_iv };
        assert_eq!(text(&frame, "nonce"), format!("{}{:016x}", hex::encode(iv), 1));
    }

    // And the committed ciphertext decrypts on the other end.
    let on_machine = RelayDataFrame::from_bytes(&bytes(&f.frame(0), "frame")).expect("frame 0");
    let plain = f.cipher(Role::Machine).open(&on_machine).expect("machine opens c2m");
    assert_eq!(String::from_utf8(plain).expect("utf8"), text(&f.frame(0), "plaintext"));
    let on_client = RelayDataFrame::from_bytes(&bytes(&f.frame(1), "frame")).expect("frame 1");
    let plain = f.cipher(Role::Client).open(&on_client).expect("client opens m2c");
    assert_eq!(String::from_utf8(plain).expect("utf8"), text(&f.frame(1), "plaintext"));
}

#[test]
fn relay_crypto_rejects_tampered_ciphertext() {
    let f = fixture();
    let good = RelayDataFrame::from_bytes(&bytes(&f.frame(0), "frame")).expect("frame");

    // Every ciphertext byte, including the tag, is authenticated.
    for index in [0, good.ciphertext.len() / 2, good.ciphertext.len() - 1] {
        let mut bad = good.clone();
        bad.ciphertext[index] ^= 0x01;
        assert!(f.cipher(Role::Machine).open(&bad).is_err(), "ciphertext byte {index}");
    }
    // A truncated ciphertext fails authentication.
    let mut short = good.clone();
    short.ciphertext.pop();
    assert!(f.cipher(Role::Machine).open(&short).is_err(), "truncated");

    // The header is AAD and the sequence is part of the nonce: changing either fails.
    let mut other_sequence = good.clone();
    other_sequence.sequence = 2;
    assert!(
        f.cipher(Role::Machine).open(&other_sequence).is_err(),
        "sequence 2 before 1"
    );
    let mut other_channel = good.clone();
    other_channel.channel_id[0] ^= 0xff;
    assert!(
        f.cipher(Role::Machine).open(&other_channel).is_err(),
        "unknown channel id"
    );

    // Header bytes the parser accepts are still authenticated: flipping a flag bit is refused by
    // the parser, and re-encoding with a different version byte is refused too.
    let mut wire = bytes(&f.frame(0), "frame");
    wire[7] = 1;
    assert!(RelayDataFrame::from_bytes(&wire).is_err(), "flags must stay zero");
    wire[7] = 0;
    wire[4] = 2;
    assert!(RelayDataFrame::from_bytes(&wire).is_err(), "version must stay 1");

    // A frame sealed by the wrong direction's key does not open (no reflection).
    let reflected = RelayDataFrame::from_bytes(&bytes(&f.frame(0), "frame")).expect("frame");
    assert!(
        f.cipher(Role::Client).open(&reflected).is_err(),
        "c2m ciphertext must not open as m2c"
    );

    // The right frame still opens afterwards on a fresh cipher: the failures changed nothing.
    assert!(f.cipher(Role::Machine).open(&good).is_ok());
}

#[test]
fn relay_crypto_enforces_strict_sequence_order() {
    let f = fixture();
    let mut machine = f.cipher(Role::Machine);
    let mut client = f.cipher(Role::Client);
    let one = client.seal(b"one").expect("seal 1");
    let two = client.seal(b"two").expect("seal 2");
    let three = client.seal(b"three").expect("seal 3");
    assert_eq!((one.sequence, two.sequence, three.sequence), (1, 2, 3));
    assert!(machine.open(&two).is_err(), "a skipped frame is refused");
    assert_eq!(machine.open(&one).expect("in order"), b"one");
    assert!(machine.open(&one).is_err(), "a replay is refused");
    assert!(machine.open(&three).is_err(), "still waiting for 2");
    assert_eq!(machine.open(&two).expect("2 after 1"), b"two");
    assert_eq!(machine.open(&three).expect("3 after 2"), b"three");
}

#[test]
fn relay_handshake_signature_is_checked_before_keys_are_derived() {
    let f = fixture();
    let client = EphemeralKey::from_bytes(&bytes(&f.v, "client_private_key")).expect("client");
    let machine = EphemeralKey::from_bytes(&bytes(&f.v, "machine_private_key")).expect("machine");
    let identity = RelayKey::from_secret(bytes(&f.v, "identity_secret").try_into().expect("32 bytes"));
    let input = AcceptInput {
        channel_id: text(&f.v, "channel_id"),
        client_key: &f.client_key,
        client_nonce: text(&f.v, "client_nonce"),
        machine_id: text(&f.v, "machine_id"),
        account_id: text(&f.v, "account_id"),
        ticket_jti: text(&f.v, "ticket_jti"),
    };
    let accept = machine_accept_with(&input, &identity, &machine, text(&f.v, "machine_nonce").to_owned())
        .expect("machine accept");
    assert_eq!(accept.machine_signature, text(&f.v, "machine_signature"));
    assert_eq!(accept.machine_key, f.machine_key);

    let mkey = text(&f.v, "identity_public");
    let mut client_cipher = client_finish(
        &client,
        &input,
        &accept.machine_key,
        &accept.machine_nonce,
        &accept.machine_signature,
        mkey,
    )
    .expect("client verifies and derives");
    let mut machine_cipher = accept.cipher;
    let request = client_cipher.seal(b"ping").expect("seal");
    assert_eq!(machine_cipher.open(&request).expect("open"), b"ping");

    // A relay that substitutes its own machine key signs with another identity: the client uses the
    // ticket's mkey and refuses before deriving anything.
    let relay_identity = RelayKey::from_secret([0x44; 32]);
    let forged = machine_accept_with(&input, &relay_identity, &machine, accept.machine_nonce.clone()).expect("forge");
    let error = client_finish(
        &client,
        &input,
        &forged.machine_key,
        &forged.machine_nonce,
        &forged.machine_signature,
        mkey,
    )
    .expect_err("a signature by another key must fail");
    assert!(matches!(error, RelayError::Crypto(_)), "{error}");

    // A signature over a different transcript (a swapped machine key) also fails.
    let other = EphemeralKey::from_bytes(&[0x55; 32]).expect("other key");
    let error = client_finish(
        &client,
        &input,
        &other.jwk(),
        &accept.machine_nonce,
        &accept.machine_signature,
        mkey,
    )
    .expect_err("transcript binds the machine ephemeral key");
    assert!(matches!(error, RelayError::Crypto(_)));

    // A different ticket jti changes the transcript.
    let swapped = AcceptInput {
        ticket_jti: "jti_other",
        ..input.clone()
    };
    assert!(
        client_finish(
            &client,
            &swapped,
            &accept.machine_key,
            &accept.machine_nonce,
            &accept.machine_signature,
            mkey,
        )
        .is_err(),
        "the transcript binds the ticket jti"
    );
}

#[test]
fn relay_crypto_rejects_invalid_peer_keys() {
    let client = EphemeralKey::from_bytes(&[0x11; 32]).expect("client");
    let good = client.jwk();
    for bad in [
        Jwk {
            kty: "OKP".to_owned(),
            ..good.clone()
        },
        Jwk {
            crv: "P-384".to_owned(),
            ..good.clone()
        },
        Jwk {
            x: "AAAA".to_owned(),
            ..good.clone()
        },
        Jwk {
            x: "!!!".to_owned(),
            ..good.clone()
        },
        // A well-formed pair that is not on the curve.
        Jwk {
            y: good.x.clone(),
            ..good.clone()
        },
    ] {
        assert!(client.agree(&bad).is_err(), "{bad:?}");
    }
    assert!(EphemeralKey::from_bytes(&[0u8; 32]).is_err(), "zero scalar");
}

#[test]
fn machine_auth_canonical_string_and_signature_match_the_fixed_vector() {
    let f = fixture();
    let auth = &f.v["machine_auth"];
    let canonical = machine_auth_canonical(
        text(auth, "machine_id"),
        text(auth, "account_id"),
        text(auth, "key_id"),
        text(auth, "relay_id"),
        text(auth, "session_id"),
        text(auth, "nonce"),
        text(auth, "server_time"),
    );
    assert_eq!(canonical, text(auth, "canonical"));
    assert!(canonical.starts_with("STACKSPIRIT-RELAY-AUTH-V1\nmch_"));
    assert_eq!(canonical.matches('\n').count(), 7, "eight fields, seven separators");
    assert!(!canonical.ends_with('\n'), "no trailing newline");

    let identity = RelayKey::from_secret(bytes(&f.v, "identity_secret").try_into().expect("32 bytes"));
    assert_eq!(sign_machine_auth(&identity, &canonical), text(auth, "signature"));
    verify_machine_auth(&identity.public_key_hex(), &canonical, text(auth, "signature")).expect("verifies");

    // Any change to any field invalidates the signature.
    let tweaked = canonical.replace("sess-01", "sess-02");
    assert!(verify_machine_auth(&identity.public_key_hex(), &tweaked, text(auth, "signature")).is_err());
    let with_newline = format!("{canonical}\n");
    assert!(verify_machine_auth(&identity.public_key_hex(), &with_newline, text(auth, "signature")).is_err());
}
