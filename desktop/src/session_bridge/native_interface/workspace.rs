//! Native document tabs and File transactions. The engine/publisher remain
//! authoritative; a path or delayed file result never identifies a document.

use super::{check_owner, context, NativeMutationResult};
use crate::{
    session_bridge::{parse_engine_envelope, SessionBridgeState},
    state::AppState,
};
use limo_cad_interface::DocumentContext;
use limo_cad_project_file::{ProjectArchive, SaveMetadata};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Instant,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentReceipt {
    pub owner: DocumentContext,
    pub revision: u64,
}

struct Tab {
    owner: DocumentContext,
    file_epoch: u64,
    name: String,
    path: Option<PathBuf>,
    archive: Option<Arc<Mutex<ProjectArchive>>>,
    saved: Option<DocumentReceipt>,
    saving: Weak<()>,
    last_used: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct TabSummary {
    pub owner: DocumentContext,
    pub name: String,
    pub path: Option<PathBuf>,
    pub active: bool,
    pub dirty: bool,
    #[cfg(test)]
    pub saving: bool,
}

impl TabSummary {
    pub(crate) fn display_name(&self) -> &str {
        if self.name == "Untitled" {
            if let Some(name) = self
                .path
                .as_ref()
                .and_then(|path| path.file_stem())
                .and_then(|name| name.to_str())
                .filter(|name| !name.trim().is_empty())
            {
                return name;
            }
        }
        &self.name
    }
}

#[derive(Default)]
pub(crate) struct DocumentWorkspace {
    tabs: Vec<Tab>,
    initial_publication_pending: bool,
}

/// Owned bytes may be written off the UI thread. Dropping cancelled work also
/// releases its save lease; no caller can forge a successful write receipt.
pub(crate) struct PreparedSave {
    receipt: DocumentReceipt,
    path: PathBuf,
    bytes: Vec<u8>,
    replace: bool,
    lease: Arc<()>,
}
pub(crate) struct CompletedSave {
    work: PreparedSave,
    result: Result<(), String>,
}
impl PreparedSave {
    pub(crate) fn write(self) -> CompletedSave {
        let result = if self.replace {
            limo_cad_project_file::write_binary_file_atomic(&self.path, &self.bytes)
        } else {
            limo_cad_project_file::write_binary_file_new(&self.path, &self.bytes)
        };
        CompletedSave {
            work: self,
            result: result.map_err(|error| error.to_string()),
        }
    }
}

impl SessionBridgeState {
    pub(crate) fn native_document_receipt(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
    ) -> Result<DocumentReceipt, String> {
        let publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get(&expected.window_id)
            .ok_or("Native window no longer exists")?;
        check_owner(publisher, engine, expected)?;
        Ok(DocumentReceipt {
            owner: expected.clone(),
            revision: publisher.by_project[&expected.document_id].engine_revision,
        })
    }

    /// New/Activate/Close use the retained AppState workspace under the same
    /// publisher fence as model operations, with no validate-then-switch gap.
    fn native_transition<T>(
        &self,
        engine: &AppState,
        expected: &DocumentReceipt,
        target: Option<&DocumentContext>,
        validate: impl FnOnce() -> Result<(), String>,
        transition: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get_mut(&expected.owner.window_id)
            .ok_or("Native window no longer exists")?;
        check_owner(publisher, engine, &expected.owner)?;
        if publisher.active_mut().engine_revision != expected.revision {
            return Err("The document changed before this File operation could complete".into());
        }
        if let Some(target) = target {
            let project = publisher
                .by_project
                .get(&target.document_id)
                .ok_or("The target document tab no longer exists")?;
            if target.window_id != expected.owner.window_id
                || project.native_interface_epoch != target.epoch
            {
                return Err("The target document tab was replaced".into());
            }
        }
        validate()?;
        let result = transition();
        let active = engine.active_project_session_id();
        publisher.rebind_to(&active);
        drop(publishers);
        let _ = self.write_process_instance_file();
        result
    }
}

mod retention;
pub(crate) use retention::MemoryPressure;

impl DocumentWorkspace {
    pub(crate) fn observe(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        window: &str,
    ) -> Result<DocumentReceipt, String> {
        if self.tabs.is_empty()
            && engine.active_project_session_id() == crate::state::BOOTSTRAP_SESSION_ID
        {
            self.initial_publication_pending = true;
            let id = uuid::Uuid::new_v4().to_string();
            parse_engine_envelope(bridge.with_project_session_transition(window, engine, || {
                engine.bind_project_session(&id)
            }))?;
        }
        let owner = bridge.native_document_context(window, engine)?;
        if self.initial_publication_pending {
            // Rebinding retains the startup session, but its published metadata
            // still names the bootstrap document. Publish the real owner before
            // exposing the first tab; retain this obligation if storage fails.
            bridge.publish_native_document(engine, &owner, "solid")?;
            self.initial_publication_pending = false;
        }
        let (receipt, name, file_epoch) = {
            let publishers = bridge
                .publishers
                .lock()
                .map_err(|_| "Session publisher lock poisoned")?;
            let publisher = publishers
                .get(window)
                .ok_or("Native window no longer exists")?;
            check_owner(publisher, engine, &owner)?;
            (
                DocumentReceipt {
                    owner: owner.clone(),
                    revision: publisher.by_project[&owner.document_id].engine_revision,
                },
                engine.document_name(),
                publisher.by_project[&owner.document_id].native_file_epoch,
            )
        };
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.owner.document_id == owner.document_id)
        {
            if tab.owner != owner {
                tab.owner = owner;
                if tab.file_epoch != file_epoch {
                    tab.path = None;
                    tab.archive = None;
                }
                tab.file_epoch = file_epoch;
                tab.saved = None;
                tab.saving = Weak::new();
            }
            tab.name = name;
            tab.last_used = Instant::now();
        } else {
            self.tabs.push(Tab {
                owner,
                file_epoch,
                name,
                path: None,
                archive: None,
                saved: Some(receipt.clone()),
                saving: Weak::new(),
                last_used: Instant::now(),
            });
        }
        Ok(receipt)
    }

