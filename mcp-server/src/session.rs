//! Headless session directories under `NBCAD_SESSION_DIR` (or temp `nbcad-sessions`).
//!
//! Snapshot publish is **UI-owned**. MCP may `cad_attach` (copy) and `cad_submit`
//! an inbox op; it must **not** write `model.json` back (no last-writer-wins).
//! The desktop/engine applies inbox ops via the same `host::handle` path as
//! Tauri IPC, then the existing publisher writes a new snapshot. This is still
//! **not** in-process shared memory.
//!
//! Layout: `<session_dir>/<uuid>/{model.json,active-sketch.json?,focus.json,heartbeat.json,closed.json?,inbox/<seq>.json,inbox/applied/<seq>.json?,inbox/failed/<seq>.json?}`.
//! Live window projection reads expiring per-process leases under
//! `<session_dir>/_ui/processes/`.
//! Session ids must be UUID v4 strings.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum PresentationCommand {
    Configure,
    Note,
    Pause,
    Resume,
    Step,
    Stop,
    Status,
    Finish,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum PresentationMode {
    Fast,
    Present,
}

/// Typed presentation controls share the same endpoint as native playback.
#[derive(serde::Deserialize)]
struct PresentationRequest {
    command: PresentationCommand,
    mode: Option<PresentationMode>,
    speed: Option<f64>,
    duration_ms: Option<u64>,
    text: Option<String>,
    chapter: Option<String>,
    step_index: Option<u64>,
    step_count: Option<u64>,
}

fn validate_presentation(arguments: &Value) -> Result<(), String> {
    let request: PresentationRequest = serde_json::from_value(arguments.clone())
        .map_err(|error| format!("invalid presentation request: {error}"))?;
    if request
        .speed
        .is_some_and(|speed| !speed.is_finite() || !(0.1..=16.0).contains(&speed))
    {
        return Err("presentation speed must be from 0.1 to 16".into());
    }
    if request
        .duration_ms
        .is_some_and(|duration| duration > 10_000)
    {
        return Err("presentation duration_ms must be an integer from 0 to 10000".into());
    }
    for (name, value, limit) in [
        ("text", request.text.as_deref(), 4000),
        ("chapter", request.chapter.as_deref(), 200),
    ] {
        if value.is_some_and(|text| {
            text.chars().count() > limit
                || text
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
        }) {
            return Err(format!(
                "presentation {name} must contain at most {limit} printable characters"
            ));
        }
    }
    if matches!((request.step_index, request.step_count), (Some(index), Some(count)) if index > count)
    {
        return Err("presentation step_index must not exceed step_count".into());
    }
    // Reading both enums here ensures their spelling is validated even when
    // the command does not need an additional setting.
    let _ = (request.command, request.mode);
    Ok(())
}

fn validate_view(arguments: &Value) -> Result<(), String> {
    if let Some(duration) = arguments.get("duration_ms") {
        if !duration.as_u64().is_some_and(|duration| duration <= 10_000) {
            return Err("view duration_ms must be an integer from 0 to 10000".into());
        }
    }
    if arguments
        .get("target")
        .is_some_and(|target| target != "active_sketch")
    {
        return Err(
            "view target must be active_sketch; use body_id or component_id for a part".into(),
        );
    }
    for field in ["body_id", "component_id"] {
        if arguments
            .get(field)
            .is_some_and(|value| value.as_u64().is_none())
        {
            return Err(format!("view {field} must be an unsigned integer"));
        }
    }
    if ["target", "body_id", "component_id"]
        .iter()
        .filter(|field| arguments.get(**field).is_some())
        .count()
        > 1
    {
        return Err("view accepts only one focal target".into());
    }
    Ok(())
}

/// Heartbeats older than this are marked `stale` in list metadata (no auto-delete).
pub const HEARTBEAT_STALE_MS: u64 = 30_000;
/// A desktop process disappears from `windows[]` after three missed 10 s UI
/// keep-alives. Session heartbeat age is deliberately independent: inactive
/// tabs stay live while their owning process lease is fresh.
pub const PROCESS_LEASE_STALE_MS: u64 = 90_000;

pub fn session_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("NBCAD_SESSION_DIR") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    std::env::temp_dir().join("nbcad-sessions")
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// A separate expiring UI request; never part of the modeling inbox or script.
pub fn request_ui(arguments: &Value, attached: Option<&str>) -> Result<Value, String> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("inspect");
    if action == "view" {
        validate_view(arguments)?;
        return request_control(arguments, attached, false, None);
    }
    if !matches!(
        action,
        "inspect"
            | "click"
            | "double_click"
            | "context_menu"
            | "set_value"
            | "key"
            | "window"
            | "file"
            | "viewport"
            | "presentation"
    ) {
        return Err("unknown UI action".into());
    }
    if action == "presentation" {
        validate_presentation(arguments)?;
    }
    if matches!(
        action,
        "click" | "double_click" | "context_menu" | "set_value" | "key"
    ) && arguments.get("target").and_then(Value::as_str).is_none()
    {
        return Err("UI action requires a target from cad_interface inspect".into());
    }
    if let Some(pace) = arguments.get("pace_ms") {
        if !pace.as_u64().is_some_and(|ms| ms <= 2000) {
            return Err("pace_ms must be an integer from 0 to 2000".into());
        }
    }
    request_control(arguments, attached, true, None)
}

