//! Machine authentication and ticket verification.
//!
//! Spec: `docs/design/relay-and-packaging.md` §6.2–§6.4.
//!
//! * The machine proves it holds the relay key by signing the canonical challenge string; the
//!   credential alone (a bearer token) cannot sign it.
//! * Credentials and tickets are ES256 JWTs verified against the API JWKS. There is no
//!   "trust any signing key" fallback: an unknown `kid`, a different `alg` or a bad signature is
//!   rejected.
//! * A ticket `jti` is single-use inside its lifetime; [`ReplayCache`] remembers it for 600 s.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::PublicKey;
use p256::ecdsa::signature::{Signer as _, Verifier as _};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use vowp_protocol::{
    CREDENTIAL_DOMAIN, ClientKind, Jwk, MAX_TICKET_TTL_SECS, RELAY_AUTH_DOMAIN, Scope, TICKET_REPLAY_SECS, codes,
};

use super::RelayError;
use super::crypto::{jwk_from_public, jwk_thumbprint, public_from_jwk};
use super::key::{RelayKey, verify_hex};

/// Seconds of clock skew tolerated on `iat`.
pub const CLOCK_SKEW_SECS: i64 = 60;
/// The longest the machine trusts a cached JWKS, in seconds (24 hours).
pub const JWKS_MAX_AGE_SECS: i64 = 86_400;

/// The exact challenge string: eight fields joined by `\n`, no trailing newline.
pub fn machine_auth_canonical(
    machine_id: &str,
    account_id: &str,
    key_id: &str,
    relay_id: &str,
    session_id: &str,
    nonce: &str,
    server_time: &str,
) -> String {
    [
        RELAY_AUTH_DOMAIN,
        machine_id,
        account_id,
        key_id,
        relay_id,
        session_id,
        nonce,
        server_time,
    ]
    .join("\n")
}

/// The credential-refresh string: `STACKSPIRIT-CREDENTIAL-V1\n<machine_id>\n<key_id>\n<signed_at>`.
pub fn credential_refresh_canonical(machine_id: &str, key_id: &str, signed_at: &str) -> String {
    [CREDENTIAL_DOMAIN, machine_id, key_id, signed_at].join("\n")
}

/// Signs the challenge as the machine.
pub fn sign_machine_auth(key: &RelayKey, canonical: &str) -> String {
    key.sign_hex(canonical.as_bytes())
}

/// Verifies the machine's challenge signature with `mkey` (the credential's public key).
pub fn verify_machine_auth(mkey_hex: &str, canonical: &str, signature_hex: &str) -> Result<(), RelayError> {
    verify_hex(mkey_hex, canonical.as_bytes(), signature_hex)
}

/// Single-use challenge nonces for one relay session (the relay side; used by the fake relay).
#[derive(Debug, Default)]
pub struct NonceSet {
    seen: HashSet<String>,
}

impl NonceSet {
    /// Records `nonce`; a repeat is an authentication failure.
    pub fn consume(&mut self, nonce: &str) -> Result<(), RelayError> {
        if self.seen.insert(nonce.to_owned()) {
            Ok(())
        } else {
            Err(RelayError::Auth("replayed challenge nonce".to_owned()))
        }
    }
}

/// Remembers ticket `jti` values for 600 seconds so a ticket works once.
#[derive(Debug, Default)]
pub struct ReplayCache {
    seen: HashMap<String, i64>,
}

impl ReplayCache {
    /// Returns true when `jti` is new (and records it); false for a replay.
    pub fn first_use(&mut self, jti: &str, now_unix: i64) -> bool {
        self.seen.retain(|_, until| *until > now_unix);
        if self.seen.contains_key(jti) {
            return false;
        }
        self.seen.insert(jti.to_owned(), now_unix + TICKET_REPLAY_SECS as i64);
        true
    }
}

/// A parsed, not yet verified, compact JWT.
#[derive(Debug, Clone)]
pub struct Jwt {
    signing_input: String,
    header: Value,
    payload: Value,
    signature: Vec<u8>,
}

