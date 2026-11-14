//! The full inbox item returned by `vowp --json fleet inbox --all`.

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;
use crate::wire::enums::{InboxKind, InboxState};
use crate::wire::ids::{InboxId, JobId, ProjectName};

/// One inbox item, including its full body and suggested answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxItem {
    /// Item id.
    pub id: InboxId,
    /// Project the item belongs to, when any.
    pub project: Option<ProjectName>,
    /// Job that raised it, when any.
    pub job_id: Option<JobId>,
    /// Item kind.
    pub kind: InboxKind,
    /// Short title.
    pub title: String,
    /// Full question or notice text. Omitted from the snapshot summary.
    pub body: String,
    /// Suggested answers.
    pub options: Vec<String>,
    /// Current state.
    pub state: InboxState,
    /// Answer text, once answered.
    pub answer: Option<String>,
    /// Who answered.
    pub answered_by: Option<String>,
    /// When the item was created.
    pub created_at: Timestamp,
    /// When the item was answered or dismissed.
    pub answered_at: Option<Timestamp>,
}
