//! Native-only admission and transport completion. Compatibility callers keep
//! their original expiring request-file protocol. A disk marker is never an
//! authority: only the exact, one-use record in the owning window is.

use super::*;
use limo_cad_interface::DocumentContext;
use limo_cad_mcp::NativeControlAdmission;

const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ticket {
    marker: NativeControlAdmission,
    nonce: Uuid,
}

#[derive(Debug)]
pub(super) struct Admission {
    ticket: Ticket,
    started: bool,
    pending_receipt: Option<(Value, Option<(DocumentContext, u64)>)>,
    query_liveness: bool,
}

fn owner_matches(
    publisher: &WindowPublisher,
    engine: &AppState,
    owner: &DocumentContext,
    revision: u64,
) -> bool {
    publisher.active_project_session_id.as_deref() == Some(owner.document_id.as_str())
        && engine.active_project_session_id() == owner.document_id
        && publisher
            .by_project
            .get(&owner.document_id)
            .is_some_and(|project| {
                project.native_interface_epoch == owner.epoch && project.engine_revision == revision
            })
}

fn original_owner(ticket: &Ticket) -> DocumentContext {
    DocumentContext {
        window_id: ticket.marker.window_id.clone(),
        document_id: ticket.marker.document_id.clone(),
        epoch: ticket.marker.epoch,
    }
}

fn marker_path(ticket: &Ticket) -> PathBuf {
    session_root()
        .join(&ticket.marker.session_id)
        .join("controls")
        .join(format!("{}.admitted.json", ticket.marker.request_id))
}

/// Failed admission restores only its claimed payload, without replacing any
/// new file. If restoration fails, keep the ignored claim for diagnosis/recovery.
struct Claim {
    path: PathBuf,
    source: PathBuf,
    committed: bool,
}
impl Drop for Claim {
    fn drop(&mut self) {
        if !self.committed {
            let _ = limo_cad_session_storage::rename_no_replace(&self.path, &self.source);
        }
    }
}

pub(super) fn admit(
    publisher: &mut WindowPublisher,
    engine: &AppState,
    process_instance_id: &str,
    owner: &DocumentContext,
    revision: u64,
    path: &Path,
    request: &Value,
) -> Result<Ticket, String> {
    if publisher.native_control.is_some() {
        return Err("A native control completion is still pending for this window".into());
    }
    let session_id = request["session_id"]
        .as_str()
        .ok_or("Missing native request session")?;
    if !owner_matches(publisher, engine, owner, revision)
        || publisher.by_project[&owner.document_id].session_id != session_id
    {
        return Err("The native control owner or revision changed before admission".into());
    }
    let marker = NativeControlAdmission {
        version: 1,
        session_id: session_id.into(),
        window_id: owner.window_id.clone(),
        document_id: owner.document_id.clone(),
        process_instance_id: process_instance_id.into(),
        request_id: request["id"]
            .as_str()
            .ok_or("Missing native request id")?
            .into(),
        epoch: owner.epoch,
        revision,
        expires_ms: request["expires_ms"]
            .as_u64()
            .ok_or("Missing native request expiry")?,
        admitted_ms: now_ms(),
    };
    let encoded_marker = marker.encode()?;
    let ticket = Ticket {
        marker,
        nonce: Uuid::new_v4(),
    };
    let claimed = marker_path(&ticket);
    if path
        != session_root()
            .join(session_id)
            .join("controls")
            .join(format!("{}.request.json", ticket.marker.request_id))
        || request.to_string().len() > MAX_REQUEST_BYTES
    {
        return Err("Native control request path or byte limit is invalid".into());
    }
    // This same-directory move claims the actual source in one atomic step and
    // refuses to overwrite a pre-existing marker. A requester may unlink only
    // the old request path; it cannot retire the claimed record after admission.
    limo_cad_session_storage::rename_no_replace(path, &claimed)
        .map_err(|error| format!("Could not claim native control request: {error}"))?;
    let mut claim = Claim {
        path: claimed,
        source: path.into(),
        committed: false,
    };
    let body = limo_cad_session_storage::read_to_string_bounded(&claim.path, MAX_REQUEST_BYTES)
        .map_err(|error| format!("Could not validate claimed native request: {error}"))?;
    let mut actual: Value =
        serde_json::from_str(&body).map_err(|_| "Invalid claimed native request")?;
    if !actual.is_object() {
        return Err("Invalid claimed native request".into());
    }
    // The broker sets this field itself; caller-supplied session strings do
    // not change the request's original queue ownership.
    actual["session_id"] = json!(session_id);
    if actual != *request
        || now_ms() > ticket.marker.expires_ms
        || !owner_matches(publisher, engine, owner, revision)
    {
        return Err("Native control request changed or expired before admission".into());
    }
    atomic_write(&claim.path, &encoded_marker)?;
    publisher.native_control = Some(Admission {
        ticket: ticket.clone(),
        started: false,
        pending_receipt: None,
        query_liveness: false,
    });
    claim.committed = true;
    Ok(ticket)
}

