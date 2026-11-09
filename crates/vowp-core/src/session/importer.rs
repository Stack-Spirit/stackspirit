//! Claude Code transcript importer (`vowp session watch`), optional in v0.1.
//!
//! The importer is a nice-to-have in the n07c job map. It is deliberately not shipped in
//! this build: `session watch` has no command implementation yet, and recording an
//! interactive Claude Code transcript needs the `vowp_adapters::claude_code` transcript
//! parser plus a `watch.json` byte-offset file. The file is left as an explicit,
//! documented open item rather than a stub that pretends to work.
