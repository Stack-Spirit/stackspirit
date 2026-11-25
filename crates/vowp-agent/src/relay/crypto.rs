//! The end-to-end channel: ephemeral P-256 ECDH, HKDF-SHA256 and AES-256-GCM.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.5. The relay forwards the handshake frames and
//! the binary data frames but never holds `c_priv` or `m_priv`, so it cannot derive the keys. The
//! machine proves its long-lived Ed25519 identity by signing the canonical transcript; the client
//! verifies that signature with the ticket's `mkey` **before** it derives any key.
//!
//! The byte layouts here (transcript, HKDF labels, nonce, AAD) are pinned by
//! `tests/fixtures/relay-vectors.json`; a change fails the cross-language vector test.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::{FromSec1Point, ToSec1Point};
use p256::{PublicKey, Sec1Point, SecretKey};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use vowp_protocol::{DATA_HEADER_BYTES, E2E_TRANSCRIPT_DOMAIN, Jwk, MAX_RELAY_FRAME_BYTES, RelayDataFrame};

use super::RelayError;

/// Bytes of a handshake nonce before base64url encoding.
pub const NONCE_BYTES: usize = 32;
/// The AES-GCM tag length.
pub const TAG_BYTES: usize = 16;

/// An ephemeral P-256 key pair. The private half never leaves memory and is never printed.
pub struct EphemeralKey {
    secret: SecretKey,
}

impl std::fmt::Debug for EphemeralKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EphemeralKey").finish_non_exhaustive()
    }
}

impl EphemeralKey {
    /// Generates a fresh key from the operating system's randomness.
    pub fn generate() -> Self {
        loop {
            let mut bytes = [0u8; 32];
            OsRng.fill_bytes(&mut bytes);
            if let Ok(secret) = SecretKey::from_slice(&bytes) {
                return Self { secret };
            }
        }
    }

    /// Builds a key from a fixed 32-byte scalar. Used by the committed test vector only.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RelayError> {
        SecretKey::from_slice(bytes)
            .map(|secret| Self { secret })
            .map_err(|_| RelayError::Crypto("invalid P-256 scalar".to_owned()))
    }

    /// The public key as a JWK.
    pub fn jwk(&self) -> Jwk {
        jwk_from_public(&self.secret.public_key())
    }

    /// The raw ECDH x coordinate with `peer`. An all-zero result is an error.
    pub fn agree(&self, peer: &Jwk) -> Result<[u8; 32], RelayError> {
        let peer = public_from_jwk(peer)?;
        let shared = p256::ecdh::diffie_hellman(self.secret.to_nonzero_scalar(), peer.as_affine());
        let mut z = [0u8; 32];
        z.copy_from_slice(shared.raw_secret_bytes().as_slice());
        if z.iter().all(|byte| *byte == 0) {
            return Err(RelayError::Crypto("all-zero ECDH result".to_owned()));
        }
        Ok(z)
    }
}

/// Encodes a public key as a P-256 JWK (`x` and `y` base64url, no padding).
pub fn jwk_from_public(public: &PublicKey) -> Jwk {
    let point = public.to_sec1_point(false);
    // invariant: an uncompressed P-256 point always has both coordinates.
    let x = point.x().map(|x| URL_SAFE_NO_PAD.encode(x)).unwrap_or_default();
    let y = point.y().map(|y| URL_SAFE_NO_PAD.encode(y)).unwrap_or_default();
    Jwk {
        kty: "EC".to_owned(),
        crv: "P-256".to_owned(),
        x,
        y,
    }
}

/// Decodes and validates a P-256 JWK: type, curve, coordinate lengths and the point on the curve.
pub fn public_from_jwk(jwk: &Jwk) -> Result<PublicKey, RelayError> {
    if jwk.kty != "EC" || jwk.crv != "P-256" {
        return Err(RelayError::Crypto("the key is not an EC P-256 JWK".to_owned()));
    }
    let coordinate = |text: &str| -> Result<[u8; 32], RelayError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| RelayError::Crypto("a JWK coordinate is not base64url".to_owned()))?;
        <[u8; 32]>::try_from(bytes.as_slice())
            .map_err(|_| RelayError::Crypto("a JWK coordinate is not 32 bytes".to_owned()))
    };
    let point = Sec1Point::from_affine_coordinates(&coordinate(&jwk.x)?.into(), &coordinate(&jwk.y)?.into(), false);
    Option::<PublicKey>::from(PublicKey::from_sec1_point(&point))
        .ok_or_else(|| RelayError::Crypto("the JWK point is not on the P-256 curve".to_owned()))
}

