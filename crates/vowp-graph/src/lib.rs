//! Job graph, ownership locks and deterministic scheduling for VOWP.
//!
//! The crate is pure: no I/O, no clock reads, no threads. `vowp-core` feeds it rows from the
//! store and applies the effects it returns. Spec: `docs/design/crates/vowp-graph.md`.
//!
//! This job (n04a) builds [`Graph`] and the [`transition`] state machine; the ownership,
//! dispatch, rendering, YAML and guard modules are placeholders owned by n04b.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod dispatch;
pub mod error;
pub mod graph;
pub mod guards;
pub mod machine;
pub mod owns;
pub mod render;
pub mod yaml;

pub use error::GraphError;
pub use graph::Graph;
pub use machine::{
    Effect, JobEvent, MAX_CONTINUES, MAX_HANG_RESTARTS, Transition, TransitionBounds, attempts_left, is_exhausted,
    transition, transition_with,
};

// Glob re-exports so the placeholder modules of n04b gain a crate-root path without lib.rs
// being touched again.
#[allow(unused_imports)]
pub use dispatch::*;
#[allow(unused_imports)]
pub use guards::*;
#[allow(unused_imports)]
pub use owns::*;
#[allow(unused_imports)]
pub use render::*;
#[allow(unused_imports)]
pub use yaml::*;