impl Jwt {
    /// Splits and decodes `header.payload.signature`; verifies nothing.
    pub fn parse(token: &str) -> Result<Self, RelayError> {
        let bad = || RelayError::Auth("malformed JWT".to_owned());
        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(bad());
        };
        let decode = |part: &str| URL_SAFE_NO_PAD.decode(part).map_err(|_| bad());
        Ok(Self {
            signing_input: format!("{header}.{payload}"),
            header: serde_json::from_slice(&decode(header)?).map_err(|_| bad())?,
            payload: serde_json::from_slice(&decode(payload)?).map_err(|_| bad())?,
            signature: decode(signature)?,
        })
    }

    /// The decoded claims. They are untrusted until [`Jwt::verify`] succeeds.
    pub fn claims<T: serde::de::DeserializeOwned>(&self) -> Result<T, RelayError> {
        serde_json::from_value(self.payload.clone()).map_err(|_| RelayError::Auth("unexpected JWT claims".to_owned()))
    }

    /// Verifies the ES256 signature against `jwks`. The header must say `ES256` and name a `kid`
    /// present in the set.
    pub fn verify(&self, jwks: &Jwks) -> Result<(), RelayError> {
        if self.header.get("alg").and_then(Value::as_str) != Some("ES256") {
            return Err(RelayError::Auth("the JWT algorithm is not ES256".to_owned()));
        }
        let kid = self
            .kid()
            .ok_or_else(|| RelayError::Auth("the JWT has no kid".to_owned()))?;
        let key = jwks
            .keys
            .iter()
            .find(|key| key.kid == kid)
            .ok_or_else(|| RelayError::Auth("the JWT kid is not in the JWKS".to_owned()))?;
        let verifying = key.verifying_key()?;
        let signature = Signature::from_slice(&self.signature)
            .map_err(|_| RelayError::Auth("the JWT signature is malformed".to_owned()))?;
        verifying
            .verify(self.signing_input.as_bytes(), &signature)
            .map_err(|_| RelayError::Auth("the JWT signature is invalid".to_owned()))
    }

    /// The header `kid`, if any.
    pub fn kid(&self) -> Option<&str> {
        self.header.get("kid").and_then(Value::as_str)
    }
}

/// Signs `claims` as an ES256 JWT. The machine never calls this; the fake API/relay in tests do.
pub fn sign_es256(key: &SigningKey, kid: &str, claims: &Value) -> String {
    let header = serde_json::json!({"alg": "ES256", "typ": "JWT", "kid": kid});
    let header = URL_SAFE_NO_PAD.encode(header.to_string());
    let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
    let input = format!("{header}.{payload}");
    let signature: Signature = key.sign(input.as_bytes());
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

/// One ES256 signing key in a JWKS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwksKey {
    /// Key id.
    pub kid: String,
    /// Key type, `EC`.
    pub kty: String,
    /// Curve, `P-256`.
    pub crv: String,
    /// Base64url x.
    pub x: String,
    /// Base64url y.
    pub y: String,
}

impl JwksKey {
    /// Builds a JWKS entry for an ECDSA verifying key.
    pub fn from_verifying(kid: &str, key: &VerifyingKey) -> Self {
        let jwk = jwk_from_public(&PublicKey::from(key));
        Self {
            kid: kid.to_owned(),
            kty: jwk.kty,
            crv: jwk.crv,
            x: jwk.x,
            y: jwk.y,
        }
    }

    fn verifying_key(&self) -> Result<VerifyingKey, RelayError> {
        let jwk = Jwk {
            kty: self.kty.clone(),
            crv: self.crv.clone(),
            x: self.x.clone(),
            y: self.y.clone(),
        };
        Ok(VerifyingKey::from(public_from_jwk(&jwk)?))
    }
}

/// The API's published signing keys (`/.well-known/jwks.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwks {
    /// Current and, during a rotation overlap, previous keys.
    pub keys: Vec<JwksKey>,
}

/// A JWKS with the time it was fetched, as cached in `<home>/relay-jwks.json` (public data).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedJwks {
    /// Unix seconds when the set was fetched.
    pub fetched_at: i64,
    /// The keys.
    pub jwks: Jwks,
}

impl CachedJwks {
    /// Loads the cache file when it exists and is at most 24 hours old.
    pub fn load_fresh(path: &Path, now_unix: i64) -> Option<Self> {
        let cached: Self = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        (now_unix - cached.fetched_at <= JWKS_MAX_AGE_SECS && cached.fetched_at <= now_unix + CLOCK_SKEW_SECS)
            .then_some(cached)
    }

    /// Writes the cache (0644: it holds no secret).
    pub fn save(&self, path: &Path) -> Result<(), RelayError> {
        let text = serde_json::to_string(self).map_err(|_| RelayError::Config("jwks cache".to_owned()))?;
        super::key::write_atomic(path, text.as_bytes(), 0o644)
    }
}

/// The machine credential claims (§6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialClaims {
    /// Issuer, the API URL.
    pub iss: String,
    /// Audience, the relay host.
    pub aud: String,
    /// `machine:<machine_id>`.
    pub sub: String,
    /// Account id.
    pub acct: String,
    /// `ed25519:<hex>`.
    pub key_id: String,
    /// Public key hex.
    pub mkey: String,
    /// Scopes, `machine.connect`.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Issued at.
    pub iat: i64,
    /// Expiry.
    pub exp: i64,
    /// Token id.
    pub jti: String,
}

