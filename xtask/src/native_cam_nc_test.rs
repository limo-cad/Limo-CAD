//! NC simulation through the native multiline editor, with an applied setup
//! and cutter but no CAM operations. This fixture never seeds a CAM document.
//! Kept separate from the generated-toolpath fixture so a planner fallback
//! cannot accidentally make the imported-program assertions pass.
use crate::{
    native_fixture::{
        begin_sketch, capture, control, control_in, controls, panel_field, start, ui,
    },
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

const NC_SURFACE: &str = "cam-nc-source";
const PROGRAM_NAME: &str = "native-workpiece.nc";

fn clean(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    value
}
fn document(c: &mut Client) -> Result<Value> {
    c.call("cam_get_document", json!({})).map(clean)
}
fn model(c: &mut Client) -> Result<Value> {
    let value = c.call("cad_project_model", json!({}))?;
    serde_json::from_str(value.as_str().context("Project export was not JSON text")?)
        .context("Parse the complete project model")
}
fn field(c: &mut Client, label: &str, value: &str) -> Result<()> {
    panel_field(c, label, Some(value), "Previous fields", "More fields")?;
    Ok(())
}
fn caption<'a>(inspect: &'a Value, surface: &str) -> &'a str {
    inspect["ui"]["surfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["name"] == surface)
        .and_then(|s| s["text"].as_str())
        .unwrap_or("")
}
fn nc_block(text: &str) -> Option<u32> {
    let mut words = text.split_whitespace();
    while let Some(word) = words.next() {
        if word.trim_end_matches(':').eq_ignore_ascii_case("block") {
            return words
                .next()?
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse()
                .ok();
        }
    }
    None
}
fn inspect(c: &mut Client) -> Result<Value> {
    ui(c, json!({"action":"inspect"}))
}
fn wait_enabled(c: &mut Client, label: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let state = inspect(c)?;
        if controls(&state).any(|v| v["label"] == label && v["disabled"] == false) {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Native control {label} did not become enabled: {}",
            caption(&state, "cam/view")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn rejected(c: &mut Client, label: &str, value: Option<&str>, expected: &str) -> Result<()> {
    let state = inspect(c)?;
    let target = controls(&state)
        .find(|v| v["surface"] == NC_SURFACE && v["label"] == label && v["disabled"] == false)
        .context("Missing enabled NC control for rejection check")?;
    let action = if let Some(value) = value {
        json!({"action":"set_value","target":target["id"],"value":value})
    } else {
        json!({"action":"click","target":target["id"]})
    };
    let result = c.rpc(
        "tools/call",
        json!({"name":"cad_interface","arguments":action}),
    )?;
    let text = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["type"] == "text")
        .and_then(|v| v["text"].as_str())
        .context("Structured rejection was missing")?;
    let response: Value = serde_json::from_str(text)?;
    ensure!(
        response["status"] == "failed"
            && response["error"]
                .as_str()
                .is_some_and(|s| s.contains(expected)),
        "Expected NC rejection {expected:?}, received {response}"
    );
    Ok(())
}
fn source(c: &mut Client, source: &str) -> Result<()> {
    let state = inspect(c)?;
    let control = controls(&state)
        .find(|v| v["surface"] == NC_SURFACE && v["label"] == "Controller code")
        .context("Native NC editor was missing")?;
    ensure!(
        control["role"] == "multiline_textbox",
        "NC source is not using the native multiline editor"
    );
    control_in(c, NC_SURFACE, "Controller code", Some(source))?;
    Ok(())
}
fn open(c: &mut Client) -> Result<()> {
    wait_enabled(c, "NC simulation")?;
    control(c, "NC simulation", None)?;
    control_in(c, NC_SURFACE, "Program name", Some(PROGRAM_NAME))?;
    control_in(c, NC_SURFACE, "Controller language", Some("iso"))?;
    Ok(())
}
fn unchanged(c: &mut Client, expected: &Value, stage: &str) -> Result<()> {
    ensure!(
        &model(c)? == expected,
        "{stage} changed the saved project intent"
    );
    ensure!(
        document(c)?["setups"][0]["operations"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "{stage} invented CAM operations for NC input"
    );
    Ok(())
}
fn parser_rejection(c: &mut Client, code: &str, error: &str, expected: &Value) -> Result<String> {
    open(c)?;
    source(c, code)?;
    wait_enabled(c, "Build NC simulation")?;
    control_in(c, NC_SURFACE, "Build NC simulation", None)?;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let state = inspect(c)?;
        let message = caption(&state, "cam/view");
        if message.contains(error) {
            let message = message.to_owned();
            unchanged(c, expected, "Rejected NC program")?;
            return Ok(message);
        }
        ensure!(
            !message.starts_with("Simulation:"),
            "Invalid NC program unexpectedly completed: {message}"
        );
        ensure!(
            Instant::now() < deadline,
            "Expected NC parser error {error:?}, received {message:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn completed(c: &mut Client) -> Result<(Value, f64)> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let state = inspect(c)?;
        let message = caption(&state, "cam/view");
        if message.starts_with("Simulation:") {
            let duration: f64 = message
                .split_whitespace()
                .nth(1)
                .context("NC duration missing")?
                .parse()?;
            ensure!(
                duration.is_finite() && duration > 0.,
                "NC simulation returned no physical timeline"
            );
            let removed = message
                .split('·')
                .find(|part| part.contains("mm³ removed"))
                .and_then(|part| part.split_whitespace().next())
                .and_then(|number| number.parse::<f64>().ok());
            ensure!(
                removed.is_some_and(|value| value.is_finite() && value > 0.),
                "NC code did not remove real setup stock: {message}"
            );
            return Ok((state, duration));
        }
        let pending =
            controls(&state).any(|v| v["label"] == "Cancel simulation" && v["disabled"] == false);
        ensure!(pending, "Native NC simulation stopped: {message}");
        ensure!(
            Instant::now() < deadline,
            "Native NC simulation timed out: {message}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn paused_at(c: &mut Client, target: f64) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let state = inspect(c)?;
        let text = caption(&state, "cam/view");
        let time = controls(&state)
            .find(|v| v["label"] == "Playback time")
            .and_then(|v| {
                v["value"]
                    .as_f64()
                    .or_else(|| v["value"].as_str()?.parse().ok())
            });
        if text.starts_with("Paused") && time.is_some_and(|t| (t - target).abs() <= 0.06) {
            return Ok(state);
        }
        ensure!(
            Instant::now() < deadline,
            "NC playback did not reach {target:.2}s: {text}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn save(c: &mut Client, path: &Path, expected: &Value) -> Result<()> {
    ui(c, json!({"action":"file","command":"save","path":path}))?;
    let mut archive = zip::ZipArchive::new(fs::File::open(path)?)?;
    let actual: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        &actual == expected,
        "Native Save changed intent or persisted transient NC playback"
    );
    Ok(())
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-cam-nc")?;
    let c = &mut fixture.client;
    let initial = document(c)?;
    ensure!(
        initial["setups"].as_array().is_some_and(Vec::is_empty)
            && initial["tools"].as_array().is_some_and(Vec::is_empty),
        "Choose a blank CAM document"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}))?;
    let solid = clean(c.call("solid_scene", json!({}))?);
    let body_id = solid["bodies"][0]["id"]
        .as_u64()
        .context("NC fixture real solid missing")?;
    control(c, "Switch workspace", None)?;
    control(c, "Manufacture", None)?;
    control(c, "New setup", None)?;
    field(c, "Name", "NC stock and model")?;
    field(c, "Model body · click to cycle", &body_id.to_string())?;
    control(c, "Create", None)?;
    let before_tool = model(c)?;
    control(c, "Project tools", None)?;
    crate::native_cam_test::verify_tool_library_isolation(c, &fixture.out)?;
    control(c, "New project tool", None)?;
    for (label, value) in [
        ("Name", "NC 6 mm flat"),
        ("Tool number (optional)", "3"),
        ("Cutter type", "flat_end_mill"),
        ("Diameter (mm)", "6"),
        ("Flute length (mm)", "20"),
        ("Overall length (mm)", "50"),
        ("Default spindle (rpm)", "6000"),
        ("Default cutting feed (mm/min)", "600"),
        ("Default plunge feed (mm/min)", "300"),
    ] {
        field(c, label, value)?;
    }
    control(c, "Create", None)?;
    let expected = model(c)?;
    let cam = document(c)?;
    ensure!(
        cam["setups"].as_array().is_some_and(|v| v.len() == 1)
            && cam["tools"].as_array().is_some_and(|v| v.len() == 1)
            && cam["tools"][0]["number"] == 3,
        "Native NC setup/cutter creation failed"
    );
    unchanged(c, &expected, "Native NC fixture setup")?;
    let stock = &cam["setups"][0]["stock"];
    let number = |side: &str, axis: &str| -> Result<f64> {
        stock[side][axis]
            .as_f64()
            .with_context(|| format!("NC stock {side}.{axis} missing"))
    };
    let x0 = number("min", "x")? + 3.;
    let x1 = number("max", "x")? - 3.;
    let y = (number("min", "y")? + number("max", "y")?) / 2.;
    let top = number("max", "z")?;
    let safe = top + 3.;
    let cut = top - 2.;
    ensure!(
        x1 > x0 && cut > number("min", "z")?,
        "Fixture stock cannot contain the NC cut"
    );
    let program = format!("N10 G21 G17 G90 G94\nN20 T3 M6\nN30 S6000 M3\nN40 G0 X{x0:.6} Y{y:.6} Z{safe:.6}\nN50 G1 Z{cut:.6} F300\nN60 G1 X{x1:.6} F600\nN70 G0 Z{safe:.6}\nN80 M5\nN90 M30\n");
    fs::write(fixture.out.join(PROGRAM_NAME), &program)?;
    control(c, "Setups", None)?;
    control(c, "Fit", None)?;
    control(c, "Simulation settings", None)?;
    control(c, "Simulation detail", Some("fast"))?;
    control(c, "Comparison tolerance (mm)", Some("0.5"))?;
    control(c, "Close simulation settings", None)?;

    let dialect_error = parser_rejection(
        c,
        &program.replace("G21", "G71"),
        "G70/G71 have controller-specific semantics",
        &expected,
    )?;
    capture(c, &fixture.out, "nc-wrong-controller")?;
    let tool_error = parser_rejection(
        c,
        &program.replace("T3", "T999"),
        "no exact matching project-library tool",
        &expected,
    )?;
    capture(c, &fixture.out, "nc-unknown-tool")?;
    control(c, "Report", None)?;
    ensure!(
        caption(&inspect(c)?, "cam-report") == tool_error,
        "NC error report omitted parser diagnostics"
    );
    capture(c, &fixture.out, "nc-error-report")?;
    control(c, "Close report", None)?;
    open(c)?;
    source(c, &program)?;
    let oversized = "é".repeat(4 * 1024 * 1024) + "x";
    rejected(
        c,
        "Controller code",
        Some(&oversized),
        "NC source exceeds 8388608 bytes",
    )?;
    let rejected_state = inspect(c)?;
    ensure!(
        controls(&rejected_state).any(|v| v["surface"] == NC_SURFACE
            && v["label"] == "Controller code"
            && v["value"] == program),
        "Rejected oversized NC paste changed the prior visible source"
    );
    control_in(
        c,
        NC_SURFACE,
        "Program name",
        Some("renamed-after-invalid.nc"),
    )?;
    rejected(
        c,
        "Build NC simulation",
        None,
        "NC source exceeds 8388608 bytes",
    )?;
    unchanged(c, &expected, "Oversized NC input")?;
    source(c, &program)?;
    control_in(c, NC_SURFACE, "Program name", Some(PROGRAM_NAME))?;
    capture(c, &fixture.out, "nc-source-editor")?;
    wait_enabled(c, "Build NC simulation")?;
    control_in(c, NC_SURFACE, "Build NC simulation", None)?;
    let (completed_state, duration) = completed(c)?;
    unchanged(c, &expected, "Completed NC simulation")?;
    control(c, "Report", None)?;
    let report = inspect(c)?;
    let report_text = caption(&report, "cam-report");
    ensure!(
        report_text.contains(PROGRAM_NAME)
            && report_text.contains("NC block")
            && report_text.to_lowercase().contains("source line"),
        "NC report omitted imported-source provenance: {report_text}"
    );
    ensure!(
        report_text.contains("workpiece-only") && report_text.contains("Target comparison"),
        "NC report omitted shared verification or interpreter limits: {report_text}"
    );
    fs::write(fixture.out.join("nc-report.txt"), report_text)?;
    capture(c, &fixture.out, "nc-simulation-report")?;
    control(c, "Next page", None)?;
    capture(c, &fixture.out, "nc-simulation-report-page-2")?;
    control(c, "Close report", None)?;
    control(c, "Stock", None)?;
    capture(c, &fixture.out, "nc-stock-complete")?;
    control(c, "Start", None)?;
    let start_state = paused_at(c, 0.)?;
    capture(c, &fixture.out, "nc-stock-start")?;
    let target = (duration * 0.5 * 100.).round() / 100.;
    control(c, "Playback time", Some(&target.to_string()))?;
    let middle_state = paused_at(c, target)?;
    ensure!(
        nc_block(caption(&middle_state, "cam/view")) == Some(60),
        "NC playback did not expose cutting block 60: {}",
        caption(&middle_state, "cam/view")
    );
    capture(c, &fixture.out, "nc-stock-seek")?;
    control(c, "Start", None)?;
    paused_at(c, 0.)?;
    capture(c, &fixture.out, "nc-stock-rewind")?;
    control(c, "Play", None)?;
    std::thread::sleep(Duration::from_millis(350));
    control(c, "Pause", None)?;
    ensure!(
        controls(&inspect(c)?).any(|v| v["label"] == "Play" && v["disabled"] == false),
        "NC Pause did not stop playback"
    );
    control(c, "Cancel simulation", None)?;
    unchanged(c, &expected, "NC playback and cancellation")?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == before_tool,
        "NC playback polluted the native Undo history"
    );
    control(c, "Redo", None)?;
    unchanged(c, &expected, "NC Redo history proof")?;
    let preview_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let state = inspect(c)?;
        if caption(&state, "cam/view").starts_with("No enabled generated toolpaths") {
            break;
        }
        ensure!(
            Instant::now() < preview_deadline,
            "Valid NC-only setup did not return to a neutral preview: {}",
            caption(&state, "cam/view")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    ensure!(
        clean(c.call("solid_scene", json!({}))?) == solid,
        "NC simulation changed the CAD solid"
    );
    save(c, &fixture.project, &expected)?;
    capture(c, &fixture.out, "native-cam-nc")?;
    let captures = [
        "nc-wrong-controller",
        "nc-unknown-tool",
        "nc-error-report",
        "nc-source-editor",
        "nc-simulation-report",
        "nc-simulation-report-page-2",
        "nc-stock-complete",
        "nc-stock-start",
        "nc-stock-seek",
        "nc-stock-rewind",
        "native-cam-nc",
    ];
    for name in captures {
        let png = fs::read(fixture.out.join(format!("{name}.png")))?;
        ensure!(
            png.starts_with(b"\x89PNG\r\n\x1a\n") && png.len() > 1024,
            "Missing live NC capture {name}"
        );
    }
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required","session":fixture.session,"server":fixture.server,
            "provenance":{"cad":"shared sketch and extrusion commands","cam":"native setup/project-tool controls; zero operations","nc":"native multiline SetValue, then Build NC simulation; no simulation API seeding"},
            "checks":["wrong-controller-language","unknown-project-tool","oversized-byte-limit-survives-filename-edit","imported-source-report","zero-operation-physical-timeline","numbered-NC-block-metadata","stock-seek-rewind-play-pause","exact-model-and-undo-redo","real-solid-preserved","saved-archive-equality"],
            "source":program,"duration_seconds":duration,"dialect_rejection":dialect_error,"tool_rejection":tool_error,
            "completed":completed_state,"start":start_state,"middle":middle_state,"captures":captures,"final_model":expected
        }))?,
    )?;
    println!(
        "PASS native imported-NC simulation; pixel review required: {}",
        fixture.report.display()
    );
    Ok(())
}
