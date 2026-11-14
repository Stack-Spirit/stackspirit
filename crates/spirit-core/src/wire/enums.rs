//! Tolerant enums for every VOWP document.
//!
//! Known values are exhaustive. A value this build does not know becomes
//! [`Unknown`](JobState::Unknown) and keeps its raw wire string, so the UI can show an
//! explicit **Unknown** chip instead of guessing a known state.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! tolerant_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($variant:ident => $wire:literal),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        $vis enum $name {
            $(
                #[doc = concat!("The `", $wire, "` wire value.")]
                $variant,
            )*
            /// A value this build does not know; the raw string is preserved.
            Unknown(String),
        }

        impl $name {
            /// Parses a wire string, mapping unknown values to [`Self::Unknown`].
            #[must_use]
            pub fn parse(input: &str) -> Self {
                match input {
                    $($wire => Self::$variant,)*
                    other => Self::Unknown(other.to_owned()),
                }
            }

            /// Returns the exact wire string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire,)*
                    Self::Unknown(raw) => raw.as_str(),
                }
            }

            /// Returns true when the value is known to this build.
            #[must_use]
            pub fn is_known(&self) -> bool {
                !matches!(self, Self::Unknown(_))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Ok(Self::parse(&raw))
            }
        }
    };
}

tolerant_enum!(
    /// Where VOWP got the answer.
    pub enum FleetSource {
        Local => "local",
        Remote => "remote",
        Cache => "cache",
    }
);

tolerant_enum!(
    /// What a job does in the plan.
    pub enum JobKind {
        Plan => "plan",
        Spec => "spec",
        Build => "build",
        Test => "test",
        Verify => "verify",
        Fix => "fix",
        Merge => "merge",
        Docs => "docs",
        Research => "research",
        Promote => "promote",
        Release => "release",
        Decision => "decision",
    }
);

tolerant_enum!(
    /// The lifecycle state of a job.
    pub enum JobState {
        Pending => "pending",
        Ready => "ready",
        Running => "running",
        Stalled => "stalled",
        Gating => "gating",
        Reviewing => "reviewing",
        Fixing => "fixing",
        Merging => "merging",
        Done => "done",
        Failed => "failed",
        Parked => "parked",
        Replanned => "replanned",
        Cancelled => "cancelled",
    }
);

impl JobState {
    /// Returns true for states after which no further work happens.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Replanned | Self::Cancelled)
    }

    /// Returns true when the job holds its ownership paths and worktree.
    #[must_use]
    pub fn holds_lock(&self) -> bool {
        matches!(
            self,
            Self::Running
                | Self::Stalled
                | Self::Gating
                | Self::Reviewing
                | Self::Fixing
                | Self::Merging
        )
    }
}

tolerant_enum!(
    /// Registry state of one project.
    pub enum ProjectState {
        Active => "active",
        Paused => "paused",
        Archived => "archived",
    }
);

tolerant_enum!(
    /// Lifecycle state of a session.
    pub enum SessionState {
        Idle => "idle",
        Running => "running",
        Blocked => "blocked",
        Switching => "switching",
        Closed => "closed",
    }
);

tolerant_enum!(
    /// Who a session serves.
    pub enum SessionRole {
        Planner => "planner",
        Keeper => "keeper",
        Worker => "worker",
        Verifier => "verifier",
        User => "user",
        Orchestrator => "orchestrator",
    }
);

tolerant_enum!(
    /// Kind of client holding a surface.
    pub enum SurfaceKind {
        Chat => "chat",
        Opencode => "opencode",
        ClaudeCode => "claude_code",
        Mcp => "mcp",
        Mirror => "mirror",
        Api => "api",
        Agent => "agent",
    }
);

tolerant_enum!(
    /// Application an orchestrator runs in.
    pub enum OrchestratorApp {
        Opencode => "opencode",
        ClaudeCode => "claude_code",
        Chat => "chat",
    }
);