    pub(crate) fn summaries(
        &self,
        bridge: &SessionBridgeState,
        active: &DocumentContext,
    ) -> Result<Vec<TabSummary>, String> {
        let publishers = bridge
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get(&active.window_id)
            .ok_or("Native window no longer exists")?;
        self.tabs
            .iter()
            .map(|tab| {
                let project = publisher
                    .by_project
                    .get(&tab.owner.document_id)
                    .ok_or("A document tab is no longer resident")?;
                let owner = context(&active.window_id, &tab.owner.document_id, project);
                let receipt = DocumentReceipt {
                    owner: owner.clone(),
                    revision: project.engine_revision,
                };
                Ok(TabSummary {
                    owner,
                    name: tab.name.clone(),
                    path: tab.path.clone(),
                    active: tab.owner.document_id == active.document_id,
                    dirty: tab.saved.as_ref() != Some(&receipt),
                    #[cfg(test)]
                    saving: tab.saving.upgrade().is_some(),
                })
            })
            .collect()
    }

    pub(crate) fn new_tab_guarded(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<DocumentReceipt, String> {
        let id = uuid::Uuid::new_v4().to_string();
        bridge.native_transition(engine, expected, None, validate, || {
            parse_engine_envelope(engine.create_project_session(&id))
        })?;
        self.touch(&expected.owner);
        self.observe(bridge, engine, &expected.owner.window_id)
    }

    pub(crate) fn activate_guarded(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
        target: &DocumentContext,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<DocumentReceipt, String> {
        let started = Instant::now();
        let outcome = (|| {
            if expected.owner.window_id != target.window_id
                || !self.tabs.iter().any(|tab| &tab.owner == target)
            {
                return Err("The requested document tab was removed or replaced".into());
            }
            bridge.native_transition(engine, expected, Some(target), validate, || {
                if parse_engine_envelope(engine.activate_project_session(&target.document_id))?
                    != true
                {
                    return Err("The document tab is no longer resident".into());
                }
                Ok(())
            })?;
            self.touch(&expected.owner);
            let receipt = self.observe(bridge, engine, &expected.owner.window_id)?;
            if &receipt.owner != target {
                return Err("The document tab was replaced before activation".into());
            }
            Ok(receipt)
        })();
        if let Ok(receipt) = &outcome {
            super::switch_timing::record_document_switch(
                &receipt.owner.window_id,
                &receipt.owner.document_id,
                &bridge.process_instance_id,
                started.elapsed(),
            );
        }
        outcome
    }

    pub(crate) fn prepare_save_guarded(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
        (path, overwrite): (PathBuf, bool),
        metadata: SaveMetadata<'_>,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<PreparedSave, String> {
        validate_path(&path, false)?;
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| tab.owner == expected.owner)
            .ok_or("The document tab was removed or replaced")?;
        if tab.saving.upgrade().is_some() {
            return Err("This document already has a save in progress".into());
        }
        let model = {
            let publishers = bridge
                .publishers
                .lock()
                .map_err(|_| "Session publisher lock poisoned")?;
            let publisher = publishers
                .get(&expected.owner.window_id)
                .ok_or("Native window no longer exists")?;
            check_owner(publisher, engine, &expected.owner)?;
            if publisher.by_project[&expected.owner.document_id].engine_revision
                != expected.revision
            {
                return Err("The document changed before Save was prepared".into());
            }
            validate()?;
            parse_engine_envelope(engine.engine_call("project_export_model", ""))?
                .as_str()
                .ok_or("Save requires a completed project model")?
                .to_owned()
        };
        let bytes = if let Some(archive) = &tab.archive {
            let mut archive = archive
                .lock()
                .map_err(|_| "Project archive lock poisoned")?;
            archive
                .update_model(model, metadata)
                .map_err(|error| error.to_string())?;
            archive.encode().map_err(|error| error.to_string())?
        } else {
            let archive =
                ProjectArchive::new(model, metadata).map_err(|error| error.to_string())?;
            let bytes = archive.encode().map_err(|error| error.to_string())?;
            tab.archive = Some(Arc::new(Mutex::new(archive)));
            bytes
        };
        let lease = Arc::new(());
        tab.saving = Arc::downgrade(&lease);
        let replace = overwrite;
        Ok(PreparedSave {
            receipt: expected.clone(),
            path,
            bytes,
            replace,
            lease,
        })
    }

    pub(crate) fn complete_save(
        &mut self,
        bridge: &SessionBridgeState,
        completed: CompletedSave,
    ) -> Result<DocumentReceipt, String> {
        completed.result?;
        let work = completed.work;
        let publishers = bridge
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get(&work.receipt.owner.window_id)
            .ok_or("The file was saved, but its window was closed")?;
        let project = publisher
            .by_project
            .get(&work.receipt.owner.document_id)
            .ok_or("The file was saved, but its document was closed")?;
        if project.native_interface_epoch != work.receipt.owner.epoch {
            return Err("The file was saved, but its document was replaced".into());
        }
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| tab.owner == work.receipt.owner)
            .ok_or("The file was saved, but its document was closed or replaced")?;
        if !tab
            .saving
            .upgrade()
            .is_some_and(|lease| Arc::ptr_eq(&lease, &work.lease))
        {
            return Err("The file was saved, but a newer save owns this document".into());
        }
        tab.path = Some(work.path);
        tab.saved = Some(work.receipt.clone());
        Ok(work.receipt)
    }

