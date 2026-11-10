//! Fair ordering of projects. C0 leaves the order unchanged; E2 fills the rule.

use vowp_protocol::ProjectEntry;

/// Orders projects by fairness weight and last admission.
pub fn order(_projects: &mut Vec<ProjectEntry>, _last: Option<&vowp_protocol::ProjectName>) {}