pub(super) fn ticket_for(
    state: &SessionBridgeState,
    owner: &DocumentContext,
    session_id: &str,
    id: &str,
) -> Result<Ticket, String> {
    let publishers = state
        .publishers
        .lock()
        .map_err(|_| "Session publisher lock poisoned")?;
    let admission = publishers
        .get(&owner.window_id)
        .and_then(|p| p.native_control.as_ref())
        .ok_or("No private native control admission exists")?;
    if original_owner(&admission.ticket) != *owner
        || admission.ticket.marker.session_id != session_id
        || admission.ticket.marker.request_id != id
        || admission.ticket.marker.process_instance_id != state.process_instance_id
    {
        return Err("Native control admission belongs to another request or owner".into());
    }
    Ok(admission.ticket.clone())
}

pub(super) fn validate_start(
    state: &SessionBridgeState,
    engine: &AppState,
    ticket: &Ticket,
) -> Result<(), String> {
    validate_start_at(state, engine, ticket, now_ms())
}

fn validate_start_at(
    state: &SessionBridgeState,
    engine: &AppState,
    ticket: &Ticket,
    now: u64,
) -> Result<(), String> {
    let mut publishers = state
        .publishers
        .lock()
        .map_err(|_| "Session publisher lock poisoned")?;
    let publisher = publishers
        .get_mut(&ticket.marker.window_id)
        .ok_or("Native control window is gone")?;
    validate_start_locked(publisher, engine, ticket, now)
}

pub(super) fn validate_start_locked(
    publisher: &mut WindowPublisher,
    engine: &AppState,
    ticket: &Ticket,
    now: u64,
) -> Result<(), String> {
    if !owner_matches(
        publisher,
        engine,
        &original_owner(ticket),
        ticket.marker.revision,
    ) || now > ticket.marker.expires_ms
    {
        return Err(
            "Native control owner, revision or execution deadline changed before dispatch".into(),
        );
    }
    let admission = publisher
        .native_control
        .as_mut()
        .filter(|a| a.ticket == *ticket && !a.started && a.pending_receipt.is_none())
        .ok_or("Native control was already started or its private admission changed")?;
    admission.started = true;
    Ok(())
}

#[derive(Debug)]
pub(super) enum CompletionError {
    Rejected(String),
    Publication(String),
}

impl std::fmt::Display for CompletionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(error) | Self::Publication(error) => formatter.write_str(error),
        }
    }
}

/// Deliver an already-produced outcome; this performs no engine or UI action.
/// Original admission identity authorizes transport even after expiry/close.
/// A still-current presentation receipt independently authorizes UI data.
pub(super) fn complete(
    state: &SessionBridgeState,
    engine: &AppState,
    ticket: &Ticket,
    presentation: Option<(&DocumentContext, u64)>,
    response: &mut Value,
) -> Result<(), CompletionError> {
    complete_at(state, engine, ticket, presentation, response, now_ms())
}