    pub(crate) fn open_guarded(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
        path: PathBuf,
        discard_changes: bool,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<NativeMutationResult, String> {
        validate_path(&path, true)?;
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.owner == expected.owner)
            .ok_or("The document tab was removed or replaced")?;
        if tab.saving.upgrade().is_some() {
            return Err("Wait for this document's save before opening another file".into());
        }
        if tab.saved.as_ref() != Some(expected) && !discard_changes {
            return Err("Save this document or explicitly discard its changes before Open".into());
        }
        let archive = ProjectArchive::decode(
            limo_cad_project_file::read_binary_file(&path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let result = bridge.apply_native_mutation_at(
            engine,
            &expected.owner,
            expected.revision,
            "cad_load_project_model",
            &json!({"model_json":archive.model_json()}),
            validate,
        )?;
        self.observe(bridge, engine, &expected.owner.window_id)?;
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| tab.owner == result.context)
            .ok_or("Opened document is unavailable")?;
        tab.path = Some(path);
        tab.archive = Some(Arc::new(Mutex::new(archive)));
        tab.saved = Some(DocumentReceipt {
            owner: result.context.clone(),
            revision: result.engine_revision,
        });
        Ok(result)
    }

    /// Close one tab. The last tab becomes a fresh Untitled design, matching
    /// the current desktop; application Exit is a separate guarded intent.
    pub(crate) fn close_active_guarded(
        &mut self,
        bridge: &SessionBridgeState,
        engine: &AppState,
        expected: &DocumentReceipt,
        discard_changes: bool,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<DocumentReceipt, String> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.owner == expected.owner)
            .ok_or("The document tab was removed or replaced")?;
        if tab.saving.upgrade().is_some() {
            return Err("Wait for this document's save before closing it".into());
        }
        if tab.saved.as_ref() != Some(expected) && !discard_changes {
            return Err("Save this document or explicitly discard its changes before Close".into());
        }
        let next = self
            .tabs
            .iter()
            .find(|tab| tab.owner.document_id != expected.owner.document_id)
            .map(|tab| tab.owner.clone());
        bridge.native_transition(engine, expected, next.as_ref(), validate, || {
            match next.as_ref() {
                Some(next) => {
                    if parse_engine_envelope(engine.activate_project_session(&next.document_id))?
                        != true
                    {
                        return Err("The next document tab is unavailable".into());
                    }
                }
                None => {
                    parse_engine_envelope(
                        engine.create_project_session(&uuid::Uuid::new_v4().to_string()),
                    )?;
                }
            }
            parse_engine_envelope(engine.drop_project_session(&expected.owner.document_id))?;
            Ok(())
        })?;
        bridge.drop_bound_project_session(&expected.owner.window_id, &expected.owner.document_id);
        self.tabs.retain(|tab| tab.owner != expected.owner);
        self.observe(bridge, engine, &expected.owner.window_id)
    }
}

fn validate_path(path: &Path, opening: bool) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Use an absolute project file path".into());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !extension.eq_ignore_ascii_case("limo")
        && !extension.eq_ignore_ascii_case("nbcad")
        && !(opening && extension.eq_ignore_ascii_case("tfcad"))
    {
        return Err("Use a .limo project file".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
