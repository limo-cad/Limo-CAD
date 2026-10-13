//! Inactive-tab policy, serialized under the same publisher -> engine fence.
use super::*;
use std::{collections::HashSet, time::Duration};

pub(crate) const IDLE_LIMIT: Duration = Duration::from_secs(60 * 60);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoryPressure {
    Normal,
    Constrained,
    Critical,
}
impl MemoryPressure {
    pub(crate) fn from_physical_memory(total: u64, available: u64) -> Self {
        if total == 0 {
            return Self::Normal;
        }
        if available <= (total / 20).max(512 * 1024 * 1024) {
            Self::Critical
        } else if available <= (total / 10).max(1024 * 1024 * 1024) {
            Self::Constrained
        } else {
            Self::Normal
        }
    }
}
impl DocumentWorkspace {
    pub(super) fn touch(&mut self, owner: &DocumentContext) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| &tab.owner == owner) {
            tab.last_used = Instant::now();
        }
    }
    pub(crate) fn has_eviction_candidates(
        &self,
        engine: &AppState,
        owner: &DocumentContext,
        pressure: MemoryPressure,
        now: Instant,
    ) -> bool {
        self.tabs.iter().any(|tab| {
            tab.owner.document_id != owner.document_id
                && tab.saving.upgrade().is_none()
                && (pressure != MemoryPressure::Normal
                    || now.saturating_duration_since(tab.last_used) >= IDLE_LIMIT)
                && engine.can_evict_project_session(&tab.owner.document_id)
        })
    }
    pub(crate) fn evict_inactive(
        &self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
        pressure: MemoryPressure,
        now: Instant,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<usize, String> {
        bridge.with_native_document_receipt(engine, &expected.owner, |revision| {
            if revision != expected.revision {
                return Err("Document changed before memory maintenance".into());
            }
            validate()?;
            let mut candidates: Vec<_> = self
                .tabs
                .iter()
                .filter(|tab| {
                    tab.owner.document_id != expected.owner.document_id
                        && tab.saving.upgrade().is_none()
                })
                .collect();
            candidates.sort_by_key(|tab| tab.last_used);
            let mut released = 0;
            for tab in candidates {
                let idle = now.saturating_duration_since(tab.last_used) >= IDLE_LIMIT;
                if pressure == MemoryPressure::Critical
                    || idle
                    || (pressure == MemoryPressure::Constrained && released == 0)
                {
                    released +=
                        usize::from(engine.evict_inactive_project_session(&tab.owner.document_id)?);
                }
            }
            Ok(released)
        })
    }
    pub(crate) fn cold_documents(&self, engine: &AppState) -> Vec<DocumentContext> {
        let cold: HashSet<_> = engine.cold_project_sessions().into_iter().collect();
        self.tabs
            .iter()
            .filter(|tab| cold.contains(&tab.owner.document_id))
            .map(|tab| tab.owner.clone())
            .collect()
    }
}
