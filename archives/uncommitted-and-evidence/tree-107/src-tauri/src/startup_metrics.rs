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
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupMilestone {
    Feedback,
    Workspace,
    Interaction,
    CadInteraction,
    Error,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupMeasurementSettings {
    locale: Option<String>,
    document_path: Option<String>,
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
    let _guard = REPORT.lock().map_err(|_| "startup report lock poisoned")?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("cannot write startup report: {error}"))?;
    serde_json::to_writer(
        &mut file,
        &serde_json::json!({
            "phase": phase,
            "processElapsedMs": STARTED.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0,
            "processStartedUnixMs": STARTED_UNIX_MS.get().copied(),
            "webElapsedMs": elapsed_ms,
            "durationMs": duration_ms,
            "detail": detail.map(|text| text.chars().take(4096).collect::<String>()),
            "pid": std::process::id(),
        }),
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}
