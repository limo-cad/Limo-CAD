//! Run the real short lesson from the native Scripts card in an owned blank
//! document, then verify editable intent, the nonblank guard, capture and Save.
use crate::native_fixture::{controls, start};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
mod imported;

fn ui(client: &mut Client, request: Value) -> Result<Value> {
    let label = json!({"action":request["action"],"command":request["command"],
        "path":request["path"],"target":request["target"]});
    crate::native_fixture::ui(client, request)
        .map_err(|error| anyhow::anyhow!("Native Scripts request {label}: {error:#}"))
}
fn control(client: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    crate::native_fixture::control(client, label, value)
        .map_err(|error| anyhow::anyhow!("Native Scripts control {label:?}: {error:#}"))
}
fn capture(client: &mut Client, out: &std::path::Path, name: &str) -> Result<()> {
    crate::native_fixture::capture(client, out, name)
        .map_err(|error| anyhow::anyhow!("Native Scripts capture {name:?}: {error:#}"))
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-lessons")?;
    let c = &mut fixture.client;
    let catalog = c.call("cad_interface", json!({"action":"recipes"}))?;
    let entries = catalog.as_array().context("Recipe catalog missing")?;
    let lessons: Vec<_> = entries
        .iter()
        .filter(|entry| entry["kind"] == "lesson")
        .collect();
    ensure!(
        lessons.len() == 4,
        "Expected the four installed short lessons"
    );
    let lesson = lessons
        .iter()
        .find(|entry| entry["id"] == "fillet-basics")
        .context("Fillet lesson missing")?;
    let title = lesson["name"].as_str().context("Lesson title missing")?;
    let before = c.call("cad_project_model", json!({}))?;
    control(c, "Scripts", None)?;
    let offered = ui(c, json!({"action":"inspect"}))?;
    for entry in entries {
        let name = entry["name"].as_str().context("Recipe title missing")?;
        let matching: Vec<_> = controls(&offered)
            .filter(|row| row["label"] == name)
            .collect();
        if entry["kind"] == "lesson" {
            ensure!(
                matching.len() == 1 && matching[0]["disabled"] == false,
                "Blank document did not offer enabled short lesson {name}"
            );
        } else {
            ensure!(
                matching.is_empty(),
                "Scripts exposed non-lesson recipe {name}"
            );
        }
    }
    ensure!(
        c.call("cad_project_model", json!({}))? == before,
        "Opening Scripts changed the blank document"
    );
    capture(c, &fixture.out, "lessons-blank-catalog")?;
    let started = control(c, title, None)?;
    ensure!(
        started["value"]["lesson_started"] == "fillet-basics",
        "Lesson control did not start the installed runner: {started}"
    );
    ui(c, json!({"action":"presentation","command":"pause"}))?;
    let caption_deadline = Instant::now() + Duration::from_secs(20);
    let paused = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(
            state["stopped"] != true,
            "Lesson stopped before its first caption: {state}"
        );
        if state["chapter"] == "Locate the stock" {
            ensure!(
                state["paused"] == true && state["step_index"] == 1,
                "Lesson did not pause at its first caption: {state}"
            );
            break state.clone();
        }
        ensure!(
            Instant::now() < caption_deadline,
            "First lesson caption did not arrive: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    std::thread::sleep(Duration::from_millis(600));
    let still_paused = ui(c, json!({"action":"presentation","command":"status"}))?;
    ensure!(
        still_paused["presentation"]["paused"] == true
            && still_paused["presentation"]["step_index"] == paused["step_index"]
            && still_paused["presentation"]["wait_ms"] == paused["wait_ms"],
        "Paused caption advanced: {still_paused}"
    );
    ensure!(
        c.call("cad_project_model", json!({}))? == before,
        "The real lesson edited the blank document while paused"
    );
    capture(c, &fixture.out, "lessons-paused-caption")?;

    ui(c, json!({"action":"presentation","command":"step"}))?;
    let step_deadline = Instant::now() + Duration::from_secs(30);
    let stepped = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(
            state["stopped"] != true,
            "Single-step stopped the lesson: {state}"
        );
        if state["step_pending"] == false && state["operation"] == "begin" {
            ensure!(
                state["paused"] == true,
                "Single-step resumed the entire lesson: {state}"
            );
            break state.clone();
        }
        ensure!(
            Instant::now() < step_deadline,
            "Single-step did not apply exactly sketch_begin: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let stepped_model = c.call("cad_project_model", json!({}))?;
    let stepped_active = c.call("sketch_active", json!({}))?;
    ensure!(
        stepped_model == before && !stepped_active.is_null(),
        "Single-step did not begin the actual editable sketch"
    );
    std::thread::sleep(Duration::from_millis(600));
    let held = ui(c, json!({"action":"presentation","command":"status"}))?;
    ensure!(
        held["presentation"]["paused"] == true
            && held["presentation"]["step_index"] == stepped["step_index"]
            && held["presentation"]["operation"] == stepped["operation"]
            && c.call("sketch_active", json!({}))? == stepped_active
            && c.call("cad_project_model", json!({}))? == stepped_model,
        "Single-step allowed a second modeling operation: {held}"
    );
    control(c, "Resume", None)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    let presentation = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(
            state["stopped"] != true,
            "Lesson presentation stopped: {state}"
        );
        if state["finished"] == true && state["chapter"] == "Three editable features" {
            break state.clone();
        }
        ensure!(Instant::now() < deadline, "Lesson did not finish: {state}");
        std::thread::sleep(Duration::from_millis(100));
    };
    let scene = c.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"].as_array().is_some_and(Vec::is_empty),
        "Lesson left solid errors"
    );
    ensure!(
        scene["bodies"]
            .as_array()
            .is_some_and(|rows| rows.len() == 1),
        "Lesson did not create one real solid"
    );
    let sketches = c.call("sketch_finished", json!({}))?;
    ensure!(
        sketches.as_array().is_some_and(|rows| rows.len() == 1) && sketches[0]["dof"]["value"] == 0,
        "Lesson did not keep its fully constrained sketch"
    );
    let exported = c.call("cad_project_model", json!({}))?;
    let model: Value = serde_json::from_str(exported.as_str().context("Model export missing")?)?;
    ensure!(
        model["document"]["history"]["features"]
            .as_array()
            .is_some_and(|rows| rows.len() == 3),
        "Lesson lost its three editable features"
    );
    ensure!(
        model["extrudes"][0]["extent"]["distance"] == 12. && model["fillets"][0]["radius"] == 2.,
        "Lesson did not restore its 12 mm extrusion and 2 mm fillet"
    );
    control(c, "Scripts", None)?;
    let blocked = ui(c, json!({"action":"inspect"}))?;
    for lesson in &lessons {
        let rows: Vec<_> = controls(&blocked)
            .filter(|row| row["label"] == lesson["name"])
            .collect();
        ensure!(
            rows.len() == 1 && rows[0]["disabled"] == true,
            "Nonblank document still offers runnable lesson {}",
            lesson["name"]
        );
    }
    let target = controls(&blocked)
        .find(|row| row["label"] == title)
        .context("Completed lesson control missing")?["id"]
        .clone();
    let rerun = c.call("cad_interface", json!({"action":"click","target":target}));
    ensure!(
        rerun.is_err()
            || rerun
                .as_ref()
                .is_ok_and(|value| value["status"] == "failed"),
        "Disabled lesson accepted a second run"
    );
    ensure!(
        c.call("cad_project_model", json!({}))? == exported,
        "Rejected rerun replaced or edited the completed design"
    );
    capture(c, &fixture.out, "lessons-nonblank-guard")?;
    control(c, "Scripts", None)?;
    ui(c, json!({"action":"capture","path":fixture.capture}))?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(saved == model, "Save lost the lesson's editable model");
    for name in [
        "lessons-blank-catalog.png",
        "lessons-paused-caption.png",
        "lessons-nonblank-guard.png",
        "native-lessons.png",
    ] {
        let png = std::fs::read(fixture.out.join(name))?;
        ensure!(
            png.starts_with(b"\x89PNG\r\n\x1a\n") && png.len() > 1024,
            "Live window capture {name} missing or empty"
        );
    }
    let imported = imported::exercise(c, &fixture.out)?;
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required","session":fixture.session,
            "lesson":"fillet-basics","presentation":presentation,"paused":paused,"stepped":stepped,
            "stepped_active_sketch":stepped_active,"model":model,
            "scene":scene,"sketches":sketches,"imported_script":imported,
            "checks":["four-short-lessons-only","blank-only-run","native-scripts-button",
                "shared-runner-presentation","pause-holds-caption-and-blank-model",
                "single-step-one-owned-model-operation","native-resume-button","editable-solid-and-fully-constrained-sketch",
                "nonblank-disabled-and-rejected-rerun","live-window-captures","saved-model-equality"],
        }))?,
    )?;
    println!("PASS native Scripts lesson, editable model, nonblank guard, window capture and Save");
    Ok(())
}