/// The RFC 7638 SHA-256 thumbprint of `jwk`, base64url without padding (the ticket's `cnf.jkt`).
pub fn jwk_thumbprint(jwk: &Jwk) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(jwk.canonical().as_bytes()))
}

/// Returns 32 random bytes as base64url, for a handshake nonce.
pub fn random_nonce() -> String {
    let mut bytes = [0u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The inputs that bind a handshake to one channel, one ticket and both ephemeral keys.
#[derive(Debug, Clone)]
pub struct TranscriptInput<'a> {
    /// The 26-character ULID channel id.
    pub channel_id: &'a str,
    /// The client's ephemeral key.
    pub client_key: &'a Jwk,
    /// The machine's ephemeral key.
    pub machine_key: &'a Jwk,
    /// The client nonce as sent.
    pub client_nonce: &'a str,
    /// The machine nonce as sent.
    pub machine_nonce: &'a str,
    /// The machine id.
    pub machine_id: &'a str,
    /// The account id.
    pub account_id: &'a str,
    /// The ticket `jti`.
    pub ticket_jti: &'a str,
}

/// The canonical transcript `T`: nine lines joined by `\n`, no trailing newline.
pub fn transcript(input: &TranscriptInput<'_>) -> Vec<u8> {
    [
        E2E_TRANSCRIPT_DOMAIN,
        input.channel_id,
        &input.client_key.canonical(),
        &input.machine_key.canonical(),
        input.client_nonce,
        input.machine_nonce,
        input.machine_id,
        input.account_id,
        input.ticket_jti,
    ]
    .join("\n")
    .into_bytes()
}

/// The two directional keys and IV prefixes derived from one handshake.
#[derive(Clone, PartialEq, Eq)]
pub struct ChannelKeys {
    /// Client-to-machine AES-256 key.
    pub c2m: [u8; 32],
    /// Machine-to-client AES-256 key.
    pub m2c: [u8; 32],
    /// Client-to-machine 4-byte IV prefix.
    pub c2m_iv: [u8; 4],
    /// Machine-to-client 4-byte IV prefix.
    pub m2c_iv: [u8; 4],
}

impl std::fmt::Debug for ChannelKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelKeys").finish_non_exhaustive()
    }
}

impl ChannelKeys {
    /// Derives the keys: `salt = SHA-256(T)`, `prk = HKDF-Extract(salt, Z)`, then four expands
    /// with the labels `stackspirit c2m key`+`0x01`, `stackspirit m2c key`+`0x01`,
    /// `stackspirit c2m iv` and `stackspirit m2c iv`.
    pub fn derive(shared: &[u8; 32], transcript: &[u8]) -> Result<Self, RelayError> {
        let salt = Sha256::digest(transcript);
        let hkdf = Hkdf::<Sha256>::new(Some(salt.as_slice()), shared);
        let fail = |_| RelayError::Crypto("HKDF expand failed".to_owned());
        let mut keys = Self {
            c2m: [0; 32],
            m2c: [0; 32],
            c2m_iv: [0; 4],
            m2c_iv: [0; 4],
        };
        hkdf.expand(b"stackspirit c2m key\x01", &mut keys.c2m).map_err(fail)?;
        hkdf.expand(b"stackspirit m2c key\x01", &mut keys.m2c).map_err(fail)?;
        hkdf.expand(b"stackspirit c2m iv", &mut keys.c2m_iv).map_err(fail)?;
        hkdf.expand(b"stackspirit m2c iv", &mut keys.m2c_iv).map_err(fail)?;
        Ok(keys)
    }
}

