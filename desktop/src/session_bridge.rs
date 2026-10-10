//! Desktop → disk snapshot publisher and UI-owned inbox apply for MCP.
//!
//! Writes `<LIMO_CAD_SESSION_DIR>/<uuid>/{model.json,active-sketch.json?,focus.json,heartbeat.json,closed.json?}`
//! and `<LIMO_CAD_SESSION_DIR>/_ui/processes/<process-instance>.json` for each
//! live desktop process,
//! with atomic temp+rename. MCP `cad_submit` drops `inbox/<seq>.json`; this
//! module applies those ops on the live SketchManager (shared
//! `limo_cad_mcp_mutate` name→engine-method map + solid replay) and then the
//! native publisher emits a new snapshot. MCP never writes model.json
//! (no last-writer-wins).
//!
//! # Authoritative engine revision
//!
//! Per native project-session `engine_revision` is the sole OCC gate for inbox
//! apply. It is advanced atomically with the live engine mutation under the
//! publisher lock (lock order: publisher → engine):
//! - UI and agent edits share the native mutation controller, which checks
//!   document ownership and holds the publisher lock across the engine call.
//!   Successful changes advance `engine_revision` and heartbeat; native
//!   snapshot publication does not advance the revision again.
//! - Successful inbox apply requires `base_generation == engine_revision`,
//!   applies on the live engine, then mutations advance `engine_revision` and
//!   write heartbeat.json. Reads retain that revision; two same-base mutations
//!   therefore cannot both apply.
//! - Conflicting or malformed head inbox entries are dead-lettered to
//!   `inbox/failed/` so the queue cannot wedge forever.
//! - Snapshot publication and heartbeat refresh never advance the engine
//!   revision. Export reservations have a separate sequence for rejecting
//!   older writes; published generations identify the captured engine state.
//! - Every heartbeat carries the last fully written `published_generation`
//!   plus the latest `model_generation` / `active_sketch_generation`.
//!   Keepalives preserve those fences, so MCP never mistakes liveness for a
//!   completed snapshot write.
//! - `reserve` captures `engine_revision`; `write` rejects the snapshot if
//!   the revision advanced between export and write. Publication retries with
//!   a fresh reservation.
//!
//! # Native project-session identity
//!
//! Each window publisher is bound to the active native project-session id
//! (`AppState` tab identity). Bind/create/activate/drop rebind under the
//! publisher lock. Per-project MCP sessions are retained, so an inbox op
//! queued for tab A is never dispatched onto tab B. Apply also rejects a
//! bound/active mismatch so a bypassed transition cannot retarget the op.
//! Replacing a whole document within one tab retires its MCP UUID. The tab id
//! remains stable, while scripts and queued work stay tied to the old document.
//!
//! # Publish reservation identity
//!
//! `reserve` returns `session_id` + `project_session_id` with the generation.
//! `write` must carry that reserved identity and is resolved against the
//! matching per-project publisher — never `active_mut()` at write time. A
//! delayed write from tab A cannot consume tab B's reservation or publish
//! A's model into B's session. Missing or mismatched identity is rejected.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use limo_cad_mcp_mutate::ExecutionKind;

use crate::state::{AppState, BOOTSTRAP_SESSION_ID};

mod native_history;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) mod native_interface;

/// Placeholder key used before the window is bound to a native project tab.
const UNBOUND_PROJECT: &str = "__unbound__";

#[derive(Debug)]
struct ProjectPublisher {
    session_id: String,
    /// Native control ownership follows document incarnation, not the mutable
    /// engine revision or a reusable tab id. Retiring this publisher also
    /// retires every queued native action for its incarnation.
    native_interface_epoch: u64,
    /// File destination lineage survives successful native Undo/Redo, while
    /// external model replacement starts a new lineage. Control epochs still
    /// retire on every replacement, including history restoration.
    native_file_epoch: u64,
    native_history: native_history::SolidHistory,
    /// Monotonic export ticket; independent of model mutations.
    next_export_sequence: u64,
    /// Latest export ticket written, including repeated exports of one revision.
    last_export_sequence: u64,
    /// Latest generation whose snapshot files were fully written. This stays
    /// behind `engine_revision` while a post-mutation export is pending.
    published_generation: u64,
    /// Latest published generation that replaced `model.json`. Active-sketch
    /// snapshots deliberately retain the previous completed model.
    last_model_generation: Option<u64>,
    /// Latest published generation represented by `active-sketch.json`.
    active_sketch_generation: Option<u64>,
    /// Authoritative live-engine revision for inbox OCC (see module docs).
    engine_revision: u64,
    /// Export ticket → `engine_revision` captured at reserve. Write rejects if
    /// the live revision moved during export.
    pending_exports: HashMap<u64, u64>,
}

impl ProjectPublisher {
    fn new() -> Self {
        static NEXT_NATIVE_EPOCH: AtomicU64 = AtomicU64::new(1);
        let epoch = NEXT_NATIVE_EPOCH
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |epoch| {
                epoch.checked_add(1)
            })
            .expect("native document incarnations exhausted");
        Self {
            session_id: Uuid::new_v4().to_string(),
            native_interface_epoch: epoch,
            native_file_epoch: epoch,
            next_export_sequence: 0,
            native_history: native_history::SolidHistory::default(),
            last_export_sequence: 0,
            published_generation: 0,
            last_model_generation: None,
            active_sketch_generation: None,

            engine_revision: 1,
            pending_exports: HashMap::new(),
        }
    }
}

#[derive(Debug)]
struct WindowPublisher {
    /// Native project-session identity this window currently publishes/applies.
    active_project_session_id: Option<String>,
    /// Retained per-tab MCP publishers (inbox + revision). Switching A→B
    /// rebinds the active pointer; A's session stays isolated.
    by_project: HashMap<String, ProjectPublisher>,
    /// Delivered control request -> (source MCP session, expiry). A tab may
    /// close before replying; ownership must outlive the resident project.
    pending_controls: HashMap<String, (String, u64)>,
}

impl WindowPublisher {
    fn new() -> Self {
        Self {
            active_project_session_id: None,
            by_project: HashMap::new(),
            pending_controls: HashMap::new(),
        }
    }

    fn active_key(&self) -> &str {
        self.active_project_session_id
            .as_deref()
            .unwrap_or(UNBOUND_PROJECT)
    }

    fn active_mut(&mut self) -> &mut ProjectPublisher {
        let key = self.active_key().to_string();
        self.by_project
            .entry(key)
            .or_insert_with(ProjectPublisher::new)
    }

    fn rebind_to(&mut self, project_session_id: &str) {
        if self.active_project_session_id.as_deref() == Some(project_session_id) {
            self.by_project
                .entry(project_session_id.to_string())
                .or_insert_with(ProjectPublisher::new);
            return;
        }
        let previous = self.active_key().to_string();

        if previous == UNBOUND_PROJECT || previous == BOOTSTRAP_SESSION_ID {
            if let Some(existing) = self.by_project.remove(&previous) {
                self.by_project
                    .insert(project_session_id.to_string(), existing);
            }
        }
        self.active_project_session_id = Some(project_session_id.to_string());
        self.by_project
            .entry(project_session_id.to_string())
            .or_insert_with(ProjectPublisher::new);
    }

    fn drop_project(&mut self, project_session_id: &str) {
        self.by_project.remove(project_session_id);
    }
}

/// Process-lifetime bridge state. Native document activations retain its generations.
#[derive(Debug)]
pub struct SessionBridgeState {
    publishers: Mutex<HashMap<String, WindowPublisher>>,
    /// Stable for this desktop process; stamped into heartbeats and its lease.
    process_instance_id: String,
    /// Serializes lease snapshots/writes and remembers the last path so normal
    /// shutdown removes exactly this process's lease (including tests that
    /// change `LIMO_CAD_SESSION_DIR` after constructing the state).
    process_lease_path: Mutex<Option<PathBuf>>,
}

impl Default for SessionBridgeState {
    fn default() -> Self {
        let process_instance_id = Uuid::new_v4().to_string();
        let state = Self {
            publishers: Mutex::new(HashMap::new()),
            process_instance_id: process_instance_id.clone(),
            process_lease_path: Mutex::new(None),
        };
        if let Err(error) = state.write_process_instance_file() {
            eprintln!("session bridge could not write process instance: {error}");
        }
        state
    }
}

#[derive(Debug, Deserialize)]
struct PublishPayload {
    focus: String,
    /// Project export is unavailable while a sketch transaction is active.
    /// In that state retain the last completed model.json and still publish
    /// the live sketch snapshot below.
    #[serde(default)]
    model_json: Option<String>,
    /// The normal project export intentionally refuses an active sketch.
    /// Carry its live read-only DTO beside model.json so diagnostics can see
    /// exactly what the user is editing without making the project format
    /// accept half-finished history state.
    #[serde(default)]
    active_sketch_json: Option<String>,
    generation: u64,
    /// MCP session UUID captured at reserve. Required so write cannot target
    /// whichever project is active after a tab switch.
    #[serde(default)]
    session_id: Option<String>,
    /// Native project-session id captured at reserve. When present it must
    /// match the reserved project; write never falls back to the active tab.
    #[serde(default)]
    project_session_id: Option<String>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn session_root() -> PathBuf {
    limo_cad_session_storage::root()
}

fn closed_tombstone_path(session_id: &str) -> PathBuf {
    session_root().join(session_id).join("closed.json")
}

fn write_closed_tombstone(session_id: &str) -> Result<(), String> {
    let dir = session_root().join(session_id);
    limo_cad_session_storage::create_dir_all(&dir)
        .map_err(|error| format!("create session dir: {error}"))?;
    let body = serde_json::to_string_pretty(&json!({
        "closed_ms": now_ms(),
        "session_id": session_id,
    }))
    .map_err(|error| format!("encode closed.json: {error}"))?;
    atomic_write(&closed_tombstone_path(session_id), &body)
}

fn clear_closed_tombstone(session_id: &str) -> Result<(), String> {
    let path = closed_tombstone_path(session_id);
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("remove closed.json: {error}"))?;
    }
    Ok(())
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    limo_cad_session_storage::atomic_write(path, content.as_bytes())
        .map_err(|error| format!("could not publish session state: {error}"))
}

impl SessionBridgeState {
    /// Capture the current native tab's published session before a background
    /// script starts. A stale or switched tab cannot silently retarget the run.
    pub(crate) fn active_script_session(
        &self,
        window_label: &str,
        engine: &AppState,
        expected_document: &str,
        expected_session: &str,
    ) -> Result<String, String> {
        let publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        let document = engine.active_project_session_id();
        if document != expected_document {
            return Err("The active design changed before script playback started".into());
        }
        let publisher = publishers
            .get(window_label)
            .ok_or("Publish the current design before running its script")?;
        if publisher.active_project_session_id.as_deref() != Some(&document) {
            return Err("The active design changed before script playback started".into());
        }
        let project = publisher
            .by_project
            .get(&document)
            .ok_or("The current design has no published script session")?;
        if project.session_id != expected_session {
            return Err("The design was replaced before script playback started".into());
        }
        if project.published_generation != project.engine_revision
            || project.last_model_generation != Some(project.engine_revision)
        {
            return Err("Publish the current completed design before running its script".into());
        }
        Ok(project.session_id.clone())
    }

    #[cfg(test)]
    fn reserve_for_window(&self, window_label: &str) -> Result<serde_json::Value, String> {
        self.reserve_for_window_on_project(window_label, None)
    }

    #[cfg(test)]
    fn reserve_for_window_on_project(
        &self,
        window_label: &str,
        project_session_id: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        let publisher = publishers
            .entry(window_label.to_string())
            .or_insert_with(WindowPublisher::new);
        if let Some(project_session_id) = project_session_id {
            publisher.rebind_to(project_session_id);
        }
        let result = reserve_project_export(publisher, window_label)?;
        drop(publishers);
        let _ = self.write_process_instance_file();
        Ok(result)
    }

    fn write_for_window(
        &self,
        window_label: &str,
        parsed: PublishPayload,
    ) -> Result<serde_json::Value, String> {
        let process_instance_id = self.process_instance_id.clone();
        let _ = self.write_process_instance_file();
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        let publisher = publishers
            .get_mut(window_label)
            .ok_or_else(|| "session publish requires a reserved generation".to_string())?;
        let reserved_session = parsed
            .session_id
            .as_deref()
            .ok_or_else(|| "session write requires reserved session_id".to_string())?;
        let target_key = match parsed.project_session_id.as_deref() {
            Some(project_session_id) => {
                let Some(project) = publisher.by_project.get(project_session_id) else {
                    return Ok(json!({
                        "skipped": true,
                        "reason": "session_identity_mismatch",
                        "session_id": reserved_session,
                        "project_session_id": project_session_id,
                        "generation": parsed.generation,
                        "session_mode": "read_only_snapshot",
                    }));
                };
                if project.session_id != reserved_session {
                    return Ok(json!({
                        "skipped": true,
                        "reason": "session_identity_mismatch",
                        "session_id": reserved_session,
                        "reserved_session_id": project.session_id,
                        "project_session_id": project_session_id,
                        "generation": parsed.generation,
                        "session_mode": "read_only_snapshot",
                    }));
                }
                project_session_id.to_string()
            }
            None => {
                let matches: Vec<String> = publisher
                    .by_project
                    .iter()
                    .filter(|(_, project)| project.session_id == reserved_session)
                    .map(|(key, _)| key.clone())
                    .collect();
                match matches.as_slice() {
                    [key] => key.clone(),
                    [] => {
                        return Ok(json!({
                            "skipped": true,
                            "reason": "session_identity_mismatch",
                            "session_id": reserved_session,
                            "generation": parsed.generation,
                            "session_mode": "read_only_snapshot",
                        }));
                    }
                    _ => {
                        return Err(
                            "session_id matches multiple projects; project_session_id required"
                                .to_string(),
                        );
                    }
                }
            }
        };
        let project_session_id = if target_key == UNBOUND_PROJECT {
            None
        } else {
            Some(target_key.clone())
        };
        let project = publisher
            .by_project
            .get_mut(&target_key)
            .ok_or_else(|| format!("session write project '{target_key}' was not reserved"))?;
        if parsed.generation == 0 || parsed.generation > project.next_export_sequence {
            return Err(format!(
                "session generation {} was not reserved",
                parsed.generation
            ));
        }
        let Some(captured_revision) = project.pending_exports.remove(&parsed.generation) else {
            return Err(format!(
                "session generation {} was not reserved",
                parsed.generation
            ));
        };
        if parsed.generation <= project.last_export_sequence {
            return Ok(json!({
                "skipped": true,
                "reason": "stale_generation",
                "session_id": project.session_id,
                "generation": parsed.generation,
                "last_export_sequence": project.last_export_sequence,
                "project_session_id": project_session_id,
                "session_mode": "read_only_snapshot",
            }));
        }
        if project.engine_revision != captured_revision {
            return Ok(json!({
                "skipped": true,
                "reason": "engine_revision_changed",
                "session_id": project.session_id,
                "generation": parsed.generation,
                "reserved_engine_revision": captured_revision,
                "engine_revision": project.engine_revision,
                "project_session_id": project_session_id,
                "session_mode": "read_only_snapshot",
            }));
        }

        let dir = session_root().join(&project.session_id);
        limo_cad_session_storage::create_dir_all(&dir)
            .map_err(|error| format!("create session dir: {error}"))?;
        let _ = clear_closed_tombstone(&project.session_id);

        let published_generation = captured_revision;
        let model_generation = if parsed.model_json.is_some() {
            Some(published_generation)
        } else {
            project.last_model_generation
        };
        let active_sketch_generation = parsed
            .active_sketch_json
            .as_ref()
            .map(|_| published_generation);

        let focus_body = serde_json::to_string_pretty(&json!({
            "focus": parsed.focus,
            "session_id": project.session_id,
            "window_id": window_label,
            "updated_ms": now_ms(),
            "generation": published_generation,
            "project_session_id": project_session_id,
            "document_id": project_session_id,
            "process_instance_id": process_instance_id,
            "session_mode": "read_only_snapshot",
        }))
        .map_err(|error| format!("encode focus.json: {error}"))?;

        let heartbeat_body = serde_json::to_string_pretty(&json!({
            "interface_version": 1,
            "updated_ms": now_ms(),
            "generation": published_generation,
            "published_generation": published_generation,
            "model_generation": model_generation,
            "active_sketch_generation": active_sketch_generation,
            "session_id": project.session_id,
            "window_id": window_label,
            "project_session_id": project_session_id,
            "document_id": project_session_id,
            "process_instance_id": process_instance_id,
            "session_mode": "read_only_snapshot",
            "kind": "snapshot",
        }))
        .map_err(|error| format!("encode heartbeat.json: {error}"))?;

        if let Some(model_json) = parsed.model_json.as_deref() {
            atomic_write(&dir.join("model.json"), model_json)?;
        }
        let active_sketch_path = dir.join("active-sketch.json");
        if let Some(active_sketch_json) = parsed.active_sketch_json.as_deref() {
            atomic_write(&active_sketch_path, active_sketch_json)?;
        } else if active_sketch_path.exists() {
            fs::remove_file(&active_sketch_path)
                .map_err(|error| format!("remove stale active-sketch.json: {error}"))?;
        }
        atomic_write(&dir.join("focus.json"), &focus_body)?;
        atomic_write(&dir.join("heartbeat.json"), &heartbeat_body)?;

        project.last_export_sequence = parsed.generation;
        project.published_generation = published_generation;
        project.last_model_generation = model_generation;
        project.active_sketch_generation = active_sketch_generation;

        Ok(json!({
            "skipped": false,
            "session_id": project.session_id,
            "window_id": window_label,
            "session_dir": dir.display().to_string(),
            "generation": published_generation,
            "published_generation": published_generation,
            "model_generation": model_generation,
            "active_sketch_generation": active_sketch_generation,
            "engine_revision": project.engine_revision,
            "project_session_id": project_session_id,
            "document_id": project_session_id,
            "session_mode": "read_only_snapshot",
            "writeback": false,
        }))
    }