fn request_control(
    arguments: &Value,
    attached: Option<&str>,
    ui: bool,
    query: Option<Value>,
) -> Result<Value, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .or(attached)
        .ok_or("Live control needs session_id or an attached desktop session")?;
    require_valid_session_id(session_id)?;
    let view = arguments
        .get("view")
        .and_then(Value::as_str)
        .unwrap_or("current");
    if !ui
        && !matches!(
            view,
            "current" | "isometric" | "top" | "bottom" | "front" | "back" | "left" | "right"
        )
    {
        return Err("invalid camera view".into());
    }
    let heartbeat = heartbeat_meta(session_id);
    if heartbeat.get("stale").and_then(Value::as_bool) != Some(false) {
        return Err("desktop heartbeat is stale; refresh cad_list_sessions".into());
    }
    let id = format!(
        "{}-{}-{}",
        now_ms(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let request_name = format!("controls/{id}.request.json");
    let result_name = format!("controls/{id}.result.json");
    // Effective camera motion is capped at ten seconds by the shared
    // presentation controller, including a slow playback speed.
    let lifetime = if ui || arguments.get("duration_ms").is_some() {
        30_000
    } else {
        5_000
    };
    let mut request = json!({
        "id":id, "view":view, "fit":arguments.get("fit").and_then(Value::as_bool).unwrap_or(false),
        "expires_ms":now_ms()+lifetime,
    });
    if !ui {
        for field in ["duration_ms", "target", "body_id", "component_id"] {
            if let Some(value) = arguments.get(field) {
                request[field] = value.clone();
            }
        }
    }
    if ui {
        request["ui"] = arguments.clone();
        request["ui"]["action"] = arguments.get("action").cloned().unwrap_or(json!("inspect"));
    }
    if let Some(query) = query {
        request["sketch_query"] = query;
        request.as_object_mut().unwrap().remove("ui");
    }
    write_session(session_id, &request_name, &request.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(lifetime + 1000);
    while std::time::Instant::now() < deadline {
        if let Ok(body) = read_session_file(session_id, &result_name) {
            let result: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(session_path(session_id, &result_name)?);
            let _ = fs::remove_file(session_path(session_id, &request_name)?);
            return Ok(result);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = fs::remove_file(session_path(session_id, &request_name)?);
    Ok(
        json!({"status":"timeout","request_id":id,"session_id":session_id,
        "hint":"No UI acknowledgement. Check that the target tab is active and the desktop supports cad_interface."}),
    )
}

pub fn request_engine_query(
    session_id: &str,
    method: &str,
    payload: &str,
) -> Result<Value, String> {
    if !nbcad_mcp_mutate::is_live_engine_query(method) {
        return Err("unsupported live engine query".into());
    }
    let result = request_control(
        &json!({}),
        Some(session_id),
        true,
        Some(json!({"method":method,"payload":payload})),
    )?;
    if result["status"] != "applied" {
        return Err(format!("live engine query failed: {result}"));
    }
    result
        .get("value")
        .cloned()
        .ok_or_else(|| "live engine query omitted its result".into())
}

/// UUID v4 string form (8-4-4-4-12 hex with version nibble `4` and RFC variant).
pub fn is_valid_session_id(session_id: &str) -> bool {
    let bytes = session_id.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            8 | 13 | 18 | 23 => {
                if *byte != b'-' {
                    return false;
                }
            }
            14 => {
                if *byte != b'4' {
                    return false;
                }
            }
            19 => {
                let lower = byte.to_ascii_lowercase();
                if !matches!(lower, b'8' | b'9' | b'a' | b'b') {
                    return false;
                }
            }
            _ => {
                if !byte.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

pub fn require_valid_session_id(session_id: &str) -> Result<(), String> {
    if is_valid_session_id(session_id) {
        Ok(())
    } else {
        Err(format!(
            "session_id must be a UUID v4 string (got '{session_id}')"
        ))
    }
}

/// List attachable session directories. Skips control dirs (`_*`) and non-UUID names.
pub fn list_sessions() -> Result<Vec<String>, String> {
    let root = session_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('_') || !is_valid_session_id(&name) {
                continue;
            }
            sessions.push(name);
        }
    }
    sessions.sort();
    Ok(sessions)
}

pub fn read_session_file(session_id: &str, filename: &str) -> Result<String, String> {
    let path = session_path(session_id, filename)?;
    fs::read_to_string(&path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

/// Require `model.json` for the session. Missing file → hard error (Jack §3).
pub fn require_model_json(session_id: &str) -> Result<String, String> {
    require_valid_session_id(session_id)?;
    read_session_file(session_id, "model.json").map_err(|error| {
        format!("session '{session_id}' has no valid model.json ({error}); attach refused")
    })
}

/// Write a session file via temp + rename so readers never see a partial file.
pub fn write_session(session_id: &str, filename: &str, content: &str) -> Result<(), String> {
    let path = session_path(session_id, filename)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension(format!(
        "{}.tmp.{}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("json"),
        std::process::id()
    ));
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("could not create temp {}: {error}", temporary.display()))?;
        file.write_all(content.as_bytes())
            .map_err(|error| format!("could not write temp {}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("could not flush temp {}: {error}", temporary.display()))?;
    }
    fs::rename(&temporary, &path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("could not replace {}: {error}", path.display())
    })
}

/// Heartbeat age / staleness for a session directory (no auto-delete).
pub fn heartbeat_meta(session_id: &str) -> Value {
    let snapshot = read_heartbeat_snapshot(session_id);
    heartbeat_meta_from_snapshot(snapshot.as_ref())
}

fn read_heartbeat_snapshot(session_id: &str) -> Option<Value> {
    read_session_file(session_id, "heartbeat.json")
        .ok()
        .map(|body| serde_json::from_str(&body).unwrap_or(json!({})))
}

fn heartbeat_meta_from_snapshot(snapshot: Option<&Value>) -> Value {
    match snapshot {
        Some(parsed) => {
            let updated_ms = read_optional_u64(parsed, "updated_ms").unwrap_or(0);
            let age_ms = now_ms().saturating_sub(updated_ms);
            json!({
                "updated_ms": updated_ms,
                "age_ms": age_ms,
                "stale": age_ms > HEARTBEAT_STALE_MS,
                "generation": parsed.get("generation").cloned().unwrap_or(Value::Null),
                "interface_version": parsed.get("interface_version").cloned().unwrap_or(Value::Null),
                "window_id": parsed.get("window_id").cloned().unwrap_or(Value::Null),
                "document_id": parsed
                    .get("document_id")
                    .or_else(|| parsed.get("project_session_id"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "project_session_id": parsed
                    .get("project_session_id")
                    .or_else(|| parsed.get("document_id"))
                    .cloned()
                    .unwrap_or(Value::Null),
            })
        }
        None => json!({
            "updated_ms": null,
            "age_ms": null,
            "stale": true,
            "generation": null,
            "interface_version": null,
            "window_id": null,
            "document_id": null,
            "project_session_id": null,
        }),
    }
}

/// Stable identities published beside a session snapshot (UI-owned).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentity {
    pub session_id: String,
    pub window_id: Option<String>,
    pub document_id: Option<String>,
}

fn optional_id(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Read window/document identity from heartbeat (focus.json fallback).
pub fn session_identity(session_id: &str) -> SessionIdentity {
    let snapshot = read_heartbeat_snapshot(session_id).unwrap_or(json!({}));
    session_identity_from_snapshot(session_id, &snapshot)
}

fn session_identity_from_snapshot(session_id: &str, snapshot: &Value) -> SessionIdentity {
    let mut window_id = optional_id(snapshot, "window_id");
    let mut document_id = optional_id(snapshot, "document_id")
        .or_else(|| optional_id(snapshot, "project_session_id"));
    if window_id.is_none() || document_id.is_none() {
        if let Ok(body) = read_session_file(session_id, "focus.json") {
            let parsed: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            if window_id.is_none() {
                window_id = optional_id(&parsed, "window_id");
            }
            if document_id.is_none() {
                document_id = optional_id(&parsed, "document_id")
                    .or_else(|| optional_id(&parsed, "project_session_id"));
            }
        }
    }
    SessionIdentity {
        session_id: session_id.to_string(),
        window_id,
        document_id,
    }
}

/// Explicit close marker written when the UI drops a tab's publisher.
pub const CLOSED_TOMBSTONE: &str = "closed.json";

pub fn is_session_closed(session_id: &str) -> bool {
    session_path(session_id, CLOSED_TOMBSTONE)
        .map(|path| path.is_file())
        .unwrap_or(false)
}

/// Mark a session directory closed so it leaves the live `windows[]` set.
pub fn write_closed_tombstone(session_id: &str) -> Result<(), String> {
    let body = serde_json::to_string_pretty(&json!({
        "closed_ms": now_ms(),
        "session_id": session_id,
    }))
    .map_err(|error| error.to_string())?;
    write_session(session_id, CLOSED_TOMBSTONE, &body)
}

/// Clear a close marker when the same session UUID is republished.
pub fn clear_closed_tombstone(session_id: &str) -> Result<(), String> {
    let path = session_path(session_id, CLOSED_TOMBSTONE)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveWindowLease {
    active_document_id: String,
    active_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessLease {
    process_instance_id: String,
    updated_ms: u64,
    windows: BTreeMap<String, ActiveWindowLease>,
    /// Old `_ui/process.json` files did not include a window inventory. Keep
    /// those usable during migration, but never grant this wildcard to the
    /// new per-process format (where an empty list means no live windows).
    accepts_unlisted_windows: bool,
}

#[derive(Debug, Clone, Default)]
struct ProcessRegistry {
    /// Distinguishes a legacy/headless session root from a UI-managed root
    /// whose last process has exited and removed its lease.
    present: bool,
    leases: BTreeMap<String, ProcessLease>,
}

fn parse_process_lease(parsed: &Value, accepts_unlisted_windows: bool) -> Option<ProcessLease> {
    let process_instance_id = optional_id(parsed, "process_instance_id")?;
    let updated_ms = parsed.get("updated_ms").and_then(Value::as_u64)?;
    if now_ms().saturating_sub(updated_ms) > PROCESS_LEASE_STALE_MS {
        return None;
    }
    let mut windows = BTreeMap::new();
    for value in parsed
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(window_id) = optional_id(value, "window_id") else {
            continue;
        };
        let Some(active_document_id) = optional_id(value, "active_document_id") else {
            continue;
        };
        let Some(active_session_id) = optional_id(value, "active_session_id") else {
            continue;
        };
        windows.insert(
            window_id.clone(),
            ActiveWindowLease {
                active_document_id,
                active_session_id,
            },
        );
    }
    Some(ProcessLease {
        process_instance_id,
        updated_ms,
        windows,
        accepts_unlisted_windows,
    })
}

fn read_process_lease(path: &Path, accepts_unlisted_windows: bool) -> Option<ProcessLease> {
    let body = fs::read_to_string(path).ok()?;
    let parsed: Value = serde_json::from_str(&body).ok()?;
    parse_process_lease(&parsed, accepts_unlisted_windows)
}

fn process_registry() -> ProcessRegistry {
    let ui_dir = session_dir().join("_ui");
    let processes_dir = ui_dir.join("processes");
    let legacy_path = ui_dir.join("process.json");
    let mut registry = ProcessRegistry {
        present: processes_dir.is_dir() || legacy_path.is_file(),
        leases: BTreeMap::new(),
    };

    if let Ok(entries) = fs::read_dir(&processes_dir) {
        for entry in entries.flatten() {
            if !entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            if let Some(lease) = read_process_lease(&entry.path(), false) {
                let replace = registry
                    .leases
                    .get(&lease.process_instance_id)
                    .map(|existing| existing.updated_ms < lease.updated_ms)
                    .unwrap_or(true);
                if replace {
                    registry
                        .leases
                        .insert(lease.process_instance_id.clone(), lease);
                }
            }
        }
    }

    // Read the old singleton only as a migration fallback. Its timestamp must
    // be fresh, so a pre-registry crash cannot keep sessions live forever.
    if let Some(lease) = read_process_lease(&legacy_path, true) {
        let replace = registry
            .leases
            .get(&lease.process_instance_id)
            .map(|existing| existing.updated_ms < lease.updated_ms)
            .unwrap_or(true);
        if replace {
            registry
                .leases
                .insert(lease.process_instance_id.clone(), lease);
        }
    }
    registry
}

fn heartbeat_process_instance_id(session_id: &str) -> Option<String> {
    let body = read_session_file(session_id, "heartbeat.json").ok()?;
    let parsed: Value = serde_json::from_str(&body).ok()?;
    optional_id(&parsed, "process_instance_id")
}

fn heartbeat_process_window(session_id: &str) -> Option<(String, String)> {
    let body = read_session_file(session_id, "heartbeat.json").ok()?;
    let parsed: Value = serde_json::from_str(&body).ok()?;
    Some((
        optional_id(&parsed, "process_instance_id")?,
        optional_id(&parsed, "window_id")?,
    ))
}

/// Live for window projection: not closed and owned by a non-expired process
/// lease. Stale per-tab heartbeats still count — inactive open tabs remain
/// visible while their owning process is alive.
fn is_live_for_windows(session_id: &str, registry: &ProcessRegistry) -> bool {
    if is_session_closed(session_id) {
        return false;
    }
    if !registry.present {
        // Backward-compatible headless/legacy roots have no UI lease registry.
        return true;
    }
    heartbeat_process_window(session_id).is_some_and(|(process_id, window_id)| {
        registry.leases.get(&process_id).is_some_and(|lease| {
            lease.accepts_unlisted_windows || lease.windows.contains_key(&window_id)
        })
    })
}

/// Resolve attach target to a UUID session dir.
///
/// Accepts `session_id` (UUID), `window_id` (Tauri label), and/or `document_id`
/// (native project-session id). UUID `document_id` remains an alias for
/// `session_id` for compatibility. All provided selectors are intersected;
/// ambiguity is reported only after every supplied filter is applied. Closed
/// sessions are excluded from window/document matching (explicit `session_id`
/// still resolves for recovery).
pub fn resolve_attach_target(
    session_id: Option<&str>,
    window_id: Option<&str>,
    document_id: Option<&str>,
) -> Result<SessionIdentity, String> {
    let session_id = session_id.map(str::trim).filter(|s| !s.is_empty());
    let window_id = window_id.map(str::trim).filter(|s| !s.is_empty());
    let document_id = document_id.map(str::trim).filter(|s| !s.is_empty());
    if session_id.is_none() && window_id.is_none() && document_id.is_none() {
        return Err(
            "missing attach target: provide session_id, window_id, and/or document_id".to_string(),
        );
    }

    let mut candidates: Vec<String> = if let Some(id) = session_id {
        require_valid_session_id(id)?;
        if !list_sessions()?.iter().any(|existing| existing == id) {
            return Err(format!(
                "session '{id}' was not found under {}",
                session_dir().display()
            ));
        }
        vec![id.to_string()]
    } else {
        // Window/document matching uses the live set only. UI-managed roots
        // require a fresh owning process lease; explicit UUID remains the
        // recovery path for closed or prior-run sessions.
        let registry = process_registry();
        list_sessions()?
            .into_iter()
            .filter(|id| is_live_for_windows(id, &registry))
            .collect()
    };

    if let Some(window) = window_id {
        candidates.retain(|id| session_identity(id).window_id.as_deref() == Some(window));
        if candidates.is_empty() {
            return Err(format!(
                "window_id '{window}' was not found under {}",
                session_dir().display()
            ));
        }
    }

    if let Some(document) = document_id {
        // Compat: UUID document_id still means the session directory name.
        if is_valid_session_id(document) && list_sessions()?.iter().any(|id| id == document) {
            candidates.retain(|id| id == document);
        } else {
            candidates.retain(|id| session_identity(id).document_id.as_deref() == Some(document));
        }
        if candidates.is_empty() {
            return Err(format!(
                "document_id '{document}' was not found under {}",
                session_dir().display()
            ));
        }
    }

    candidates.sort();
    candidates.dedup();
    match candidates.as_slice() {
        [] => Err("could not resolve attach target".to_string()),
        [only] => Ok(session_identity(only)),
        many => {
            let labels: Vec<&str> = [
                session_id.map(|_| "session_id"),
                window_id.map(|_| "window_id"),
                document_id.map(|_| "document_id"),
            ]
            .into_iter()
            .flatten()
            .collect();
            Err(format!(
                "attach target is ambiguous after filters ({}); matches {} ({})",
                labels.join("+"),
                many.len(),
                many.join(", ")
            ))
        }
    }
}

fn windows_projection(detailed: &[Value], registry: &ProcessRegistry) -> Vec<Value> {
    let mut by_window: BTreeMap<(Option<String>, String), Vec<Value>> = BTreeMap::new();
    for detail in detailed {
        if detail.get("closed").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if detail.get("live_for_windows").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let Some(window_id) = detail.get("window_id").and_then(Value::as_str) else {
            continue;
        };
        let process_instance_id = detail
            .get("process_instance_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let doc = json!({
            "session_id": detail.get("session_id"),
            "document_id": detail.get("document_id"),
            "has_model": detail.get("has_model"),
            "heartbeat": detail.get("heartbeat"),
        });
        by_window
            .entry((process_instance_id, window_id.to_string()))
            .or_default()
            .push(doc);
    }

    by_window
        .into_iter()
        .map(|((process_instance_id, window_id), mut documents)| {
            documents.sort_by(|a, b| {
                let a_id = a.get("document_id").and_then(Value::as_str).unwrap_or("");
                let b_id = b.get("document_id").and_then(Value::as_str).unwrap_or("");
                a_id.cmp(b_id)
            });
            let authoritative = process_instance_id
                .as_ref()
                .and_then(|id| registry.leases.get(id))
                .and_then(|lease| lease.windows.get(&window_id));
            let active = authoritative.and_then(|active| {
                documents.iter().find(|doc| {
                    doc.get("session_id").and_then(Value::as_str)
                        == Some(active.active_session_id.as_str())
                        && doc.get("document_id").and_then(Value::as_str)
                            == Some(active.active_document_id.as_str())
                })
            });
            json!({
                "window_id": window_id,
                "process_instance_id": process_instance_id,
                "active_document_id": active.as_ref().and_then(|d| d.get("document_id").cloned()).unwrap_or(Value::Null),
                "active_session_id": active.as_ref().and_then(|d| d.get("session_id").cloned()).unwrap_or(Value::Null),
                "documents": documents,
            })
        })
        .collect()
}

pub fn sessions_list_json() -> Value {
    match list_sessions() {
        Ok(sessions) => {
            let registry = process_registry();
            let process_instance_id = match registry.leases.keys().collect::<Vec<_>>().as_slice() {
                [only] => Some((*only).clone()),
                _ => None,
            };
            let process_instance_ids: Vec<String> = registry.leases.keys().cloned().collect();
            let detailed: Vec<Value> = sessions
                .iter()
                .map(|session_id| {
                    let has_model = session_path(session_id, "model.json")
                        .map(|path| path.is_file())
                        .unwrap_or(false);
                    let identity = session_identity(session_id);
                    let closed = is_session_closed(session_id);
                    let live = is_live_for_windows(session_id, &registry);
                    json!({
                        "session_id": session_id,
                        "window_id": identity.window_id,
                        "document_id": identity.document_id,
                        "has_model": has_model,
                        "closed": closed,
                        "live_for_windows": live,
                        "process_instance_id": heartbeat_process_instance_id(session_id),
                        "heartbeat": heartbeat_meta(session_id),
                    })
                })
                .collect();
            let windows = windows_projection(&detailed, &registry);
            let process_instances: Vec<Value> = registry
                .leases
                .values()
                .map(|lease| {
                    json!({
                        "process_instance_id": lease.process_instance_id,
                        "updated_ms": lease.updated_ms,
                        "age_ms": now_ms().saturating_sub(lease.updated_ms),
                    })
                })
                .collect();
            json!({
                "session_mode": "read_only_snapshot",
                "sessions": sessions,
                "session_details": detailed,
                "windows": windows,
                "process_instance_id": process_instance_id,
                "process_instance_ids": process_instance_ids,
                "process_instances": process_instances,
                "session_dir": session_dir().display().to_string(),
                "heartbeat_stale_ms": HEARTBEAT_STALE_MS,
                "process_lease_ms": PROCESS_LEASE_STALE_MS,
                "process_lease_stale_ms": PROCESS_LEASE_STALE_MS,
            })
        }
        Err(error) => json!({
            "session_mode": "read_only_snapshot",
            "sessions": [],
            "session_details": [],
            "windows": [],
            "process_instance_id": null,
            "process_instance_ids": [],
            "process_instances": [],
            "session_dir": session_dir().display().to_string(),
            "heartbeat_stale_ms": HEARTBEAT_STALE_MS,
            "process_lease_ms": PROCESS_LEASE_STALE_MS,
            "process_lease_stale_ms": PROCESS_LEASE_STALE_MS,
            "error": error,
        }),
    }
}

fn session_path(session_id: &str, filename: &str) -> Result<PathBuf, String> {
    require_valid_session_id(session_id)?;
    if filename.is_empty() || filename.contains('\\') || filename.contains("..") {
        return Err("invalid filename".to_string());
    }
    let parts: Vec<&str> = filename.split('/').collect();
    if parts.is_empty()
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || !part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        })
    {
        return Err("invalid filename".to_string());
    }
    let mut path = session_dir().join(session_id);
    for part in parts {
        path.push(part);
    }
    Ok(path)
}

/// One MCP-submitted modeling op. UI/engine is the only live-document writer.
#[derive(Debug, Clone)]
pub struct InboxOp {
    pub name: String,
    pub arguments: Value,
    pub base_generation: u64,
    /// Optional identity stamp from the attached session at submit time.
    pub session_id: Option<String>,
    pub window_id: Option<String>,
    pub document_id: Option<String>,
}

impl InboxOp {
    /// Unstamped op (compat / tests). Production `cad_submit` stamps identity.
    pub fn unstamped(name: impl Into<String>, arguments: Value, base_generation: u64) -> Self {
        Self {
            name: name.into(),
            arguments,
            base_generation,
            session_id: None,
            window_id: None,
            document_id: None,
        }
    }

    pub fn with_identity(mut self, identity: &SessionIdentity) -> Self {
        self.session_id = Some(identity.session_id.clone());
        self.window_id = identity.window_id.clone();
        self.document_id = identity.document_id.clone();
        self
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "name": self.name,
            "arguments": self.arguments,
            "base_generation": self.base_generation,
        });
        if let Some(object) = value.as_object_mut() {
            if let Some(session_id) = &self.session_id {
                object.insert("session_id".to_string(), json!(session_id));
            }
            if let Some(window_id) = &self.window_id {
                object.insert("window_id".to_string(), json!(window_id));
            }
            if let Some(document_id) = &self.document_id {
                object.insert("document_id".to_string(), json!(document_id));
            }
        }
        value
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "inbox op missing 'name'".to_string())?
            .to_string();
        let arguments = value.get("arguments").cloned().unwrap_or(json!({}));
        let base_generation = value
            .get("base_generation")
            .and_then(Value::as_u64)
            .ok_or_else(|| "inbox op missing 'base_generation'".to_string())?;
        Ok(Self {
            name,
            arguments,
            base_generation,
            session_id: optional_id(value, "session_id"),
            window_id: optional_id(value, "window_id"),
            document_id: optional_id(value, "document_id"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub seq: u64,
    pub op: InboxOp,
    pub host_result: Value,
}

/// Current heartbeat `generation`, if the file is present and parseable.
pub fn read_heartbeat_generation(session_id: &str) -> Result<u64, String> {
    let meta = heartbeat_meta(session_id);
    meta.get("generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            format!("session '{session_id}' has no heartbeat generation; UI must publish first")
        })
}

/// Generation fence for the model.json snapshot being (or about to be) loaded.
///
/// Prefer `model_generation`, then `published_generation` when the model field
/// is absent. An explicit null fence stays unknown. Fall back to live
/// `generation` only when no publication fields exist
/// (legacy/minimal heartbeats). Returns `None` when heartbeat is missing or
/// has no usable generation — callers must not treat that as fresh.
pub fn read_model_publication_generation(session_id: &str) -> Option<u64> {
    model_publication_generation_from_heartbeat(&read_heartbeat_snapshot(session_id)?)
}

fn model_publication_generation_from_heartbeat(parsed: &Value) -> Option<u64> {
    // A present but null model fence means the completed model is unknown.
    // An active-sketch publication must never stand in for that model fence.
    if parsed.get("model_generation").is_some() {
        read_optional_u64(parsed, "model_generation")
    } else if parsed.get("published_generation").is_some() {
        read_optional_u64(parsed, "published_generation")
    } else {
        // Legacy heartbeats carried only the publication generation.
        read_optional_u64(parsed, "generation")
    }
}

/// Structured writer-lock error. MCP never writes model.json (`writeback: false`).
pub fn generation_conflict_error(
    session_id: &str,
    base_generation: u64,
    current_generation: Option<u64>,
) -> String {
    serde_json::to_string(&json!({
        "code": "generation_conflict",
        "writeback": false,
        "session_mode": "ui_owned_apply",
        "session_id": session_id,
        "base_generation": base_generation,
        "current_generation": current_generation,
        "hint": "UI moved; cad_refresh then resubmit with the new heartbeat generation",
    }))
    .unwrap_or_else(|_| {
        format!(
            "{{\"code\":\"generation_conflict\",\"writeback\":false,\"session_mode\":\"ui_owned_apply\",\"session_id\":\"{session_id}\"}}"
        )
    })
}

/// Structured error when a stamped inbox op targets a different session/window.
pub fn session_identity_mismatch_error(
    destination_session_id: &str,
    destination_window_id: Option<&str>,
    stamped_session_id: Option<&str>,
    stamped_window_id: Option<&str>,
) -> String {
    serde_json::to_string(&json!({
        "code": "session_identity_mismatch",
        "writeback": false,
        "session_mode": "ui_owned_apply",
        "session_id": destination_session_id,
        "window_id": destination_window_id,
        "stamped_session_id": stamped_session_id,
        "stamped_window_id": stamped_window_id,
        "hint": "inbox op identity does not match the destination session/window; dead-letter and do not apply",
    }))
    .unwrap_or_else(|_| {
        format!(
            "{{\"code\":\"session_identity_mismatch\",\"writeback\":false,\"session_mode\":\"ui_owned_apply\",\"session_id\":\"{destination_session_id}\"}}"
        )
    })
}

/// Return a structured identity-mismatch error when a stamped op does not
/// match the destination session / window this apply is bound to.
/// Unstamped ops (missing fields) keep current behavior.
pub fn inbox_op_identity_mismatch(
    destination_session_id: &str,
    destination_window_id: Option<&str>,
    op: &InboxOp,
) -> Option<String> {
    let session_mismatch = op
        .session_id
        .as_deref()
        .is_some_and(|stamped| stamped != destination_session_id);
    let window_mismatch = match (op.window_id.as_deref(), destination_window_id) {
        (Some(stamped), Some(dest)) => stamped != dest,
        (Some(_), None) => true,
        (None, _) => false,
    };
    if session_mismatch || window_mismatch {
        Some(session_identity_mismatch_error(
            destination_session_id,
            destination_window_id,
            op.session_id.as_deref(),
            op.window_id.as_deref(),
        ))
    } else {
        None
    }
}

pub fn not_attached_error() -> String {
    serde_json::to_string(&json!({
        "code": "not_attached",
        "writeback": false,
        "session_mode": "ui_owned_apply",
        "session_id": Value::Null,
        "hint": "cad_submit requires cad_attach; headless goldens call modeling tools directly",
    }))
    .unwrap_or_else(|_| {
        "{\"code\":\"not_attached\",\"writeback\":false,\"session_mode\":\"ui_owned_apply\"}"
            .to_string()
    })
}

fn parse_inbox_seq(name: &str) -> Option<u64> {
    name.strip_suffix(".json")?.parse().ok()
}

fn inbox_seqs_in(dir: &std::path::Path) -> Vec<u64> {
    let mut seqs = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
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

/// Next inbox sequence (1-based), considering pending, applied, and failed ops.
pub fn next_inbox_seq(session_id: &str) -> Result<u64, String> {
    require_valid_session_id(session_id)?;
    let root = session_dir().join(session_id);
    let inbox = root.join("inbox");
    let mut max = 0u64;
    for seq in inbox_seqs_in(&inbox) {
        max = max.max(seq);
    }
    for seq in inbox_seqs_in(&inbox.join("applied")) {
        max = max.max(seq);
    }
    for seq in inbox_seqs_in(&inbox.join("failed")) {
        max = max.max(seq);
    }
    Ok(max.saturating_add(1))
}

/// Pending inbox seqs, lowest first.
pub fn pending_inbox_seqs(session_id: &str) -> Result<Vec<u64>, String> {
    require_valid_session_id(session_id)?;
    Ok(inbox_seqs_in(&session_dir().join(session_id).join("inbox")))
}

/// Write `inbox/<seq>.json` with an exclusive sequence reservation.
///
/// `next_inbox_seq` is only a hint. The durable allocation is `create_new` on
/// `inbox/<seq>.json` so two `cad_submit` callers cannot share a sequence or
/// overwrite each other while both report success. On collision the loser
/// retries the next free sequence.
pub fn write_inbox_op(session_id: &str, op: &InboxOp) -> Result<u64, String> {
    require_valid_session_id(session_id)?;
    let body = serde_json::to_string_pretty(&op.to_json())
        .map_err(|error| format!("encode inbox op: {error}"))?;
    const MAX_ATTEMPTS: u32 = 1024;
    for _ in 0..MAX_ATTEMPTS {
        let seq = next_inbox_seq(session_id)?;
        let path = session_path(session_id, &format!("inbox/{seq}.json"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        match exclusive_create_file(&path, &body) {
            Ok(()) => return Ok(seq),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!("could not create inbox/{seq}.json: {error}"));
            }
        }
    }
    Err("could not reserve an exclusive inbox sequence".to_string())
}

fn exclusive_create_file(path: &Path, content: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let result = file
        .write_all(content.as_bytes())
        .and_then(|_| file.sync_all());
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

pub fn read_inbox_op(session_id: &str, seq: u64) -> Result<InboxOp, String> {
    let body = read_session_file(session_id, &format!("inbox/{seq}.json"))?;
    let parsed: Value = serde_json::from_str(&body)
        .map_err(|error| format!("invalid inbox/{seq}.json: {error}"))?;
    InboxOp::from_json(&parsed)
}

fn archive_inbox_op(session_id: &str, seq: u64) -> Result<(), String> {
    let src = session_path(session_id, &format!("inbox/{seq}.json"))?;
    let dest = session_path(session_id, &format!("inbox/applied/{seq}.json"))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    match fs::rename(&src, &dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            let body = fs::read_to_string(&src)
                .map_err(|error| format!("archive read inbox/{seq}.json: {error}"))?;
            write_session(session_id, &format!("inbox/applied/{seq}.json"), &body)?;
            fs::remove_file(&src).map_err(|error| format!("remove applied inbox op: {error}"))
        }
    }
}

fn dead_letter_inbox_op(session_id: &str, seq: u64, error: &str) -> Result<(), String> {
    let src = session_path(session_id, &format!("inbox/{seq}.json"))?;
    if let Some(parent) = session_path(session_id, "inbox/failed")?.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let original = fs::read_to_string(&src).unwrap_or_default();
    let body = match serde_json::from_str::<Value>(&original) {
        Ok(mut parsed) => {
            if let Some(object) = parsed.as_object_mut() {
                object.insert("error".to_string(), Value::String(error.to_string()));
                object.insert("failed_ms".to_string(), json!(now_ms()));
            }
            serde_json::to_string_pretty(&parsed).unwrap_or(original.clone())
        }
        Err(_) => serde_json::to_string_pretty(&json!({
            "error": error,
            "failed_ms": now_ms(),
            "raw": original,
        }))
        .map_err(|e| e.to_string())?,
    };
    write_session(session_id, &format!("inbox/failed/{seq}.json"), &body)?;
    if src.exists() {
        fs::remove_file(&src).map_err(|e| format!("remove dead-lettered inbox op: {e}"))?;
    }
    Ok(())
}

/// Read the lowest pending inbox op, check `base_generation` against heartbeat,
/// call `host_apply`, then archive the op. Does **not** write model.json —
/// the caller (UI publisher or test) publishes the new snapshot.
///
/// `host_apply` must target the **live** engine (desktop) or a **separate**
/// SketchManager loaded from the published model (tests). Never the attached
/// MCP in-memory copy.
pub fn apply_inbox_op<F>(session_id: &str, host_apply: F) -> Result<ApplyResult, String>
where
    F: FnOnce(&str, Value) -> Result<Value, String>,
{
    require_valid_session_id(session_id)?;
    let seqs = pending_inbox_seqs(session_id)?;
    let seq = seqs
        .first()
        .copied()
        .ok_or_else(|| format!("session '{session_id}' has no pending inbox op"))?;
    let op = match read_inbox_op(session_id, seq) {
        Ok(op) => op,
        Err(error) => {
            dead_letter_inbox_op(session_id, seq, &error)?;
            return Err(error);
        }
    };
    let destination_window = session_identity(session_id).window_id;
    if let Some(error) = inbox_op_identity_mismatch(session_id, destination_window.as_deref(), &op)
    {
        dead_letter_inbox_op(session_id, seq, &error)?;
        return Err(error);
    }
    let current = match read_heartbeat_generation(session_id) {
        Ok(generation) => generation,
        Err(_) => {
            let error = generation_conflict_error(session_id, op.base_generation, None);
            // Match native apply: generation_conflict (including a missing /
            // unreadable heartbeat generation) must not wedge later seqs.
            // Age-only stale heartbeats still apply when generation matches —
            // listing staleness is not a writer lock.
            dead_letter_inbox_op(session_id, seq, &error)?;
            return Err(error);
        }
    };
    if op.base_generation != current {
        let error = generation_conflict_error(session_id, op.base_generation, Some(current));
        // Match native apply: a stale head must not wedge later seqs.
        dead_letter_inbox_op(session_id, seq, &error)?;
        return Err(error);
    }
    if nbcad_mcp_mutate::lookup_mutate(&op.name).is_none() {
        let error = format!("unsupported inbox mutate '{}'", op.name);
        // Match native apply: reject before host so inspect/unknown names
        // cannot archive as applied.
        dead_letter_inbox_op(session_id, seq, &error)?;
        return Err(error);
    }
    let host_result = match host_apply(&op.name, op.arguments.clone()) {
        Ok(result) => result,
        Err(error) => {
            // Match native apply: a failed head must not wedge later seqs.
            dead_letter_inbox_op(session_id, seq, &error)?;
            return Err(error);
        }
    };
    archive_inbox_op(session_id, seq)?;
    Ok(ApplyResult {
        seq,
        op,
        host_result,
    })
}

/// Default / max wait for [`await_inbox_apply`] (MCP `cad_await_apply`).
pub const AWAIT_APPLY_DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub const AWAIT_APPLY_MAX_TIMEOUT_MS: u64 = 30_000;
pub const AWAIT_APPLY_DEFAULT_POLL_MS: u64 = 50;

/// Disk receipt for one inbox sequence after UI apply or dead-letter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxReceipt {
    Pending,
    Applied {
        base_generation: u64,
        name: Option<String>,
    },
    Failed {
        error: Option<String>,
        name: Option<String>,
        base_generation: Option<u64>,
    },
}

fn read_optional_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn read_optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn parse_receipt_file(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or(json!({}))
}

/// Observe whether `inbox/<seq>.json` was archived (`applied/`) or dead-lettered
/// (`failed/`). Does not mutate disk. Missing receipts are [`InboxReceipt::Pending`]
/// even if the pending file is gone (race / unknown seq).
pub fn inbox_op_receipt(session_id: &str, seq: u64) -> Result<InboxReceipt, String> {
    require_valid_session_id(session_id)?;
    let applied = session_path(session_id, &format!("inbox/applied/{seq}.json"))?;
    if applied.is_file() {
        let parsed = parse_receipt_file(&applied);
        return Ok(InboxReceipt::Applied {
            base_generation: read_optional_u64(&parsed, "base_generation").unwrap_or(0),
            name: read_optional_string(&parsed, "name"),
        });
    }
    let failed = session_path(session_id, &format!("inbox/failed/{seq}.json"))?;
    if failed.is_file() {
        let parsed = parse_receipt_file(&failed);
        return Ok(InboxReceipt::Failed {
            error: read_optional_string(&parsed, "error"),
            name: read_optional_string(&parsed, "name"),
            base_generation: read_optional_u64(&parsed, "base_generation"),
        });
    }
    Ok(InboxReceipt::Pending)
}

/// A fully-written publisher snapshot observed after an inbox op's base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotPublication {
    pub published_generation: u64,
    pub model_generation: Option<u64>,
    pub active_sketch_generation: Option<u64>,
}

impl SnapshotPublication {
    pub fn model_published(self) -> bool {
        self.model_generation == Some(self.published_generation)
    }

    pub fn active_sketch_published(self) -> bool {
        self.active_sketch_generation == Some(self.published_generation)
    }

    pub fn snapshot_kind(self) -> &'static str {
        match (self.model_published(), self.active_sketch_published()) {
            (true, true) => "model_and_active_sketch",
            (true, false) => "model",
            (false, true) => "active_sketch",
            (false, false) => "metadata",
        }
    }
}

/// Return a publisher snapshot only when it is newer than the submitted base
/// and has caught up to the current engine revision.
///
/// `generation` alone is not a publish fence: native apply and lightweight
/// keepalives both write heartbeat.json without writing model.json. The native
/// publisher therefore carries `published_generation` plus separate model and
/// active-sketch generations through every heartbeat. Requiring the published
/// generation to equal the engine generation also waits past a later UI mutate
/// that landed while this call was polling.
pub fn snapshot_publication_after(
    session_id: &str,
    base_generation: u64,
) -> Option<SnapshotPublication> {
    let Ok(body) = read_session_file(session_id, "heartbeat.json") else {
        return None;
    };
    let parsed: Value = serde_json::from_str(&body).unwrap_or(json!({}));
    let engine_generation = parsed.get("generation").and_then(Value::as_u64)?;
    let published_generation = parsed.get("published_generation").and_then(Value::as_u64)?;
    if published_generation <= base_generation || published_generation != engine_generation {
        return None;
    }
    if parsed
        .get("session_id")
        .and_then(Value::as_str)
        .is_some_and(|published_session| published_session != session_id)
    {
        return None;
    }
    Some(SnapshotPublication {
        published_generation,
        model_generation: read_optional_u64(&parsed, "model_generation"),
        active_sketch_generation: read_optional_u64(&parsed, "active_sketch_generation"),
    })
}

fn empty_publication_fields() -> Value {
    json!({
        "published_generation": Value::Null,
        "model_generation": Value::Null,
        "active_sketch_generation": Value::Null,
        "model_published": false,
        "active_sketch_published": false,
        "snapshot_kind": Value::Null,
    })
}

fn insert_publication_fields(target: &mut Value, fields: &Value) {
    let (Some(target), Some(fields)) = (target.as_object_mut(), fields.as_object()) else {
        return;
    };
    for (key, value) in fields {
        target.insert(key.clone(), value.clone());
    }
}

fn clamp_await_timeout_ms(timeout_ms: u64) -> u64 {
    timeout_ms.min(AWAIT_APPLY_MAX_TIMEOUT_MS)
}

/// Poll disk until the inbox seq has an applied/failed receipt and (for
/// applied) an explicit publisher generation has caught up to the engine, or
/// until `timeout_ms` elapses. `timeout_ms == 0` is a single observation.
///
/// Does **not** write `model.json`. Does **not** claim in-process co-link.
pub fn await_inbox_apply(
    session_id: &str,
    seq: u64,
    timeout_ms: u64,
    poll_ms: u64,
) -> Result<Value, String> {
    require_valid_session_id(session_id)?;
    let timeout_ms = clamp_await_timeout_ms(timeout_ms);
    let poll_ms = poll_ms.max(1).min(1_000);
    let started = now_ms();
    let deadline = started.saturating_add(timeout_ms);

    loop {
        let elapsed_ms = now_ms().saturating_sub(started);
        let receipt = inbox_op_receipt(session_id, seq)?;
        let current_generation = read_heartbeat_generation(session_id).ok();

        match receipt {
            InboxReceipt::Failed {
                error,
                name,
                base_generation,
            } => {
                let mut result = json!({
                    "status": "failed",
                    "timed_out": false,
                    "seq": seq,
                    "session_id": session_id,
                    "name": name,
                    "error": error,
                    "base_generation": base_generation,
                    "current_generation": current_generation,
                    "applied": false,
                    "dead_lettered": true,
                    "published": false,
                    "refreshed": false,
                    "session_mode": "ui_owned_apply",
                    "writeback": false,
                    "elapsed_ms": elapsed_ms,
                    "hint": "inbox op was dead-lettered; cad_refresh will not see a successful apply",
                });
                insert_publication_fields(&mut result, &empty_publication_fields());
                return Ok(result);
            }
            InboxReceipt::Applied {
                base_generation,
                name,
            } => {
                if let Some(publication) = snapshot_publication_after(session_id, base_generation) {
                    let model_published = publication.model_published();
                    let active_sketch_published = publication.active_sketch_published();
                    let hint = if model_published {
                        "UI applied and completed model snapshot published; call cad_refresh (or await with refresh:true) to load it"
                    } else if active_sketch_published {
                        "UI applied and active-sketch snapshot published; completed model.json is unchanged, so MCP model refresh is intentionally skipped"
                    } else {
                        "UI applied and publisher metadata advanced, but no completed model or active-sketch snapshot was published"
                    };
                    return Ok(json!({
                        "status": "applied",
                        "timed_out": false,
                        "seq": seq,
                        "session_id": session_id,
                        "name": name,
                        "base_generation": base_generation,
                        "current_generation": publication.published_generation,
                        "applied": true,
                        "dead_lettered": false,
                        "published": true,
                        "published_generation": publication.published_generation,
                        "model_generation": publication.model_generation,
                        "active_sketch_generation": publication.active_sketch_generation,
                        "model_published": model_published,
                        "active_sketch_published": active_sketch_published,
                        "snapshot_kind": publication.snapshot_kind(),
                        "refreshed": false,
                        "session_mode": "ui_owned_apply",
                        "writeback": false,
                        "elapsed_ms": elapsed_ms,
                        "hint": hint,
                    }));
                }
                // Applied, but the explicit publisher generation has not caught
                // up to the current engine generation yet.
                if timeout_ms == 0 || now_ms() >= deadline {
                    let mut result = json!({
                        "status": "timeout",
                        "timed_out": true,
                        "seq": seq,
                        "session_id": session_id,
                        "name": name,
                        "base_generation": base_generation,
                        "current_generation": current_generation,
                        "applied": true,
                        "dead_lettered": false,
                        "published": false,
                        "refreshed": false,
                        "session_mode": "ui_owned_apply",
                        "writeback": false,
                        "elapsed_ms": elapsed_ms,
                        "hint": "apply receipt present but publisher snapshot has not caught up to the engine generation; retry cad_await_apply",
                    });
                    insert_publication_fields(&mut result, &empty_publication_fields());
                    return Ok(result);
                }
            }
            InboxReceipt::Pending => {
                if timeout_ms == 0 || now_ms() >= deadline {
                    let status = if timeout_ms == 0 {
                        "pending"
                    } else {
                        "timeout"
                    };
                    let mut result = json!({
                        "status": status,
                        "timed_out": timeout_ms != 0,
                        "seq": seq,
                        "session_id": session_id,
                        "current_generation": current_generation,
                        "applied": false,
                        "dead_lettered": false,
                        "published": false,
                        "refreshed": false,
                        "session_mode": "ui_owned_apply",
                        "writeback": false,
                        "elapsed_ms": elapsed_ms,
                        "hint": "still waiting for UI inbox apply receipt (inbox/applied/<seq>.json or inbox/failed/<seq>.json)",
                    });
                    insert_publication_fields(&mut result, &empty_publication_fields());
                    return Ok(result);
                }
            }
        }

        let remaining = deadline.saturating_sub(now_ms());
        let sleep_ms = poll_ms.min(remaining.max(1));
        std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
    }
}

/// Headless / detached status probe — not an error.
pub fn not_attached_status_json() -> Value {
    json!({
        "attached": false,
        "code": "not_attached",
        "session_id": Value::Null,
        "window_id": Value::Null,
        "document_id": Value::Null,
        "attached_generation": Value::Null,
        "generation": Value::Null,
        "published_generation": Value::Null,
        "model_generation": Value::Null,
        "active_sketch_generation": Value::Null,
        "stale": false,
        "heartbeat_stale": Value::Null,
        "heartbeat_age_ms": Value::Null,
        "heartbeat_kind": Value::Null,
        "pending_inbox": [],
        "pending_inbox_count": 0,
        "last_apply_receipt": Value::Null,
        "session_mode": "headless",
        "writeback": false,
        "hint": "no session attached; cad_session_status is a status probe (not an error). call cad_attach to observe a published session",
    })
}

/// Highest applied/failed inbox receipt for a session, if any.
pub fn last_apply_receipt(session_id: &str) -> Result<Option<Value>, String> {
    require_valid_session_id(session_id)?;
    let inbox = session_dir().join(session_id).join("inbox");
    let max_applied = inbox_seqs_in(&inbox.join("applied")).into_iter().max();
    let max_failed = inbox_seqs_in(&inbox.join("failed")).into_iter().max();
    let seq = match (max_applied, max_failed) {
        (Some(a), Some(f)) => a.max(f),
        (Some(a), None) => a,
        (None, Some(f)) => f,
        (None, None) => return Ok(None),
    };
    Ok(Some(match inbox_op_receipt(session_id, seq)? {
        InboxReceipt::Applied {
            base_generation,
            name,
        } => json!({
            "seq": seq,
            "status": "applied",
            "base_generation": base_generation,
            "name": name,
        }),
        InboxReceipt::Failed {
            error,
            name,
            base_generation,
        } => json!({
            "seq": seq,
            "status": "failed",
            "error": error,
            "name": name,
            "base_generation": base_generation,
        }),
        InboxReceipt::Pending => json!({
            "seq": seq,
            "status": "pending",
        }),
    }))
}

/// Structured status for an attached session: attach generation vs live
/// heartbeat/engine generation, age/stale, pending inbox, last receipt.
/// Surfaces only fields the publisher / inbox protocol already writes.
///
/// All heartbeat-derived fields come from **one** `heartbeat.json` parse so a
/// publisher rewrite between reads cannot yield impossible combinations.
pub fn session_status_json(
    session_id: &str,
    attached_generation: Option<u64>,
) -> Result<Value, String> {
    require_valid_session_id(session_id)?;
    let snapshot = read_heartbeat_snapshot(session_id);
    let parsed = snapshot.as_ref().cloned().unwrap_or(json!({}));
    let identity = session_identity_from_snapshot(session_id, &parsed);
    let live_generation = read_optional_u64(&parsed, "generation");
    let published_generation = parsed
        .get("published_generation")
        .cloned()
        .unwrap_or(Value::Null);
    let model_generation = parsed
        .get("model_generation")
        .cloned()
        .unwrap_or(Value::Null);
    let active_sketch_generation = parsed
        .get("active_sketch_generation")
        .cloned()
        .unwrap_or(Value::Null);
    let heartbeat_kind = parsed.get("kind").cloned().unwrap_or(Value::Null);

    let heartbeat = heartbeat_meta_from_snapshot(snapshot.as_ref());
    let heartbeat_stale = heartbeat["stale"].as_bool().unwrap_or(true);
    let heartbeat_age_ms = heartbeat["age_ms"].clone();

    // Unknown attachment fence is not fresh (recovery attach without heartbeat).
    let stale = match (attached_generation, live_generation) {
        (Some(attached), Some(live)) => attached != live,
        (Some(_), None) => true,
        (None, _) => true,
    };
    let pending = pending_inbox_seqs(session_id)?;
    let last_receipt = last_apply_receipt(session_id)?;
    let hint = if attached_generation.is_none() {
        "Attached without a usable publication fence; treat snapshot freshness as unknown — cad_refresh after UI publishes heartbeat generations"
    } else if stale {
        "UI generation advanced since attach/refresh; cad_refresh (or cad_await_apply after submit) to catch up"
    } else {
        "Attached snapshot matches live heartbeat generation"
    };
    Ok(json!({
        "attached": true,
        "code": "attached",
        "session_id": session_id,
        "window_id": identity.window_id,
        "document_id": identity.document_id.unwrap_or_else(|| session_id.to_string()),
        "attached_generation": attached_generation,
        "generation": live_generation,
        "published_generation": published_generation,
        "model_generation": model_generation,
        "active_sketch_generation": active_sketch_generation,
        "stale": stale,
        "heartbeat_stale": heartbeat_stale,
        "heartbeat_age_ms": heartbeat_age_ms,
        "heartbeat_kind": heartbeat_kind,
        "pending_inbox": pending,
        "pending_inbox_count": pending.len(),
        "last_apply_receipt": last_receipt,
        "heartbeat": heartbeat,
        "session_mode": if parsed["interface_version"] == 1 { "live" } else { "read_only_snapshot" },
        "writeback": false,
        "hint": hint,
    }))
}

/// Test/helper: replace model.json and bump heartbeat generation.
/// Used after a successful host apply on a **separate** SketchManager.
/// Not an MCP writeback path — the live UI publisher is the production writer.
pub fn publish_applied_snapshot(session_id: &str, model_json: &str) -> Result<u64, String> {
    let next = read_heartbeat_generation(session_id)
        .unwrap_or(0)
        .saturating_add(1);
    write_session(session_id, "model.json", model_json)?;
    let heartbeat = serde_json::to_string_pretty(&json!({
        "updated_ms": now_ms(),
        "generation": next,
        "published_generation": next,
        "model_generation": next,
        "active_sketch_generation": Value::Null,
        "session_id": session_id,
        "session_mode": "ui_owned_apply",
        "writeback": false,
        "kind": "snapshot",
    }))
    .map_err(|error| format!("encode heartbeat: {error}"))?;
    write_session(session_id, "heartbeat.json", &heartbeat)?;
    Ok(next)
}

/// Test UUIDs remain distinct even when sessions are created in one clock tick.
#[cfg(test)]
pub fn test_session_uuid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:08x}-{:04x}-4{:03x}-8000-{:012x}",
        now_ms() as u32,
        std::process::id() & 0xffff,
        (std::process::id() >> 16) & 0xfff,
        serial
    )
}

