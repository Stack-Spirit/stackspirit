//! Connection state machine, poll scheduling, latency and event fan-out.

pub mod manager;
pub mod poll;
pub mod state;

pub use manager::{ConnectionManager, SpiritEvent};
pub use poll::{CancellationToken, PollPhase, PollPolicy, PollScheduler, StaleAssessment};
pub use state::{
    ConnectStage, ConnectionSnapshot, ConnectionState, LatencyStats, Revision, accepts_revision,
};