    fn heartbeat_for_window(&self, window_label: &str) -> Result<serde_json::Value, String> {
        let process_instance_id = self.process_instance_id.clone();
        let _ = self.write_process_instance_file();
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        let Some(publisher) = publishers.get_mut(window_label) else {
            return Ok(json!({
                "skipped": true,
                "reason": "no_window_session",
                "session_mode": "read_only_snapshot",
            }));
        };
        let project_session_id = publisher.active_project_session_id.clone();
        let project = publisher.active_mut();

        let dir = session_root().join(&project.session_id);
        if !dir.is_dir() {
            return Ok(json!({
                "skipped": true,
                "reason": "no_session_dir",
                "session_id": project.session_id,
                "project_session_id": project_session_id,
                "session_mode": "read_only_snapshot",
            }));
        }

        let _ = clear_closed_tombstone(&project.session_id);
        let heartbeat_body = serde_json::to_string_pretty(&json!({
            "interface_version": 1,
            "updated_ms": now_ms(),
            "generation": project.engine_revision,
            "published_generation": project.published_generation,
            "model_generation": project.last_model_generation,
            "active_sketch_generation": project.active_sketch_generation,
            "session_id": project.session_id,
            "window_id": window_label,
            "project_session_id": project_session_id,
            "document_id": project_session_id,
            "process_instance_id": process_instance_id,
            "session_mode": "read_only_snapshot",
            "kind": "heartbeat",
        }))
        .map_err(|error| format!("encode heartbeat.json: {error}"))?;
        atomic_write(&dir.join("heartbeat.json"), &heartbeat_body)?;

        Ok(json!({
            "skipped": false,
            "session_id": project.session_id,
            "window_id": window_label,
            "generation": project.engine_revision,
            "published_generation": project.published_generation,
            "model_generation": project.last_model_generation,
            "active_sketch_generation": project.active_sketch_generation,
            "engine_revision": project.engine_revision,
            "project_session_id": project_session_id,
            "document_id": project_session_id,
            "session_mode": "read_only_snapshot",
            "writeback": false,
        }))
    }
}

fn inbox_dir(session_id: &str) -> PathBuf {
    session_root().join(session_id).join("inbox")
}

fn parse_inbox_seq(name: &str) -> Option<u64> {
    name.strip_suffix(".json")?.parse().ok()
}

fn pending_inbox_seqs(session_id: &str) -> Vec<u64> {
    let mut seqs = Vec::new();
    let Ok(entries) = limo_cad_session_storage::read_dir(inbox_dir(session_id)) else {
        return seqs;
    };
    for entry in entries.flatten() {
        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        if let Some(seq) = parse_inbox_seq(&entry.file_name().to_string_lossy()) {
            seqs.push(seq);
        }
    }
    seqs.sort_unstable();
    seqs
}

#[cfg(test)]
fn read_session_generation(session_id: &str) -> Option<u64> {
    let body = limo_cad_session_storage::read_to_string(
        session_root().join(session_id).join("heartbeat.json"),
    )
    .ok()?;
    let parsed: Value = serde_json::from_str(&body).ok()?;
    parsed.get("generation").and_then(Value::as_u64)
}

fn generation_conflict(session_id: &str, base: u64, current: Option<u64>) -> String {
    serde_json::to_string(&json!({
        "code": "generation_conflict",
        "writeback": false,
        "session_mode": "ui_owned_apply",
        "session_id": session_id,
        "base_generation": base,
        "current_generation": current,
        "hint": "UI moved; cad_refresh then resubmit with the new heartbeat generation",
    }))
    .unwrap_or_else(|_| "generation_conflict".to_string())
}

pub(crate) fn parse_engine_envelope(raw: String) -> Result<Value, String> {
    let envelope: Value =
        serde_json::from_str(&raw).map_err(|error| format!("invalid engine response: {error}"))?;
    if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(envelope.get("value").cloned().unwrap_or(Value::Null))
    } else {
        Err(envelope
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown Limo CAD engine error")
            .to_string())
    }
}

fn write_engine_revision_heartbeat(
    project: &ProjectPublisher,
    window_id: &str,
    project_session_id: Option<&str>,
    process_instance_id: &str,
) -> Result<(), String> {
    write_project_heartbeat(
        project,
        window_id,
        project_session_id,
        process_instance_id,
        "engine_revision",
    )
}

fn write_project_heartbeat(
    project: &ProjectPublisher,
    window_id: &str,
    project_session_id: Option<&str>,
    process_instance_id: &str,
    kind: &str,
) -> Result<(), String> {
    let dir = session_root().join(&project.session_id);
    limo_cad_session_storage::create_dir_all(&dir)
        .map_err(|error| format!("create session dir: {error}"))?;
    let _ = clear_closed_tombstone(&project.session_id);
    let heartbeat_body = serde_json::to_string_pretty(&json!({
        "interface_version": 1,
        "updated_ms": now_ms(),
        "generation": project.engine_revision,
        "published_generation": project.published_generation,
        "model_generation": project.last_model_generation,
        "active_sketch_generation": project.active_sketch_generation,
        "session_id": project.session_id,
        "window_id": window_id,
        "project_session_id": project_session_id,
        "document_id": project_session_id,
        "process_instance_id": process_instance_id,
        "session_mode": "ui_owned_apply",
        "kind": kind,
    }))
    .map_err(|error| format!("encode heartbeat.json: {error}"))?;
    atomic_write(&dir.join("heartbeat.json"), &heartbeat_body)
}

/// Dispatch an MCP mutate onto the live desktop engine using the shared map.
fn dispatch_inbox_on_engine(
    engine: &AppState,
    name: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let spec = limo_cad_mcp_mutate::lookup_mutate(name)
        .ok_or_else(|| format!("unsupported inbox mutate '{name}'"))?;
    let encoded = limo_cad_mcp_mutate::encode_payload(spec.payload, arguments)?;
    let solid = matches!(spec.execution, ExecutionKind::SolidReplay);
    parse_engine_envelope(engine.apply_encoded_mutate(spec.engine_method, &encoded, solid))
}

fn reserve_project_export(
    publisher: &mut WindowPublisher,
    window_label: &str,
) -> Result<Value, String> {
    let project = publisher.active_mut();
    project.next_export_sequence = project
        .next_export_sequence
        .checked_add(1)
        .ok_or("Session generation exhausted")?;
    project
        .pending_exports
        .insert(project.next_export_sequence, project.engine_revision);
    Ok(json!({
        "session_id": project.session_id,
        "window_id": window_label,
        "generation": project.next_export_sequence,
        "engine_revision": project.engine_revision,
        "project_session_id": publisher.active_project_session_id,
        "document_id": publisher.active_project_session_id,
        "session_mode": "read_only_snapshot",
    }))
}

fn is_project_replacement(name: &str) -> bool {
    matches!(name, "cad_new_project" | "cad_load_project_model")
}

/// The native interface and inbox use the same replacement dispatcher and
/// unchanged marker. The caller owns the publisher lock and retirement.
fn dispatch_project_replacement(
    engine: &AppState,
    name: &str,
    arguments: &Value,
) -> (Result<Value, String>, bool) {
    let Some(spec) =
        limo_cad_mcp_mutate::lookup_mutate(name).filter(|_| is_project_replacement(name))
    else {
        return (
            Err("Not a whole-project replacement operation".into()),
            false,
        );
    };
    match limo_cad_mcp_mutate::encode_payload(spec.payload, arguments) {
        Ok(payload) => {
            let raw = if name == "cad_load_project_model" {
                engine.project_load(&payload)
            } else {
                engine.project_new()
            };
            let changed = !project_replacement_is_unchanged(&raw);
            (parse_engine_envelope(raw), changed)
        }
        Err(error) => (Err(error), false),
    }
}

