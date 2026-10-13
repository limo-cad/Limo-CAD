//! Application-level solid history policy, independent of the interface host.
//!
//! At the latest timeline marker, Undo deletes the last feature after exporting
//! a complete model, and Redo loads that model
//! through ordinary project replacement. Earlier timeline markers move by one.
//! Destructive feature edits retain a bounded pre-edit snapshot, so Undo restores
//! a deleted or edited feature instead of deleting an unrelated earlier feature.
//! Sketch, assembly and drawing histories retain their own command boundaries.
//!
//! The publisher owner lock must cover planning, engine execution and commit.
//! Tickets do not grant engine authority and never mutate history on creation.
//! Commit only after verified engine success. An unchanged rejection/cancel
//! leaves the stack intact; an unverified partial project load must retire the
//! publisher and its history, not reauthorize snapshots against unknown state.

use limo_cad_interface::DocumentContext;
use std::sync::Arc;

const SOLID_REDO_LIMIT: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryState {
    pub context: DocumentContext,
    pub engine_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UndoStep {
    DeleteLatest,
    Rollback(usize),
}

#[derive(Clone, Debug)]
pub(crate) enum RedoStep {
    Rollback(usize),
    Restore(RedoTicket),
}

#[derive(Clone, Debug)]
struct Ticket {
    identity: Arc<()>,
    serial: u64,
    expected: HistoryState,
    model_json: Arc<str>,
}

#[derive(Clone, Debug)]
pub(crate) struct UndoTicket(Ticket);

#[derive(Clone, Debug)]
pub(crate) struct RedoTicket(Ticket);

#[derive(Clone, Debug)]
pub(crate) struct EditUndoTicket(Ticket);
impl EditUndoTicket {
    pub(crate) fn model_json(&self) -> &str {
        &self.0.model_json
    }
}

#[derive(Clone, Debug)]
struct RedoEntry {
    model: Arc<str>,
    /// Destructive edits restore a snapshot in both directions. Feature-create
    /// Undo retains its established delete-latest behavior.
    edit_before: Option<Arc<str>>,
}

impl RedoTicket {
    pub(crate) fn model_json(&self) -> &str {
        &self.0.model_json
    }
}

/// Cheap cloning retains immutable model bytes when a successful Redo must
/// carry the remaining stack across deliberate ProjectPublisher retirement.
/// Ordinary Open/New gets a fresh default value with no inherited history.
#[derive(Clone, Debug)]
pub(crate) struct SolidHistory {
    identity: Arc<()>,
    serial: u64,
    authorized: Option<HistoryState>,
    redo: Vec<RedoEntry>,
    edits: Vec<Arc<str>>,
    preserve_visibility: bool,
}

impl Default for SolidHistory {
    fn default() -> Self {
        Self {
            identity: Arc::new(()),
            serial: 0,
            authorized: None,
            redo: Vec::new(),
            edits: Vec::new(),
            preserve_visibility: false,
        }
    }
}

/// A rollback marker is a feature count, not the last feature's ID or index.
pub(crate) fn undo_step(
    rollback_index: usize,
    feature_count: usize,
) -> Result<Option<UndoStep>, &'static str> {
    check_marker(rollback_index, feature_count)?;
    Ok(if rollback_index == 0 {
        None
    } else if rollback_index == feature_count {
        Some(UndoStep::DeleteLatest)
    } else {
        Some(UndoStep::Rollback(rollback_index - 1))
    })
}

fn check_marker(index: usize, count: usize) -> Result<(), &'static str> {
    if index > count {
        return Err("The timeline marker is outside the current feature history");
    }
    Ok(())
}

