//! Surfaces and leases repository (filled by F2).
//!
//! The tables exist from day one (`surfaces`, `leases`). The frozen signatures from
//! `docs/design/v0.2/home-and-surfaces.md` §13.3 are:
//!
//! ```text
//! pub fn upsert_surface(&self, s: &Surface) -> Result<(), StoreError>;
//! pub fn surface(&self, id: &SurfaceId) -> Result<Option<Surface>, StoreError>;
//! pub fn delete_surface(&self, id: &SurfaceId) -> Result<bool, StoreError>;
//! pub fn lease(&self, session: &SessionId) -> Result<Option<Lease>, StoreError>;
//! pub fn take_lease(&self, l: &Lease) -> Result<Option<Lease>, StoreError>;
//! pub fn heartbeat_lease(&self, session: &SessionId, token: &str, at: Timestamp) -> Result<bool, StoreError>;
//! pub fn release_lease(&self, session: &SessionId, token: &str) -> Result<bool, StoreError>;
//! pub fn stale_leases(&self, before: Timestamp) -> Result<Vec<Lease>, StoreError>;
//! pub fn list_leases(&self) -> Result<Vec<Lease>, StoreError>;
//! ```
//!
//! `Surface`, `SurfaceId` and `Lease` live in `vowp-protocol/src/surface.rs`, which the C0
//! skeleton job creates in the same wave. This file stays empty until those types land so the
//! C1 branch compiles and passes its gates without depending on an unmerged sibling job.
