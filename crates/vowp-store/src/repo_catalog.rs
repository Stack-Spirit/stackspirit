//! Catalog repository (filled by H1).
//!
//! The `catalog` table exists from day one. The frozen signatures from
//! `docs/design/v0.2/home-and-surfaces.md` §13.3 are:
//!
//! ```text
//! pub fn upsert_catalog(&self, e: &CatalogEntry) -> Result<(), StoreError>;
//! pub fn catalog_entry(&self, path: &str) -> Result<Option<CatalogEntry>, StoreError>;
//! pub fn list_catalog(&self, f: &CatalogFilter) -> Result<Vec<CatalogEntry>, StoreError>;
//! pub fn mark_catalog_deleted(&self, id: &str, at: Timestamp) -> Result<(), StoreError>;
//! pub fn delete_catalog(&self, id: &str) -> Result<bool, StoreError>;
//! ```
//!
//! `CatalogEntry`, `CatalogFilter`, `CatalogClass`, `GcReport` and `BackupReport` live in
//! `vowp-protocol/src/catalog.rs`, which the C0 skeleton job creates in the same wave. This
//! file stays empty until those types land so the C1 branch compiles and passes its gates
//! without depending on an unmerged sibling job.
