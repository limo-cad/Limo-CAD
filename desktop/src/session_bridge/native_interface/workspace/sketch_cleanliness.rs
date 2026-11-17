//! A clean existing-sketch mode round trip is not a persistent edit. Engine
//! revisions still advance; only the File clean marker may be reconciled.
use super::*;

impl DocumentWorkspace {
    pub(crate) fn invalidate_clean_sketch(&mut self, owner: &DocumentContext) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| &tab.owner == owner) {
            tab.clean_sketch = None;
        }
    }

    pub(crate) fn begin_clean_sketch(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| tab.owner == expected.owner)
            .ok_or("The document tab was removed or replaced")?;
        tab.clean_sketch = None;
        let publishers = bridge
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get(&expected.owner.window_id)
            .ok_or("Native window no longer exists")?;
        check_owner(publisher, engine, &expected.owner)?;
        if publisher.by_project[&expected.owner.document_id].engine_revision != expected.revision {
            return Err("The document changed before sketch editing".into());
        }
        if tab.saved.as_ref() != Some(expected) || tab.saving.upgrade().is_some() {
            return Ok(());
        }
        if let (Some(archive), Some(expected_revision)) =
            (&tab.archive, expected.revision.checked_add(2))
        {
            tab.clean_sketch = Some(CleanSketchEntry {
                saved: expected.clone(),
                archive: Arc::downgrade(archive),
                file_epoch: tab.file_epoch,
                expected_revision,
                awaiting_recompute: false,
            });
        }
        Ok(())
    }

    pub(crate) fn finish_clean_sketch(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
        recompute_pending: bool,
    ) -> Result<bool, String> {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.owner == expected.owner) else {
            return Ok(false);
        };
        let Some(mut entry) = tab.clean_sketch.take() else {
            return Ok(false);
        };
        if tab.saved.as_ref() != Some(&entry.saved)
            || tab.file_epoch != entry.file_epoch
            || tab.saving.upgrade().is_some()
            || expected.revision != entry.expected_revision
        {
            return Ok(false);
        }
        let Some(archive) = entry.archive.upgrade() else {
            return Ok(false);
        };
        if !tab
            .archive
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &archive))
        {
            return Ok(false);
        }
        let publishers = bridge
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get(&expected.owner.window_id)
            .ok_or("Native window no longer exists")?;
        check_owner(publisher, engine, &expected.owner)?;
        if publisher.by_project[&expected.owner.document_id].engine_revision != expected.revision {
            return Ok(false);
        }
        if recompute_pending {
            if entry.awaiting_recompute {
                return Ok(false);
            }
            let Some(next) = expected.revision.checked_add(1) else {
                return Ok(false);
            };
            entry.expected_revision = next;
            entry.awaiting_recompute = true;
            tab.clean_sketch = Some(entry);
            return Ok(false);
        }
        let snapshot = engine.project_snapshot()?;
        let archive = archive
            .lock()
            .map_err(|_| "Project archive lock poisoned")?;
        let saved = archive.snapshot().map_err(|error| error.to_string())?;
        // Outer model whitespace is insignificant; domain intent must match
        // exactly. Other formatting/migration differences keep the tab dirty.
        if snapshot.model_json().trim_matches([' ', '\r', '\n', '\t'])
            != saved.model_json().trim_matches([' ', '\r', '\n', '\t'])
            || snapshot.intents() != saved.intents()
        {
            return Ok(false);
        }
        tab.saved = Some(expected.clone());
        Ok(true)
    }
}
