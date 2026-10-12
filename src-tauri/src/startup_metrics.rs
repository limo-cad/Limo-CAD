//! Opt-in launch telemetry. Compilation is deliberately outside the interval.
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static STARTED: OnceLock<Instant> = OnceLock::new();
static STARTED_UNIX_MS: OnceLock<u128> = OnceLock::new();
static REPORT: Mutex<()> = Mutex::new(());

pub fn begin() {
    STARTED.get_or_init(Instant::now);
    STARTED_UNIX_MS.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    });
    record_native_milestone("main");
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupMilestone {
    Feedback,
    Workspace,
    Interaction,
    CadInteraction,
    Document,
    WindowExpansion,
    AppDocumentSetup,
    FileRead,
    FileValidate,
    FileGeometry,
    FileAdoption,
    FileReady,
    Error,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupMeasurementSettings {
    locale: Option<String>,
    document_path: Option<String>,
    document_mode: &'static str,
}

/// Diagnostic launches opt in explicitly; ordinary preferences and files are
/// never changed by benchmark environment variables alone.
#[tauri::command]
pub fn startup_measurement_settings() -> Option<StartupMeasurementSettings> {
    std::env::var_os("NBCAD_STARTUP_REPORT_FILE")?;
    Some(StartupMeasurementSettings {
        locale: std::env::var("NBCAD_STARTUP_LOCALE")
            .ok()
            .filter(|locale| matches!(locale.as_str(), "en" | "zh-CN" | "es" | "de")),
        document_path: std::env::var("NBCAD_STARTUP_DOCUMENT").ok(),
        document_mode: if std::env::var("NBCAD_STARTUP_DOCUMENT_MODE").as_deref() == Ok("runtime") {
            "runtime"
        } else {
            "startup"
        },
    })
}

#[tauri::command]
pub fn startup_record(
    phase: StartupMilestone,
    elapsed_ms: f64,
    duration_ms: Option<f64>,
    detail: Option<String>,
) -> Result<(), String> {
    let Some(path) = std::env::var_os("NBCAD_STARTUP_REPORT_FILE") else {
        return Ok(());
    };
    if !elapsed_ms.is_finite() || elapsed_ms < 0.0 {
        return Err("invalid startup time".into());
    }
    if duration_ms.is_some_and(|duration| !duration.is_finite() || duration < 0.0) {
        return Err("invalid interaction duration".into());
    }
    let scope = match &phase {
        StartupMilestone::FileRead
        | StartupMilestone::FileValidate
        | StartupMilestone::FileGeometry
        | StartupMilestone::FileAdoption
        | StartupMilestone::FileReady
        | StartupMilestone::Interaction
        | StartupMilestone::CadInteraction => "document",
        // Retained for historical comparisons; this aggregate can include a
        // startup fixture and must not be confused with app-only readiness.
        StartupMilestone::Document => "startupComposite",
        _ => "application",
    };
    write_record(
        path,
        serde_json::json!({
            "phase": phase,
            "webElapsedMs": elapsed_ms,
            "durationMs": duration_ms,
            "scope": scope,
            "recordKind": if duration_ms.is_some() { "span" } else { "milestone" },
            // WebView clock bounds stay explicit. processElapsedMs is IPC
            // receipt time and is not the endpoint of a frontend operation.
            "webSpanStartMs": duration_ms.map(|duration| (elapsed_ms - duration).max(0.0)),
            "webSpanEndMs": duration_ms.map(|_| elapsed_ms),
            "detail": detail.map(|text| text.chars().take(4096).collect::<String>()),
        }),
    )
}

/// Diagnostic spans are after-only observations, not additional readiness gates.
/// Worker and document spans overlap and must not be summed as a critical path.
pub fn record_native_duration(phase: &'static str, duration_ms: f64) {
    let Some(path) = std::env::var_os("NBCAD_STARTUP_REPORT_FILE") else {
        return;
    };
    if !duration_ms.is_finite() || duration_ms < 0.0 {
        return;
    }
    let end_ms = STARTED.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
    if let Err(error) = write_record(
        path,
        serde_json::json!({
            "phase": phase, "webElapsedMs": null, "durationMs": duration_ms, "detail": null,
            "scope": "application", "recordKind": "span",
            "processSpanStartMs": (end_ms - duration_ms).max(0.0),
            "processSpanEndMs": end_ms, "processElapsedMs": end_ms,
        }),
    ) {
        eprintln!("Startup span measurement failed: {error}");
    }
}

/// Capture real process-clock interval bounds before report locking/file IO.
/// Overlapping application and document spans are observations, never gates.
pub fn record_native_span(phase: &'static str, started: Instant) {
    record_span(phase, started, "application");
}

pub fn record_document_span(phase: &'static str, started: Instant) {
    record_span(phase, started, "document");
}

fn span_record(
    phase: &'static str,
    scope: &'static str,
    origin: Instant,
    started: Instant,
    ended: Instant,
) -> serde_json::Value {
    let start_ms = started.saturating_duration_since(origin).as_secs_f64() * 1000.0;
    let end_ms = ended.saturating_duration_since(origin).as_secs_f64() * 1000.0;
    serde_json::json!({
        "phase": phase, "scope": scope, "recordKind": "span",
        "webElapsedMs": null, "detail": null,
        "durationMs": ended.saturating_duration_since(started).as_secs_f64() * 1000.0,
        "processSpanStartMs": start_ms, "processSpanEndMs": end_ms,
        "processElapsedMs": end_ms,
    })
}

fn record_span(phase: &'static str, started: Instant, scope: &'static str) {
    let Some(path) = std::env::var_os("NBCAD_STARTUP_REPORT_FILE") else {
        return;
    };
    let ended = Instant::now();
    let origin = *STARTED.get_or_init(Instant::now);
    if let Err(error) = write_record(path, span_record(phase, scope, origin, started, ended)) {
        eprintln!("Startup span measurement failed: {error}");
    }
}

pub fn record_native_milestone(phase: &'static str) {
    let Some(path) = std::env::var_os("NBCAD_STARTUP_REPORT_FILE") else {
        return;
    };
    let elapsed_ms = STARTED.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
    if let Err(error) = write_record(
        path,
        serde_json::json!({
            "phase": phase, "scope": "application", "recordKind": "milestone",
            "processElapsedMs": elapsed_ms, "webElapsedMs": null,
            "durationMs": null, "detail": null,
        }),
    ) {
        eprintln!("Startup milestone measurement failed: {error}");
    }
}

fn write_record(path: std::ffi::OsString, mut record: serde_json::Value) -> Result<(), String> {
    let _guard = REPORT.lock().map_err(|_| "startup report lock poisoned")?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("cannot write startup report: {error}"))?;
    if record.get("processElapsedMs").is_none() {
        record["processElapsedMs"] =
            serde_json::json!(STARTED.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0);
    }
    record["processStartedUnixMs"] = serde_json::json!(STARTED_UNIX_MS.get().copied());
    record["pid"] = serde_json::json!(std::process::id());
    serde_json::to_writer(&mut file, &record).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn process_span_bounds_measure_overlap_without_report_io_time() {
        let origin = Instant::now();
        let record = span_record(
            "gpuDevice",
            "application",
            origin,
            origin + Duration::from_millis(20),
            origin + Duration::from_millis(50),
        );
        assert_eq!(record["processSpanStartMs"], 20.0);
        assert_eq!(record["processSpanEndMs"], 50.0);
        assert_eq!(record["durationMs"], 30.0);
        assert_eq!(record["processElapsedMs"], 50.0);
        assert_eq!(record["scope"], "application");
    }

    #[test]
    fn runtime_document_spans_keep_document_scope() {
        let origin = Instant::now();
        let record = span_record("nativeProjectLoad", "document", origin, origin, origin);
        assert_eq!(record["scope"], "document");
        assert_eq!(record["durationMs"], 0.0);
    }
}
