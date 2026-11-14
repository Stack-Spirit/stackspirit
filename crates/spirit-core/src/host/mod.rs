//! Host profiles, persistence, pins and probing.

pub mod probe;
pub mod profile;
pub mod store;

pub use probe::{
    HostProbe, HostProbeInput, HostProbeStage, classify_ssh_failure, parse_version_line, probe_host,
};
pub use profile::{
    HostDraft, HostId, HostKeyPin, HostProfile, HostProfilePatch, NewHostProfile,
    verify_pin_against_scan,
};
pub use store::{HOST_STORE_SCHEMA_VERSION, HostStore, InMemoryHostStore, JsonHostStore};

// `SshTarget` lives with the SSH transport; re-export it here so the host API is
// reachable from one module, as documented in `docs/PLAN.md` §5.2.
pub use crate::ssh::SshTarget;
