//! Home-side v0.4 local-node agent support.
//!
//! Spec: `docs/design/v0.4/local-orchestrator.md` §4.1, §4.2, §4.6, §8.2–§8.3 and §12.1.
//! Phase 0 implements enrollment, node-key authentication, heartbeat presence/capacity and the
//! stdio gateway. Phase 1 adds surface pairing: a one-time code becomes a scoped, node-bound
//! surface token. Leases, local chat, offload and job dispatch are later phases.

pub mod auth;
pub mod code;
pub mod enroll;
pub mod gateway;
pub mod pair;

pub use code::{ENROLLMENT_CODE_TTL, NewEnrollmentCode, hash_code, new_enrollment_code};
pub use enroll::{EnrollRequest, Enrolled, PendingEnrollment, create_pending, redeem, revoke};
pub use gateway::{GatewayOptions, GatewayOutcome, HANDSHAKE_TIMEOUT, HEARTBEAT_SECS, MAX_FRAME_BYTES, serve};
pub use pair::{
    IssuedSurface, NewSurfaceCode, PendingSurface, SURFACE_CODE_TTL, SURFACE_TOKEN_TTL, VerifiedSurface,
    create_pending_surface, issue_surface_token, new_surface_code, revoke_surface, revoke_surface_token,
    verify_surface_token,
};