tolerant_enum!(
    /// Why an inbox item exists.
    pub enum InboxKind {
        Question => "question",
        Approval => "approval",
        Decision => "decision",
        Notice => "notice",
    }
);

tolerant_enum!(
    /// Lifecycle of an inbox item.
    pub enum InboxState {
        Open => "open",
        Answered => "answered",
        Dismissed => "dismissed",
    }
);

tolerant_enum!(
    /// Kind of provider limit row.
    pub enum LimitRowKind {
        Quota => "quota",
        Credits => "credits",
        Rate => "rate",
        Context => "context",
        Auth => "auth",
        Unavailable => "unavailable",
    }
);

tolerant_enum!(
    /// How the worker target is chosen.
    pub enum TargetMode {
        Auto => "auto",
        Fixed => "fixed",
    }
);

tolerant_enum!(
    /// Why a run is waiting for capacity.
    pub enum WaitCode {
        Ram => "ram",
        Cpu => "cpu",
        Disk => "disk",
        Provider => "provider",
        Brake => "brake",
        Target => "target",
    }
);

tolerant_enum!(
    /// Why the node brake tripped.
    pub enum BrakeCode {
        MemoryPressure => "memory_pressure",
        SwapGrowth => "swap_growth",
        DiskLow => "disk_low",
        Provider => "provider",
    }
);

tolerant_enum!(
    /// Where a host sample came from.
    pub enum SampleSource {
        Proc => "proc",
        Fallback => "fallback",
    }
);

tolerant_enum!(
    /// Inference about how warm a build target was.
    pub enum BuildMode {
        Cold => "cold",
        Warm => "warm",
        Incremental => "incremental",
        Unspecified => "unknown",
    }
);

tolerant_enum!(
    /// Where a usage estimate came from.
    pub enum EstimateSource {
        Measured => "measured",
        Pooled => "pooled",
        Default => "default",
    }
);

tolerant_enum!(
    /// A harness that runs a model.
    pub enum HarnessKind {
        ClaudeCode => "claude-code",
        Opencode => "opencode",
        Cli => "cli",
        Acp => "acp",
        Vowp => "vowp",
    }
);

tolerant_enum!(
    /// Executor role of a running or queued job.
    pub enum Role {
        Planner => "planner",
        Keeper => "keeper",
        Worker => "worker",
        Verifier => "verifier",
    }
);

tolerant_enum!(
    /// Stable VOWP `--json` error class.
    pub enum ErrorKind {
        InvalidInput => "invalid_input",
        NotFound => "not_found",
        Conflict => "conflict",
        Io => "io",
        Db => "db",
        Parse => "parse",
        Config => "config",
        AdapterUnavailable => "adapter_unavailable",
        AdapterFailed => "adapter_failed",
        Limit => "limit",
        Auth => "auth",
        Timeout => "timeout",
        Guard => "guard",
        Refused => "refused",
        Remote => "remote",
        Internal => "internal",
    }
);

impl ErrorKind {
    /// Maps a VOWP error kind to a StackSpirit error code.
    #[must_use]
    pub fn to_spirit_code(&self) -> crate::error::SpiritErrorCode {
        use crate::error::SpiritErrorCode;
        match self {
            Self::InvalidInput => SpiritErrorCode::InvalidInput,
            Self::NotFound => SpiritErrorCode::NotFound,
            Self::Conflict => SpiritErrorCode::Conflict,
            Self::Io | Self::Db | Self::Parse | Self::Config => SpiritErrorCode::Parse,
            Self::AdapterUnavailable | Self::AdapterFailed => SpiritErrorCode::Engine,
            Self::Limit => SpiritErrorCode::Limit,
            Self::Auth => SpiritErrorCode::Auth,
            Self::Timeout => SpiritErrorCode::RemoteTimeout,
            Self::Guard | Self::Refused => SpiritErrorCode::Refused,
            Self::Remote => SpiritErrorCode::Remote,
            Self::Internal | Self::Unknown(_) => SpiritErrorCode::VowpCommand,
        }
    }
}