/// Which end of the channel a [`ChannelCipher`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The machine seals with `m2c` and opens with `c2m`.
    Machine,
    /// The client seals with `c2m` and opens with `m2c`.
    Client,
}

/// One end of an established channel: two keys, two sequences and the channel id.
pub struct ChannelCipher {
    channel_id: [u8; 16],
    send: Aes256Gcm,
    send_iv: [u8; 4],
    recv: Aes256Gcm,
    recv_iv: [u8; 4],
    send_seq: u64,
    recv_seq: u64,
}

impl std::fmt::Debug for ChannelCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelCipher")
            .field("channel", &RelayDataFrame::channel_string(self.channel_id))
            .field("send_seq", &self.send_seq)
            .field("recv_seq", &self.recv_seq)
            .finish_non_exhaustive()
    }
}

fn nonce(iv: [u8; 4], sequence: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&iv);
    nonce[4..].copy_from_slice(&sequence.to_be_bytes());
    nonce
}

impl ChannelCipher {
    /// Builds one end from derived keys. Sequences start at 1 in both directions.
    pub fn new(keys: &ChannelKeys, channel_id: &str, role: Role) -> Result<Self, RelayError> {
        let channel_id = RelayDataFrame::channel_bytes(channel_id)
            .ok_or_else(|| RelayError::Crypto("the channel id is not a ULID".to_owned()))?;
        let cipher = |key: &[u8; 32]| {
            Aes256Gcm::new_from_slice(key).map_err(|_| RelayError::Crypto("bad AES key length".to_owned()))
        };
        let (send_key, send_iv, recv_key, recv_iv) = match role {
            Role::Machine => (&keys.m2c, keys.m2c_iv, &keys.c2m, keys.c2m_iv),
            Role::Client => (&keys.c2m, keys.c2m_iv, &keys.m2c, keys.m2c_iv),
        };
        Ok(Self {
            channel_id,
            send: cipher(send_key)?,
            send_iv,
            recv: cipher(recv_key)?,
            recv_iv,
            send_seq: 0,
            recv_seq: 0,
        })
    }

    /// The raw channel id.
    pub fn channel_id(&self) -> [u8; 16] {
        self.channel_id
    }

    /// Encrypts one plaintext into the next data frame. Nothing is sent on failure, so a caller
    /// never emits a partial frame.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<RelayDataFrame, RelayError> {
        if plaintext.len() + TAG_BYTES + vowp_protocol::DATA_PREFIX_BYTES > MAX_RELAY_FRAME_BYTES {
            return Err(RelayError::Crypto("the frame is over the 1 MiB cap".to_owned()));
        }
        let sequence = self
            .send_seq
            .checked_add(1)
            .ok_or_else(|| RelayError::Crypto("the send sequence is exhausted".to_owned()))?;
        let aad = RelayDataFrame::aad(self.channel_id, sequence);
        let ciphertext = self
            .send
            .encrypt(
                &Nonce::from(nonce(self.send_iv, sequence)),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| RelayError::Crypto("encryption failed".to_owned()))?;
        self.send_seq = sequence;
        Ok(RelayDataFrame {
            channel_id: self.channel_id,
            sequence,
            ciphertext,
        })
    }

    /// Decrypts one frame. The channel id must match, the sequence must be exactly one more than
    /// the last accepted, and the header is authenticated as AAD. A failure leaves the receive
    /// state unchanged; the caller closes the channel with `crypto_error`.
    pub fn open(&mut self, frame: &RelayDataFrame) -> Result<Vec<u8>, RelayError> {
        if frame.channel_id != self.channel_id {
            return Err(RelayError::Crypto("unknown channel id".to_owned()));
        }
        let expected = self
            .recv_seq
            .checked_add(1)
            .ok_or_else(|| RelayError::Crypto("the receive sequence is exhausted".to_owned()))?;
        if frame.sequence != expected {
            return Err(RelayError::Crypto("unexpected sequence".to_owned()));
        }
        let aad = RelayDataFrame::aad(frame.channel_id, frame.sequence);
        debug_assert_eq!(aad.len(), DATA_HEADER_BYTES);
        let plaintext = self
            .recv
            .decrypt(
                &Nonce::from(nonce(self.recv_iv, frame.sequence)),
                Payload {
                    msg: &frame.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| RelayError::Crypto("authentication failed".to_owned()))?;
        self.recv_seq = expected;
        Ok(plaintext)
    }
}

