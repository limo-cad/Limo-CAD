//! Durations for document activation. The clock is `std::time::Instant`, already
//! used for desktop deadlines. Recording happens after the activation receipt
//! returns, so a sample never changes the switch result.
//!
//! Drawing and part labels are applied by the File tab completion once the
//! restored workspace is known. A second desktop process is a separate
//! `SessionBridgeState`; its activation stays kind `document`.

use serde::Serialize;
use serde_json::json;
#[cfg(test)]
use std::fs;
use std::{path::PathBuf, sync::Mutex, time::Duration};

const RETAINED_SAMPLES: usize = 64;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SwitchTiming {
    pub kind: String,
    pub duration_ms: f64,
    pub window_id: String,
    pub document_id: String,
    pub process_instance_id: String,
    pub workspace: Option<String>,
}

static SAMPLES: Mutex<Vec<SwitchTiming>> = Mutex::new(Vec::new());

fn lock() -> std::sync::MutexGuard<'static, Vec<SwitchTiming>> {
    SAMPLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn session_dir() -> PathBuf {
    limo_cad_session_storage::root()
}

pub(crate) fn samples_path() -> PathBuf {
    session_dir()
        .join("_ui")
        .join(format!("switch-timings-{}.json", std::process::id()))
}

fn enabled() -> bool {
    #[cfg(test)]
    if TEST_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    std::env::var("LIMO_CAD_NATIVE_SWITCH_TIMING").as_deref() == Ok("1")
}

pub(crate) fn record_document_switch(
    window_id: &str,
    document_id: &str,
    process_instance_id: &str,
    elapsed: Duration,
) {
    if !enabled() {
        return;
    }
    let sample = SwitchTiming {
        kind: "document".into(),
        duration_ms: elapsed.as_secs_f64() * 1000.0,
        window_id: window_id.to_owned(),
        document_id: document_id.to_owned(),
        process_instance_id: process_instance_id.to_owned(),
        workspace: None,
    };
    emit(&sample);
    let mut samples = lock();
    samples.push(sample);
    if samples.len() > RETAINED_SAMPLES {
        let overflow = samples.len() - RETAINED_SAMPLES;
        samples.drain(0..overflow);
    }
    persist(&samples);
}

/// Label the latest unlabeled activation of this document. Drawing and Solid
/// become the drawing and part switch kinds. Other workspaces stay `document`.
pub(crate) fn annotate(window_id: &str, document_id: &str, workspace: &str) {
    if !enabled() {
        return;
    }
    let mut samples = lock();
    let Some(sample) = samples.iter_mut().rev().find(|sample| {
        sample.window_id == window_id
            && sample.document_id == document_id
            && sample.kind == "document"
            && sample.workspace.is_none()
    }) else {
        return;
    };
    sample.workspace = Some(workspace.to_owned());
    if workspace == "drawing" || workspace == "part" {
        sample.kind = workspace.to_owned();
    }
    let labeled = sample.clone();
    persist(&samples);
    drop(samples);
    emit(&labeled);
}

fn emit(sample: &SwitchTiming) {
    if let Ok(line) = serde_json::to_string(sample) {
        eprintln!("limo_cad_switch_timing {line}");
    }
}

fn persist(samples: &[SwitchTiming]) {
    if let Err(error) = write_samples(samples) {
        eprintln!("limo_cad_switch_timing could not write samples: {error}");
    }
}

fn write_samples(samples: &[SwitchTiming]) -> Result<(), String> {
    let path = samples_path();
    let body = serde_json::to_string_pretty(&json!({
        "clock": "std::time::Instant",
        "unit": "milliseconds",
        "samples": samples,
    }))
    .map_err(|error| error.to_string())?;
    limo_cad_session_storage::atomic_write(&path, body.as_bytes())
        .map_err(|error| error.to_string())
}
#[cfg(test)]
static TEST_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(crate) struct TestRecording;

#[cfg(test)]
impl Drop for TestRecording {
    fn drop(&mut self) {
        TEST_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        lock().clear();
        let _ = fs::remove_file(samples_path());
    }
}

#[cfg(test)]
pub(crate) fn record_for_test() -> TestRecording {
    lock().clear();
    let _ = fs::remove_file(samples_path());
    TEST_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
    TestRecording
}

#[cfg(test)]
pub(crate) fn measurement_fields(instance_window_id: &str) -> serde_json::Value {
    let samples = lock().clone();
    let duration = |kind: &str| {
        samples
            .iter()
            .rev()
            .find(|sample| sample.kind == kind)
            .map(|sample| sample.duration_ms)
    };
    json!({
        "clock": "std::time::Instant",
        "unit": "milliseconds",
        "drawing_switch_ms": duration("drawing"),
        "part_switch_ms": duration("part"),
        "instance_document_switch_ms": samples.iter().rev().find(|sample| {
            sample.kind == "document" && sample.window_id == instance_window_id
        }).map(|sample| sample.duration_ms),
    })
}