fn complete_at(
    state: &SessionBridgeState,
    engine: &AppState,
    ticket: &Ticket,
    presentation: Option<(&DocumentContext, u64)>,
    response: &mut Value,
    published_ms: u64,
) -> Result<(), CompletionError> {
    let reject = |message: &str| CompletionError::Rejected(message.into());
    let mut publishers = state
        .publishers
        .lock()
        .map_err(|_| reject("Session publisher lock poisoned"))?;
    let publisher = publishers
        .get_mut(&ticket.marker.window_id)
        .ok_or_else(|| reject("Native control window is gone"))?;
    complete_locked(
        publisher,
        &state.process_instance_id,
        engine,
        ticket,
        presentation,
        response,
        published_ms,
    )
}

pub(super) fn complete_locked(
    publisher: &mut WindowPublisher,
    process_instance_id: &str,
    engine: &AppState,
    ticket: &Ticket,
    presentation: Option<(&DocumentContext, u64)>,
    response: &mut Value,
    published_ms: u64,
) -> Result<(), CompletionError> {
    prepare_completion_locked(
        publisher,
        process_instance_id,
        engine,
        ticket,
        presentation,
        response,
        published_ms,
    )?;
    let result =
        marker_path(ticket).with_file_name(format!("{}.result.json", ticket.marker.request_id));
    if let Err(error) =
        limo_cad_session_storage::atomic_write_new(&result, response.to_string().as_bytes())
    {
        retain_receipt(publisher, response, presentation);
        return Err(CompletionError::Publication(format!(
            "Native completion receipt was not published: {error}"
        )));
    }
    publisher.native_control = None;
    // The receipt is already durable and wins during status polling even if
    // cleanup fails or the process exits immediately afterward.
    let _ = fs::remove_file(marker_path(ticket));
    Ok(())
}

/// Preserve the genuine supported-query outcome before fallible liveness
/// publication. Only delivery may be retried; admission still blocks dispatch.
pub(super) fn retain_query_locked(
    publisher: &mut WindowPublisher,
    process_instance_id: &str,
    engine: &AppState,
    ticket: &Ticket,
    presentation: Option<(&DocumentContext, u64)>,
    response: &mut Value,
) -> Result<(), CompletionError> {
    prepare_completion_locked(
        publisher,
        process_instance_id,
        engine,
        ticket,
        presentation,
        response,
        now_ms(),
    )?;
    retain_receipt(publisher, response, presentation);
    publisher
        .native_control
        .as_mut()
        .expect("validated private admission")
        .query_liveness = true;
    Ok(())
}

fn retain_receipt(
    publisher: &mut WindowPublisher,
    response: &Value,
    presentation: Option<(&DocumentContext, u64)>,
) {
    publisher
        .native_control
        .as_mut()
        .expect("validated private admission")
        .pending_receipt = Some((
        response.clone(),
        presentation.map(|(owner, revision)| (owner.clone(), revision)),
    ));
}

