//! Per-tool target selection and independent Mac / Home connection status.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::SpiritError;
use crate::tools::model::ToolEntry;

/// Where a tool should be connected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTarget {
    /// Local Mac and Always-On Home. The default and recommended choice.
    #[default]
    Both,
    /// Local Mac only.
    Local,
    /// Always-On Home only.
    Home,
}

impl ToolTarget {
    /// Whether this selection includes the Mac.
    #[must_use]
    pub fn includes_local(self) -> bool {
        matches!(self, Self::Both | Self::Local)
    }

    /// Whether this selection includes the Home.
    #[must_use]
    pub fn includes_home(self) -> bool {
        matches!(self, Self::Both | Self::Home)
    }
}

/// Connection state on one side.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolConnectionState {
    /// Not connected.
    #[default]
    NotConnected,
    /// Connected.
    Connected,
    /// Connect was attempted and failed.
    NeedsAttention,
}

/// Mac and Home status, tracked independently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStatus {
    /// Local Mac.
    pub mac: ToolConnectionState,
    /// Always-On Home.
    pub home: ToolConnectionState,
}

#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    target: ToolTarget,
    status: ToolStatus,
}

/// In-memory board of target selection and status per tool id.
#[derive(Debug, Clone, Default)]
pub struct ToolConnections {
    slots: BTreeMap<String, Slot>,
}

impl ToolConnections {
    /// An empty board; every tool defaults to [`ToolTarget::Both`], not connected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The selected target for a tool.
    #[must_use]
    pub fn target(&self, id: &str) -> ToolTarget {
        self.slots
            .get(id)
            .map_or_else(ToolTarget::default, |s| s.target)
    }

    /// Selects a target for a tool.
    pub fn set_target(&mut self, id: &str, target: ToolTarget) -> Result<(), SpiritError> {
        ToolEntry::validate_id(id)?;
        self.slots.entry(id.to_owned()).or_default().target = target;
        Ok(())
    }

    /// Current Mac / Home status for a tool.
    #[must_use]
    pub fn status(&self, id: &str) -> ToolStatus {
        self.slots
            .get(id)
            .map_or_else(ToolStatus::default, |s| s.status)
    }

    /// Records the result of connecting on the sides the selected target includes.
    /// A side outside the selection is left untouched.
    pub fn record_connect(
        &mut self,
        id: &str,
        mac: Option<ToolConnectionState>,
        home: Option<ToolConnectionState>,
    ) -> Result<ToolStatus, SpiritError> {
        ToolEntry::validate_id(id)?;
        let slot = self.slots.entry(id.to_owned()).or_default();
        if let Some(state) = mac.filter(|_| slot.target.includes_local()) {
            slot.status.mac = state;
        }
        if let Some(state) = home.filter(|_| slot.target.includes_home()) {
            slot.status.home = state;
        }
        Ok(slot.status)
    }

    /// Marks both sides disconnected.
    pub fn disconnect(&mut self, id: &str) {
        if let Some(slot) = self.slots.get_mut(id) {
            slot.status = ToolStatus::default();
        }
    }
}
