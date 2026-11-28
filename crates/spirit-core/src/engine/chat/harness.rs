//! Harness chat gates and the transport-less dry run for Phase 1a.
//!
//! This module is deliberately inert: it resolves the persisted default, reads one
//! registry manifest, enforces the surface/capability rules and returns a typed
//! [`HarnessChatGate`]. It never starts a harness, spawns a child, opens a socket, calls
//! the engine or writes a file. OpenCode is the only `ready` value in this phase; every
//! other orchestrator harness is `harness_not_wired` even though `harnesses::acp` exists.

use serde::{Deserialize, Serialize};

use crate::engine::chat::selection::{
    ChatHarnessCapture, ChatPreferencesSource, ChatSettingsError,
};
use crate::harnesses::{
    DEFAULT_HARNESS_ID, HarnessManifest, HarnessProtocol, HarnessRegistry, HarnessSurface,
};

/// The safe outcome of resolving one chat selection.
///
/// Every variant and field is safe to serialize: a registry id, a protocol, a fixed
/// reason token or a typed settings problem. It carries no path, URL, port, token,
/// native session id or user-facing prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HarnessChatGate {
    /// The selection resolves to the existing bundled OpenCode provider.
    Ready {
        /// Selected registry id.
        harness_id: String,
        /// Selected manifest protocol.
        protocol: HarnessProtocol,
    },
    /// A valid orchestrator harness is selected, but no embedded-chat transport is wired.
    HarnessNotWired {
        /// Selected registry id.
        harness_id: String,
        /// Selected manifest protocol.
        protocol: HarnessProtocol,
    },
    /// No manifest is registered for the selected id.
    HarnessMissing {
        /// Safe form of the selected registry id.
        harness_id: String,
    },
    /// The manifest is workers-only and must not back the embedded chat.
    HarnessWorkerOnly {
        /// Selected registry id.
        harness_id: String,
    },
    /// The manifest claims embedded chat with a protocol the chat cannot use.
    HarnessProtocolUnsupported {
        /// Selected registry id.
        harness_id: String,
        /// Rejected manifest protocol.
        protocol: HarnessProtocol,
    },
    /// The settings file is corrupt or has an unsupported version.
    ChatSettingsUnreadable {
        /// Typed, safe settings problem.
        problem: ChatSettingsError,
    },
}

impl HarnessChatGate {
    /// Returns the stable safe gate token.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "ready",
            Self::HarnessNotWired { .. } => "harness_not_wired",
            Self::HarnessMissing { .. } => "harness_missing",
            Self::HarnessWorkerOnly { .. } => "harness_worker_only",
            Self::HarnessProtocolUnsupported { .. } => "harness_protocol_unsupported",
            Self::ChatSettingsUnreadable { .. } => "chat_settings_unreadable",
        }
    }

    /// True only for the existing OpenCode provider.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// Returns the resolved harness id, when one was named.
    #[must_use]
    pub fn harness_id(&self) -> Option<&str> {
        match self {
            Self::Ready { harness_id, .. }
            | Self::HarnessNotWired { harness_id, .. }
            | Self::HarnessMissing { harness_id }
            | Self::HarnessWorkerOnly { harness_id }
            | Self::HarnessProtocolUnsupported { harness_id, .. } => Some(harness_id),
            Self::ChatSettingsUnreadable { .. } => None,
        }
    }

    /// Returns the resolved protocol, when one was named.
    #[must_use]
    pub fn protocol(&self) -> Option<HarnessProtocol> {
        match self {
            Self::Ready { protocol, .. }
            | Self::HarnessNotWired { protocol, .. }
            | Self::HarnessProtocolUnsupported { protocol, .. } => Some(*protocol),
            Self::HarnessMissing { .. }
            | Self::HarnessWorkerOnly { .. }
            | Self::ChatSettingsUnreadable { .. } => None,
        }
    }

    /// Returns the in-memory capture a session may hold for a resolvable harness.
    ///
    /// A `harness_not_wired` selection still captures the id and protocol so a session
    /// can explain what the owner chose; missing, worker-only, unsupported and settings
    /// gates do not.
    #[must_use]
    pub fn capture(&self) -> Option<ChatHarnessCapture> {
        match self {
            Self::Ready {
                harness_id,
                protocol,
            }
            | Self::HarnessNotWired {
                harness_id,
                protocol,
            } => Some(ChatHarnessCapture::new(harness_id.clone(), *protocol)),
            Self::HarnessMissing { .. }
            | Self::HarnessWorkerOnly { .. }
            | Self::HarnessProtocolUnsupported { .. }
            | Self::ChatSettingsUnreadable { .. } => None,
        }
    }
}

