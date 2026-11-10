//! The node snapshot: build, write, read and the four node hooks. D1 fills this module.
//!
//! C0's writer is a no-op so v0.1 leaves no new file behind; the read returns not-found.

use vowp_protocol::{CapacitySnapshot, SNAPSHOT_FORMAT, SNAPSHOT_VERSION, Snapshot, SupervisorSnapshot, Timestamp};
use vowp_store::{NodePaths, Store};

use crate::CoreError;
use crate::deps::Deps;

/// Builds the derived snapshot from the node's state.
pub fn build_snapshot(deps: &Deps, _store: &Store, now: Timestamp) -> Result<Snapshot, CoreError> {
    Ok(Snapshot {
        schema: SNAPSHOT_FORMAT.to_owned(),
        version: SNAPSHOT_VERSION,
        sequence: 0,
        generated_at: now,
        home: deps.node.clone(),
        supervisor: SupervisorSnapshot::default(),
        projects: Vec::new(),
        capacity: CapacitySnapshot::default(),
        limits: Vec::new(),
        inbox: Vec::new(),
        sessions: Vec::new(),
        orchestrators: Vec::new(),
    })
}

/// Writes `snapshot.json` atomically. C0 writes nothing.
pub fn write_snapshot(_deps: &Deps, _store: &Store, _now: Timestamp) -> Result<(), CoreError> {
    Ok(())
}

/// Reads `snapshot.json`.
pub fn read_snapshot(paths: &NodePaths) -> Result<Snapshot, CoreError> {
    Err(CoreError::NotFound(
        paths.root().join("snapshot.json").display().to_string(),
    ))
}

/// Runs the four node-level hooks in the tick order.
pub fn node_tick(deps: &Deps, store: &Store, now: Timestamp) -> Result<(), CoreError> {
    let _ = crate::capacity::sample(deps, store, now)?;
    let _ = crate::surface::sweep_leases(deps, store, now)?;
    crate::lifecycle::tick(deps, store, now)?;
    write_snapshot(deps, store, now)
}
