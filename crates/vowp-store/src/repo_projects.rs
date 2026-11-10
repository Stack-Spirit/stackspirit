//! Project-registry repository (filled by the project/registry feature jobs).
//!
//! The v0.2 schema and binding rules are in place in [`crate::repo_nodes`]. The frozen
//! signatures from `docs/design/v0.2/home-and-surfaces.md` §13.3 are:
//!
//! ```text
//! pub fn project_entry(&self, name: &ProjectName) -> Result<Option<ProjectEntry>, StoreError>;
//! pub fn list_project_entries(&self) -> Result<Vec<ProjectEntry>, StoreError>;
//! pub fn project_by_path(&self, path: &std::path::Path) -> Result<Option<ProjectEntry>, StoreError>;
//! pub fn set_project_state(&self, name: &ProjectName, state: ProjectState, reason: &str)
//!     -> Result<ProjectEntry, StoreError>;
//! pub fn set_project_weight(&self, name: &ProjectName, weight: u8) -> Result<(), StoreError>;
//! pub fn remove_project_record(&self, name: &ProjectName) -> Result<(), StoreError>;
//! ```
//!
//! `ProjectEntry` and `ProjectState` live in `vowp-protocol/src/snapshot.rs`, which the C0
//! skeleton job creates in the same wave. This file stays empty until that type lands so the
//! C1 branch compiles and passes its gates without depending on an unmerged sibling job.