/// Serialize tests that mutate `NBCAD_SESSION_DIR`.
#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_ids_are_unique_in_a_burst() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..4096 {
            let id = test_session_uuid();
            assert!(is_valid_session_id(&id));
            assert!(
                seen.insert(id),
                "test sessions must never share a directory"
            );
        }
    }

    #[test]
    fn ui_requests_reject_invalid_actions_targets_and_pacing_before_io() {
        for action in ["click", "double_click", "context_menu", "set_value", "key"] {
            assert!(request_ui(&json!({"action":action}), None)
                .unwrap_err()
                .contains("target"));
        }
        assert!(request_ui(&json!({"action":"eval"}), None)
            .unwrap_err()
            .contains("unknown"));
        for pace in [
            json!(-1),
            json!(2001),
            json!(0.5),
            json!("fast"),
            Value::Null,
        ] {
            assert!(request_ui(&json!({"pace_ms":pace}), None)
                .unwrap_err()
                .contains("pace_ms"));
        }
    }

    #[test]
    fn presentation_controls_are_typed_and_bounded_before_live_io() {
        for command in [
            "configure",
            "note",
            "pause",
            "resume",
            "step",
            "stop",
            "status",
            "finish",
        ] {
            validate_presentation(&json!({"command":command,"mode":"present","speed":2.5,"duration_ms":800,"text":"A repeatable cut\nfrom the datum","chapter":"Pickets","step_index":2,"step_count":20})).unwrap();
        }
        for arguments in [
            json!({"command":"eval"}),
            json!({"command":"status","mode":"random"}),
            json!({"command":"configure","speed":0}),
            json!({"command":"configure","speed":16.1}),
            json!({"command":"note","duration_ms":-1}),
            json!({"command":"note","duration_ms":10001}),
            json!({"command":"note","duration_ms":0.5}),
            json!({"command":"note","text":true}),
            json!({"command":"note","text":"control\u{0001}"}),
            json!({"command":"note","chapter":"x".repeat(201)}),
            json!({"command":"note","step_index":3,"step_count":2}),
        ] {
            assert!(validate_presentation(&arguments).is_err(), "{arguments}");
        }
    }

    #[test]
    fn view_focus_rejects_ambiguous_targets_and_invalid_duration() {
        for arguments in [
            json!({"target":"active_sketch"}),
            json!({"body_id":5,"duration_ms":0}),
            json!({"component_id":3,"duration_ms":10000}),
        ] {
            validate_view(&arguments).unwrap();
        }
        for arguments in [
            json!({"target":"body"}),
            json!({"body_id":-1}),
            json!({"component_id":"3"}),
            json!({"body_id":1,"component_id":3}),
            json!({"duration_ms":10001}),
        ] {
            assert!(validate_view(&arguments).is_err(), "{arguments}");
        }
    }

    #[test]
    fn view_request_needs_live_ui_ack_but_no_model() {
        let _guard = ENV_LOCK.lock().unwrap();
        let id = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-view-{}-{id}", std::process::id()));
        let previous = std::env::var_os("NBCAD_SESSION_DIR");
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(
            &id,
            "heartbeat.json",
            &json!({"updated_ms":now_ms(),"generation":1}).to_string(),
        )
        .unwrap();
        let ui_dir = dir.join(&id).join("controls");
        let ui = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                if let Ok(entries) = fs::read_dir(&ui_dir) {
                    for entry in entries.flatten() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".request.json")
                        {
                            continue;
                        }
                        let body: Value =
                            serde_json::from_str(&fs::read_to_string(entry.path()).unwrap())
                                .unwrap();
                        assert_eq!(body["view"], "top");
                        assert_eq!(body["fit"], true);
                        assert_eq!(body["body_id"], 7);
                        assert_eq!(body["duration_ms"], 450);
                        let result =
                            ui_dir.join(format!("{}.result.json", body["id"].as_str().unwrap()));
                        let temporary = result.with_extension("tmp");
                        fs::write(
                            &temporary,
                            r#"{"status":"applied","camera":{"target":[0,0,0]}}"#,
                        )
                        .unwrap();
                        fs::rename(temporary, result).unwrap();
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            panic!("UI did not receive view request");
        });
        let result = request_ui(
            &json!({"action":"view","session_id":id,"view":"top","fit":true,"body_id":7,"duration_ms":450}),
            None,
        )
        .unwrap();
        ui.join().unwrap();
        assert_eq!(result["status"], "applied");
        assert!(!dir.join(&id).join("model.json").exists());
        assert!(request_ui(&json!({"action":"view","view":"top"}), None).is_err());
        assert!(request_ui(
            &json!({"action":"view","session_id":id,"view":"invalid"}),
            None
        )
        .is_err());
        write_session(&id, "heartbeat.json", r#"{"updated_ms":0,"generation":1}"#).unwrap();
        assert!(request_ui(&json!({"action":"view","session_id":id,"view":"top"}), None).is_err());
        if let Some(value) = previous {
            std::env::set_var("NBCAD_SESSION_DIR", value);
        } else {
            std::env::remove_var("NBCAD_SESSION_DIR");
        }
        let _ = fs::remove_dir_all(dir);
    }

    fn write_process_lease(root: &Path, process_id: &str, updated_ms: u64, windows: Value) {
        let processes = root.join("_ui").join("processes");
        fs::create_dir_all(&processes).unwrap();
        fs::write(
            processes.join(format!("{process_id}.json")),
            serde_json::to_string_pretty(&json!({
                "process_instance_id": process_id,
                "updated_ms": updated_ms,
                "windows": windows,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn uuid_v4_validation_accepts_and_rejects() {
        assert!(is_valid_session_id("123e4567-e89b-42d3-a456-426614174000"));
        assert!(!is_valid_session_id("123e4567-e89b-12d3-a456-426614174000")); // not version 4
        assert!(!is_valid_session_id("My Document"));
        assert!(!is_valid_session_id("../escape"));
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn session_snapshot_roundtrip_skips_control_and_non_uuid() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-test-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        fs::create_dir_all(dir.join("_ui")).unwrap();
        fs::create_dir_all(dir.join("document-name")).unwrap();
        let listed = list_sessions().unwrap();
        assert_eq!(listed, vec![unique.clone()]);
        assert!(!listed.iter().any(|session| session == "_ui"));
        let body = require_model_json(&unique).unwrap();
        assert!(body.contains("\"version\":1"));
        let list = sessions_list_json();
        assert_eq!(list["sessions"][0], unique);
        assert_eq!(list["session_details"][0]["has_model"], true);
        assert_eq!(list["session_details"][0]["heartbeat"]["stale"], false);
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_and_resolve_expose_window_and_document_ids() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let other = format!(
            "00000000-0000-4000-8000-{:012x}",
            (now_ms().wrapping_add(7)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-mw-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_process_lease(
            &dir,
            "proc-list",
            now_ms(),
            json!([
                {
                    "window_id": "main",
                    "active_document_id": "tab-a",
                    "active_session_id": unique,
                },
                {
                    "window_id": "secondary",
                    "active_document_id": "tab-b",
                    "active_session_id": other,
                }
            ]),
        );
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}","window_id":"main","document_id":"tab-a","project_session_id":"tab-a","process_instance_id":"proc-list"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        write_session(&other, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &other,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"session_id":"{other}","window_id":"secondary","document_id":"tab-b","project_session_id":"tab-b","process_instance_id":"proc-list"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let list = sessions_list_json();
        assert_eq!(list["session_details"].as_array().unwrap().len(), 2);
        let main = list["session_details"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["session_id"] == unique)
            .cloned()
            .unwrap();
        assert_eq!(main["window_id"], "main");
        assert_eq!(main["document_id"], "tab-a");
        assert_eq!(list["windows"].as_array().unwrap().len(), 2);
        let main_window = list["windows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["window_id"] == "main")
            .cloned()
            .unwrap();
        assert_eq!(main_window["documents"].as_array().unwrap().len(), 1);
        assert_eq!(main_window["active_document_id"], "tab-a");

        let by_window = resolve_attach_target(None, Some("main"), None).unwrap();
        assert_eq!(by_window.session_id, unique);
        assert_eq!(by_window.window_id.as_deref(), Some("main"));
        let by_document = resolve_attach_target(None, None, Some("tab-b")).unwrap();
        assert_eq!(by_document.session_id, other);
        let by_uuid_document = resolve_attach_target(None, None, Some(&unique)).unwrap();
        assert_eq!(by_uuid_document.session_id, unique);
        assert!(resolve_attach_target(Some(&unique), Some("secondary"), None).is_err());
        assert!(resolve_attach_target(None, Some("missing-window"), None).is_err());
        assert!(resolve_attach_target(None, None, None).is_err());

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_intersects_window_and_document_before_ambiguity() {
        let _guard = ENV_LOCK.lock().unwrap();
        let tab_a = test_session_uuid();
        let tab_b = format!(
            "00000000-0000-4000-8000-{:012x}",
            (now_ms().wrapping_add(11)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-intersect-{tab_a}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        for (sid, doc) in [(&tab_a, "tab-a"), (&tab_b, "tab-b")] {
            write_session(sid, "model.json", r#"{"version":1}"#).unwrap();
            write_session(
                sid,
                "heartbeat.json",
                &format!(
                    r#"{{"updated_ms":{},"generation":1,"session_id":"{sid}","window_id":"main","document_id":"{doc}","project_session_id":"{doc}"}}"#,
                    now_ms()
                ),
            )
            .unwrap();
        }

        // window_id alone is ambiguous with two tabs in main.
        let err = resolve_attach_target(None, Some("main"), None).expect_err("ambiguous window");
        assert!(err.contains("ambiguous"), "{err}");

        // Combined window + document selects exactly one.
        let hit = resolve_attach_target(None, Some("main"), Some("tab-a")).unwrap();
        assert_eq!(hit.session_id, tab_a);
        assert_eq!(hit.document_id.as_deref(), Some("tab-a"));

        let list = sessions_list_json();
        assert_eq!(list["windows"].as_array().unwrap().len(), 1);
        let main = &list["windows"][0];
        assert_eq!(main["window_id"], "main");
        assert_eq!(main["documents"].as_array().unwrap().len(), 2);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn closed_tombstone_and_process_instance_shape_live_windows() {
        let _guard = ENV_LOCK.lock().unwrap();
        let live = test_session_uuid();
        let closed = format!(
            "00000000-0000-4000-8000-{:012x}",
            (now_ms().wrapping_add(13)) & 0xffffffffffff
        );
        let prior = format!(
            "00000000-0000-4000-8000-{:012x}",
            (now_ms().wrapping_add(17)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-tombstone-{live}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_process_lease(
            &dir,
            "proc-live",
            now_ms(),
            json!([{
                "window_id": "main",
                "active_document_id": "open",
                "active_session_id": live,
            }]),
        );

        write_session(&live, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &live,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{live}","window_id":"main","document_id":"open","project_session_id":"open","process_instance_id":"proc-live"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        write_session(&closed, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &closed,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{closed}","window_id":"main","document_id":"gone","project_session_id":"gone","process_instance_id":"proc-live"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        write_closed_tombstone(&closed).unwrap();

        write_session(&prior, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &prior,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{prior}","window_id":"main","document_id":"old-run","project_session_id":"old-run","process_instance_id":"proc-old"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let list = sessions_list_json();
        assert_eq!(list["process_instance_id"], "proc-live");
        assert_eq!(list["windows"].as_array().unwrap().len(), 1);
        assert_eq!(list["windows"][0]["documents"].as_array().unwrap().len(), 1);
        assert_eq!(list["windows"][0]["active_document_id"], "open");

        // Closed / prior-run tabs are not window-selectable.
        assert!(resolve_attach_target(None, Some("main"), Some("gone")).is_err());
        assert!(resolve_attach_target(None, None, Some("old-run")).is_err());
        // Explicit session_id still resolves the closed dir for recovery.
        let recovered = resolve_attach_target(Some(&closed), None, None).unwrap();
        assert_eq!(recovered.session_id, closed);

        clear_closed_tombstone(&closed).unwrap();
        assert!(!is_session_closed(&closed));

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn process_lease_owns_liveness_without_expiring_inactive_tabs() {
        let _guard = ENV_LOCK.lock().unwrap();
        let active = test_session_uuid();
        let inactive = format!(
            "00000000-0000-4000-8000-{:012x}",
            (now_ms().wrapping_add(19)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-lease-{active}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);

        write_process_lease(
            &dir,
            "proc-tabs",
            now_ms(),
            json!([{
                "window_id": "main",
                "active_document_id": "tab-a",
                "active_session_id": active,
            }]),
        );
        for (session_id, document_id, updated_ms) in [
            (&active, "tab-a", now_ms().saturating_sub(120_000)),
            (&inactive, "tab-b", now_ms()),
        ] {
            write_session(session_id, "model.json", r#"{"version":1}"#).unwrap();
            write_session(
                session_id,
                "heartbeat.json",
                &format!(
                    r#"{{"updated_ms":{updated_ms},"generation":1,"session_id":"{session_id}","window_id":"main","document_id":"{document_id}","project_session_id":"{document_id}","process_instance_id":"proc-tabs"}}"#
                ),
            )
            .unwrap();
        }

        let list = sessions_list_json();
        assert_eq!(list["windows"].as_array().unwrap().len(), 1);
        let main = &list["windows"][0];
        assert_eq!(main["documents"].as_array().unwrap().len(), 2);
        assert_eq!(main["active_document_id"], "tab-a");
        assert_eq!(main["active_session_id"], active);
        assert!(list["session_details"]
            .as_array()
            .unwrap()
            .iter()
            .all(|detail| detail["live_for_windows"] == true));

        // The active document is authoritative state in the process lease. A
        // newer inactive-tab heartbeat must never steal it.
        let picked = resolve_attach_target(None, Some("main"), Some("tab-a")).unwrap();
        assert_eq!(picked.session_id, active);

        // A live process with no lease entry for this window means the window
        // was destroyed; its retained session directories must not reappear.
        write_process_lease(&dir, "proc-tabs", now_ms(), json!([]));
        assert!(sessions_list_json()["windows"]
            .as_array()
            .unwrap()
            .is_empty());

        write_process_lease(
            &dir,
            "proc-tabs",
            now_ms().saturating_sub(PROCESS_LEASE_STALE_MS + 1),
            json!([{
                "window_id": "main",
                "active_document_id": "tab-a",
                "active_session_id": active,
            }]),
        );
        let expired = sessions_list_json();
        assert!(expired["windows"].as_array().unwrap().is_empty());
        assert!(resolve_attach_target(None, Some("main"), Some("tab-a")).is_err());
        // Explicit UUID remains a recovery path after process exit/crash.
        assert_eq!(
            resolve_attach_target(Some(&active), None, None)
                .unwrap()
                .session_id,
            active
        );

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiple_process_leases_are_projected_independently() {
        let _guard = ENV_LOCK.lock().unwrap();
        let first = test_session_uuid();
        let second = format!(
            "00000000-0000-4000-8000-{:012x}",
            (now_ms().wrapping_add(23)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-processes-{first}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);

        for (process_id, session_id, document_id) in
            [("proc-a", &first, "doc-a"), ("proc-b", &second, "doc-b")]
        {
            write_process_lease(
                &dir,
                process_id,
                now_ms(),
                json!([{
                    "window_id": "main",
                    "active_document_id": document_id,
                    "active_session_id": session_id,
                }]),
            );
            write_session(session_id, "model.json", r#"{"version":1}"#).unwrap();
            write_session(
                session_id,
                "heartbeat.json",
                &format!(
                    r#"{{"updated_ms":{},"generation":1,"session_id":"{session_id}","window_id":"main","document_id":"{document_id}","project_session_id":"{document_id}","process_instance_id":"{process_id}"}}"#,
                    now_ms()
                ),
            )
            .unwrap();
        }

        let list = sessions_list_json();
        assert_eq!(list["process_instance_id"], Value::Null);
        assert_eq!(list["process_instance_ids"].as_array().unwrap().len(), 2);
        assert_eq!(list["process_instances"].as_array().unwrap().len(), 2);
        assert_eq!(
            list["process_lease_ms"].as_u64(),
            Some(PROCESS_LEASE_STALE_MS)
        );
        assert_eq!(list["windows"].as_array().unwrap().len(), 2);
        let projected: BTreeMap<_, _> = list["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|window| {
                (
                    window["process_instance_id"].as_str().unwrap(),
                    window["active_document_id"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(projected.get("proc-a"), Some(&"doc-a"));
        assert_eq!(projected.get("proc-b"), Some(&"doc-b"));
        assert!(resolve_attach_target(None, Some("main"), None)
            .unwrap_err()
            .contains("ambiguous"));

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_singleton_is_a_freshness_checked_migration_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        let session_id = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-legacy-{session_id}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        fs::create_dir_all(dir.join("_ui")).unwrap();
        fs::write(
            dir.join("_ui").join("process.json"),
            serde_json::to_string(&json!({
                "process_instance_id": "proc-legacy",
                "updated_ms": now_ms(),
            }))
            .unwrap(),
        )
        .unwrap();
        write_session(&session_id, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &session_id,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{session_id}","window_id":"main","document_id":"legacy-tab","project_session_id":"legacy-tab","process_instance_id":"proc-legacy"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let fresh = sessions_list_json();
        assert_eq!(fresh["windows"].as_array().unwrap().len(), 1);
        assert_eq!(fresh["windows"][0]["active_document_id"], Value::Null);

        fs::write(
            dir.join("_ui").join("process.json"),
            serde_json::to_string(&json!({
                "process_instance_id": "proc-legacy",
                "updated_ms": now_ms().saturating_sub(PROCESS_LEASE_STALE_MS + 1),
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(sessions_list_json()["windows"]
            .as_array()
            .unwrap()
            .is_empty());

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_session_rejects_non_uuid() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-bad-{}", now_ms()));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        assert!(write_session("not-a-uuid", "model.json", "{}").is_err());
        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_write_and_stale_apply_are_generation_locked() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-inbox-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":3}}"#, now_ms()),
        )
        .unwrap();

        let seq = write_inbox_op(
            &unique,
            &InboxOp::unstamped("solid_mirror".to_string(), json!({"body_ids": [1]}), 3),
        )
        .unwrap();
        assert_eq!(seq, 1);
        let pending = pending_inbox_seqs(&unique).unwrap();
        assert_eq!(pending, vec![1]);
        let body = read_session_file(&unique, "inbox/1.json").unwrap();
        assert!(body.contains("solid_mirror"));
        assert!(body.contains("base_generation"));

        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":4}}"#, now_ms()),
        )
        .unwrap();
        // Valid follow-up at the new generation. After seq 1 dead-letters,
        // the next apply_inbox_op must take the lowest remaining pending.
        let seq2 = write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "AfterStaleHead"}),
                4,
            ),
        )
        .unwrap();
        assert_eq!(seq2, 2);
        let err = apply_inbox_op(&unique, |_name, _args| Ok(json!({"applied": true})))
            .expect_err("stale base_generation must not apply");
        let parsed: Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["code"], "generation_conflict");
        assert_eq!(parsed["writeback"], false);
        assert_eq!(parsed["session_mode"], "ui_owned_apply");
        assert_eq!(
            pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "stale head must dead-letter so seq 2 can apply"
        );
        let failed = session_dir().join(&unique).join("inbox/failed/1.json");
        assert!(failed.exists(), "expected inbox/failed/1.json");
        let failed_body = fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("generation_conflict"),
            "dead-letter must record the conflict reason: {failed_body}"
        );

        let applied = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "AfterStaleHead");
            Ok(json!({"name": "AfterStaleHead"}))
        })
        .unwrap();
        assert_eq!(applied.seq, 2);
        assert_eq!(applied.op.name, "cad_set_document_name");

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_inbox_alloc_gives_distinct_durable_entries() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-inbox-race-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();

        const THREADS: usize = 16;
        const PER_THREAD: usize = 8;
        let session_id = unique.clone();
        let mut handles = Vec::new();
        for thread in 0..THREADS {
            let sid = session_id.clone();
            handles.push(std::thread::spawn(move || {
                let mut allocated = Vec::new();
                for index in 0..PER_THREAD {
                    let marker = format!("{thread}-{index}");
                    let seq = write_inbox_op(
                        &sid,
                        &InboxOp::unstamped(
                            "solid_mirror".to_string(),
                            json!({"body_ids": [1], "marker": marker}),
                            1,
                        ),
                    )
                    .expect("exclusive inbox reserve must succeed");
                    allocated.push((seq, format!("{thread}-{index}")));
                }
                allocated
            }));
        }
        let mut all = Vec::new();
        for handle in handles {
            all.extend(handle.join().expect("inbox alloc thread"));
        }
        let expected = THREADS * PER_THREAD;
        assert_eq!(all.len(), expected);
        let mut seqs: Vec<u64> = all.iter().map(|(seq, _)| *seq).collect();
        seqs.sort_unstable();
        let mut unique_seqs = seqs.clone();
        unique_seqs.dedup();
        assert_eq!(
            unique_seqs.len(),
            expected,
            "duplicate inbox seq under contention: {seqs:?}"
        );
        for (seq, marker) in &all {
            let body = read_session_file(&session_id, &format!("inbox/{seq}.json")).unwrap();
            assert!(
                body.contains(marker),
                "seq {seq} lost marker {marker}: {body}"
            );
        }
        assert_eq!(pending_inbox_seqs(&session_id).unwrap().len(), expected);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_inbox_json_is_dead_lettered_and_unblocks_queue() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-badjson-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        let inbox = session_dir().join(&unique).join("inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join("1.json"), "{not-json").unwrap();
        let seq = write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "AfterBadJson"}),
                1,
            ),
        )
        .unwrap();
        assert_eq!(seq, 2);
        let err = apply_inbox_op(&unique, |_name, _args| Ok(json!({"applied": true})))
            .expect_err("malformed json must fail apply");
        assert!(
            err.contains("invalid inbox") || err.contains("expected"),
            "expected a JSON parse error, got {err}"
        );
        assert_eq!(
            pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "malformed head must dead-letter so seq 2 can apply"
        );
        let failed = session_dir().join(&unique).join("inbox/failed/1.json");
        assert!(failed.exists(), "expected inbox/failed/1.json");
        let failed_body = fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("raw") && failed_body.contains("{not-json"),
            "dead-letter must keep the raw malformed bytes: {failed_body}"
        );

        let applied = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "AfterBadJson");
            Ok(json!({"name": "AfterBadJson"}))
        })
        .unwrap();
        assert_eq!(applied.seq, 2);
        assert_eq!(applied.op.name, "cad_set_document_name");

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_base_generation_second_op_is_dead_lettered() {
        // Match native: first apply + publish advances generation; the leftover
        // same-base head dead-letters with a reason so later seqs can run.
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-samebase-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "First"}),
                1,
            ),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "Second"}),
                1,
            ),
        )
        .unwrap();
        let first = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "First");
            Ok(json!({"name": "First"}))
        })
        .unwrap();
        assert_eq!(first.seq, 1);
        publish_applied_snapshot(&unique, r#"{"version":1,"name":"First"}"#).unwrap();
        assert_eq!(pending_inbox_seqs(&unique).unwrap(), vec![2]);

        let err = apply_inbox_op(&unique, |_name, _args| {
            panic!("host must not run on generation_conflict")
        })
        .expect_err("same-base leftover must conflict");
        let parsed: Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["code"], "generation_conflict");
        assert!(
            pending_inbox_seqs(&unique).unwrap().is_empty(),
            "conflicted same-base head must dead-letter"
        );
        let failed = session_dir().join(&unique).join("inbox/failed/2.json");
        assert!(failed.exists(), "expected inbox/failed/2.json");
        let failed_body = fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("generation_conflict"),
            "dead-letter must record the reason: {failed_body}"
        );

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsupported_inbox_mutate_is_dead_lettered_and_unblocks_queue() {
        // Match native: a head that is not in the shared mutate map must
        // dead-letter before host_apply so later seqs can run.
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-unsupported-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped("assembly_document".to_string(), json!({}), 1),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "AfterUnsupported"}),
                1,
            ),
        )
        .unwrap();
        assert!(
            nbcad_mcp_mutate::lookup_mutate("assembly_document").is_none(),
            "assembly_document is inspect-only and must not be an inbox mutate"
        );
        let err = apply_inbox_op(&unique, |_name, _args| {
            panic!("host must not run on unsupported inbox mutate")
        })
        .expect_err("unsupported mutate must fail apply");
        assert!(
            err.contains("unsupported inbox mutate") && err.contains("assembly_document"),
            "expected unsupported mutate error, got {err}"
        );
        assert_eq!(
            pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "unsupported head must dead-letter so seq 2 can apply"
        );
        let failed = session_dir().join(&unique).join("inbox/failed/1.json");
        assert!(failed.exists(), "expected inbox/failed/1.json");
        let failed_body = fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("unsupported inbox mutate"),
            "dead-letter must record the reason: {failed_body}"
        );

        let applied = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "AfterUnsupported");
            Ok(json!({"name": "AfterUnsupported"}))
        })
        .unwrap();
        assert_eq!(applied.seq, 2);
        assert_eq!(applied.op.name, "cad_set_document_name");

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn already_applied_inbox_seq_second_apply_is_noop() {
        // applyInboxNow of an already-archived seq must not call host again.
        // Native returns applied:false / empty; helper errors "no pending".
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-already-applied-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "Once"}),
                1,
            ),
        )
        .unwrap();
        let mut host_calls = 0u32;
        let first = apply_inbox_op(&unique, |name, arguments| {
            host_calls += 1;
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "Once");
            Ok(json!({"name": "Once"}))
        })
        .unwrap();
        assert_eq!(first.seq, 1);
        assert_eq!(host_calls, 1);
        assert!(
            pending_inbox_seqs(&unique).unwrap().is_empty(),
            "applied seq must leave the pending queue"
        );
        let applied_path = session_dir().join(&unique).join("inbox/applied/1.json");
        assert!(applied_path.exists(), "expected inbox/applied/1.json");

        let err = apply_inbox_op(&unique, |_name, _args| {
            host_calls += 1;
            panic!("host must not run on already-applied seq")
        })
        .expect_err("already-applied seq must be a no-op");
        assert!(
            err.contains("no pending inbox op"),
            "expected empty-inbox no-op, got {err}"
        );
        assert_eq!(
            host_calls, 1,
            "already-applied seq must not host-apply again"
        );
        assert!(pending_inbox_seqs(&unique).unwrap().is_empty());

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_heartbeat_generation_is_dead_lettered_and_unblocks_queue() {
        // Leftover apply reads heartbeat.json generation. If that source is
        // missing/unreadable, treat it as generation_conflict and dead-letter
        // so later seqs are not wedged. Native apply uses in-memory
        // engine_revision and never waits on the file.
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-no-hb-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "MissingHb"}),
                1,
            ),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "AfterMissingHb"}),
                1,
            ),
        )
        .unwrap();
        let hb = session_dir().join(&unique).join("heartbeat.json");
        fs::remove_file(&hb).unwrap();
        let err = apply_inbox_op(&unique, |_name, _args| {
            panic!("host must not run without a heartbeat generation")
        })
        .expect_err("missing heartbeat must generation_conflict");
        let parsed: Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["code"], "generation_conflict");
        assert_eq!(parsed["writeback"], false);
        assert_eq!(parsed["session_mode"], "ui_owned_apply");
        assert_eq!(
            pending_inbox_seqs(&unique).unwrap(),
            vec![2],
            "missing-heartbeat head must dead-letter so seq 2 can apply"
        );
        let failed = session_dir().join(&unique).join("inbox/failed/1.json");
        assert!(failed.exists(), "expected inbox/failed/1.json");
        let failed_body = fs::read_to_string(&failed).unwrap();
        assert!(
            failed_body.contains("generation_conflict"),
            "dead-letter must record the reason: {failed_body}"
        );

        // Restore a matching generation so the leftover helper can apply seq 2.
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        let applied = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "AfterMissingHb");
            Ok(json!({"name": "AfterMissingHb"}))
        })
        .unwrap();
        assert_eq!(applied.seq, 2);
        assert_eq!(applied.op.name, "cad_set_document_name");

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn age_stale_heartbeat_with_matching_generation_still_applies() {
        // Listing staleness (age > HEARTBEAT_STALE_MS) is not a writer lock.
        // Matching generation must apply, leftover and native alike.
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-age-stale-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        let stale_ms = now_ms().saturating_sub(HEARTBEAT_STALE_MS + 5_000);
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{stale_ms},"generation":1}}"#),
        )
        .unwrap();
        let meta = heartbeat_meta(&unique);
        assert_eq!(meta["stale"], true, "fixture must be age-stale: {meta}");
        assert_eq!(meta["generation"], 1);
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "AgeStaleOk"}),
                1,
            ),
        )
        .unwrap();
        let applied = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "AgeStaleOk");
            Ok(json!({"name": "AgeStaleOk"}))
        })
        .unwrap();
        assert_eq!(applied.seq, 1);
        assert!(
            pending_inbox_seqs(&unique).unwrap().is_empty(),
            "matching generation must archive, not dead-letter on age"
        );
        let failed = session_dir().join(&unique).join("inbox/failed/1.json");
        assert!(
            !failed.exists(),
            "age-stale matching gen must not dead-letter"
        );
        let archived = session_dir().join(&unique).join("inbox/applied/1.json");
        assert!(archived.exists(), "expected inbox/applied/1.json");

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_takes_lowest_pending_seq_even_when_higher_exists() {
        // Out-of-order: seq 2 must not apply while seq 1 is still pending.
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-seq-order-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(r#"{{"updated_ms":{},"generation":1}}"#, now_ms()),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "First"}),
                1,
            ),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "Second"}),
                1,
            ),
        )
        .unwrap();
        assert_eq!(pending_inbox_seqs(&unique).unwrap(), vec![1, 2]);
        let first = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "First");
            Ok(json!({"name": "First"}))
        })
        .unwrap();
        assert_eq!(first.seq, 1);
        assert_eq!(pending_inbox_seqs(&unique).unwrap(), vec![2]);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamped_inbox_identity_mismatch_is_dead_lettered_and_unblocks() {
        let _guard = ENV_LOCK.lock().unwrap();
        let session_a = test_session_uuid();
        let session_b = format!(
            "00000000-0000-4000-8000-{:012x}",
            (now_ms().wrapping_add(31)) & 0xffffffffffff
        );
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-id-mismatch-{session_a}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);

        for (sid, window, doc, marker) in [
            (&session_a, "main", "tab-a", "model-a"),
            (&session_b, "secondary", "tab-b", "model-b"),
        ] {
            write_session(
                sid,
                "model.json",
                &format!(r#"{{"version":1,"marker":"{marker}"}}"#),
            )
            .unwrap();
            write_session(
                sid,
                "heartbeat.json",
                &format!(
                    r#"{{"updated_ms":{},"generation":1,"session_id":"{sid}","window_id":"{window}","document_id":"{doc}","project_session_id":"{doc}"}}"#,
                    now_ms()
                ),
            )
            .unwrap();
        }

        // A's stamped op copied into B's inbox — must not apply against B.
        write_inbox_op(
            &session_b,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "FromA"}),
                1,
            )
            .with_identity(&SessionIdentity {
                session_id: session_a.clone(),
                window_id: Some("main".to_string()),
                document_id: Some("tab-a".to_string()),
            }),
        )
        .unwrap();
        // Matching B op behind the mismatched head — must stay unwedged.
        write_inbox_op(
            &session_b,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "FromB"}),
                1,
            )
            .with_identity(&session_identity(&session_b)),
        )
        .unwrap();

        let err = apply_inbox_op(&session_b, |_name, _args| {
            panic!("mismatched identity must not call host_apply")
        })
        .expect_err("stamped A op must not apply on B");
        let parsed: Value = serde_json::from_str(&err).unwrap();
        assert_eq!(parsed["code"], "session_identity_mismatch");
        assert_eq!(parsed["writeback"], false);
        assert_eq!(parsed["session_mode"], "ui_owned_apply");
        assert!(
            session_dir()
                .join(&session_b)
                .join("inbox/failed/1.json")
                .exists(),
            "mismatched head must dead-letter"
        );
        assert_eq!(pending_inbox_seqs(&session_b).unwrap(), vec![2]);
        let model_b = read_session_file(&session_b, "model.json").unwrap();
        assert!(model_b.contains("model-b"));
        assert!(!model_b.contains("FromA"));

        let applied = apply_inbox_op(&session_b, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "FromB");
            Ok(json!({"name": "FromB"}))
        })
        .expect("matching B op must apply after dead-letter");
        assert_eq!(applied.seq, 2);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unstamped_inbox_op_still_applies_compat() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-unstamped-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}","window_id":"main","document_id":"tab"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "Compat"}),
                1,
            ),
        )
        .unwrap();
        let body = read_session_file(&unique, "inbox/1.json").unwrap();
        assert!(
            !body.contains("session_id"),
            "unstamped op omits identity: {body}"
        );
        let applied = apply_inbox_op(&unique, |name, arguments| {
            assert_eq!(name, "cad_set_document_name");
            assert_eq!(arguments["name"], "Compat");
            Ok(json!({"name": "Compat"}))
        })
        .expect("unstamped ops keep current apply behavior");
        assert_eq!(applied.seq, 1);
        assert!(applied.op.session_id.is_none());
        assert!(applied.op.window_id.is_none());

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inbox_op_receipt_pending_applied_failed() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-receipt-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        fs::create_dir_all(dir.join(&unique)).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();

        assert_eq!(inbox_op_receipt(&unique, 1).unwrap(), InboxReceipt::Pending);

        write_inbox_op(
            &unique,
            &InboxOp::unstamped("cad_set_document_name".to_string(), json!({"name": "A"}), 1),
        )
        .unwrap();
        assert_eq!(inbox_op_receipt(&unique, 1).unwrap(), InboxReceipt::Pending);

        apply_inbox_op(&unique, |_name, _args| Ok(json!({}))).expect("apply should archive");
        match inbox_op_receipt(&unique, 1).unwrap() {
            InboxReceipt::Applied {
                base_generation,
                name,
            } => {
                assert_eq!(base_generation, 1);
                assert_eq!(name.as_deref(), Some("cad_set_document_name"));
            }
            other => panic!("expected Applied, got {other:?}"),
        }

        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "B"}),
                99,
            ),
        )
        .unwrap();
        let _ = apply_inbox_op(&unique, |_name, _args| Ok(json!({}))).expect_err("stale");
        match inbox_op_receipt(&unique, 2).unwrap() {
            InboxReceipt::Failed {
                error,
                base_generation,
                ..
            } => {
                assert!(error.unwrap_or_default().contains("generation_conflict"));
                assert_eq!(base_generation, Some(99));
            }
            other => panic!("expected Failed, got {other:?}"),
        }

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_publication_rejects_engine_and_keepalive_until_snapshot() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-pubready-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        fs::create_dir_all(dir.join(&unique)).unwrap();

        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"session_id":"{unique}","kind":"engine_revision","session_mode":"ui_owned_apply"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        assert!(
            snapshot_publication_after(&unique, 1).is_none(),
            "engine_revision heartbeat must not count as published"
        );

        // The lightweight keepalive overwrites kind but preserves the last
        // completed publication fence. It must not unblock await.
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"session_id":"{unique}","kind":"heartbeat","session_mode":"read_only_snapshot"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        assert!(
            snapshot_publication_after(&unique, 1).is_none(),
            "keepalive must not masquerade as a completed publish"
        );

        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":2,"model_generation":2,"active_sketch_generation":null,"session_id":"{unique}","kind":"snapshot","session_mode":"read_only_snapshot"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        let publication = snapshot_publication_after(&unique, 1).expect("snapshot ready");
        assert_eq!(publication.published_generation, 2);
        assert!(publication.model_published());
        assert!(!publication.active_sketch_published());
        assert_eq!(publication.snapshot_kind(), "model");
        assert!(
            snapshot_publication_after(&unique, 2).is_none(),
            "generation must advance past base"
        );

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn await_inbox_apply_sees_publish_after_delayed_host() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-await-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        fs::create_dir_all(dir.join(&unique)).unwrap();
        write_session(&unique, "model.json", r#"{"version":1,"name":"before"}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        let seq = write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "After"}),
                1,
            ),
        )
        .unwrap();

        let session_for_worker = unique.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            // OCC still sees generation 1; archive the op, then simulate the native
            // engine_revision heartbeat bump, then the TS publisher snapshot.
            apply_inbox_op(&session_for_worker, |_name, _args| Ok(json!({"ok": true}))).unwrap();
            write_session(
                &session_for_worker,
                "heartbeat.json",
                &format!(
                    r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{session_for_worker}","kind":"engine_revision","session_mode":"ui_owned_apply"}}"#,
                    now_ms()
                ),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            write_session(
                &session_for_worker,
                "heartbeat.json",
                &format!(
                    r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{session_for_worker}","kind":"heartbeat","session_mode":"read_only_snapshot"}}"#,
                    now_ms()
                ),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(60));
            publish_applied_snapshot(&session_for_worker, r#"{"version":1,"name":"After"}"#)
                .unwrap();
        });

        let result = await_inbox_apply(&unique, seq, 2_000, 20).unwrap();
        worker.join().unwrap();
        assert_eq!(result["status"], "applied");
        assert_eq!(result["timed_out"], false);
        assert_eq!(result["applied"], true);
        assert_eq!(result["published"], true);
        assert_eq!(result["model_published"], true);
        assert_eq!(result["active_sketch_published"], false);
        assert_eq!(result["snapshot_kind"], "model");
        assert_eq!(result["seq"], seq);
        assert_eq!(result["writeback"], false);
        assert!(result["current_generation"].as_u64().unwrap() > 1);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn await_inbox_apply_reports_active_sketch_without_model_publish() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-await-sketch-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        fs::create_dir_all(dir.join(&unique)).unwrap();
        write_session(&unique, "model.json", r#"{"version":1,"name":"Completed"}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{unique}","kind":"snapshot"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        let seq = write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "sketch_begin".to_string(),
                json!({"plane": {"type": "origin_plane", "plane": "xy"}}),
                1,
            ),
        )
        .unwrap();
        apply_inbox_op(&unique, |_name, _args| Ok(json!({"ok": true}))).unwrap();
        write_session(
            &unique,
            "active-sketch.json",
            r#"{"name":"Sketch1","entities":[]}"#,
        )
        .unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":2,"model_generation":1,"active_sketch_generation":2,"session_id":"{unique}","kind":"snapshot"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let result = await_inbox_apply(&unique, seq, 0, 20).unwrap();
        assert_eq!(result["status"], "applied");
        assert_eq!(result["published"], true);
        assert_eq!(result["published_generation"], 2);
        assert_eq!(result["model_generation"], 1);
        assert_eq!(result["active_sketch_generation"], 2);
        assert_eq!(result["model_published"], false);
        assert_eq!(result["active_sketch_published"], true);
        assert_eq!(result["snapshot_kind"], "active_sketch");
        assert_eq!(result["refreshed"], false);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn await_inbox_apply_timeout_while_pending() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-await-to-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        fs::create_dir_all(dir.join(&unique)).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        let seq = write_inbox_op(
            &unique,
            &InboxOp::unstamped("cad_set_document_name".to_string(), json!({"name": "X"}), 1),
        )
        .unwrap();

        let result = await_inbox_apply(&unique, seq, 60, 15).unwrap();
        assert_eq!(result["status"], "timeout");
        assert_eq!(result["timed_out"], true);
        assert_eq!(result["applied"], false);
        assert_eq!(result["published"], false);

        let probe = await_inbox_apply(&unique, seq, 0, 15).unwrap();
        assert_eq!(probe["status"], "pending");
        assert_eq!(probe["timed_out"], false);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn await_inbox_apply_reports_failed_receipt() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-await-fail-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        fs::create_dir_all(dir.join(&unique)).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"session_id":"{unique}"}}"#,
                now_ms()
            ),
        )
        .unwrap();
        write_inbox_op(
            &unique,
            &InboxOp::unstamped(
                "cad_set_document_name".to_string(),
                json!({"name": "X"}),
                99,
            ),
        )
        .unwrap();
        let _ = apply_inbox_op(&unique, |_n, _a| Ok(json!({}))).expect_err("stale");

        let result = await_inbox_apply(&unique, 1, 500, 20).unwrap();
        assert_eq!(result["status"], "failed");
        assert_eq!(result["dead_lettered"], true);
        assert_eq!(result["applied"], false);
        assert_eq!(result["timed_out"], false);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_status_reports_attach_vs_live_and_pending_inbox() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-status-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{unique}","window_id":"main","document_id":"tab-a","kind":"snapshot","session_mode":"read_only_snapshot"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let fresh = session_status_json(&unique, Some(1)).unwrap();
        assert_eq!(fresh["attached"], true);
        assert_eq!(fresh["code"], "attached");
        assert_eq!(fresh["session_id"], unique);
        assert_eq!(fresh["window_id"], "main");
        assert_eq!(fresh["document_id"], "tab-a");
        assert_eq!(fresh["attached_generation"], 1);
        assert_eq!(fresh["generation"], 1);
        assert_eq!(fresh["published_generation"], 1);
        assert_eq!(fresh["stale"], false);
        assert_eq!(fresh["heartbeat_stale"], false);
        assert_eq!(fresh["heartbeat_kind"], "snapshot");
        assert_eq!(fresh["pending_inbox_count"], 0);
        assert!(fresh["last_apply_receipt"].is_null());

        write_inbox_op(
            &unique,
            &InboxOp::unstamped("solid_mirror".to_string(), json!({}), 1),
        )
        .unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":2,"model_generation":2,"session_id":"{unique}","window_id":"main","document_id":"tab-a","kind":"snapshot","session_mode":"read_only_snapshot"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let stale = session_status_json(&unique, Some(1)).unwrap();
        assert_eq!(stale["stale"], true);
        assert_eq!(stale["attached_generation"], 1);
        assert_eq!(stale["generation"], 2);
        assert_eq!(stale["pending_inbox"], json!([1]));
        assert_eq!(stale["pending_inbox_count"], 1);
        assert!(stale["last_apply_receipt"].is_null());

        // Apply then bump — last receipt should surface.
        let _ = apply_inbox_op(&unique, |_n, _a| Ok(json!({}))).expect_err("stale base");
        let after_fail = session_status_json(&unique, Some(1)).unwrap();
        assert_eq!(after_fail["pending_inbox_count"], 0);
        assert_eq!(after_fail["last_apply_receipt"]["seq"], 1);
        assert_eq!(after_fail["last_apply_receipt"]["status"], "failed");

        let detached = not_attached_status_json();
        assert_eq!(detached["attached"], false);
        assert_eq!(detached["code"], "not_attached");
        assert_eq!(detached["writeback"], false);

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_status_derives_all_fields_from_one_heartbeat_snapshot() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-status-one-hb-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":2,"published_generation":1,"model_generation":1,"active_sketch_generation":null,"session_id":"{unique}","window_id":"main","document_id":"tab-a","kind":"engine_revision","session_mode":"ui_owned_apply"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let status = session_status_json(&unique, Some(1)).unwrap();
        assert_eq!(status["generation"], 2);
        assert_eq!(status["published_generation"], 1);
        assert_eq!(status["model_generation"], 1);
        assert_eq!(status["heartbeat_kind"], "engine_revision");
        assert_eq!(status["window_id"], "main");
        assert_eq!(status["document_id"], "tab-a");
        assert_eq!(status["heartbeat"]["generation"], 2);
        assert_eq!(status["stale"], true);
        assert_eq!(
            read_model_publication_generation(&unique),
            Some(1),
            "publication fence prefers model_generation over live generation"
        );

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn model_publication_fence_never_substitutes_an_explicit_unknown_model() {
        for (heartbeat, expected) in [
            (json!({"generation":4}), Some(4)),
            (json!({"generation":4,"published_generation":3}), Some(3)),
            (
                json!({"generation":4,"published_generation":4,"model_generation":3}),
                Some(3),
            ),
            (
                json!({"generation":4,"published_generation":4,"model_generation":null,
                "active_sketch_generation":4}),
                None,
            ),
            (json!({"generation":4,"published_generation":null}), None),
            (json!({}), None),
        ] {
            assert_eq!(
                model_publication_generation_from_heartbeat(&heartbeat),
                expected,
                "{heartbeat}"
            );
        }
    }

    #[test]
    fn session_status_unknown_attached_generation_is_stale_not_fresh() {
        let _guard = ENV_LOCK.lock().unwrap();
        let unique = test_session_uuid();
        let dir = std::env::temp_dir().join(format!("nbcad-sessions-status-unknown-{unique}"));
        std::env::set_var("NBCAD_SESSION_DIR", &dir);
        write_session(&unique, "model.json", r#"{"version":1}"#).unwrap();
        write_session(
            &unique,
            "heartbeat.json",
            &format!(
                r#"{{"updated_ms":{},"generation":1,"published_generation":1,"model_generation":1,"session_id":"{unique}","kind":"snapshot"}}"#,
                now_ms()
            ),
        )
        .unwrap();

        let status = session_status_json(&unique, None).unwrap();
        assert_eq!(status["stale"], true);
        assert!(status["attached_generation"].is_null());
        assert_eq!(status["generation"], 1);
        let hint = status["hint"].as_str().unwrap_or("");
        assert!(
            hint.contains("publication fence") || hint.contains("unknown"),
            "hint must not claim a fresh match when fence is unknown: {hint}"
        );
        assert!(!hint.contains("matches live"));

        // Recovery attach with no heartbeat at all.
        let orphan = test_session_uuid();
        write_session(&orphan, "model.json", r#"{"version":1}"#).unwrap();
        let orphan_status = session_status_json(&orphan, None).unwrap();
        assert_eq!(orphan_status["stale"], true);
        assert!(orphan_status["generation"].is_null());

        std::env::remove_var("NBCAD_SESSION_DIR");
        let _ = fs::remove_dir_all(&dir);
    }
}