impl SolidHistory {
    fn next_serial(&self) -> Result<u64, &'static str> {
        self.serial
            .checked_add(1)
            .ok_or("Application history revision exhausted")
    }

    /// Call before history availability/commands using the authoritative model
    /// receipt. Any ordinary branch change or document incarnation expires
    /// Redo. Camera/selection changes must not advance that model receipt.
    pub(crate) fn observe(&mut self, state: &HistoryState) -> Result<(), &'static str> {
        if self.authorized.as_ref() == Some(state) {
            return Ok(());
        }
        let serial = self.next_serial()?;
        self.redo.clear();
        self.edits.clear();
        self.preserve_visibility = false;
        self.authorized = Some(state.clone());
        self.serial = serial;
        Ok(())
    }

    /// Adopt a successful visibility mutation without capturing geometry or
    /// branching edit history. Prepared tickets still expire at the new revision.
    pub(crate) fn advance_visibility(
        &mut self,
        before: &HistoryState,
        after: HistoryState,
    ) -> Result<(), &'static str> {
        if before.context != after.context || after.engine_revision <= before.engine_revision {
            return Err("Visibility history requires a newer revision of its owned document");
        }
        self.observe(before)?;
        let serial = self.next_serial()?;
        self.authorized = Some(after);
        self.serial = serial;
        self.preserve_visibility = true;
        Ok(())
    }

    pub(crate) fn preserves_visibility(&self) -> bool {
        self.preserve_visibility
    }

    pub(crate) fn can_redo(&self, state: &HistoryState) -> bool {
        self.authorized.as_ref() == Some(state) && !self.redo.is_empty()
    }

    pub(crate) fn redo_step(
        &self,
        state: &HistoryState,
        rollback_index: usize,
        feature_count: usize,
    ) -> Result<Option<RedoStep>, &'static str> {
        check_marker(rollback_index, feature_count)?;
        if rollback_index < feature_count {
            return Ok(Some(RedoStep::Rollback(rollback_index + 1)));
        }
        self.next_serial()?;
        Ok(self.peek_redo(state).map(RedoStep::Restore))
    }

    pub(crate) fn prepare_undo(
        &self,
        state: &HistoryState,
        model_json: String,
    ) -> Result<UndoTicket, &'static str> {
        self.next_serial()?;
        if model_json.is_empty() {
            return Err("Undo needs a complete engine model export");
        }
        Ok(UndoTicket(Ticket {
            identity: self.identity.clone(),
            serial: self.serial,
            expected: state.clone(),
            model_json: model_json.into(),
        }))
    }

    /// Snapshot capture alone must never add a phantom Redo. The caller has
    /// already deleted the actual last feature successfully under the lock.
    pub(crate) fn commit_undo(
        &mut self,
        ticket: UndoTicket,
        after: HistoryState,
    ) -> Result<(), &'static str> {
        self.check_ticket(&ticket.0)?;
        if ticket.0.expected.context != after.context
            || after.engine_revision <= ticket.0.expected.engine_revision
        {
            return Err("Undo did not produce a newer revision of its owned document");
        }
        let serial = self.next_serial()?;
        if self.authorized.as_ref() != Some(&ticket.0.expected) {
            self.redo.clear();
        }
        self.redo.push(RedoEntry {
            model: ticket.0.model_json,
            edit_before: None,
        });
        if self.redo.len() > SOLID_REDO_LIMIT {
            self.redo.remove(0);
        }
        self.authorized = Some(after);
        self.serial = serial;
        Ok(())
    }

    /// Peek without popping: failed/cancelled loads retain the same entry.
    pub(crate) fn peek_redo(&self, state: &HistoryState) -> Option<RedoTicket> {
        if !self.can_redo(state) {
            return None;
        }
        Some(RedoTicket(Ticket {
            identity: self.identity.clone(),
            serial: self.serial,
            expected: state.clone(),
            model_json: self.redo.last()?.model.clone(),
        }))
    }

    /// Complete a verified whole-model load. The new incarnation may reset
    /// engine_revision to one, but must still belong to this window/project.
    pub(crate) fn commit_redo(
        &mut self,
        ticket: RedoTicket,
        after: HistoryState,
    ) -> Result<(), &'static str> {
        self.check_ticket(&ticket.0)?;
        if self.authorized.as_ref() != Some(&ticket.0.expected)
            || !self
                .redo
                .last()
                .is_some_and(|entry| Arc::ptr_eq(&entry.model, &ticket.0.model_json))
        {
            return Err("Redo no longer belongs to the current model branch");
        }
        let before = &ticket.0.expected.context;
        if before.window_id != after.context.window_id
            || before.document_id != after.context.document_id
            || before.epoch == after.context.epoch
            || after.engine_revision == 0
        {
            return Err("Redo must replace its owned document with a fresh incarnation");
        }
        let serial = self.next_serial()?;
        if let Some(model) = self.redo.pop().and_then(|entry| entry.edit_before) {
            self.edits.push(model);
        }
        self.authorized = Some(after);
        self.serial = serial;
        Ok(())
    }

    fn check_ticket(&self, ticket: &Ticket) -> Result<(), &'static str> {
        if !Arc::ptr_eq(&self.identity, &ticket.identity) || self.serial != ticket.serial {
            return Err("Application history changed before the operation could commit");
        }
        Ok(())
    }

    /// Plan on a clone before running the kernel, then adopt only after success.
    pub(crate) fn record_edit(
        &mut self,
        before: &HistoryState,
        model: String,
        after: HistoryState,
    ) -> Result<(), &'static str> {
        if before.context != after.context
            || after.engine_revision <= before.engine_revision
            || model.is_empty()
        {
            return Err("Edit history requires a newer revision and a complete owned model");
        }
        self.observe(before)?;
        let serial = self.next_serial()?;
        self.edits.push(model.into());
        if self.edits.len() > SOLID_REDO_LIMIT {
            self.edits.remove(0);
        }
        self.redo.clear();
        self.authorized = Some(after);
        self.serial = serial;
        Ok(())
    }
    pub(crate) fn peek_edit_undo(&self, state: &HistoryState) -> Option<EditUndoTicket> {
        if self.authorized.as_ref() != Some(state) {
            return None;
        }
        Some(EditUndoTicket(Ticket {
            identity: self.identity.clone(),
            serial: self.serial,
            expected: state.clone(),
            model_json: self.edits.last()?.clone(),
        }))
    }
    pub(crate) fn commit_edit_undo(
        &mut self,
        ticket: EditUndoTicket,
        current_model: String,
        after: HistoryState,
    ) -> Result<(), &'static str> {
        self.check_ticket(&ticket.0)?;
        let before = &ticket.0.expected.context;
        if self.authorized.as_ref() != Some(&ticket.0.expected)
            || !self
                .edits
                .last()
                .is_some_and(|model| Arc::ptr_eq(model, &ticket.0.model_json))
            || current_model.is_empty()
            || before.window_id != after.context.window_id
            || before.document_id != after.context.document_id
            || before.epoch == after.context.epoch
            || after.engine_revision == 0
        {
            return Err("Edit Undo must restore its owned document with a fresh incarnation");
        }
        let serial = self.next_serial()?;
        self.edits.pop();
        self.redo.push(RedoEntry {
            model: current_model.into(),
            edit_before: Some(ticket.0.model_json),
        });
        if self.redo.len() > SOLID_REDO_LIMIT {
            self.redo.remove(0);
        }
        self.authorized = Some(after);
        self.serial = serial;
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_history/tests.rs"]
mod tests;