fn archive_inbox_op(session_id: &str, seq: u64) -> Result<(), String> {
    let src = inbox_dir(session_id).join(format!("{seq}.json"));
    let dest_dir = inbox_dir(session_id).join("applied");
    limo_cad_session_storage::create_dir_all(&dest_dir).map_err(|error| error.to_string())?;
    let dest = dest_dir.join(format!("{seq}.json"));
    if fs::rename(&src, &dest).is_err() {
        let body =
            limo_cad_session_storage::read_to_string(&src).map_err(|error| error.to_string())?;
        atomic_write(&dest, &body)?;
        fs::remove_file(&src).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Move a failed/invalid/conflicting op out of the pending queue so later
/// ops are not wedged forever.
fn dead_letter_inbox_op(session_id: &str, seq: u64, error: &str) -> Result<(), String> {
    let src = inbox_dir(session_id).join(format!("{seq}.json"));
    let dest_dir = inbox_dir(session_id).join("failed");
    limo_cad_session_storage::create_dir_all(&dest_dir).map_err(|error| error.to_string())?;
    let original = limo_cad_session_storage::read_to_string(&src).unwrap_or_default();
    let body = match serde_json::from_str::<Value>(&original) {
        Ok(mut parsed) => {
            if let Some(object) = parsed.as_object_mut() {
                object.insert("error".to_string(), Value::String(error.to_string()));
                object.insert("failed_ms".to_string(), json!(now_ms()));
            }
            serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| original.clone())
        }
        Err(_) => serde_json::to_string_pretty(&json!({
            "error": error,
            "failed_ms": now_ms(),
            "raw": original,
        }))
        .map_err(|error| error.to_string())?,
    };
    let dest = dest_dir.join(format!("{seq}.json"));
    atomic_write(&dest, &body)?;
    if src.exists() {
        fs::remove_file(&src).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
fn engine_envelope_ok(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|envelope| envelope.get("ok").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn project_replacement_is_unchanged(raw: &str) -> bool {
    serde_json::from_str::<Value>(raw).is_ok_and(|envelope| {
        envelope["ok"] == false && envelope["data"]["project_load_state"] == "unchanged"
    })
}

/// Caller holds the publisher lock across native replacement and retirement.
/// Delivered control replies remain window-owned; pending exports and inbox
/// work remain attached to the retired UUID rather than the replacement.
fn retire_project_publisher(
    publisher: &mut WindowPublisher,
    window_label: &str,
    document: &str,
    process_instance_id: &str,
    replacement: ProjectPublisher,
) -> String {
    let previous = publisher.active_mut().session_id.clone();
    if let Err(error) = write_closed_tombstone(&previous) {
        eprintln!("session bridge could not retire replaced document: {error}");
    }
    publisher
        .by_project
        .insert(document.to_string(), replacement);
    let project = publisher.active_mut();
    if let Err(error) =
        write_engine_revision_heartbeat(project, window_label, Some(document), process_instance_id)
    {
        eprintln!("session bridge could not publish replacement identity: {error}");
    }
    project.session_id.clone()
}

fn bump_engine_revision(
    project: &mut ProjectPublisher,
    window_id: &str,
    project_session_id: Option<&str>,
    process_instance_id: &str,
) -> Result<(), String> {
    project.engine_revision = project
        .engine_revision
        .checked_add(1)
        .ok_or_else(|| "session engine revision exhausted".to_string())?;
    write_engine_revision_heartbeat(project, window_id, project_session_id, process_instance_id)
}

impl SessionBridgeState {
    #[allow(dead_code)]
    fn session_id_for_window(&self, window_label: &str) -> Result<Option<String>, String> {
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        Ok(publishers
            .get_mut(window_label)
            .map(|publisher| publisher.active_mut().session_id.clone()))
    }

    /// Advance the authoritative engine revision immediately.
    /// Prefer [`Self::run_ui_mutation`] so the bump shares the publisher lock
    /// with the live engine call; this remains for tests and rare callers.
    #[cfg(test)]
    fn note_mutation_for_window(&self, window_label: &str) -> Result<Value, String> {
        let process_instance_id = self.process_instance_id.clone();
        let _ = self.write_process_instance_file();
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        let publisher = publishers
            .entry(window_label.to_string())
            .or_insert_with(WindowPublisher::new);
        let project_session_id = publisher.active_project_session_id.clone();
        let project = publisher.active_mut();
        bump_engine_revision(
            project,
            window_label,
            project_session_id.as_deref(),
            &process_instance_id,
        )?;
        Ok(json!({
            "session_id": project.session_id,
            "window_id": window_label,
            "engine_revision": project.engine_revision,
            "generation": project.engine_revision,
            "project_session_id": project_session_id,
            "document_id": project_session_id,
            "session_mode": "ui_owned_apply",
            "writeback": false,
        }))
    }

    /// Run a live UI engine mutation under the publisher lock and advance
    /// `engine_revision` on success — same critical section inbox apply uses.
    ///
    /// If this window has no publisher yet (MCP session never reserved), the
    /// mutate still runs but revision is not tracked (no inbox race).
    #[cfg(test)]
    pub fn run_ui_mutation(&self, window_label: &str, mutate: impl FnOnce() -> String) -> String {
        let process_instance_id = self.process_instance_id.clone();
        let _ = self.write_process_instance_file();
        let Ok(mut publishers) = self.publishers.lock() else {
            return mutate();
        };
        let Some(publisher) = publishers.get_mut(window_label) else {
            drop(publishers);
            return mutate();
        };
        let project_session_id = publisher.active_project_session_id.clone();
        let result = mutate();
        if engine_envelope_ok(&result) {
            if let Err(error) = bump_engine_revision(
                publisher.active_mut(),
                window_label,
                project_session_id.as_deref(),
                &process_instance_id,
            ) {
                eprintln!("session bridge could not bump engine_revision: {error}");
            }
        }
        result
    }

    /// Open / whole-document replacement is stronger than an ordinary edit:
    /// retire the old MCP identity even when the native tab id stays the same.
    /// The replacement and identity change share the inbox publisher lock.
    /// Only a verified pre-mutation rejection can retain the previous identity.
    #[cfg(test)]
    pub fn run_project_replacement(
        &self,
        window_label: &str,
        engine: &AppState,
        replace: impl FnOnce() -> String,
    ) -> String {
        let Ok(mut publishers) = self.publishers.lock() else {
            return json!({"ok":false,"error":"Session publisher lock poisoned"}).to_string();
        };
        let publisher = publishers
            .entry(window_label.to_string())
            .or_insert_with(WindowPublisher::new);
        let document = engine.active_project_session_id();
        publisher.rebind_to(&document);
        let result = replace();
        if !project_replacement_is_unchanged(&result) {
            retire_project_publisher(
                publisher,
                window_label,
                &document,
                &self.process_instance_id,
                ProjectPublisher::new(),
            );
        }
        drop(publishers);
        let _ = self.write_process_instance_file();
        result
    }

    fn engine_revision_for_window(&self, window_label: &str) -> Result<Option<u64>, String> {
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        Ok(publishers.get_mut(window_label).map(|publisher| {
            publisher
                .by_project
                .get(publisher.active_key())
                .map(|project| project.engine_revision)
                .unwrap_or(0)
        }))
    }

    /// Run a native project-session transition under the publisher lock, then
    /// rebind this window's MCP publisher to the engine's new active identity.
    /// Lock order: publisher → engine (same as inbox apply / UI mutate).
    pub fn with_project_session_transition<R>(
        &self,
        window_label: &str,
        engine: &AppState,
        transition: impl FnOnce() -> R,
    ) -> R {
        let Ok(mut publishers) = self.publishers.lock() else {
            return transition();
        };
        let previous = engine.active_project_session_id();
        let result = transition();
        let next = engine.active_project_session_id();
        let publisher = publishers
            .entry(window_label.to_string())
            .or_insert_with(WindowPublisher::new);
        if previous != next || publisher.active_project_session_id.is_none() {
            publisher.rebind_to(&next);
        }
        drop(publishers);
        let _ = self.write_process_instance_file();
        result
    }

    /// Drop a retained inactive project's MCP publisher (inbox + revision)
    /// and tombstone its on-disk session so it leaves live `windows[]`.
    pub fn drop_bound_project_session(&self, window_label: &str, project_session_id: &str) {
        if let Ok(mut publishers) = self.publishers.lock() {
            if let Some(publisher) = publishers.get_mut(window_label) {
                if let Some(project) = publisher.by_project.get(project_session_id) {
                    let session_id = project.session_id.clone();
                    if let Err(error) = write_closed_tombstone(&session_id) {
                        eprintln!("session bridge could not tombstone closed tab: {error}");
                    }
                }
                publisher.drop_project(project_session_id);
            }
        }
        let _ = self.write_process_instance_file();
    }

    /// Remove a destroyed window from the process lease immediately. Retained
    /// tab publishers are tombstoned so their directories remain recoverable
    /// by explicit UUID but cannot be mistaken for live windows.
    pub fn drop_window(&self, window_label: &str) {
        let removed = self
            .publishers
            .lock()
            .ok()
            .and_then(|mut publishers| publishers.remove(window_label));
        if let Some(publisher) = removed {
            for project in publisher.by_project.values() {
                if let Err(error) = write_closed_tombstone(&project.session_id) {
                    eprintln!(
                        "session bridge could not tombstone destroyed window session: {error}"
                    );
                }
            }
        }
        let _ = self.write_process_instance_file();
    }

    fn write_process_instance_file(&self) -> Result<(), String> {
        let mut remembered_path = self
            .process_lease_path
            .lock()
            .map_err(|_| "session process lease lock poisoned".to_string())?;
        let publishers = self
            .publishers
            .lock()
            .map_err(|_| "session publisher lock poisoned".to_string())?;
        self.write_process_instance_file_locked(&publishers, &mut remembered_path)
    }

    fn write_process_instance_file_locked(
        &self,
        publishers: &HashMap<String, WindowPublisher>,
        remembered_path: &mut Option<PathBuf>,
    ) -> Result<(), String> {
        let mut windows = publishers
            .iter()
            .filter_map(|(window_id, publisher)| {
                let document_id = publisher.active_project_session_id.as_ref()?;
                let project = publisher.by_project.get(document_id)?;
                Some(json!({
                    "window_id": window_id,
                    "active_document_id": document_id,
                    "active_session_id": project.session_id,
                }))
            })
            .collect::<Vec<_>>();
        windows.sort_by(|a, b| {
            a.get("window_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .cmp(b.get("window_id").and_then(Value::as_str).unwrap_or(""))
        });

        let dir = session_root().join("_ui").join("processes");
        limo_cad_session_storage::create_dir_all(&dir)
            .map_err(|error| format!("create process lease dir: {error}"))?;
        let path = dir.join(format!("{}.json", self.process_instance_id));
        if remembered_path
            .as_ref()
            .is_some_and(|previous| previous != &path)
        {
            if let Some(previous) = remembered_path.as_ref() {
                let _ = fs::remove_file(previous);
            }
        }
        let body = serde_json::to_string_pretty(&json!({
            "process_instance_id": self.process_instance_id,
            "pid": std::process::id(),
            "updated_ms": now_ms(),
            "windows": windows,
        }))
        .map_err(|error| format!("encode process lease: {error}"))?;
        atomic_write(&path, &body)?;
        *remembered_path = Some(path);
        Ok(())
    }
}

impl Drop for SessionBridgeState {
    fn drop(&mut self) {
        if let Ok(path) = self.process_lease_path.get_mut() {
            if let Some(path) = path.take() {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// Apply one pending inbox op under the publisher lock so revision check,
/// live engine apply, and revision advance are atomic w.r.t. other applies
/// and UI mutation notes.
#[cfg(test)]
fn apply_one_inbox_op(
    state: &SessionBridgeState,
    window_label: &str,
    engine: &AppState,
) -> Result<Value, String> {
    apply_or_reject_one_inbox_op(state, window_label, engine, None, None)
}

/// Apply a supported whole-project inbox operation while retaining its own
/// receipt, but retire all other queued work and export tickets for that UUID.
/// The caller already holds the publisher lock; calling run_project_replacement
/// here would recursively acquire it and deadlock.
fn apply_project_replacement_inbox(
    publisher: &mut WindowPublisher,
    window_label: &str,
    engine: &AppState,
    process_instance_id: &str,
    request: &Value,
    name: &str,
    seq: u64,
) -> Result<Value, String> {
    let session_id = publisher.active_mut().session_id.clone();
    let document = engine.active_project_session_id();
    let arguments = request.get("arguments").cloned().unwrap_or(json!({}));
    let (outcome, changed) = dispatch_project_replacement(engine, name, &arguments);
    let replacement = changed.then(ProjectPublisher::new);
    let mut receipt = request.clone();
    let mut response = json!({
        "applied": outcome.is_ok(), "seq": seq, "name": name,
        "session_id": session_id, "session_mode": "ui_owned_apply", "writeback": false,
    });
    if let Some(replacement) = &replacement {
        for (key, value) in [
            ("project_replaced", json!(true)),
            ("previous_session_id", json!(session_id)),
            ("active_session_id", json!(replacement.session_id)),
            ("document_id", json!(document)),
        ] {
            receipt[key] = value.clone();
            response[key] = value;
        }
    }
    let receipt_directory = match &outcome {
        Ok(result) => {
            response["result"] = result.clone();
            "applied"
        }
        Err(error) => {
            response["dead_lettered"] = json!(true);
            response["error"] = json!(error);
            receipt["error"] = json!(error);
            receipt["failed_ms"] = json!(now_ms());
            "failed"
        }
    };
    let written = (|| -> Result<(), String> {
        if let Ok(result) = &outcome {
            atomic_write(
                &inbox_dir(&session_id)
                    .join("results")
                    .join(format!("{seq}.json")),
                &result.to_string(),
            )?;
        }

        atomic_write(
            &inbox_dir(&session_id)
                .join(receipt_directory)
                .join(format!("{seq}.json")),
            &receipt.to_string(),
        )?;
        fs::remove_file(inbox_dir(&session_id).join(format!("{seq}.json")))
            .map_err(|error| error.to_string())
    })();
    if let Some(replacement) = replacement {
        retire_project_publisher(
            publisher,
            window_label,
            &document,
            process_instance_id,
            replacement,
        );
    }
    written?;
    let project = publisher.active_mut();
    response["engine_revision"] = json!(project.engine_revision);
    response["pending"] = json!(pending_inbox_seqs(&project.session_id).len());
    Ok(response)
}

#[cfg(test)]
fn apply_or_reject_one_inbox_op(
    state: &SessionBridgeState,
    window_label: &str,
    engine: &AppState,
    reject_reason: Option<&str>,
    expected_owner: Option<(&str, &str)>,
) -> Result<Value, String> {
    apply_or_reject_one_inbox_op_with_presentation_guard(
        state,
        window_label,
        engine,
        reject_reason,
        expected_owner,
        false,
    )
}

#[cfg(test)]
fn apply_or_reject_one_inbox_op_with_presentation_guard(
    state: &SessionBridgeState,
    window_label: &str,
    engine: &AppState,
    reject_reason: Option<&str>,
    expected_owner: Option<(&str, &str)>,
    presentation_editor_active: bool,
) -> Result<Value, String> {
    apply_or_reject_one_inbox_op_with_editor_guards(
        state,
        window_label,
        engine,
        reject_reason,
        expected_owner,
        presentation_editor_active,
        None,
    )
}

fn apply_or_reject_one_inbox_op_with_editor_guards(
    state: &SessionBridgeState,
    window_label: &str,
    engine: &AppState,
    reject_reason: Option<&str>,
    expected_owner: Option<(&str, &str)>,
    presentation_editor_active: bool,
    replacement_reject_reason: Option<&str>,
) -> Result<Value, String> {
    let process_instance_id = state.process_instance_id.clone();
    let _ = state.write_process_instance_file();
    let mut publishers = state
        .publishers
        .lock()
        .map_err(|_| "session publisher lock poisoned".to_string())?;
    let Some(publisher) = publishers.get_mut(window_label) else {
        return Ok(json!({
            "applied": false,
            "reason": "no_window_session",
            "session_mode": "ui_owned_apply",
            "writeback": false,
        }));
    };
    let engine_active = engine.active_project_session_id();

    if let Some((document, session)) = expected_owner {
        if document != engine_active
            || publisher.active_project_session_id.as_deref() != Some(document)
            || publisher
                .by_project
                .get(document)
                .map(|project| project.session_id.as_str())
                != Some(session)
        {
            return Ok(Value::Null);
        }
    }
    match publisher.active_project_session_id.as_deref() {
        Some(bound) if bound != engine_active => {
            return Ok(json!({
                "applied": false,
                "reason": "project_session_mismatch",
                "bound_project_session_id": bound,
                "active_project_session_id": engine_active,
                "session_mode": "ui_owned_apply",
                "writeback": false,
            }));
        }
        None => publisher.rebind_to(&engine_active),
        Some(_) => {}
    }
    let project_session_id = publisher.active_project_session_id.clone();
    let project = publisher.active_mut();
    let session_id = project.session_id.clone();
    let seqs = pending_inbox_seqs(&session_id);
    let Some(seq) = seqs.first().copied() else {
        return Ok(json!({
            "applied": false,
            "reason": "empty",
            "session_id": session_id,
            "session_mode": "ui_owned_apply",
            "writeback": false,
            "pending": 0,
            "engine_revision": project.engine_revision,
        }));
    };
    if let Some(reason) = reject_reason {
        dead_letter_inbox_op(&session_id, seq, reason)?;
        return Ok(json!({
            "applied": false, "dead_lettered": true, "reason": "playback_stopped",
            "seq": seq, "error": reason, "session_id": session_id,
            "session_mode": "ui_owned_apply", "writeback": false,
            "pending": pending_inbox_seqs(&session_id).len(),
            "engine_revision": project.engine_revision,
        }));
    }
    let path = inbox_dir(&session_id).join(format!("{seq}.json"));
    let body = match limo_cad_session_storage::read_to_string(&path) {
        Ok(body) => body,
        Err(error) => {
            let message = format!("read inbox/{seq}.json: {error}");
            dead_letter_inbox_op(&session_id, seq, &message)?;
            return Ok(json!({
                "applied": false,
                "dead_lettered": true,
                "seq": seq,
                "error": message,
                "session_id": session_id,
                "session_mode": "ui_owned_apply",
                "writeback": false,
                "pending": pending_inbox_seqs(&session_id).len(),
                "engine_revision": project.engine_revision,
            }));
        }
    };
    let parsed: Value = match serde_json::from_str(&body) {
        Ok(parsed) => parsed,
        Err(error) => {
            let message = format!("invalid inbox/{seq}.json: {error}");
            dead_letter_inbox_op(&session_id, seq, &message)?;
            return Ok(json!({
                "applied": false,
                "dead_lettered": true,
                "seq": seq,
                "error": message,
                "reason": "malformed",
                "session_id": session_id,
                "session_mode": "ui_owned_apply",
                "writeback": false,
                "pending": pending_inbox_seqs(&session_id).len(),
                "engine_revision": project.engine_revision,
            }));
        }
    };
    let name = match parsed.get("name").and_then(Value::as_str) {
        Some(name) => name.to_string(),
        None => {
            let message = "inbox op missing name".to_string();
            dead_letter_inbox_op(&session_id, seq, &message)?;
            return Ok(json!({
                "applied": false,
                "dead_lettered": true,
                "seq": seq,
                "error": message,
                "reason": "malformed",
                "session_id": session_id,
                "session_mode": "ui_owned_apply",
                "writeback": false,
                "pending": pending_inbox_seqs(&session_id).len(),
                "engine_revision": project.engine_revision,
            }));
        }
    };
    let arguments = parsed.get("arguments").cloned().unwrap_or(json!({}));
    let base_generation = match parsed.get("base_generation").and_then(Value::as_u64) {
        Some(base) => base,
        None => {
            let message = "inbox op missing base_generation".to_string();
            dead_letter_inbox_op(&session_id, seq, &message)?;
            return Ok(json!({
                "applied": false,
                "dead_lettered": true,
                "seq": seq,
                "name": name,
                "error": message,
                "reason": "malformed",
                "session_id": session_id,
                "session_mode": "ui_owned_apply",
                "writeback": false,
                "pending": pending_inbox_seqs(&session_id).len(),
                "engine_revision": project.engine_revision,
            }));
        }
    };
    let stamped_session = parsed
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let stamped_window = parsed
        .get("window_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let session_mismatch = stamped_session.is_some_and(|s| s != session_id);
    let window_mismatch = stamped_window.is_some_and(|w| w != window_label);
    if session_mismatch || window_mismatch {
        let conflict = serde_json::to_string(&json!({
            "code": "session_identity_mismatch",
            "writeback": false,
            "session_mode": "ui_owned_apply",
            "session_id": session_id,
            "window_id": window_label,
            "stamped_session_id": stamped_session,
            "stamped_window_id": stamped_window,
            "hint": "inbox op identity does not match the destination session/window; dead-letter and do not apply",
        }))
        .unwrap_or_else(|_| {
            format!(
                "{{\"code\":\"session_identity_mismatch\",\"writeback\":false,\"session_mode\":\"ui_owned_apply\",\"session_id\":\"{session_id}\"}}"
            )
        });
        dead_letter_inbox_op(&session_id, seq, &conflict)?;
        return Ok(json!({
            "applied": false,
            "dead_lettered": true,
            "seq": seq,
            "name": name,
            "error": conflict,
            "reason": "session_identity_mismatch",
            "session_id": session_id,
            "window_id": window_label,
            "stamped_session_id": stamped_session,
            "stamped_window_id": stamped_window,
            "session_mode": "ui_owned_apply",
            "writeback": false,
            "pending": pending_inbox_seqs(&session_id).len(),
            "engine_revision": project.engine_revision,
        }));
    }
    let current = project.engine_revision;
    if current != base_generation {
        let conflict = generation_conflict(&session_id, base_generation, Some(current));
        dead_letter_inbox_op(&session_id, seq, &conflict)?;
        return Ok(json!({
            "applied": false,
            "dead_lettered": true,
            "seq": seq,
            "name": name,
            "error": conflict,
            "reason": "generation_conflict",
            "base_generation": base_generation,
            "current_generation": current,
            "session_id": session_id,
            "session_mode": "ui_owned_apply",
            "writeback": false,
            "pending": pending_inbox_seqs(&session_id).len(),
            "engine_revision": project.engine_revision,
        }));
    }
    let Some(spec) = limo_cad_mcp_mutate::lookup_mutate(&name) else {
        let error = format!("unsupported inbox mutate '{name}'");
        dead_letter_inbox_op(&session_id, seq, &error)?;
        return Ok(json!({
            "applied": false,
            "dead_lettered": true,
            "seq": seq,
            "name": name,
            "error": error,
            "session_id": session_id,
            "session_mode": "ui_owned_apply",
            "writeback": false,
            "pending": pending_inbox_seqs(&session_id).len(),
            "engine_revision": project.engine_revision,
        }));
    };
    let model_changed = !spec.is_read_only();
    if presentation_editor_active
        && matches!(
            name.as_str(),
            "upsert_named_view"
                | "set_named_views"
                | "rename_named_view"
                | "delete_named_view"
                | "recall_named_view"
                | "clear_named_view"
        )
    {
        let error = "Apply or cancel the source feature, joint, motion or study editor before changing named views";
        dead_letter_inbox_op(&session_id, seq, error)?;
        return Ok(
            json!({"applied":false,"dead_lettered":true,"seq":seq,"name":name,
            "error":error,"reason":"presentation_editor_active","session_id":session_id,
            "session_mode":"ui_owned_apply","writeback":false,
            "pending":pending_inbox_seqs(&session_id).len(),"engine_revision":project.engine_revision}),
        );
    }
    if is_project_replacement(&name) {
        if let Some(error) = replacement_reject_reason {
            dead_letter_inbox_op(&session_id, seq, error)?;
            return Ok(json!({
                "applied":false,"dead_lettered":true,"seq":seq,"name":name,
                "error":error,"reason":"document_editor_draft","session_id":session_id,
                "session_mode":"ui_owned_apply","writeback":false,
                "pending":pending_inbox_seqs(&session_id).len(),"engine_revision":project.engine_revision
            }));
        }
        let result = apply_project_replacement_inbox(
            publisher,
            window_label,
            engine,
            &process_instance_id,
            &parsed,
            &name,
            seq,
        );
        drop(publishers);
        let _ = state.write_process_instance_file();
        return result;
    }
    let outcome = (|| {
        let edit_history = if model_changed {
            let next_revision = project
                .engine_revision
                .checked_add(1)
                .ok_or("Session engine revision exhausted")?;
            let owner = limo_cad_interface::DocumentContext {
                window_id: window_label.to_owned(),
                document_id: engine_active.clone(),
                epoch: project.native_interface_epoch,
            };
            native_interface::prepare_edit_history(engine, project, &owner, next_revision, &name)?
        } else {
            None
        };
        let result = dispatch_inbox_on_engine(engine, &name, &arguments)?;
        if let Some(history) = edit_history {
            project.native_history = history;
        }
        Ok::<_, String>(result)
    })();
    match outcome {
        Ok(result) => {
            if model_changed {
                bump_engine_revision(
                    project,
                    window_label,
                    project_session_id.as_deref(),
                    &process_instance_id,
                )?;
            } else {
                write_project_heartbeat(
                    project,
                    window_label,
                    project_session_id.as_deref(),
                    &process_instance_id,
                    "query_completed",
                )?;
            }
            atomic_write(
                &inbox_dir(&session_id)
                    .join("results")
                    .join(format!("{seq}.json")),
                &result.to_string(),
            )?;
            archive_inbox_op(&session_id, seq)?;
            let mut response = json!({
                "applied": true,
                "model_changed": model_changed,
                "seq": seq,
                "name": name,
                "result": result,
                "session_id": session_id,
                "session_mode": "ui_owned_apply",
                "writeback": false,
                "pending": pending_inbox_seqs(&session_id).len(),
                "engine_revision": project.engine_revision,
            });

            if let Some(progress) = parsed.get("script_progress") {
                response["script_progress"] = progress.clone();
            }
            Ok(response)
        }
        Err(error) => {
            dead_letter_inbox_op(&session_id, seq, &error)?;
            Ok(json!({
                "applied": false,
                "dead_lettered": true,
                "seq": seq,
                "name": name,
                "error": error,
                "session_id": session_id,
                "session_mode": "ui_owned_apply",
                "writeback": false,
                "pending": pending_inbox_seqs(&session_id).len(),
                "engine_revision": project.engine_revision,
            }))
        }
    }
}

fn control_for_window(
    state: &SessionBridgeState,
    window_label: &str,
    engine: &AppState,
    response: Option<Value>,
) -> Result<Value, String> {
    control_for_window_owned(state, window_label, engine, response, None)
}

/// The native worker captures a specific document incarnation and request
/// before leaving the render thread. Validate both before consuming its file.
fn control_for_window_owned(
    state: &SessionBridgeState,
    window_label: &str,
    engine: &AppState,
    response: Option<Value>,
    expected: Option<(&limo_cad_interface::DocumentContext, &str)>,
) -> Result<Value, String> {
    let mut lease_path = state
        .process_lease_path
        .lock()
        .map_err(|_| "session process lease lock poisoned")?;
    let mut publishers = state
        .publishers
        .lock()
        .map_err(|_| "publisher lock poisoned")?;
    let Some(publisher) = publishers.get_mut(window_label) else {
        return Ok(Value::Null);
    };
    if let Some((owner, _)) = expected {
        if owner.window_id != window_label
            || publisher.active_project_session_id.as_deref() != Some(&owner.document_id)
            || engine.active_project_session_id() != owner.document_id
            || !publisher
                .by_project
                .get(&owner.document_id)
                .is_some_and(|project| project.native_interface_epoch == owner.epoch)
        {
            return Err("The control request's document was replaced before it could run".into());
        }
    }
    let session_id = if let Some(response) = response.as_ref() {
        let requested = response
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or("missing response session")?;
        publisher
            .pending_controls
            .get(
                response["request_id"]
                    .as_str()
                    .ok_or("missing request id")?,
            )
            .filter(|(session, expiry)| session == requested && *expiry >= now_ms())
            .map(|(session, _)| session.clone())
            .ok_or("response belongs to another window")?
    } else {
        if publisher.active_project_session_id.as_deref()
            != Some(engine.active_project_session_id().as_str())
        {
            return Ok(Value::Null);
        }
        publisher.active_mut().session_id.clone()
    };
    let dir = session_root().join(&session_id).join("controls");
    if let Some(mut response) = response {
        let id = response
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or("missing request id")?
            .to_owned();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            return Err("invalid request id".into());
        }
        if response.get("session_id").and_then(Value::as_str) != Some(session_id.as_str()) {
            return Err("camera response belongs to an inactive document".into());
        }
        let request = dir.join(format!("{id}.request.json"));
        if !request.is_file() {
            return Err("camera request expired".into());
        }
        response["active_session_id"] = publisher
            .active_project_session_id
            .as_ref()
            .and_then(|id| publisher.by_project.get(id))
            .map(|project| json!(project.session_id))
            .unwrap_or(Value::Null);
        atomic_write(
            &dir.join(format!("{id}.result.json")),
            &response.to_string(),
        )?;
        publisher.pending_controls.remove(&id);
        let _ = fs::remove_file(request);
        return Ok(Value::Null);
    }
    for (path, mut request) in pending_control_requests(&dir) {
        if expected.is_some_and(|(_, id)| request["id"].as_str() != Some(id)) {
            continue;
        }
        request["session_id"] = json!(session_id);
        let generation = publisher.active_mut().engine_revision;
        if let Some(error) = limo_cad_mcp::control_owner_error(
            &request,
            &session_id,
            window_label,
            publisher.active_project_session_id.as_deref(),
            &state.process_instance_id,
            generation,
        ) {
            let id = request["id"].as_str().unwrap();
            atomic_write(
                &dir.join(format!("{id}.result.json")),
                &json!({"status":"failed","error":error}).to_string(),
            )?;
            let _ = fs::remove_file(path);
            continue;
        }
        if let Some(query) = request.get("sketch_query") {
            let method = query.get("method").and_then(Value::as_str).unwrap_or("");
            let payload = query.get("payload").and_then(Value::as_str).unwrap_or("");
            let mut projections = std::collections::VecDeque::new();
            let mut projection_bytes = 0;
            let mut completed =
                |request: &limo_cad_occt::DrawingProjectionRequest,
                 projection: &limo_cad_occt::DrawingProjectionDto| {
                    let value = json!({"request": request, "projection": projection});
                    let bytes = value.to_string().len();
                    projections.push_back((value, bytes));
                    projection_bytes += bytes;

                    while projections.len() > 12
                        || (projection_bytes > 16 * 1024 * 1024 && projections.len() > 1)
                    {
                        projection_bytes -= projections.pop_front().unwrap().1;
                    }
                };
            let supported = limo_cad_mcp_mutate::is_live_engine_query(method)
                || (request.get("owner").is_some()
                    && limo_cad_mcp_mutate::is_routed_engine_query(method));
            let result = if supported {
                if method == "drawing_export" {
                    parse_engine_envelope(engine.drawing_export_observing(payload, &mut completed))
                } else {
                    parse_engine_envelope(engine.engine_call(method, payload))
                }
            } else {
                Err("unsupported live engine query".into())
            };
            let response = match result {
                Ok(value) => json!({"status":"applied","value":value}),
                Err(error) => json!({"status":"failed","error":error}),
            };

            if method == "drawing_projection" && response["status"] == "applied" {
                let request: limo_cad_occt::DrawingProjectionRequest =
                    serde_json::from_str(payload).map_err(|error| {
                        format!("invalid completed projection request: {error}")
                    })?;
                projections.push_back((
                    json!({"request": request, "projection": response["value"]}),
                    0,
                ));
            }
            let handoff = if response["status"] == "applied" && !projections.is_empty() {
                json!({
                    "session_id": session_id,
                    "document_id": publisher.active_project_session_id,
                    "engine_revision": publisher.active_mut().engine_revision,
                    "drawing_projections": projections.into_iter().map(|(value, _)| value).collect::<Vec<_>>(),
                })
            } else {
                Value::Null
            };

            if supported {
                let document_id = publisher.active_project_session_id.clone();
                write_project_heartbeat(
                    publisher.active_mut(),
                    window_label,
                    document_id.as_deref(),
                    &state.process_instance_id,
                    "query_completed",
                )?;
                state.write_process_instance_file_locked(&publishers, &mut lease_path)?;
            }
            let id = request["id"].as_str().unwrap();
            atomic_write(
                &dir.join(format!("{id}.result.json")),
                &response.to_string(),
            )?;
            let _ = fs::remove_file(path);
            return Ok(handoff);
        }
        publisher
            .pending_controls
            .retain(|_, (_, expiry)| *expiry >= now_ms());
        publisher.pending_controls.insert(
            request["id"].as_str().unwrap().to_owned(),
            (session_id.clone(), request["expires_ms"].as_u64().unwrap()),
        );
        return Ok(request);
    }
    Ok(Value::Null)
}

/// Filesystem-only discovery shared by normal control dispatch and native
/// busy rejection. It performs no model operation or publisher lock access.
fn pending_control_requests(dir: &Path) -> Vec<(PathBuf, Value)> {
    let Ok(entries) = limo_cad_session_storage::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".request.json"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let request: Value =
                serde_json::from_str(&limo_cad_session_storage::read_to_string(&path).ok()?)
                    .ok()?;
            let id = request["id"].as_str();
            let valid = id.is_some_and(|id| {
                !id.is_empty()
                    && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
                    && path
                        .file_name()
                        .is_some_and(|name| name == format!("{id}.request.json").as_str())
            });
            if !valid || request["expires_ms"].as_u64().unwrap_or(0) < now_ms() {
                let _ = fs::remove_file(&path);
                return None;
            }
            if dir.join(format!("{}.result.json", id.unwrap())).exists() {
                return None;
            }
            Some((path, request))
        })
        .collect()
}

fn reject_native_control(session: &str, id: &str, code: &str, reason: &str) -> Result<(), String> {
    let dir = session_root().join(session).join("controls");
    if let Some((path, _)) = pending_control_requests(&dir)
        .into_iter()
        .find(|(_, request)| request["id"].as_str() == Some(id))
    {
        atomic_write(
            &dir.join(format!("{id}.result.json")),
            &json!({"status":"failed", "code":code, "mutation_applied":false, "error":reason})
                .to_string(),
        )?;
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn reject_busy_controls(session: &str, except_id: Option<&str>) -> Result<(), String> {
    let dir = session_root().join(session).join("controls");
    for (path, request) in pending_control_requests(&dir) {
        let id = request["id"].as_str().expect("validated control id");
        let view_request = request.get("ui").is_none()
            && request.get("sketch_query").is_none()
            && request["view"].as_str().is_some_and(|view| {
                view == "current" || native_interface::ViewDirection::parse(view).is_ok()
            });
        if except_id == Some(id) || request["ui"]["action"] == "presentation" || view_request {
            continue;
        }
        atomic_write(&dir.join(format!("{id}.result.json")), &json!({
            "status":"failed", "code":"native_busy", "mutation_applied":false,
            "error":"A modeling operation is still running. This request was not applied; retry after it completes."
        }).to_string())?;
        let _ = fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize bridge tests because they share `LIMO_CAD_SESSION_DIR`.
    pub(super) static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct SessionTestEnvironment(Option<std::ffi::OsString>);

    impl Drop for SessionTestEnvironment {
        fn drop(&mut self) {
            if let Some(previous) = &self.0 {
                std::env::set_var("LIMO_CAD_SESSION_DIR", previous);
            } else {
                std::env::remove_var("LIMO_CAD_SESSION_DIR");
            }
        }
    }

    /// For tests owning fresh fixtures with no detached workers or native-global
    /// mutations. Unwind their fixtures, restore the environment under the unit
    /// lock, then propagate the original panic after releasing that lock.
    /// Production mutexes and poison from any other test remain untouched.
    pub(super) fn with_isolated_session_test(test: impl FnOnce()) {
        let serialization = TEST_LOCK.lock().unwrap();
        let environment = SessionTestEnvironment(std::env::var_os("LIMO_CAD_SESSION_DIR"));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
        drop(environment);
        drop(serialization);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    fn reserve(state: &SessionBridgeState, window_label: &str) -> (String, u64) {
        let result = state.reserve_for_window(window_label).unwrap();
        (
            result["session_id"].as_str().unwrap().to_string(),
            result["generation"].as_u64().unwrap(),
        )
    }

    fn payload(session_id: &str, generation: u64, marker: &str) -> PublishPayload {
        PublishPayload {
            focus: "solid".to_string(),
            model_json: Some(format!(r#"{{"version":1,"marker":"{marker}"}}"#)),
            active_sketch_json: None,
            generation,
            session_id: Some(session_id.to_string()),
            project_session_id: None,
        }
    }

    fn payload_on_project(
        session_id: &str,
        project_session_id: &str,
        generation: u64,
        marker: &str,
    ) -> PublishPayload {
        let mut parsed = payload(session_id, generation, marker);
        parsed.project_session_id = Some(project_session_id.to_string());
        parsed
    }

    fn read_process_lease(state: &SessionBridgeState, root: &Path) -> Value {
        let path = root
            .join("_ui")
            .join("processes")
            .join(format!("{}.json", state.process_instance_id));
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn active_tab_close_retains_control_reply_ownership() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-close-control-{}", Uuid::new_v4()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        {
            let state = SessionBridgeState::default();
            let engine = AppState::new();
            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.bind_project_session("tab-a")
            }));
            let (session_a, _) = reserve(&state, "main");
            let controls = dir.join(&session_a).join("controls");
            fs::create_dir_all(&controls).unwrap();
            atomic_write(&controls.join("123-1.request.json"), &json!({"id":"123-1","expires_ms":now_ms()+30_000,"ui":{"action":"click","target":"close-active-tab"}}).to_string()).unwrap();
            let request = control_for_window(&state, "main", &engine, None).unwrap();
            assert_eq!(request["session_id"], session_a);

            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.create_project_session("tab-b")
            }));
            let (session_b, _) = reserve(&state, "main");
            state.drop_bound_project_session("main", "tab-a");
            assert!(!state.publishers.lock().unwrap()["main"]
                .by_project
                .contains_key("tab-a"));
            let response = json!({"request_id":"123-1","session_id":session_a,"status":"applied"});

            let mut forged = response.clone();
            forged["session_id"] = json!(session_b);
            assert!(control_for_window(&state, "main", &engine, Some(forged)).is_err());
            control_for_window(&state, "main", &engine, Some(response.clone())).unwrap();
            let reply: Value = serde_json::from_str(
                &fs::read_to_string(controls.join("123-1.result.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(reply["active_session_id"], session_b);
            assert_eq!(reply["status"], "applied");
            assert!(!controls.join("123-1.request.json").exists());
            assert!(control_for_window(&state, "main", &engine, Some(response)).is_err());
        }
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn broker_owner_fences_reject_controls_before_live_queries_and_keep_valid_queries() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-broker-owner-{}", Uuid::new_v4()));
        let previous = std::env::var_os("LIMO_CAD_SESSION_DIR");
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        {
            let state = SessionBridgeState::default();
            let engine = AppState::new();
            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.bind_project_session("owned-design")
            }));
            let (session, generation) = reserve(&state, "main");
            let owner = json!({"session_id":session,"window_id":"main","document_id":"owned-design",
                "process_instance_id":state.process_instance_id,"base_generation":generation});
            let controls = dir.join(&session).join("controls");
            let original = engine.document_name();
            for (index, field) in [
                "session_id",
                "window_id",
                "document_id",
                "process_instance_id",
                "base_generation",
            ]
            .iter()
            .enumerate()
            {
                let id = format!("100-{index}");
                let mut stale = owner.clone();
                stale[field] = json!("changed");
                atomic_write(
                    &controls.join(format!("{id}.request.json")),
                    &json!({"id":id,
                    "expires_ms":now_ms()+30_000,"owner":stale,
                    "sketch_query":{"method":"document","payload":""}})
                    .to_string(),
                )
                .unwrap();
                assert!(control_for_window(&state, "main", &engine, None)
                    .unwrap()
                    .is_null());
                let rejected: Value = serde_json::from_str(
                    &fs::read_to_string(controls.join(format!("{id}.result.json"))).unwrap(),
                )
                .unwrap();
                assert_eq!(rejected["status"], "failed");
                assert_eq!(engine.document_name(), original);
            }
            atomic_write(
                &controls.join("200-1.request.json"),
                &json!({"id":"200-1",
                "expires_ms":now_ms()+30_000,"owner":owner,
                "sketch_query":{"method":"document","payload":""}})
                .to_string(),
            )
            .unwrap();
            control_for_window(&state, "main", &engine, None).unwrap();
            let accepted: Value = serde_json::from_str(
                &fs::read_to_string(controls.join("200-1.result.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(accepted["status"], "applied");
            assert_eq!(accepted["value"]["name"], original);
        }
        if let Some(previous) = previous {
            std::env::set_var("LIMO_CAD_SESSION_DIR", previous);
        } else {
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn completed_drawing_query_handoff_preserves_receipt_and_native_owner() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-drawing-handoff-{}", Uuid::new_v4()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        {
            let state = SessionBridgeState::default();
            let engine = AppState::new();
            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.bind_project_session("drawing-a")
            }));
            dispatch_inbox_on_engine(
                &engine,
                "sketch_begin",
                &json!({"name":"Projection fixture","plane":{"type":"origin_plane","plane":"xy"}}),
            )
            .unwrap();
            dispatch_inbox_on_engine(&engine,"sketch_add_rectangle",&json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":10.},"ctrl_held":false})).unwrap();
            dispatch_inbox_on_engine(&engine, "sketch_finish", &json!({})).unwrap();
            let solid=dispatch_inbox_on_engine(&engine,"solid_extrude",&json!({"sketch_name":"Projection fixture","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":5.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]})).unwrap();
            let body = solid["scene"]["bodies"][0]["id"].clone();
            let reserved = state
                .reserve_for_window_on_project("main", Some("drawing-a"))
                .unwrap();
            let session = reserved["session_id"].as_str().unwrap();
            let controls = dir.join(session).join("controls");
            fs::create_dir_all(&controls).unwrap();
            let request = json!({"body_ids":[body],"direction":[0.,0.,1.],"up":[0.,1.,0.]});
            let write_query = |id: &str, method: &str, payload: &str| {
                atomic_write(
                    &controls.join(format!("{id}.request.json")),
                    &json!({"id":id,"expires_ms":now_ms()+30_000,
                    "session_id":"forged", "sketch_query":{"method":method,"payload":payload}})
                    .to_string(),
                )
                .unwrap();
            };
            write_query("100-1", "drawing_projection", &request.to_string());
            let handoff = control_for_window(&state, "main", &engine, None).unwrap();
            assert_eq!(handoff["session_id"], session);
            assert_eq!(handoff["document_id"], "drawing-a");
            assert_eq!(handoff["engine_revision"], reserved["engine_revision"]);
            assert_eq!(handoff["drawing_projections"].as_array().unwrap().len(), 1);
            assert_eq!(
                handoff["drawing_projections"][0]["request"]["deflection"],
                0.05
            );
            assert_eq!(
                handoff["drawing_projections"][0]["request"]["include_hidden"],
                false
            );
            assert!(!handoff["drawing_projections"][0]["projection"]["visible"]
                .as_array()
                .unwrap()
                .is_empty());
            let receipt: Value = serde_json::from_str(
                &fs::read_to_string(controls.join("100-1.result.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(receipt["status"], "applied");
            assert_eq!(
                receipt["value"],
                handoff["drawing_projections"][0]["projection"]
            );
            assert!(!controls.join("100-1.request.json").exists());
            assert!(control_for_window(&state, "main", &engine, None)
                .unwrap()
                .is_null());
            assert!(state.publishers.lock().unwrap()["main"]
                .pending_controls
                .is_empty());
            write_query("100-2", "drawing_projection", "{}");
            assert!(control_for_window(&state, "main", &engine, None)
                .unwrap()
                .is_null());
            let failed: Value = serde_json::from_str(
                &fs::read_to_string(controls.join("100-2.result.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(failed["status"], "failed");

            dispatch_inbox_on_engine(
                &engine,
                "drawing_create_sheet",
                &json!({"name":"Export fixture","format":"a4","orientation":"landscape"}),
            )
            .unwrap();
            dispatch_inbox_on_engine(&engine, "drawing_add_view", &json!({"sheet_id":1,"view":{
                "name":"Top","kind":"top","body_ids":[body],"direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[90.,65.],"scale":2.
            }})).unwrap();
            let shape = &solid["scene"]["bodies"][0];
            let edge = shape["edges"]
                .as_array()
                .unwrap()
                .iter()
                .find(|edge| {
                    let points = edge["points"].as_array().unwrap();
                    let (a, b) = (points.first().unwrap(), points.last().unwrap());
                    a["x"] != b["x"] && a["y"] == b["y"] && a["z"] == b["z"]
                })
                .unwrap();
            let anchor = |endpoint| {
                json!({"body_id":body,"edge_id":edge["id"],"edge_key":edge["key"],
                "topology_signature":handoff["drawing_projections"][0]["projection"]["topology_signatures"][body.to_string()],"endpoint":endpoint,"fallback_point":[999.,999.,999.]})
            };
            dispatch_inbox_on_engine(&engine, "drawing_add_view", &json!({"sheet_id":1,"view":{
                "name":"Section","kind":"section","body_ids":[body],"direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[160.,100.],"scale":0.5,
                "derivation":{"type":"section","parent_view_id":1,"first":anchor("start"),"second":anchor("end"),"label":"A-A","hatch_angle_deg":45.,"hatch_spacing_mm":2.}
            }})).unwrap();
            for (id, format) in [("100-3", "svg"), ("100-4", "dxf")] {
                let payload = json!({"sheet_id":1,"format":format}).to_string();
                write_query(id, "drawing_export", &payload);
                let export_handoff = control_for_window(&state, "main", &engine, None).unwrap();
                assert_eq!(export_handoff["session_id"], session);
                assert_eq!(export_handoff["document_id"], "drawing-a");
                assert_eq!(
                    export_handoff["engine_revision"],
                    reserved["engine_revision"]
                );
                let projected = export_handoff["drawing_projections"].as_array().unwrap();
                assert_eq!(projected.len(), 2);
                assert_eq!(projected[0]["request"]["deflection"], 0.005);
                assert_eq!(projected[1]["request"]["deflection"], 0.02);
                assert_ne!(
                    projected[1]["request"]["section_plane"]["point"],
                    json!([999., 999., 999.])
                );
                assert!(projected[1]["request"]["section_plane"].is_object());
                for view in projected {
                    assert_eq!(
                        view["projection"],
                        parse_engine_envelope(
                            engine.drawing_projection(&view["request"].to_string())
                        )
                        .unwrap()
                    );
                }
                let receipt: Value = serde_json::from_str(
                    &fs::read_to_string(controls.join(format!("{id}.result.json"))).unwrap(),
                )
                .unwrap();
                assert_eq!(receipt["status"], "applied");
                assert_eq!(
                    receipt["value"],
                    parse_engine_envelope(engine.drawing_export(&payload)).unwrap()
                );
                assert!(receipt["value"].get("drawing_projections").is_none());
                assert!(!controls.join(format!("{id}.request.json")).exists());
                assert!(control_for_window(&state, "main", &engine, None)
                    .unwrap()
                    .is_null());
                assert!(state.publishers.lock().unwrap()["main"]
                    .pending_controls
                    .is_empty());
            }
            write_query(
                "100-5",
                "drawing_export",
                "{\"sheet_id\":999,\"format\":\"svg\"}",
            );
            assert!(control_for_window(&state, "main", &engine, None)
                .unwrap()
                .is_null());
            let failed: Value = serde_json::from_str(
                &fs::read_to_string(controls.join("100-5.result.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(failed["status"], "failed");
            assert_eq!(
                state.engine_revision_for_window("main").unwrap(),
                reserved["engine_revision"].as_u64()
            );
        }
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn completed_native_query_refreshes_liveness_before_next_mcp_call() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("limo-cad-query-completion-{}", Uuid::new_v4()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        {
            let state = SessionBridgeState::default();
            let engine = AppState::new();
            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.bind_project_session("query-a")
            }));
            let reservation = state
                .reserve_for_window_on_project("main", Some("query-a"))
                .unwrap();
            let session = reservation["session_id"].as_str().unwrap().to_owned();
            let mut published = payload_on_project(
                &session,
                "query-a",
                reservation["generation"].as_u64().unwrap(),
                "unused",
            );
            published.model_json = Some(
                parse_engine_envelope(engine.engine_call("project_export_model", ""))
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
            state.write_for_window("main", published).unwrap();
            let heartbeat_path = dir.join(&session).join("heartbeat.json");
            let lease_path = dir
                .join("_ui/processes")
                .join(format!("{}.json", state.process_instance_id));
            let original_model = fs::read(dir.join(&session).join("model.json")).unwrap();
            let original: Value =
                serde_json::from_str(&fs::read_to_string(&heartbeat_path).unwrap()).unwrap();
            for failing_query in [false, true] {
                let next = if failing_query {
                    json!({"id":"failed_projection","call":{"group":"drawing/views","operation":"drawing_projection",
                    "arguments":{"body_ids":[999],"direction":[0.,0.,1.],"up":[0.,1.,0.]}}})
                } else {
                    json!({"id":"next","call":{"group":"sketch/draw","operation":"sketch_active","arguments":{}}})
                };
                let source = json!({"version":1,"name":"Query completion liveness","steps":[
                {"id":"first","call":{"group":"sketch/draw","operation":"sketch_active","arguments":{}}},
                next
            ]}).to_string();
                let attached = session.clone();
                let runner = std::thread::spawn(move || {
                    limo_cad_mcp::run_script(&source, None, Some(&attached), "fast", 1.)
                });
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
                let mut completed_queries = 0;
                let mut commands = Vec::new();
                while !runner.is_finished() && std::time::Instant::now() < deadline {
                    let pending = fs::read_dir(dir.join(&session).join("controls"))
                        .ok()
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                        .filter(|entry| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .ends_with(".request.json")
                        })
                        .find_map(|entry| {
                            serde_json::from_str::<Value>(&fs::read_to_string(entry.path()).ok()?)
                                .ok()
                        });
                    if pending.is_none() {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    let querying = pending
                        .as_ref()
                        .is_some_and(|request| request.get("sketch_query").is_some());
                    if querying {
                        for path in [&heartbeat_path, &lease_path] {
                            let mut stale: Value =
                                serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
                            stale["updated_ms"] = json!(now_ms().saturating_sub(120_000));
                            atomic_write(path, &stale.to_string()).unwrap();
                        }
                    }
                    let started = now_ms();
                    let request = control_for_window(&state, "main", &engine, None).unwrap();
                    if querying {
                        completed_queries += 1;
                        let heartbeat: Value =
                            serde_json::from_str(&fs::read_to_string(&heartbeat_path).unwrap())
                                .unwrap();
                        assert!(heartbeat["updated_ms"].as_u64().unwrap() >= started);
                        assert!(
                            read_process_lease(&state, &dir)["updated_ms"]
                                .as_u64()
                                .unwrap()
                                >= started
                        );
                        for field in [
                            "generation",
                            "published_generation",
                            "model_generation",
                            "active_sketch_generation",
                            "session_id",
                            "document_id",
                        ] {
                            assert_eq!(
                                heartbeat[field], original[field],
                                "query completion preserves {field}"
                            );
                        }
                        assert_eq!(heartbeat["kind"], "query_completed");
                    }
                    if !request.is_null() {
                        assert!(request.get("ui").is_some());
                        commands.push(request["ui"]["command"].as_str().unwrap().to_owned());
                        control_for_window(
                        &state,
                        "main",
                        &engine,
                        Some(
                            json!({"request_id":request["id"],"session_id":request["session_id"],
                        "status":"applied","presentation":{"wait_ms":0}}),
                        ),
                    )
                    .unwrap();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert!(
                    runner.is_finished(),
                    "actual MCP query sequence did not complete within its bounded test deadline"
                );
                let result = runner.join().unwrap();
                if failing_query {
                    let error = result.unwrap_err();
                    assert!(
                        error.contains("failed_projection") && !error.contains("heartbeat"),
                        "preserve original native failure: {error}"
                    );
                    assert_eq!(
                        commands.last().map(String::as_str),
                        Some("stop"),
                        "failed native query must still permit the actual MCP Stop acknowledgment"
                    );
                } else {
                    let report = result.unwrap();
                    assert_eq!(report["steps_completed"], 2, "{report}");
                    assert_eq!(report["checks_completed"], 0, "{report}");
                    assert_eq!(commands.last().map(String::as_str), Some("finish"));
                }
                assert!(
                    completed_queries >= 3,
                    "blank precondition and both actual script queries ran"
                );
                assert_eq!(
                    fs::read(dir.join(&session).join("model.json")).unwrap(),
                    original_model
                );
            }
        }
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn process_lease_tracks_active_document_and_is_removed_on_drop() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-lease-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let lease_path;
        {
            let state = SessionBridgeState::default();
            let engine = AppState::new();
            lease_path = dir
                .join("_ui")
                .join("processes")
                .join(format!("{}.json", state.process_instance_id));
            assert!(lease_path.exists());
            assert!(read_process_lease(&state, &dir)["windows"]
                .as_array()
                .unwrap()
                .is_empty());

            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.bind_project_session("tab-a")
            }));
            let reserved_a = state
                .reserve_for_window_on_project("main", Some("tab-a"))
                .unwrap();
            let lease_a = read_process_lease(&state, &dir);
            assert_eq!(lease_a["windows"][0]["active_document_id"], "tab-a");
            assert_eq!(
                lease_a["windows"][0]["active_session_id"],
                reserved_a["session_id"]
            );

            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.create_project_session("tab-b")
            }));
            let reserved_b = state
                .reserve_for_window_on_project("main", Some("tab-b"))
                .unwrap();
            let lease_b = read_process_lease(&state, &dir);
            assert_eq!(lease_b["windows"][0]["active_document_id"], "tab-b");
            assert_eq!(
                lease_b["windows"][0]["active_session_id"],
                reserved_b["session_id"]
            );

            state.drop_window("main");
            assert!(read_process_lease(&state, &dir)["windows"]
                .as_array()
                .unwrap()
                .is_empty());
            assert!(dir
                .join(reserved_a["session_id"].as_str().unwrap())
                .join("closed.json")
                .exists());
            assert!(dir
                .join(reserved_b["session_id"].as_str().unwrap())
                .join("closed.json")
                .exists());
        }
        assert!(
            !lease_path.exists(),
            "normal process shutdown must remove its own lease"
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn older_reserved_publish_cannot_overwrite_newer_snapshot() {
        let _test = TEST_LOCK.lock().unwrap();
        let state = SessionBridgeState::default();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-test-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);

        let (session_id, older) = reserve(&state, "main");
        let (_, newer) = reserve(&state, "main");
        assert_eq!(older, 1);
        assert_eq!(newer, 2);
        let applied = state
            .write_for_window("main", payload(&session_id, newer, "newer"))
            .unwrap();
        assert_eq!(applied["skipped"], false);
        let stale = state
            .write_for_window("main", payload(&session_id, older, "older"))
            .unwrap();
        assert_eq!(stale["skipped"], true);
        assert_eq!(stale["reason"], "stale_generation");
        let model = fs::read_to_string(dir.join(session_id).join("model.json")).unwrap();
        assert!(model.contains("\"marker\":\"newer\""));

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn repeated_reservation_continues_backend_generation() {
        let _test = TEST_LOCK.lock().unwrap();
        let state = SessionBridgeState::default();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-reload-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);

        let (session_id, first) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, first, "before-reload"))
            .unwrap();
        let (same_session_id, after_reload) = reserve(&state, "main");
        assert_eq!(same_session_id, session_id);
        assert_eq!(after_reload, first + 1);
        let applied = state
            .write_for_window("main", payload(&session_id, after_reload, "after-reload"))
            .unwrap();
        assert_eq!(applied["skipped"], false);
        let model = fs::read_to_string(dir.join(session_id).join("model.json")).unwrap();
        assert!(model.contains("\"marker\":\"after-reload\""));

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn published_heartbeat_carries_stable_window_and_document_ids() {
        let _test = TEST_LOCK.lock().unwrap();
        let state = SessionBridgeState::default();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-window-id-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);

        let reserved = state
            .reserve_for_window_on_project("main", Some("tab-doc-a"))
            .unwrap();
        assert_eq!(reserved["window_id"], "main");
        assert_eq!(reserved["document_id"], "tab-doc-a");
        let session_id = reserved["session_id"].as_str().unwrap().to_string();
        let generation = reserved["generation"].as_u64().unwrap();
        let applied = state
            .write_for_window(
                "main",
                payload_on_project(&session_id, "tab-doc-a", generation, "main-doc"),
            )
            .unwrap();
        assert_eq!(applied["skipped"], false);
        assert_eq!(applied["window_id"], "main");
        assert_eq!(applied["document_id"], "tab-doc-a");
        let beat = fs::read_to_string(dir.join(&session_id).join("heartbeat.json")).unwrap();
        assert!(
            beat.contains("\"window_id\": \"main\"") || beat.contains("\"window_id\":\"main\"")
        );
        assert!(
            beat.contains("\"document_id\": \"tab-doc-a\"")
                || beat.contains("\"document_id\":\"tab-doc-a\"")
        );
        let focus = fs::read_to_string(dir.join(&session_id).join("focus.json")).unwrap();
        assert!(
            focus.contains("\"window_id\": \"main\"") || focus.contains("\"window_id\":\"main\"")
        );

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_have_independent_sessions_and_generations() {
        let _test = TEST_LOCK.lock().unwrap();
        let state = SessionBridgeState::default();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-windows-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);

        let (main_session, main_generation) = reserve(&state, "main");
        let (second_session, second_generation) = reserve(&state, "secondary");
        assert_ne!(main_session, second_session);
        assert_eq!(main_generation, 1);
        assert_eq!(second_generation, 1);
        assert_eq!(main_session.as_bytes()[14], b'4');
        assert_eq!(second_session.as_bytes()[14], b'4');

        state
            .write_for_window("main", payload(&main_session, main_generation, "main"))
            .unwrap();
        state
            .write_for_window(
                "secondary",
                payload(&second_session, second_generation, "secondary"),
            )
            .unwrap();
        let main_model = fs::read_to_string(dir.join(main_session).join("model.json")).unwrap();
        let second_model = fs::read_to_string(dir.join(second_session).join("model.json")).unwrap();
        assert!(main_model.contains("\"marker\":\"main\""));
        assert!(second_model.contains("\"marker\":\"secondary\""));

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_script_session_requires_the_current_published_document() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-script-session-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        assert!(state
            .active_script_session("main", &engine, "script-tab", "missing")
            .is_err());
        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("script-tab")
        }));
        let (session, export) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session, export, "blank"))
            .unwrap();
        assert_eq!(
            state
                .active_script_session("main", &engine, "script-tab", &session)
                .unwrap(),
            session
        );
        envelope_ok(&state.run_ui_mutation("main", || {
            engine.engine_call("document_set_name", r#""Changed""#)
        }));
        assert!(state
            .active_script_session("main", &engine, "script-tab", &session)
            .is_err());
        let (_, export) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session, export, "changed"))
            .unwrap();
        assert_eq!(
            state
                .active_script_session("main", &engine, "script-tab", &session)
                .unwrap(),
            session
        );
        envelope_ok(&engine.create_project_session("other-tab"));
        assert!(state
            .active_script_session("main", &engine, "script-tab", &session)
            .is_err());
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_project_replacement_retires_queued_work_and_preserves_its_receipt() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("limo-cad-inbox-replacement-{}", Uuid::new_v4()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        {
            for name in ["cad_load_project_model", "cad_new_project"] {
                let state = SessionBridgeState::default();
                let engine = AppState::new();
                envelope_ok(&state.with_project_session_transition("main", &engine, || {
                    engine.bind_project_session("same-tab")
                }));
                envelope_ok(&engine.engine_call("document_set_name", r#""Original""#));
                let original_model =
                    parse_engine_envelope(engine.engine_call("project_export_model", "")).unwrap();
                let (original, ticket) = reserve(&state, "main");
                let mut initial = payload(&original, ticket, "original");
                initial.model_json = Some(original_model.as_str().unwrap().to_string());
                state.write_for_window("main", initial).unwrap();
                let (_, stale_ticket) = reserve(&state, "main");

                for (index, arguments) in [
                    json!({}),
                    json!({"model_json":17}),
                    json!({"model_json":"not JSON"}),
                    json!({"model_json":r#"{"format":"limo-cad-project","schema_version":9999}"#}),
                ]
                .into_iter()
                .enumerate()
                {
                    let seq = index as u64 + 1;
                    write_inbox(&original, seq, "cad_load_project_model", 1, arguments);
                    let rejected = apply_or_reject_one_inbox_op(
                        &state,
                        "main",
                        &engine,
                        None,
                        Some(("same-tab", &original)),
                    )
                    .unwrap();
                    assert_eq!(rejected["applied"], false);
                    assert_eq!(rejected["dead_lettered"], true);
                    assert!(rejected.get("project_replaced").is_none());
                    assert_eq!(reserve(&state, "main").0, original);
                    assert!(!closed_tombstone_path(&original).exists());
                    assert_eq!(
                        parse_engine_envelope(engine.engine_call("project_export_model", ""))
                            .unwrap(),
                        original_model
                    );
                }

                let donor = AppState::new();
                envelope_ok(&donor.engine_call("document_set_name", r#""Replacement""#));
                let model =
                    parse_engine_envelope(donor.engine_call("project_export_model", "")).unwrap();
                let arguments = if name == "cad_load_project_model" {
                    json!({"model_json":model})
                } else {
                    json!({})
                };
                write_inbox(&original, 5, name, 1, arguments);
                write_inbox(
                    &original,
                    6,
                    "cad_set_document_name",
                    2,
                    json!({"name":"Stale queued edit"}),
                );
                let applied = apply_or_reject_one_inbox_op(
                    &state,
                    "main",
                    &engine,
                    None,
                    Some(("same-tab", &original)),
                )
                .unwrap();
                assert_eq!(applied["applied"], true);
                assert_eq!(applied["project_replaced"], true);
                assert_eq!(applied["session_id"], original);
                assert_eq!(applied["previous_session_id"], original);
                assert_eq!(applied["document_id"], "same-tab");
                let current = applied["active_session_id"].as_str().unwrap();
                assert_ne!(current, original);
                assert_eq!(applied["engine_revision"], 1);
                assert_eq!(
                    engine.document_snapshot().name,
                    if name == "cad_load_project_model" {
                        "Replacement"
                    } else {
                        "Untitled"
                    }
                );
                assert!(closed_tombstone_path(&original).exists());
                let receipt: Value = serde_json::from_str(
                    &fs::read_to_string(inbox_dir(&original).join("applied/5.json")).unwrap(),
                )
                .unwrap();
                for key in [
                    "project_replaced",
                    "previous_session_id",
                    "active_session_id",
                    "document_id",
                ] {
                    assert_eq!(
                        receipt[key], applied[key],
                        "the initiating receipt must retain {key}"
                    );
                }
                assert_eq!(receipt["name"], name);
                assert_eq!(receipt["base_generation"], 1);
                let result: Value = serde_json::from_str(
                    &fs::read_to_string(inbox_dir(&original).join("results/5.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(result, applied["result"]);
                assert_eq!(
                    fs::read_to_string(dir.join(&original).join("model.json")).unwrap(),
                    original_model.as_str().unwrap()
                );
                assert_eq!(pending_inbox_seqs(&original), vec![6]);
                assert!(apply_or_reject_one_inbox_op(
                    &state,
                    "main",
                    &engine,
                    None,
                    Some(("same-tab", &original))
                )
                .unwrap()
                .is_null());
                assert_eq!(
                    apply_one_inbox_op(&state, "main", &engine).unwrap()["applied"],
                    false
                );
                assert_eq!(pending_inbox_seqs(&original), vec![6]);
                assert_eq!(
                    state
                        .write_for_window("main", payload(&original, stale_ticket, "stale"))
                        .unwrap()["skipped"],
                    true
                );
                assert!(state
                    .active_script_session("main", &engine, "same-tab", current)
                    .is_err());
                let (published, ticket) = reserve(&state, "main");
                assert_eq!(published, current);
                let mut publication = payload(current, ticket, "replacement");
                publication.model_json = Some(
                    parse_engine_envelope(engine.engine_call("project_export_model", ""))
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string(),
                );
                state.write_for_window("main", publication).unwrap();
                assert_eq!(
                    state
                        .active_script_session("main", &engine, "same-tab", current)
                        .unwrap(),
                    current
                );
                assert_eq!(
                    read_process_lease(&state, &dir)["windows"][0]["active_session_id"],
                    current
                );
            }
        }
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_tab_document_replacement_retires_script_and_queued_work_ownership() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-replacement-{}", Uuid::new_v4()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        {
            let state = SessionBridgeState::default();
            let engine = AppState::new();
            envelope_ok(&state.with_project_session_transition("main", &engine, || {
                engine.bind_project_session("same-tab")
            }));
            let (original, ticket) = reserve(&state, "main");
            state
                .write_for_window("main", payload(&original, ticket, "original"))
                .unwrap();
            let original_model = engine.engine_call("project_export_model", "");
            let rejected = state.run_project_replacement("main", &engine, || {
                engine.project_load(
                    &json!(r#"{"format":"limo-cad-project","schema_version":9999}"#).to_string(),
                )
            });
            let rejected: Value = serde_json::from_str(&rejected).unwrap();
            assert_eq!(rejected["ok"], false);
            assert_eq!(rejected["data"]["project_load_state"], "unchanged");
            assert_eq!(
                engine.engine_call("project_export_model", ""),
                original_model
            );
            assert_eq!(
                state
                    .active_script_session("main", &engine, "same-tab", &original)
                    .unwrap(),
                original
            );
            assert!(!closed_tombstone_path(&original).exists());

            let controls = dir.join(&original).join("controls");
            fs::create_dir_all(&controls).unwrap();
            for id in ["123-1", "123-2"] {
                atomic_write(&controls.join(format!("{id}.request.json")), &json!({
                    "id":id,"expires_ms":now_ms()+30_000,"file":{"command":"open","path":"replacement.limo"}
                }).to_string()).unwrap();
            }
            assert_eq!(
                control_for_window(&state, "main", &engine, None).unwrap()["id"],
                "123-1"
            );
            let (_, stale_ticket) = reserve(&state, "main");
            write_inbox(
                &original,
                1,
                "cad_set_document_name",
                1,
                json!({"name":"Stale script"}),
            );

            let replacement = AppState::new();
            envelope_ok(&replacement.engine_call("document_set_name", r#""Replacement""#));
            let model =
                parse_engine_envelope(replacement.engine_call("project_export_model", "")).unwrap();
            envelope_ok(&state.run_project_replacement("main", &engine, || {
                engine.project_load(&model.to_string())
            }));
            let (current, fresh_ticket) = reserve(&state, "main");
            assert_ne!(current, original);
            assert_eq!(engine.active_project_session_id(), "same-tab");
            assert_eq!(engine.document_snapshot().name, "Replacement");
            assert!(closed_tombstone_path(&original).exists());
            assert!(
                state
                    .active_script_session("main", &engine, "same-tab", &current)
                    .is_err(),
                "replacement must first publish its own model"
            );
            assert_eq!(
                state
                    .write_for_window("main", payload(&original, stale_ticket, "stale"))
                    .unwrap()["skipped"],
                true
            );
            assert!(control_for_window(&state, "main", &engine, None)
                .unwrap()
                .is_null());
            assert_eq!(
                apply_one_inbox_op(&state, "main", &engine).unwrap()["applied"],
                false
            );
            assert_eq!(engine.document_snapshot().name, "Replacement");
            assert_eq!(pending_inbox_seqs(&original), vec![1]);

            control_for_window(
                &state,
                "main",
                &engine,
                Some(json!({
                    "request_id":"123-1","session_id":original,"status":"applied"
                })),
            )
            .unwrap();
            let reply: Value = serde_json::from_str(
                &fs::read_to_string(controls.join("123-1.result.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(reply["active_session_id"], current);
            assert!(
                controls.join("123-2.request.json").exists(),
                "queued old controls cannot be dispatched to the replacement"
            );
            state
                .write_for_window("main", payload(&current, fresh_ticket, "replacement"))
                .unwrap();
            assert!(
                state
                    .active_script_session("main", &engine, "same-tab", &original)
                    .is_err(),
                "a fresh heartbeat must not retarget a delayed Run from the original document"
            );
            assert!(state
                .active_script_session("main", &engine, "another-tab", &current)
                .is_err());
            assert_eq!(
                state
                    .active_script_session("main", &engine, "same-tab", &current)
                    .unwrap(),
                current
            );

            write_inbox(
                &current,
                1,
                "cad_set_document_name",
                1,
                json!({"name":"Current edit"}),
            );
            for reason in [None, Some("Playback stopped in original document")] {
                assert!(apply_or_reject_one_inbox_op(
                    &state,
                    "main",
                    &engine,
                    reason,
                    Some(("same-tab", &original))
                )
                .unwrap()
                .is_null());
                assert!(apply_or_reject_one_inbox_op(
                    &state,
                    "main",
                    &engine,
                    reason,
                    Some(("another-tab", &current))
                )
                .unwrap()
                .is_null());
                assert_eq!(pending_inbox_seqs(&current), vec![1]);
                assert_eq!(engine.document_snapshot().name, "Replacement");
            }
            assert_eq!(
                apply_or_reject_one_inbox_op(
                    &state,
                    "main",
                    &engine,
                    None,
                    Some(("same-tab", &current))
                )
                .unwrap()["applied"],
                true
            );
            assert_eq!(engine.document_snapshot().name, "Current edit");

            state.run_project_replacement("main", &engine, || {
                json!({"ok":false,"error":"unknown load failure"}).to_string()
            });
            let (unverified, _) = reserve(&state, "main");
            assert_ne!(unverified, current);
            assert!(closed_tombstone_path(&current).exists());
            envelope_ok(&state.run_project_replacement("main", &engine, || engine.project_new()));
            let (blank, _) = reserve(&state, "main");
            assert_ne!(
                blank, unverified,
                "clearing the last tab is also a document replacement"
            );
            assert_eq!(
                read_process_lease(&state, &dir)["windows"][0]["active_session_id"],
                blank
            );
        }
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn heartbeat_updates_without_touching_model() {
        let _test = TEST_LOCK.lock().unwrap();
        let state = SessionBridgeState::default();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-hb-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);

        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "original"))
            .unwrap();
        let before = fs::read_to_string(dir.join(&session_id).join("model.json")).unwrap();
        let result = state.heartbeat_for_window("main").unwrap();
        assert_eq!(result["skipped"], false);
        assert_eq!(result["generation"], generation);
        let after = fs::read_to_string(dir.join(&session_id).join("model.json")).unwrap();
        assert_eq!(before, after);
        let beat = fs::read_to_string(dir.join(&session_id).join("heartbeat.json")).unwrap();
        let beat: Value = serde_json::from_str(&beat).unwrap();
        assert_eq!(beat["kind"], "heartbeat");
        assert_eq!(beat["generation"], generation);
        assert_eq!(beat["published_generation"], generation);
        assert_eq!(beat["model_generation"], generation);
        assert_eq!(beat["active_sketch_generation"], Value::Null);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_sketch_snapshot_is_published_and_removed_when_editing_ends() {
        let _test = TEST_LOCK.lock().unwrap();
        let state = SessionBridgeState::default();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-sketch-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);

        let (session_id, first) = reserve(&state, "main");
        let mut editing = payload(&session_id, first, "editing");
        editing.model_json = None;
        editing.active_sketch_json = Some(r#"{"name":"Sketch1","entities":[]}"#.to_string());
        state.write_for_window("main", editing).unwrap();
        let sketch_path = dir.join(&session_id).join("active-sketch.json");
        assert!(fs::read_to_string(&sketch_path)
            .unwrap()
            .contains("Sketch1"));
        assert!(
            !dir.join(&session_id).join("model.json").exists(),
            "a live sketch must publish even before the first completed project snapshot"
        );
        let editing_beat: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(&session_id).join("heartbeat.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(editing_beat["kind"], "snapshot");
        assert_eq!(editing_beat["published_generation"], first);
        assert_eq!(editing_beat["model_generation"], Value::Null);
        assert_eq!(editing_beat["active_sketch_generation"], first);

        state.note_mutation_for_window("main").unwrap();
        let (_, second) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, second, "finished"))
            .unwrap();
        assert!(!sketch_path.exists());
        let finished_beat: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(&session_id).join("heartbeat.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(finished_beat["published_generation"], second);
        assert_eq!(finished_beat["model_generation"], second);
        assert_eq!(finished_beat["active_sketch_generation"], Value::Null);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    fn write_inbox(session_id: &str, seq: u64, name: &str, base: u64, arguments: Value) {
        write_inbox_with_identity(session_id, seq, name, base, arguments, (None, None, None));
    }

    fn write_inbox_with_identity(
        session_id: &str,
        seq: u64,
        name: &str,
        base: u64,
        arguments: Value,
        (stamped_session, stamped_window, stamped_document): (
            Option<&str>,
            Option<&str>,
            Option<&str>,
        ),
    ) {
        let inbox = session_root().join(session_id).join("inbox");
        fs::create_dir_all(&inbox).unwrap();
        let mut body = json!({
            "name": name,
            "arguments": arguments,
            "base_generation": base,
        });
        if let Some(object) = body.as_object_mut() {
            if let Some(sid) = stamped_session {
                object.insert("session_id".to_string(), json!(sid));
            }
            if let Some(wid) = stamped_window {
                object.insert("window_id".to_string(), json!(wid));
            }
            if let Some(did) = stamped_document {
                object.insert("document_id".to_string(), json!(did));
            }
        }
        let body = serde_json::to_string_pretty(&body).unwrap();
        fs::write(inbox.join(format!("{seq}.json")), body).unwrap();
    }

    #[test]
    fn stopped_playback_rejects_queued_operation_without_changing_revision() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-stop-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        write_inbox(
            &session_id,
            1,
            "cad_set_document_name",
            generation,
            json!({"name":"Must not apply"}),
        );
        let engine = AppState::new();
        let stopped =
            apply_or_reject_one_inbox_op(&state, "main", &engine, Some("Playback stopped"), None)
                .unwrap();
        assert_eq!(stopped["reason"], "playback_stopped");
        assert_eq!(stopped["engine_revision"], generation);
        assert_eq!(stopped["applied"], false);
        assert!(pending_inbox_seqs(&session_id).is_empty());
        let receipt: Value = serde_json::from_str(
            &fs::read_to_string(session_root().join(&session_id).join("inbox/failed/1.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["error"], "Playback stopped");
        assert_eq!(
            apply_one_inbox_op(&state, "main", &engine).unwrap()["reason"],
            "empty"
        );
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_generation_mismatch_is_dead_lettered_and_unblocks_queue() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-inbox-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        write_inbox(
            &session_id,
            1,
            "cad_set_document_name",
            99,
            json!({"name": "Nope"}),
        );
        write_inbox(
            &session_id,
            2,
            "cad_set_document_name",
            generation,
            json!({"name": "Next"}),
        );
        let engine = AppState::new();
        let dead = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(dead["applied"], false);
        assert_eq!(dead["dead_lettered"], true);
        assert_eq!(dead["reason"], "generation_conflict");
        assert_eq!(dead["seq"], 1);
        assert!(session_root()
            .join(&session_id)
            .join("inbox/failed/1.json")
            .exists());
        assert_eq!(pending_inbox_seqs(&session_id), vec![2]);

        let next = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(next["applied"], true);
        assert_eq!(next["seq"], 2);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn editing_guard_rejects_named_view_recall_before_native_mutation() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("limo-cad-recall-edit-guard-{}", Uuid::new_v4()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("guard-tab")
        }));
        envelope_ok(&engine.engine_call("set_named_views", r#"{"views":[{"name":"review","camera":{"position":[30,40,50],"target":[0,0,0],"up":[0,0,1]},"visible_body_ids":[],"part_offsets":[]}]}"#));
        let before = engine.engine_call("named_views", "");
        let (session, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session, generation, "guarded"))
            .unwrap();
        write_inbox(
            &session,
            1,
            "recall_named_view",
            generation,
            json!({"name":"review"}),
        );
        let result = apply_or_reject_one_inbox_op_with_presentation_guard(
            &state,
            "main",
            &engine,
            None,
            Some(("guard-tab", &session)),
            true,
        )
        .unwrap();
        assert_eq!(result["applied"], false);
        assert_eq!(result["dead_lettered"], true);
        assert_eq!(engine.engine_call("named_views", ""), before);
        assert_eq!(result["engine_revision"], generation);
        assert!(pending_inbox_seqs(&session).is_empty());
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn display_publications_do_not_invalidate_queued_model_operations() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-display-race-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        let original_model = engine.engine_call("project_export_model", "");
        envelope_ok(&original_model);
        envelope_ok(&engine.engine_call("set_grid_step", r#"{"step_mm":5.0}"#));
        assert_eq!(
            engine.engine_call("project_export_model", ""),
            original_model
        );
        let (session_id, first_export) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, first_export, "base"))
            .unwrap();
        envelope_ok(&state.run_ui_mutation("main", || {
            engine.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#)
        }));
        let original_sketch = engine.engine_call("active_sketch", "");
        envelope_ok(&original_sketch);
        let base = state.engine_revision_for_window("main").unwrap().unwrap();
        for seq in [1, 2] {
            write_inbox(
                &session_id,
                seq,
                "cad_set_document_name",
                base,
                json!({"name": "Applied once"}),
            );
        }

        for step in [0.1, 1.0, 10.0, 100.0] {
            envelope_ok(
                &engine.engine_call("set_grid_step", &json!({"step_mm": step}).to_string()),
            );
            assert_eq!(engine.engine_call("active_sketch", ""), original_sketch);
            let (_, export) = reserve(&state, "main");
            let published = state
                .write_for_window("main", payload(&session_id, export, "base"))
                .unwrap();
            assert_eq!(published["skipped"], false);
            assert_eq!(published["generation"], base);
            assert_eq!(published["model_generation"], base);
            assert_eq!(published["engine_revision"], base);
        }
        assert_eq!(read_session_generation(&session_id), Some(base));
        let applied = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(applied["applied"], true, "{applied}");
        assert_eq!(applied["engine_revision"], base + 1);
        assert_eq!(engine.document_snapshot().name, "Applied once");

        let stale = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(stale["reason"], "generation_conflict");
        assert_eq!(stale["applied"], false);
        let (_, next_export) = reserve(&state, "main");
        assert!(
            next_export > base + 1,
            "export sequence must be independent"
        );
        let published = state
            .write_for_window("main", payload(&session_id, next_export, "applied"))
            .unwrap();
        assert_eq!(published["published_generation"], base + 1);
        assert_eq!(published["model_generation"], base + 1);
        assert_eq!(published["engine_revision"], base + 1);
        let heartbeat = state.heartbeat_for_window("main").unwrap();
        assert_eq!(heartbeat["generation"], base + 1);
        assert_eq!(heartbeat["published_generation"], base + 1);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fast_script_progress_follows_only_successful_owned_inbox_apply() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-inbox-progress-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("progress-tab")
        }));
        let reservation = state
            .reserve_for_window_on_project("main", Some("progress-tab"))
            .unwrap();
        let session = reservation["session_id"].as_str().unwrap();
        let generation = reservation["generation"].as_u64().unwrap();
        state
            .write_for_window(
                "main",
                payload_on_project(session, "progress-tab", generation, "base"),
            )
            .unwrap();
        let progress = json!({"steps_completed":611,"step_count":692});
        for (seq, base, name) in [
            (1, generation, "Progress model"),
            (2, generation, "Stale"),
            (3, generation + 1, ""),
        ] {
            write_inbox(
                session,
                seq,
                "cad_set_document_name",
                base,
                json!({"name":name}),
            );
            let path = inbox_dir(session).join(format!("{seq}.json"));
            let mut op: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            op["script_progress"] = progress.clone();
            atomic_write(&path, &op.to_string()).unwrap();
        }
        assert!(apply_or_reject_one_inbox_op(
            &state,
            "main",
            &engine,
            None,
            Some(("retired-tab", session))
        )
        .unwrap()
        .is_null());
        assert_eq!(pending_inbox_seqs(session), vec![1, 2, 3]);
        let applied = apply_or_reject_one_inbox_op(
            &state,
            "main",
            &engine,
            None,
            Some(("progress-tab", session)),
        )
        .unwrap();
        assert_eq!(applied["applied"], true);
        assert_eq!(applied["script_progress"], progress);
        assert_eq!(engine.document_snapshot().name, "Progress model");
        let result: Value = serde_json::from_str(
            &fs::read_to_string(inbox_dir(session).join("results/1.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            result, applied["result"],
            "The model result is unchanged by presentation metadata"
        );
        assert!(result.get("script_progress").is_none());
        for _ in 0..2 {
            let rejected = apply_one_inbox_op(&state, "main", &engine).unwrap();
            assert_eq!(rejected["applied"], false);
            assert!(rejected.get("script_progress").is_none());
        }
        assert_eq!(engine.document_snapshot().name, "Progress model");
        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ui_native_mutation_rejects_stale_base_without_js_note() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-stale-ui-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        assert_eq!(
            state.engine_revision_for_window("main").unwrap(),
            Some(generation)
        );

        let engine = AppState::new();
        let raw = state.run_ui_mutation("main", || {
            engine.engine_call("document_set_name", r#""NativeUI""#)
        });
        assert!(engine_envelope_ok(&raw), "ui mutate should succeed: {raw}");
        assert_eq!(
            state.engine_revision_for_window("main").unwrap(),
            Some(generation + 1)
        );
        assert_eq!(read_session_generation(&session_id), Some(generation + 1));
        let revision_beat: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(&session_id).join("heartbeat.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(revision_beat["kind"], "engine_revision");
        assert_eq!(revision_beat["generation"], generation + 1);
        assert_eq!(revision_beat["published_generation"], generation);
        assert_eq!(revision_beat["model_generation"], generation);

        state.heartbeat_for_window("main").unwrap();
        let keepalive_beat: Value = serde_json::from_str(
            &fs::read_to_string(dir.join(&session_id).join("heartbeat.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(keepalive_beat["kind"], "heartbeat");
        assert_eq!(keepalive_beat["generation"], generation + 1);
        assert_eq!(keepalive_beat["published_generation"], generation);
        assert_eq!(keepalive_beat["model_generation"], generation);

        write_inbox(
            &session_id,
            1,
            "cad_set_document_name",
            generation,
            json!({"name": "Stale"}),
        );
        write_inbox(
            &session_id,
            2,
            "cad_set_document_name",
            generation + 1,
            json!({"name": "Fresh"}),
        );
        let dead = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(dead["applied"], false);
        assert_eq!(dead["dead_lettered"], true);
        assert_eq!(dead["reason"], "generation_conflict");
        assert_eq!(dead["current_generation"], generation + 1);
        assert_eq!(pending_inbox_seqs(&session_id), vec![2]);

        let fresh = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(fresh["applied"], true);
        assert_eq!(fresh["seq"], 2);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_same_base_ops_second_is_dead_lettered_after_first_advances() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-same-base-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        write_inbox(
            &session_id,
            1,
            "cad_set_document_name",
            generation,
            json!({"name": "First"}),
        );
        write_inbox(
            &session_id,
            2,
            "cad_set_document_name",
            generation,
            json!({"name": "Second"}),
        );
        write_inbox(
            &session_id,
            3,
            "cad_set_document_name",
            generation + 1,
            json!({"name": "Rebased"}),
        );
        let engine = AppState::new();
        let first = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(first["applied"], true);
        assert_eq!(first["seq"], 1);
        assert_eq!(first["engine_revision"], generation + 1);
        assert_eq!(pending_inbox_seqs(&session_id), vec![2, 3]);

        let dead = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(dead["applied"], false);
        assert_eq!(dead["dead_lettered"], true);
        assert_eq!(dead["reason"], "generation_conflict");
        assert_eq!(dead["base_generation"], generation);
        assert_eq!(dead["current_generation"], generation + 1);
        assert_eq!(pending_inbox_seqs(&session_id), vec![3]);

        let rebased = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(rebased["applied"], true);
        assert_eq!(rebased["seq"], 3);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_inbox_head_is_dead_lettered_so_next_seq_applies() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-malformed-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        let inbox = session_root().join(&session_id).join("inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join("1.json"), "{not-json").unwrap();
        write_inbox(
            &session_id,
            2,
            "cad_set_document_name",
            generation,
            json!({"name": "AfterMalformed"}),
        );
        let engine = AppState::new();
        let dead = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(dead["applied"], false);
        assert_eq!(dead["dead_lettered"], true);
        assert_eq!(dead["reason"], "malformed");
        assert_eq!(pending_inbox_seqs(&session_id), vec![2]);
        assert!(session_root()
            .join(&session_id)
            .join("inbox/failed/1.json")
            .exists());

        let second = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(second["applied"], true);
        assert_eq!(second["seq"], 2);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_inbox_op_is_dead_lettered_and_unblocks_queue() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-dead-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();

        write_inbox(&session_id, 1, "not_a_real_tool", generation, json!({}));
        write_inbox(
            &session_id,
            2,
            "cad_set_document_name",
            generation,
            json!({"name": "AfterFail"}),
        );
        let engine = AppState::new();
        let dead = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(dead["applied"], false);
        assert_eq!(dead["dead_lettered"], true);
        assert_eq!(dead["seq"], 1);
        assert!(session_root()
            .join(&session_id)
            .join("inbox/failed/1.json")
            .exists());
        assert_eq!(pending_inbox_seqs(&session_id), vec![2]);

        let second = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(second["applied"], true);
        assert_eq!(second["seq"], 2);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_joint_inbox_payload_is_dead_lettered_and_unblocks_queue() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("limo-cad-bridge-joint-malformed-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        write_inbox(
            &session_id,
            1,
            "assembly_create_joint",
            generation,
            json!({"name": "Broken"}),
        );
        write_inbox(
            &session_id,
            2,
            "cad_set_document_name",
            generation,
            json!({"name": "AfterJointFail"}),
        );
        let engine = AppState::new();
        let dead = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(dead["applied"], false);
        assert_eq!(dead["dead_lettered"], true);
        assert_eq!(dead["seq"], 1);
        assert_eq!(dead["name"], "assembly_create_joint");
        assert!(session_root()
            .join(&session_id)
            .join("inbox/failed/1.json")
            .exists());
        assert_eq!(pending_inbox_seqs(&session_id), vec![2]);

        let second = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(second["applied"], true);
        assert_eq!(second["seq"], 2);
        assert_eq!(engine.document_snapshot().name, "AfterJointFail");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn shared_mutate_map_covers_every_inbox_dispatch() {
        for spec in limo_cad_mcp_mutate::mutate_specs() {
            assert!(!spec.name.is_empty());
            assert!(!spec.engine_method.is_empty());

            let _ = limo_cad_mcp_mutate::encode_payload(spec.payload, &json!({}));
        }
        let locked = limo_cad_mcp_mutate::lookup_mutate("sketch_add_line_locked").unwrap();
        assert_eq!(locked.engine_method, "add_line_locked");
        assert_eq!(locked.execution, ExecutionKind::Direct);
    }

    #[test]
    fn native_inbox_dispatch_resolves_material_presets_before_the_host() {
        let engine = AppState::new();
        dispatch_inbox_on_engine(
            &engine,
            "sketch_begin",
            &json!({"name":"Material fixture","plane":{"type":"origin_plane","plane":"xy"}}),
        )
        .unwrap();
        dispatch_inbox_on_engine(&engine,"sketch_add_rectangle",&json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":10.},"ctrl_held":false})).unwrap();
        dispatch_inbox_on_engine(&engine, "sketch_finish", &json!({})).unwrap();
        let solid=dispatch_inbox_on_engine(&engine,"solid_extrude",&json!({"sketch_name":"Material fixture","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":5.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]})).unwrap();
        let body = solid["scene"]["bodies"][0]["id"].clone();
        let assigned = dispatch_inbox_on_engine(
            &engine,
            "set_body_appearance",
            &json!({"body_id":body,"preset_id":"bambu.petg.hf.black"}),
        )
        .unwrap();
        assert_eq!(assigned[0]["color"], json!({"r":24,"g":24,"b":24,"a":255}));
        assert_eq!(assigned[0]["material_name"], "Bambu PETG HF");
        assert_eq!(assigned[0]["brand"], "Bambu Lab");
        assert_eq!(assigned[0]["filament_type"], "PETG");
        assert_eq!(assigned[0]["filament_id"], "GFG00");
        assert_eq!(assigned[0]["density_g_cm3"], 1.27);
        let before = engine.engine_call("project_export_model", "");
        let exported = parse_engine_envelope(before.clone()).unwrap();
        let model: Value = serde_json::from_str(exported.as_str().unwrap()).unwrap();
        assert_eq!(model["body_appearances"], assigned);
        assert!(dispatch_inbox_on_engine(
            &engine,
            "set_body_appearance",
            &json!({"body_id":body,"preset_id":"missing-material"})
        )
        .unwrap_err()
        .contains("unknown material preset_id"));
        assert_eq!(engine.engine_call("project_export_model", ""), before);
    }

    #[test]
    fn table_driven_accepted_mutates_dispatch_without_name_fallback() {
        let cases = [
            (
                "sketch_add_line_locked",
                "add_line_locked",
                ExecutionKind::Direct,
            ),
            ("sketch_begin", "begin_sketch", ExecutionKind::Direct),
            (
                "cad_set_document_name",
                "document_set_name",
                ExecutionKind::Direct,
            ),
            (
                "solid_extrude",
                "solid_prepare_extrude",
                ExecutionKind::SolidReplay,
            ),
            (
                "solid_mirror",
                "solid_prepare_body_feature",
                ExecutionKind::SolidReplay,
            ),
            (
                "set_body_appearance",
                "set_body_appearance",
                ExecutionKind::Direct,
            ),
            (
                "assembly_create_joint",
                "assembly_create_joint",
                ExecutionKind::Direct,
            ),
            (
                "assembly_update_joint",
                "assembly_update_joint",
                ExecutionKind::Direct,
            ),
        ];
        for (name, method, execution) in cases {
            let spec = limo_cad_mcp_mutate::lookup_mutate(name).expect(name);
            assert_eq!(spec.engine_method, method, "{name}");
            assert_eq!(spec.execution, execution, "{name}");
        }
        assert_eq!(
            limo_cad_mcp_mutate::mutate_specs().len(),
            limo_cad_mcp_mutate::MUTATES.len()
        );

        assert_ne!(
            limo_cad_mcp_mutate::lookup_mutate("sketch_add_line_locked")
                .unwrap()
                .engine_method,
            "sketch_add_line_locked"
        );
        assert_eq!(
            limo_cad_mcp_mutate::lookup_mutate("assembly_create_component")
                .unwrap()
                .engine_method,
            "assembly_create_component"
        );
    }

    fn envelope_ok(raw: &str) {
        assert!(engine_envelope_ok(raw), "engine error: {raw}");
    }

    #[test]
    fn mutation_between_export_and_write_rejects_stale_snapshot() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-export-write-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("tab-a")
        }));

        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        let reserved_revision = state.engine_revision_for_window("main").unwrap();

        let (_, in_flight) = reserve(&state, "main");
        let mutated = state.run_ui_mutation("main", || {
            engine.engine_call("document_set_name", r#""MutatedDuringExport""#)
        });
        envelope_ok(&mutated);
        assert_eq!(engine.document_snapshot().name, "MutatedDuringExport");
        let after_mutation = state.engine_revision_for_window("main").unwrap();
        assert_ne!(after_mutation, reserved_revision);

        let stale = state
            .write_for_window("main", payload(&session_id, in_flight, "stale-export"))
            .unwrap();
        assert_eq!(stale["skipped"], true);
        assert_eq!(stale["reason"], "engine_revision_changed");
        let model = fs::read_to_string(dir.join(&session_id).join("model.json")).unwrap();
        assert!(
            model.contains("\"marker\":\"base\""),
            "stale export must not replace the last coherent snapshot: {model}"
        );
        assert!(!model.contains("stale-export"));

        let (_, fresh) = reserve(&state, "main");
        let applied = state
            .write_for_window("main", payload(&session_id, fresh, "fresh"))
            .unwrap();
        assert_eq!(applied["skipped"], false);
        let fresh_model = fs::read_to_string(dir.join(&session_id).join("model.json")).unwrap();
        assert!(fresh_model.contains("\"marker\":\"fresh\""));

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_op_for_tab_a_cannot_mutate_retained_tab_b() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-tab-switch-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("tab-a")
        }));
        envelope_ok(&state.run_ui_mutation("main", || {
            engine.engine_call("document_set_name", r#""Alpha""#)
        }));

        let (session_a, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_a, generation, "alpha"))
            .unwrap();
        let base = state
            .engine_revision_for_window("main")
            .unwrap()
            .expect("tab A revision");
        write_inbox(
            &session_a,
            1,
            "cad_set_document_name",
            base,
            json!({"name": "FromA"}),
        );

        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.create_project_session("tab-b")
        }));
        envelope_ok(&state.run_ui_mutation("main", || {
            engine.engine_call("document_set_name", r#""Beta""#)
        }));
        assert_eq!(engine.document_snapshot().name, "Beta");
        assert_eq!(engine.active_project_session_id(), "tab-b");

        let applied = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(
            applied["applied"], false,
            "tab A inbox must not apply on tab B: {applied}"
        );
        assert_ne!(applied.get("reason"), Some(&json!("generation_conflict")));
        assert_eq!(engine.document_snapshot().name, "Beta");
        assert!(
            session_root()
                .join(&session_a)
                .join("inbox/1.json")
                .exists(),
            "A op must remain in A's inbox, not land on B"
        );

        let engine_bypass = AppState::new();
        let state_bypass = SessionBridgeState::default();
        envelope_ok(
            &state_bypass.with_project_session_transition("main", &engine_bypass, || {
                engine_bypass.bind_project_session("tab-a")
            }),
        );
        envelope_ok(&state_bypass.run_ui_mutation("main", || {
            engine_bypass.engine_call("document_set_name", r#""Alpha""#)
        }));
        let (session_bypass, gen_bypass) = reserve(&state_bypass, "main");
        state_bypass
            .write_for_window("main", payload(&session_bypass, gen_bypass, "alpha"))
            .unwrap();
        let base_bypass = state_bypass
            .engine_revision_for_window("main")
            .unwrap()
            .expect("bypass A revision");
        write_inbox(
            &session_bypass,
            1,
            "cad_set_document_name",
            base_bypass,
            json!({"name": "FromA"}),
        );
        envelope_ok(&engine_bypass.create_project_session("tab-b"));
        envelope_ok(&engine_bypass.engine_call("document_set_name", r#""Beta""#));
        let mismatch = apply_one_inbox_op(&state_bypass, "main", &engine_bypass).unwrap();
        assert_eq!(mismatch["applied"], false);
        assert_eq!(mismatch["reason"], "project_session_mismatch");
        assert_eq!(engine_bypass.document_snapshot().name, "Beta");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_inbox_applies_after_switch_back_to_same_project() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-switch-back-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("tab-a")
        }));
        envelope_ok(&state.run_ui_mutation("main", || {
            engine.engine_call("document_set_name", r#""Alpha""#)
        }));

        let (session_a, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_a, generation, "alpha"))
            .unwrap();
        let base = state
            .engine_revision_for_window("main")
            .unwrap()
            .expect("tab A revision");
        write_inbox(
            &session_a,
            1,
            "cad_set_document_name",
            base,
            json!({"name": "FromA"}),
        );

        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.create_project_session("tab-b")
        }));
        envelope_ok(&state.run_ui_mutation("main", || {
            engine.engine_call("document_set_name", r#""Beta""#)
        }));
        let reserved_b = state
            .reserve_for_window_on_project("main", Some("tab-b"))
            .unwrap();
        let session_b = reserved_b["session_id"].as_str().unwrap().to_string();
        assert_ne!(session_a, session_b);

        let blocked = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(
            blocked["applied"], false,
            "A inbox must not apply on B: {blocked}"
        );
        assert_eq!(engine.document_snapshot().name, "Beta");
        assert!(
            session_root()
                .join(&session_a)
                .join("inbox/1.json")
                .exists(),
            "A pending must survive the B tab"
        );
        assert!(
            !session_root()
                .join(&session_b)
                .join("inbox/1.json")
                .exists(),
            "A pending must not land in B's inbox"
        );

        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.activate_project_session("tab-a")
        }));
        assert_eq!(engine.active_project_session_id(), "tab-a");
        assert_eq!(engine.document_snapshot().name, "Alpha");

        let applied = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(
            applied["applied"], true,
            "switch-back must apply A's pending: {applied}"
        );
        assert_eq!(applied["seq"], 1);
        assert_eq!(applied["session_id"], session_a);
        assert_eq!(engine.document_snapshot().name, "FromA");
        assert!(session_root()
            .join(&session_a)
            .join("inbox/applied/1.json")
            .exists());

        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.activate_project_session("tab-b")
        }));
        assert_eq!(engine.document_snapshot().name, "Beta");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reserve_a_activate_b_reserve_b_write_a_does_not_publish_into_b() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-reserve-id-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("tab-a")
        }));

        let reserved_a = state
            .reserve_for_window_on_project("main", Some("tab-a"))
            .unwrap();
        let session_a = reserved_a["session_id"].as_str().unwrap().to_string();
        let gen_a = reserved_a["generation"].as_u64().unwrap();
        let project_a = reserved_a["project_session_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(project_a, "tab-a");

        envelope_ok(&state.with_project_session_transition("main", &engine, || {
            engine.create_project_session("tab-b")
        }));
        assert_eq!(engine.active_project_session_id(), "tab-b");

        let reserved_b = state
            .reserve_for_window_on_project("main", Some("tab-b"))
            .unwrap();
        let session_b = reserved_b["session_id"].as_str().unwrap().to_string();
        let gen_b = reserved_b["generation"].as_u64().unwrap();
        let project_b = reserved_b["project_session_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(project_b, "tab-b");
        assert_ne!(session_a, session_b);
        assert_eq!(
            gen_a, gen_b,
            "both tabs independently reserve generation 1 — the collision window"
        );

        let no_identity = PublishPayload {
            focus: "solid".to_string(),
            model_json: Some(r#"{"version":1,"marker":"no-identity"}"#.to_string()),
            active_sketch_json: None,
            generation: gen_a,
            session_id: None,
            project_session_id: None,
        };
        let rejected = state.write_for_window("main", no_identity);
        assert!(
            rejected.is_err(),
            "generation-only write must not target the active tab: {rejected:?}"
        );

        let crossed = payload_on_project(&session_a, &project_b, gen_a, "crossed");
        let mismatch = state.write_for_window("main", crossed).unwrap();
        assert_eq!(mismatch["skipped"], true);
        assert_eq!(mismatch["reason"], "session_identity_mismatch");

        let applied_a = state
            .write_for_window(
                "main",
                payload_on_project(&session_a, &project_a, gen_a, "from-a"),
            )
            .unwrap();
        assert_eq!(applied_a["skipped"], false);
        assert_eq!(applied_a["session_id"], session_a);
        assert_eq!(applied_a["project_session_id"], project_a);
        let model_a = fs::read_to_string(dir.join(&session_a).join("model.json")).unwrap();
        assert!(model_a.contains("\"marker\":\"from-a\""));
        assert!(
            !dir.join(&session_b).join("model.json").exists(),
            "A's delayed write must not publish into B's session"
        );

        let applied_b = state
            .write_for_window(
                "main",
                payload_on_project(&session_b, &project_b, gen_b, "from-b"),
            )
            .unwrap();
        assert_eq!(applied_b["skipped"], false);
        assert_eq!(applied_b["session_id"], session_b);
        let model_b = fs::read_to_string(dir.join(&session_b).join("model.json")).unwrap();
        assert!(model_b.contains("\"marker\":\"from-b\""));
        let model_a_after = fs::read_to_string(dir.join(&session_a).join("model.json")).unwrap();
        assert!(model_a_after.contains("\"marker\":\"from-a\""));
        assert!(!model_a_after.contains("from-b"));

        let lease = read_process_lease(&state, &dir);
        assert_eq!(
            lease["windows"][0]["active_document_id"], "tab-b",
            "a delayed tab A publish must not steal the authoritative active document"
        );
        assert_eq!(lease["windows"][0]["active_session_id"], session_b);

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn already_applied_inbox_seq_second_apply_is_noop() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("limo-cad-bridge-already-applied-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        write_inbox(
            &session_id,
            1,
            "cad_set_document_name",
            generation,
            json!({"name": "Once"}),
        );
        let engine = AppState::new();
        let first = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(first["applied"], true);
        assert_eq!(first["seq"], 1);
        assert_eq!(engine.document_snapshot().name, "Once");
        let revision = first["engine_revision"].clone();
        assert!(session_root()
            .join(&session_id)
            .join("inbox/applied/1.json")
            .exists());
        assert!(pending_inbox_seqs(&session_id).is_empty());

        let second = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(second["applied"], false);
        assert_eq!(second["reason"], "empty");
        assert_ne!(second.get("dead_lettered"), Some(&json!(true)));
        assert_eq!(engine.document_snapshot().name, "Once");
        assert_eq!(second["engine_revision"], revision);
        assert!(pending_inbox_seqs(&session_id).is_empty());

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_apply_uses_engine_revision_not_heartbeat_file() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-hb-file-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_id, generation) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_id, generation, "base"))
            .unwrap();
        let hb = session_root().join(&session_id).join("heartbeat.json");
        assert!(hb.exists(), "publish must write heartbeat.json");
        fs::remove_file(&hb).unwrap();
        write_inbox(
            &session_id,
            1,
            "cad_set_document_name",
            generation,
            json!({"name": "NoHbFile"}),
        );
        let engine = AppState::new();
        let applied = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(
            applied["applied"], true,
            "native must apply from engine_revision: {applied}"
        );
        assert_eq!(applied["seq"], 1);
        assert_eq!(engine.document_snapshot().name, "NoHbFile");
        assert_ne!(applied.get("reason"), Some(&json!("generation_conflict")));
        assert!(session_root()
            .join(&session_id)
            .join("inbox/applied/1.json")
            .exists());

        let stale_ms = now_ms().saturating_sub(30_000 + 5_000);
        let next_generation = applied["engine_revision"]
            .as_u64()
            .expect("engine_revision after first apply");
        fs::write(
            &hb,
            format!(r#"{{"updated_ms":{stale_ms},"generation":{next_generation}}}"#),
        )
        .unwrap();
        write_inbox(
            &session_id,
            2,
            "cad_set_document_name",
            next_generation,
            json!({"name": "AgeStaleOk"}),
        );
        let second = apply_one_inbox_op(&state, "main", &engine).unwrap();
        assert_eq!(
            second["applied"], true,
            "age-stale heartbeat must not block native apply: {second}"
        );
        assert_eq!(second["seq"], 2);
        assert_eq!(engine.document_snapshot().name, "AgeStaleOk");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_stamped_for_a_refuses_apply_on_publisher_b() {
        let _test = TEST_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("limo-cad-bridge-inbox-id-{}", now_ms()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &dir);
        let state = SessionBridgeState::default();
        let (session_a, gen_a) = reserve(&state, "main");
        state
            .write_for_window("main", payload(&session_a, gen_a, "from-a"))
            .unwrap();
        let (session_b, gen_b) = reserve(&state, "secondary");
        state
            .write_for_window("secondary", payload(&session_b, gen_b, "from-b"))
            .unwrap();
        assert_ne!(session_a, session_b);

        write_inbox_with_identity(
            &session_b,
            1,
            "cad_set_document_name",
            gen_b,
            json!({"name": "Clobber"}),
            (Some(&session_a), Some("main"), Some("tab-a")),
        );

        write_inbox(
            &session_b,
            2,
            "cad_set_document_name",
            gen_b,
            json!({"name": "KeepB"}),
        );

        let engine = AppState::new();
        let model_b_before =
            fs::read_to_string(session_root().join(&session_b).join("model.json")).unwrap();
        let dead = apply_one_inbox_op(&state, "secondary", &engine).unwrap();
        assert_eq!(dead["applied"], false, "{dead}");
        assert_eq!(dead["dead_lettered"], true);
        assert_eq!(dead["reason"], "session_identity_mismatch");
        assert_eq!(dead["seq"], 1);
        assert!(session_root()
            .join(&session_b)
            .join("inbox/failed/1.json")
            .exists());
        assert_eq!(pending_inbox_seqs(&session_b), vec![2]);
        let model_b_after =
            fs::read_to_string(session_root().join(&session_b).join("model.json")).unwrap();
        assert_eq!(model_b_before, model_b_after);
        assert_eq!(engine.document_snapshot().name, "Untitled");

        let applied = apply_one_inbox_op(&state, "secondary", &engine).unwrap();
        assert_eq!(applied["applied"], true, "{applied}");
        assert_eq!(applied["seq"], 2);
        assert_eq!(engine.document_snapshot().name, "KeepB");

        let (session_c, gen_c) = reserve(&state, "main");

        let published = state
            .write_for_window("main", payload(&session_c, gen_c, "fresh-a"))
            .unwrap();
        write_inbox_with_identity(
            &session_c,
            1,
            "cad_set_document_name",
            published["engine_revision"].as_u64().unwrap(),
            json!({"name": "MatchA"}),
            (Some(&session_c), Some("main"), None),
        );
        let engine_a = AppState::new();
        let ok = apply_one_inbox_op(&state, "main", &engine_a).unwrap();
        assert_eq!(ok["applied"], true, "{ok}");
        assert_eq!(engine_a.document_snapshot().name, "MatchA");

        std::env::remove_var("LIMO_CAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }
}