impl CredentialClaims {
    /// Decodes the claims without verifying the signature. The machine uses this only to read
    /// `exp` and to check that the credential names its own key; the relay verifies the signature.
    pub fn decode(token: &str) -> Result<Self, RelayError> {
        Jwt::parse(token)?.claims()
    }

    /// True when the credential is expired or expires within `margin_secs`.
    pub fn expires_within(&self, now_unix: i64, margin_secs: i64) -> bool {
        self.exp - now_unix <= margin_secs
    }
}

/// The relay ticket claims (§6.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketClaims {
    /// Issuer.
    pub iss: String,
    /// Audience.
    pub aud: String,
    /// `client:<id>`.
    pub sub: String,
    /// Account id.
    pub acct: String,
    /// Machine id.
    pub machine: String,
    /// Client kind.
    pub kind: ClientKind,
    /// Granted scopes.
    pub scopes: Vec<Scope>,
    /// Confirmation: the client key thumbprint.
    pub cnf: Cnf,
    /// Machine public key hex.
    pub mkey: String,
    /// Machine key id.
    pub mkey_id: String,
    /// Issued at.
    pub iat: i64,
    /// Expiry.
    pub exp: i64,
    /// Token id.
    pub jti: String,
}

/// The `cnf` claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cnf {
    /// RFC 7638 SHA-256 thumbprint of the client's ephemeral key.
    pub jkt: String,
}

/// A ticket the machine refused, with the stable protocol code to answer with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct TicketError {
    /// One of the [`vowp_protocol::codes`] strings.
    pub code: &'static str,
    /// A path-free message that never contains the ticket.
    pub message: String,
}

impl TicketError {
    fn new(code: &'static str, message: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
        }
    }
}

/// What the machine knows when it re-verifies a ticket.
#[derive(Debug)]
pub struct TicketContext<'a> {
    /// Verified signing keys.
    pub jwks: &'a Jwks,
    /// Expected issuer (the API URL, no trailing slash).
    pub issuer: &'a str,
    /// Expected audience (the relay host).
    pub audience: &'a str,
    /// This machine's id.
    pub machine_id: &'a str,
    /// This machine's account id.
    pub account_id: &'a str,
    /// This machine's `ed25519:<hex>` key id.
    pub key_id: &'a str,
    /// This machine's public key hex.
    pub mkey: &'a str,
    /// The client key from `channel.open`.
    pub client_key: &'a Jwk,
    /// The current time, Unix seconds.
    pub now_unix: i64,
}

/// Verifies a ticket the way the machine must (§6.5 step 3) and consumes its `jti`.
///
/// Rejected, with the matching code: a bad signature or wrong issuer/audience
/// (`ticket_invalid`), an expired ticket (`ticket_expired`), an over-long lifetime, another
/// machine, account or key, a `cnf.jkt` that does not match `client_key` (key substitution; all
/// `ticket_invalid`) and a repeated `jti` (`ticket_replayed`). The `jti` is only consumed once
/// every other check has passed.
pub fn verify_ticket(
    token: &str,
    ctx: &TicketContext<'_>,
    replay: &mut ReplayCache,
) -> Result<TicketClaims, TicketError> {
    let invalid = |message: &str| TicketError::new(codes::TICKET_INVALID, message);
    let jwt = Jwt::parse(token).map_err(|_| invalid("malformed ticket"))?;
    jwt.verify(ctx.jwks).map_err(|_| invalid("ticket signature"))?;
    let claims: TicketClaims = jwt.claims().map_err(|_| invalid("ticket claims"))?;
    if claims.iss != ctx.issuer || claims.aud != ctx.audience {
        return Err(invalid("ticket issuer or audience"));
    }
    if claims.exp <= ctx.now_unix {
        return Err(TicketError::new(codes::TICKET_EXPIRED, "ticket expired"));
    }
    if claims.exp - claims.iat > MAX_TICKET_TTL_SECS as i64 || claims.iat > ctx.now_unix + CLOCK_SKEW_SECS {
        return Err(invalid("ticket lifetime"));
    }
    if claims.machine != ctx.machine_id
        || claims.acct != ctx.account_id
        || claims.mkey_id != ctx.key_id
        || claims.mkey != ctx.mkey
    {
        return Err(invalid("ticket is for another machine or key"));
    }
    if claims.cnf.jkt != jwk_thumbprint(ctx.client_key) {
        return Err(invalid("client key does not match the ticket"));
    }
    if !replay.first_use(&claims.jti, ctx.now_unix) {
        return Err(TicketError::new(codes::TICKET_REPLAYED, "ticket already used"));
    }
    Ok(claims)
}