/// The machine's half of a handshake, ready to send as `channel.accept`.
pub struct MachineAccept {
    /// The machine's ephemeral public key.
    pub machine_key: Jwk,
    /// The machine nonce.
    pub machine_nonce: String,
    /// The Ed25519 signature over the transcript, lowercase hex.
    pub machine_signature: String,
    /// The machine's end of the channel.
    pub cipher: ChannelCipher,
}

impl std::fmt::Debug for MachineAccept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MachineAccept").finish_non_exhaustive()
    }
}

/// The values the machine already verified from `channel.open` and its ticket.
#[derive(Debug, Clone)]
pub struct AcceptInput<'a> {
    /// ULID channel id.
    pub channel_id: &'a str,
    /// The client's ephemeral key (its thumbprint already matched the ticket).
    pub client_key: &'a Jwk,
    /// The client nonce as sent.
    pub client_nonce: &'a str,
    /// Machine id.
    pub machine_id: &'a str,
    /// Account id.
    pub account_id: &'a str,
    /// The ticket `jti`.
    pub ticket_jti: &'a str,
}

/// Runs steps 4 and 6 of the handshake for the machine: ephemeral key and nonce, transcript,
/// Ed25519 signature, ECDH, HKDF. Fails on an invalid client key or an all-zero ECDH result.
pub fn machine_accept(input: &AcceptInput<'_>, identity: &super::key::RelayKey) -> Result<MachineAccept, RelayError> {
    let ephemeral = EphemeralKey::generate();
    machine_accept_with(input, identity, &ephemeral, random_nonce())
}

/// [`machine_accept`] with a caller-supplied ephemeral key and nonce (the test vector).
pub fn machine_accept_with(
    input: &AcceptInput<'_>,
    identity: &super::key::RelayKey,
    ephemeral: &EphemeralKey,
    machine_nonce: String,
) -> Result<MachineAccept, RelayError> {
    let machine_key = ephemeral.jwk();
    let transcript = transcript(&TranscriptInput {
        channel_id: input.channel_id,
        client_key: input.client_key,
        machine_key: &machine_key,
        client_nonce: input.client_nonce,
        machine_nonce: &machine_nonce,
        machine_id: input.machine_id,
        account_id: input.account_id,
        ticket_jti: input.ticket_jti,
    });
    let shared = ephemeral.agree(input.client_key)?;
    let keys = ChannelKeys::derive(&shared, &transcript)?;
    Ok(MachineAccept {
        machine_signature: identity.sign_hex(&transcript),
        cipher: ChannelCipher::new(&keys, input.channel_id, Role::Machine)?,
        machine_key,
        machine_nonce,
    })
}

/// The client's side of step 5 and 6: verify the machine's signature with the ticket's `mkey`
/// **before** deriving any key, then derive the client end of the channel. The client lives in
/// slice 6; this function exists so the machine's handshake can be proven against it.
#[allow(clippy::too_many_arguments)]
pub fn client_finish(
    client: &EphemeralKey,
    input: &AcceptInput<'_>,
    machine_key: &Jwk,
    machine_nonce: &str,
    machine_signature: &str,
    ticket_mkey_hex: &str,
) -> Result<ChannelCipher, RelayError> {
    let transcript = transcript(&TranscriptInput {
        channel_id: input.channel_id,
        client_key: input.client_key,
        machine_key,
        client_nonce: input.client_nonce,
        machine_nonce,
        machine_id: input.machine_id,
        account_id: input.account_id,
        ticket_jti: input.ticket_jti,
    });
    super::key::verify_hex(ticket_mkey_hex, &transcript, machine_signature)
        .map_err(|_| RelayError::Crypto("the machine signature does not verify".to_owned()))?;
    let shared = client.agree(machine_key)?;
    let keys = ChannelKeys::derive(&shared, &transcript)?;
    ChannelCipher::new(&keys, input.channel_id, Role::Client)
}