fn prepare_completion_locked(
    publisher: &mut WindowPublisher,
    process_instance_id: &str,
    engine: &AppState,
    ticket: &Ticket,
    presentation: Option<(&DocumentContext, u64)>,
    response: &mut Value,
    published_ms: u64,
) -> Result<(), CompletionError> {
    let reject = |message: &str| CompletionError::Rejected(message.into());
    let admission = publisher
        .native_control
        .as_ref()
        .filter(|a| a.ticket == *ticket)
        .ok_or_else(|| reject("Native control completion has no matching private admission"))?;
    if admission
        .pending_receipt
        .as_ref()
        .is_some_and(|(retained, _)| retained != response)
    {
        return Err(reject(
            "A produced native receipt is already awaiting transport",
        ));
    }
    if !response.is_object()
        || !matches!(response["status"].as_str(), Some("applied" | "failed"))
        || ticket.marker.process_instance_id != process_instance_id
        || response["request_id"] != ticket.marker.request_id
        || response["session_id"] != ticket.marker.session_id
        || (!admission.started && response["status"] != "failed")
    {
        return Err(reject(
            "Native control completion does not match its admitted request",
        ));
    }
    if admission.started
        && !presentation.is_some_and(|(owner, revision)| {
            owner.window_id == ticket.marker.window_id
                && owner_matches(publisher, engine, owner, revision)
        })
    {
        if response.get("operation_status").is_none() {
            response["operation_status"] = response["status"].clone();
        }
        response["status"] = json!("failed");
        response["error"] =
            json!("Document or revision changed before native presentation completed");
        response["operation_may_have_completed"] = json!(admission.started);
        response["presented"] = json!(false);
        response["render_status"] = json!("owner_changed");
        response["value"] = Value::Null;
        remove_presentation(response);
    } else if !admission.started {
        // A pre-dispatch rejection cannot carry an inspection/presentation.
        remove_presentation(response);
    }
    response["active_session_id"] = publisher
        .active_project_session_id
        .as_ref()
        .and_then(|id| publisher.by_project.get(id))
        .map(|project| json!(project.session_id))
        .unwrap_or(Value::Null);
    response["control_admission"] = serde_json::to_value(&ticket.marker)
        .map_err(|error| CompletionError::Rejected(error.to_string()))?;
    response["receipt_published_ms"] = json!(published_ms);
    Ok(())
}

/// Retry only a retained produced receipt. No dispatch, query, capture or UI
/// action runs here; the current owner is checked again before carrying UI data.
pub(super) fn retry_pending(
    state: &SessionBridgeState,
    engine: &AppState,
    window: &str,
) -> Result<(), String> {
    let pending = {
        let publishers = state
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        publishers
            .get(window)
            .and_then(|publisher| publisher.native_control.as_ref())
            .and_then(|admission| {
                admission
                    .pending_receipt
                    .as_ref()
                    .map(|(response, presentation)| {
                        (
                            admission.ticket.clone(),
                            response.clone(),
                            presentation.clone(),
                            admission.query_liveness,
                        )
                    })
            })
    };
    if let Some((ticket, mut response, presentation, query_liveness)) = pending {
        if query_liveness {
            state.refresh_query_liveness(window)?;
        }
        complete(
            state,
            engine,
            &ticket,
            presentation
                .as_ref()
                .map(|(owner, revision)| (owner, *revision)),
            &mut response,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(super) fn has_pending(state: &SessionBridgeState, window: &str) -> Result<bool, String> {
    Ok(state
        .publishers
        .lock()
        .map_err(|_| "Session publisher lock poisoned")?
        .get(window)
        .is_some_and(|publisher| publisher.native_control.is_some()))
}

fn remove_presentation(response: &mut Value) {
    if let Some(fields) = response.as_object_mut() {
        for field in [
            "ui",
            "paper_navigation",
            "paper_image",
            "sketch_geometry",
            "view_state",
            "state",
            "presentation",
            "recipe",
            "native_window",
            "native_layout_revision",
            "native_submitted_revision",
        ] {
            fields.remove(field);
        }
    }
}

pub(super) fn reject_before_start(
    state: &SessionBridgeState,
    engine: &AppState,
    owner: &DocumentContext,
    session_id: &str,
    id: &str,
    reason: &str,
) -> Result<bool, String> {
    let has_admission = state
        .publishers
        .lock()
        .map_err(|_| "Session publisher lock poisoned")?
        .get(&owner.window_id)
        .is_some_and(|publisher| publisher.native_control.is_some());
    if !has_admission {
        return Ok(false);
    }
    let ticket = ticket_for(state, owner, session_id, id)?;
    complete(
        state,
        engine,
        &ticket,
        None,
        &mut json!({
            "request_id":id,"session_id":session_id,"status":"failed", "error":reason,
            "mutation_applied":false,
        }),
    )
    .map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests;