/// Classifies one already-resolved manifest.
///
/// This is a pure function: no I/O, no child, no spawn, no socket and no engine call. It
/// still rejects a `cli-pty` manifest that claims the embedded chat, even though
/// [`HarnessManifest::validate`] normally refuses that combination.
#[must_use]
pub fn gate_for_manifest(manifest: &HarnessManifest) -> HarnessChatGate {
    let harness_id = manifest.id.clone();
    if manifest.surface != HarnessSurface::OrchestratorAndWorkers
        || !manifest.supports_embedded_chat
    {
        return HarnessChatGate::HarnessWorkerOnly { harness_id };
    }
    if manifest.protocol == HarnessProtocol::CliPty {
        return HarnessChatGate::HarnessProtocolUnsupported {
            harness_id,
            protocol: manifest.protocol,
        };
    }
    if manifest.id == DEFAULT_HARNESS_ID {
        if manifest.protocol == HarnessProtocol::OpencodeHttp {
            return HarnessChatGate::Ready {
                harness_id,
                protocol: manifest.protocol,
            };
        }
        return HarnessChatGate::HarnessProtocolUnsupported {
            harness_id,
            protocol: manifest.protocol,
        };
    }
    HarnessChatGate::HarnessNotWired {
        harness_id,
        protocol: manifest.protocol,
    }
}

/// Resolves the default selection and returns the typed gate without touching a
/// transport.
///
/// A missing file or absent/null id is the documented OpenCode default; an unreadable or
/// future settings file is `chat_settings_unreadable`; an unknown id is
/// `harness_missing` and is never replaced with OpenCode. The registry read is the only
/// optional file access, and it is limited to one custom manifest lookup.
#[must_use]
pub fn preflight(
    registry: &HarnessRegistry,
    preferences: &dyn ChatPreferencesSource,
) -> HarnessChatGate {
    preflight_with(registry, preferences, &|_, _| false)
}

/// Like [`preflight`], with an adapter-readiness predicate.
///
/// `adapter_ready(harness_id, protocol)` answers whether a registered chat adapter serves
/// exactly that id and protocol. Only an otherwise `harness_not_wired` ACP manifest can
/// become `ready`; workers-only, missing, unsupported and settings gates are unchanged.
#[must_use]
pub fn preflight_with(
    registry: &HarnessRegistry,
    preferences: &dyn ChatPreferencesSource,
    adapter_ready: &dyn Fn(&str, HarnessProtocol) -> bool,
) -> HarnessChatGate {
    let preferences = match preferences.resolve() {
        Ok(preferences) => preferences,
        Err(problem) => return HarnessChatGate::ChatSettingsUnreadable { problem },
    };
    let selected = preferences.selected_harness_id();
    let harness_id = safe_harness_id(selected);
    match registry.get(selected) {
        Ok(Some(manifest)) => match gate_for_manifest(&manifest) {
            HarnessChatGate::HarnessNotWired {
                harness_id,
                protocol,
            } if protocol == HarnessProtocol::Acp && adapter_ready(&harness_id, protocol) => {
                HarnessChatGate::Ready {
                    harness_id,
                    protocol,
                }
            }
            gate => gate,
        },
        Ok(None) | Err(_) => HarnessChatGate::HarnessMissing { harness_id },
    }
}

/// The Phase 1a transport-less dry run.
///
/// It returns exactly [`preflight`] and is a pure read: no child, no spawn, no socket,
/// no engine call and no write. The inputs are the supplied registry (which may read one
/// custom manifest) and the supplied preferences source.
#[must_use]
pub fn dry_run(
    registry: &HarnessRegistry,
    preferences: &dyn ChatPreferencesSource,
) -> HarnessChatGate {
    preflight(registry, preferences)
}

fn safe_harness_id(id: &str) -> String {
    if HarnessManifest::validate_id(id).is_ok() {
        id.to_owned()
    } else {
        "invalid".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harnesses::builtin_harnesses;

    fn builtin(id: &str) -> HarnessManifest {
        builtin_harnesses()
            .into_iter()
            .find(|manifest| manifest.id == id)
            .unwrap_or_else(|| panic!("missing builtin {id}"))
    }

    #[test]
    fn cli_pty_claim_is_rejected_even_when_the_manifest_claims_embedded_chat() {
        let mut manifest = builtin("opencode");
        manifest.protocol = HarnessProtocol::CliPty;

        assert_eq!(
            gate_for_manifest(&manifest),
            HarnessChatGate::HarnessProtocolUnsupported {
                harness_id: "opencode".to_owned(),
                protocol: HarnessProtocol::CliPty,
            }
        );
    }

    #[test]
    fn surface_and_legacy_flag_must_both_allow_embedded_chat() {
        let mut manifest = builtin("goose");
        manifest.supports_embedded_chat = false;

        assert_eq!(
            gate_for_manifest(&manifest),
            HarnessChatGate::HarnessWorkerOnly {
                harness_id: "goose".to_owned(),
            }
        );

        let mut manifest = builtin("goose");
        manifest.surface = HarnessSurface::WorkersOnlyTerminal;

        assert_eq!(
            gate_for_manifest(&manifest),
            HarnessChatGate::HarnessWorkerOnly {
                harness_id: "goose".to_owned(),
            }
        );
    }

    #[test]
    fn only_the_opencode_id_is_ready() {
        let mut manifest = builtin("opencode");
        assert!(gate_for_manifest(&manifest).is_ready());

        manifest.id = "my-opencode".to_owned();
        assert_eq!(
            gate_for_manifest(&manifest),
            HarnessChatGate::HarnessNotWired {
                harness_id: "my-opencode".to_owned(),
                protocol: HarnessProtocol::OpencodeHttp,
            }
        );
    }
}
