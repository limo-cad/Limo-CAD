//! Shared-document CAM edits through real native retained controls. Captures
//! are evidence for visual review; state assertions do not claim pixel checks.
use crate::native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
mod central;
mod presets;
mod private_posts;
mod reorder;
mod reorder_drag;
mod wcs_pick;

pub(super) fn verify_tool_library_isolation(c: &mut Client, out: &std::path::Path) -> Result<()> {
    central::verify_isolation(c, out)
}

fn document(c: &mut Client) -> Result<Value> {
    let mut value = c.call("cam_get_document", json!({}))?;
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    Ok(value)
}
fn scene(c: &mut Client) -> Result<Value> {
    let mut value = c.call("solid_scene", json!({}))?;
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    Ok(value)
}
fn project_model(c: &mut Client) -> Result<Value> {
    let value = c.call("cad_project_model", json!({}))?;
    serde_json::from_str(value.as_str().context("Project export was not JSON text")?)
        .context("Parse the complete project model")
}
fn field(c: &mut Client, label: &str, value: &str) -> Result<()> {
    panel_field(c, label, Some(value), "Previous fields", "More fields")?;
    Ok(())
}
fn rejected(c: &mut Client, label: &str) -> Result<()> {
    let inspected = ui(c, json!({"action":"inspect"}))?;
    let target = controls(&inspected)
        .find(|r| r["label"] == label && r["disabled"] == false)
        .with_context(|| format!("No enabled {label}"))?["id"]
        .clone();
    let result = c.call("cad_interface", json!({"action":"click","target":target}));
    ensure!(
        result.is_err() || result.as_ref().is_ok_and(|v| v["status"] == "failed"),
        "Expected {label} to reject invalid CAM edit: {result:?}"
    );
    Ok(())
}
fn history(c: &mut Client, before: &Value, after: &Value) -> Result<()> {
    control(c, "Undo", None)?;
    ensure!(
        &c.call("cad_project_model", json!({}))? == before,
        "CAM Undo did not restore the exact model"
    );
    control(c, "Redo", None)?;
    ensure!(
        &c.call("cad_project_model", json!({}))? == after,
        "CAM Redo did not restore the exact model"
    );
    Ok(())
}
fn current(c: &mut Client) -> Result<()> {
    let statuses = c.call("cam_toolpath_statuses", json!({}))?;
    ensure!(
        statuses
            .as_array()
            .is_some_and(|rows| !rows.is_empty() && rows.iter().all(|r| r["state"] == "current")),
        "Generated toolpaths are not current: {statuses}"
    );
    Ok(())
}

fn advanced_setup(c: &mut Client, solid: &Value) -> Result<()> {
    let before = c.call("cad_project_model", json!({}))?;
    let second = solid["bodies"][1]["name"]
        .as_str()
        .context("Second CAM part missing")?;
    field(c, &format!("Model · {second}"), "true")?;
    field(c, "Stock +Z allowance (mm)", "2")?;
    field(c, "WCS orientation", "up90")?;
    control(c, "Apply", None)?;
    let cam = document(c)?;
    let setup = &cam["setups"][0];
    ensure!(
        setup["body_ids"]
            .as_array()
            .is_some_and(|ids| ids.len() == 2),
        "Native setup did not include both selected solids"
    );
    ensure!(
        setup["stock_spec"]["offsets"]["z_max"] == 2. && setup["wcs"]["origin"]["z"] == 8.,
        "Stock allowance did not resolve the WCS from the actual model"
    );
    ensure!(
        setup["wcs"]["x_axis"] == json!([0., 1., 0.])
            && setup["wcs"]["y_axis"] == json!([-1., 0., 0.]),
        "Native WCS rotation missing"
    );
    let after = c.call("cad_project_model", json!({}))?;
    history(c, &before, &after)?;
    Ok(())
}

fn advanced_tool(c: &mut Client) -> Result<()> {
    let before_cam = document(c)?;
    let before = c.call("cad_project_model", json!({}))?;
    field(c, "Cutter type", "bull_nose_end_mill")?;
    field(c, "Corner radius (mm)", "0.5")?;
    field(c, "Default step down (optional) (mm)", "1.5")?;
    field(c, "Default cutting feed (mm/min)", "1200")?;
    control(c, "Apply", None)?;
    let cam = document(c)?;
    ensure!(
        cam["tools"][0]["kind"] == "bull_nose_end_mill" && cam["tools"][0]["corner_radius"] == 0.5,
        "Native cutter type/corner edit missing"
    );
    ensure!(
        cam["tools"][0]["overall_length"] == before_cam["tools"][0]["overall_length"],
        "Corner edit changed the unedited overall length"
    );
    ensure!(
        cam["setups"][0]["operations"] == before_cam["setups"][0]["operations"],
        "Library defaults rewrote operation cutting data"
    );
    let after = c.call("cad_project_model", json!({}))?;
    history(c, &before, &after)?;
    Ok(())
}

fn advanced_operation(c: &mut Client, out: &std::path::Path) -> Result<()> {
    let before_cam = document(c)?;
    let before = c.call("cad_project_model", json!({}))?;
    field(c, "Facing direction", "climb")?;
    field(c, "Operation section", "heights")?;
    field(c, "Bottom / target reference", "model_top")?;
    field(c, "Bottom / target offset (mm)", "-0.5")?;
    field(c, "Clearance reference", "retract")?;
    field(c, "Clearance offset (mm)", "5")?;
    capture(c, out, "cam-operation-heights")?;
    field(c, "Operation section", "linking")?;
    field(c, "Link programming", "custom")?;
    field(c, "High feed (mm/min)", "4200")?;
    capture(c, out, "cam-operation-linking")?;
    control(c, "Apply", None)?;
    let after_cam = document(c)?;
    let operation = &after_cam["setups"][0]["operations"][0];
    let intent = &after_cam["height_expressions"][0];
    ensure!(
        operation["direction"] == "climb" && operation["target_z"] == -2.5,
        "Native operation parameters or model-relative depth were not applied"
    );
    ensure!(
        intent["bottom"]["reference"] == "model_top"
            && intent["bottom"]["offset"] == -0.5
            && intent["clearance"]["reference"] == "retract"
            && intent["clearance"]["offset"] == 5.,
        "Native height edit did not preserve shared associative intent"
    );
    ensure!(
        after_cam["linking"][0]["high_feed"] == 4200.,
        "Explicit linking controls did not update shared linking intent"
    );
    ensure!(
        after_cam["tools"] == before_cam["tools"]
            && after_cam["setups"][0]["stock"] == before_cam["setups"][0]["stock"]
            && operation["cutting"] == before_cam["setups"][0]["operations"][0]["cutting"]
            && after_cam["toolpath_generations"] == before_cam["toolpath_generations"],
        "Operation edit changed unrelated metadata or fabricated generation evidence"
    );
    let after = c.call("cad_project_model", json!({}))?;
    history(c, &before, &after)?;
    panel_field(
        c,
        "Link programming",
        Some("custom"),
        "Previous fields",
        "More fields",
    )?;
    field(c, "High feed (mm/min)", "-1")?;
    rejected(c, "Apply")?;
    ensure!(
        document(c)? == after_cam,
        "Invalid linking edit changed CAM"
    );
    control(c, "Reset", None)?;
    panel_field(
        c,
        "Link programming",
        Some("custom"),
        "Previous fields",
        "More fields",
    )?;
    field(c, "Operation section", "parameters")?;
    ensure!(
        document(c)? == after_cam,
        "Form section navigation changed CAM"
    );
    control(c, "Generate", None)?;
    current(c)?;
    let generated = document(c)?;
    ensure!(
        generated["height_expressions"] == after_cam["height_expressions"]
            && generated["linking"] == after_cam["linking"]
            && generated["setups"][0]["operations"][0]["target_z"] == -2.5,
        "Shared regeneration changed the edited heights or linking intent"
    );
    Ok(())
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-cam")?;
    let c = &mut fixture.client;
    ensure!(
        document(c)?["setups"].as_array().is_some_and(Vec::is_empty),
        "Choose a blank CAM document"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}))?;
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":60.,"y":0.},"p2":{"x":80.,"y":15.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch2","profile_indices":[0],"extent":{"type":"distance","distance":6.}}))?;
    let wcs_datum = if std::env::var("LIMO_CAD_NATIVE_CAM_WCS_INPUT").as_deref() == Ok("1") {
        Some(wcs_pick::datum(c)?)
    } else {
        None
    };
    let solid = scene(c)?;
    let body = solid["bodies"][0]["id"].clone();
    ensure!(body.is_number(), "CAM fixture solid missing");
    control(c, "Switch workspace", None)?;
    control(c, "Manufacture", None)?;
    let empty = document(c)?;
    control(c, "New setup", None)?;
    rejected(c, "Create")?;
    ensure!(
        document(c)? == empty,
        "Setup creation guessed a body without explicit selection"
    );
    field(c, "Model body · click to cycle", &body.to_string())?;
    field(c, "Name", "Top setup")?;
    capture(c, &fixture.out, "cam-new-setup")?;
    control(c, "Create", None)?;
    let setup_id = document(c)?["setups"][0]["id"].to_string();
    advanced_setup(c, &solid)?;
    if let Some(datum) = wcs_datum.as_ref() {
        wcs_pick::exercise(c, &fixture.out, &fixture.server, &solid, datum)?;
    }
    control(c, "Project tools", None)?;
    central::verify_isolation(c, &fixture.out)?;
    control(c, "New project tool", None)?;
    for (label, value) in [
        ("Name", "6 mm flat end mill"),
        ("Diameter (mm)", "6"),
        ("Flute length (mm)", "20"),
        ("Overall length (mm)", "50"),
        ("Default spindle (rpm)", "12000"),
        ("Default cutting feed (mm/min)", "800"),
        ("Default plunge feed (mm/min)", "200"),
    ] {
        field(c, label, value)?;
    }
    control(c, "Create", None)?;
    let source_tool_id = document(c)?["tools"][0]["id"].to_string();
    control(c, "Toolpaths", None)?;
    control(c, "New toolpath", None)?;
    field(c, "Setup · click to cycle", &setup_id)?;
    field(c, "Tool · click to cycle", &source_tool_id)?;
    field(c, "Name", "Face stock")?;
    capture(c, &fixture.out, "cam-new-face")?;
    control(c, "Create", None)?;
    control(c, "Setups", None)?;
    let before_name = c.call("cad_project_model", json!({}))?;
    field(c, "Name", "Top finish setup")?;
    field(c, "Work offset · click to cycle", "g55")?;
    control(c, "Apply", None)?;
    let after_name = c.call("cad_project_model", json!({}))?;
    history(c, &before_name, &after_name)?;
    ensure!(
        document(c)?["setups"][0]["name"] == "Top finish setup",
        "Setup edit missing"
    );
    control(c, "Generate", None)?;
    current(c)?;
    capture(c, &fixture.out, "cam-setup")?;
    let generated = document(c)?;

    control(c, "Project tools", None)?;
    field(c, "Diameter (mm)", "6.5")?;
    control(c, "Apply", None)?;
    advanced_tool(c)?;
    presets::check(c, &fixture.out)?;
    central::check(c, &fixture.out)?;
    let tool_edited = document(c)?;
    ensure!(
        tool_edited["tools"][0]["diameter"] == 6.5,
        "Cutter edit missing"
    );
    ensure!(
        tool_edited["height_expressions"] == generated["height_expressions"]
            && tool_edited["toolpath_generations"] == generated["toolpath_generations"],
        "Tool edit rewrote unrelated CAM intent or freshness evidence"
    );
    field(c, "Diameter (mm)", "-1")?;
    rejected(c, "Apply")?;
    ensure!(
        document(c)? == tool_edited,
        "Invalid cutter edit changed the CAM document"
    );
    capture(c, &fixture.out, "cam-invalid-tool")?;
    control(c, "Reset", None)?;
    control(c, "Duplicate", None)?;
    ensure!(
        document(c)? == tool_edited,
        "Opening an unsaved tool copy changed the project"
    );
    field(c, "Name", "Finishing mill")?;
    field(c, "Tool number (optional)", "2")?;
    control(c, "Create", None)?;
    let tools = document(c)?;
    ensure!(
        tools["tools"].as_array().is_some_and(|r| r.len() == 2),
        "Tool duplicate missing"
    );
    ensure!(
        tools["tools"][1]["name"] == "Finishing mill",
        "Copy did not stay selected for editing"
    );
    let copy_tool_id = tools["tools"][1]["id"].to_string();

    control(c, "Toolpaths", None)?;
    field(c, "Name", "Face finish")?;
    field(c, "Tool · click to cycle", &copy_tool_id)?;
    let inspected = ui(c, json!({"action":"inspect"}))?;
    let chooser = controls(&inspected)
        .find(|r| r["label"] == "Tool · click to cycle")
        .context("Selected tool chooser missing")?;
    ensure!(
        chooser["role"] == "combobox",
        "Tool field is not a named choice"
    );
    ensure!(
        chooser["options"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["label"] == "T2 · Finishing mill")),
        "Chooser did not offer named project tools"
    );
    field(c, "Cutting feed (mm/min)", "900")?;
    control(c, "Apply", None)?;
    let edited = document(c)?;
    ensure!(
        edited["setups"][0]["operations"][0]["name"] == "Face finish"
            && edited["setups"][0]["operations"][0]["tool_id"] == tools["tools"][1]["id"],
        "Toolpath edits missing"
    );
    ensure!(
        edited["setups"][0]["operations"][0]["cutting"]["feed_xy"] == 900.,
        "Feed edit missing"
    );
    ensure!(
        edited["height_expressions"] == generated["height_expressions"],
        "Cutting edit changed associative heights"
    );
    advanced_operation(c, &fixture.out)?;
    control(c, "Generate", None)?;
    current(c)?;
    capture(c, &fixture.out, "cam-toolpath")?;
    check_simulation(c, &fixture.out)?;
    check_post_review(c, &fixture.out)?;
    let row_input = if std::env::var("LIMO_CAD_NATIVE_CAM_ROW_INPUT").as_deref() == Ok("1") {
        let inspected = ui(c, json!({"action":"inspect"}))?;
        let session = inspected["active_session_id"]
            .as_str()
            .context("Current CAM session missing")?;
        let pid = crate::native_drawing_navigation_test::owned_pid(
            &fixture.out,
            session,
            &fixture.server,
        )?;
        let driver = crate::native_platform_test::Driver::new(pid, &fixture.out)?;
        Some(reorder_drag::exercise(c, &fixture.out, &driver)?)
    } else {
        None
    };
    let stamped = document(c)?;
    control(c, "Duplicate", None)?;
    let copied = document(c)?;
    ensure!(
        copied["setups"][0]["operations"]
            .as_array()
            .is_some_and(|r| r.len() == 2),
        "Toolpath duplicate missing"
    );
    ensure!(
        copied["height_expressions"]
            .as_array()
            .is_some_and(|r| r.len() == 2),
        "Toolpath copy lost associative height intent"
    );
    ensure!(
        copied["toolpath_generations"] == stamped["toolpath_generations"],
        "Duplicate fabricated generation evidence"
    );
    reorder::check_operation(c, &fixture.out)?;
    let before_delete = c.call("cad_project_model", json!({}))?;
    control(c, "Delete", None)?;
    let after_delete = c.call("cad_project_model", json!({}))?;
    history(c, &before_delete, &after_delete)?;
    ensure!(
        document(c)?["setups"][0]["operations"]
            .as_array()
            .is_some_and(|r| r.len() == 1),
        "Delete removed wrong toolpath"
    );

    control(c, "Setups", None)?;
    control(c, "Duplicate", None)?;
    let setups = document(c)?;
    ensure!(
        setups["setups"].as_array().is_some_and(|r| r.len() == 2),
        "Setup copy missing"
    );
    ensure!(
        setups["setups"][0]["stock"] == setups["setups"][1]["stock"]
            && setups["setups"][0]["wcs"] == setups["setups"][1]["wcs"],
        "Copy changed stock/WCS"
    );
    reorder::check_setup(c, &fixture.out)?;
    control(c, "Delete", None)?;
    ensure!(
        document(c)?["setups"]
            .as_array()
            .is_some_and(|r| r.len() == 1),
        "Setup deletion failed"
    );
    control(c, "Project tools", None)?;
    control(c, "T2 · Finishing mill", None)?;
    let before_rejected_delete = document(c)?;
    rejected(c, "Delete")?;
    ensure!(
        document(c)? == before_rejected_delete,
        "Used tool deletion changed the document"
    );
    control(c, "Toolpaths", None)?;
    field(c, "Tool · click to cycle", &source_tool_id)?;
    control(c, "Apply", None)?;
    control(c, "Generate", None)?;
    current(c)?;
    control(c, "Project tools", None)?;
    control(c, "T2 · Finishing mill", None)?;
    control(c, "Delete", None)?;
    ensure!(
        document(c)?["tools"]
            .as_array()
            .is_some_and(|r| r.len() == 1),
        "Unused tool deletion failed"
    );
    ensure!(scene(c)? == solid, "CAM editing changed the real solid");
    control(c, "Toolpaths", None)?;
    capture(c, &fixture.out, "native-cam")?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let exported = c.call("cad_project_model", json!({}))?;
    let model: Value = serde_json::from_str(exported.as_str().context("Model missing")?)?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        saved == model,
        "Native Save did not preserve CAM and CAD intent"
    );
    for name in [
        "cam-setup",
        "cam-new-setup",
        "cam-new-face",
        "cam-invalid-tool",
        "cam-cutting-presets",
        "cam-operation-cutting-preset",
        "cam-central-library",
        "cam-library-storage",
        "cam-private-posts",
        "cam-operation-reorder",
        "cam-setup-reorder",
        "cam-toolpath",
        "cam-operation-heights",
        "cam-operation-linking",
        "cam-simulation-stock",
        "cam-simulation-compare",
        "cam-simulation-model",
        "cam-simulation-report",
        "cam-playback-start",
        "cam-playback-paused",
        "cam-playback-seek",
        "cam-machine-editor",
        "cam-post-nc-review",
        "cam-post-events-review",
        "native-cam",
    ] {
        let png = std::fs::read(fixture.out.join(format!("{name}.png")))?;
        ensure!(
            png.starts_with(b"\x89PNG\r\n\x1a\n") && png.len() > 1024,
            "Capture {name} is missing or empty"
        );
    }
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required","session":fixture.session,
            "model":model,"cam":document(c)?,"solid":solid,
            "os_row_input":row_input,
            "checks":["real-solid","first-setup-tool-and-face-created-natively","explicit-body-setup-and-tool-selection","setup-edit","cutter-edit","toolpath-edit","named-tool-and-work-offset-choices",
                "operation-parameters-and-keyed-height-linking-edit","operation-section-retained-after-history-and-reset",
                "native-cutting-preset-create-copy-remove-validation-and-history","explicit-operation-preset-copy",
                "isolated-central-library-create-copy-delete-edit-and-cas","explicit-project-import-publish-history",
                "exact-library-storage-copy-and-disconnected-recovery","private-post-save-refresh-diagnostics",
                "setup-and-operation-reorder-exact-history",
                "invalid-edit-no-mutation","duplicate-and-delete-all-record-kinds","generation-evidence-not-copied",
                "used-tool-delete-rejected","explicit-shared-engine-generation","exact-undo-redo","live-window-capture","saved-model-equality",
                "native-machine-selection-and-machine-only-delta","native-machine-invalid-input-history-and-section-retention",
                "native-post-fields-and-machine-required","shared-verified-nc-and-event-warnings","post-result-and-back-to-settings","cancelled-post-no-mutation","stale-post-review-invalidated"]
        }))?,
    )?;
    println!("PASS native CAM setup/tool/toolpath edits, shared validation, Generate, exact history and Save");
    Ok(())
}

fn check_simulation(c: &mut Client, out: &std::path::Path) -> Result<()> {
    control(c, "Fit", None)?;
    let before = c.call("cad_project_model", json!({}))?;
    control(c, "Simulation settings", None)?;
    for detail in ["fine", "balanced", "auto", "fast"] {
        control(c, "Simulation detail", Some(detail))?;
        let inspected = ui(c, json!({"action":"inspect"}))?;
        ensure!(
            controls(&inspected).any(|v| v["label"] == "Simulation detail" && v["value"] == detail),
            "Simulation detail {detail} was not retained"
        );
    }
    control(c, "Comparison tolerance (mm)", Some("0.25"))?;
    let invalid = control(c, "Comparison tolerance (mm)", Some("-1"));
    ensure!(
        invalid.is_err(),
        "Negative simulation tolerance was accepted"
    );
    control(c, "Comparison tolerance (mm)", Some("0.25"))?;
    capture(c, out, "cam-simulation-settings")?;
    control(c, "Close simulation settings", None)?;
    ensure!(
        c.call("cad_project_model", json!({}))? == before,
        "Simulation preferences changed document intent"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let inspected = ui(c, json!({"action":"inspect"}))?;
        if controls(&inspected).any(|v| v["label"] == "Simulate" && v["disabled"] == false) {
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "CAM preview did not finish: {inspected}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    control(c, "Simulate", None)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    let duration: f64 = loop {
        let inspected = ui(c, json!({"action":"inspect"}))?;
        let caption = inspected["ui"]["surfaces"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| v["name"] == "cam/view")
            .and_then(|v| v["text"].as_str())
            .unwrap_or("");
        if caption.starts_with("Simulation:") {
            break caption
                .split_whitespace()
                .nth(1)
                .context("Simulation duration missing")?
                .parse()?;
        }
        let pending = controls(&inspected)
            .any(|v| v["label"] == "Cancel simulation" && v["disabled"] == false);
        ensure!(pending, "Native simulation failed: {caption}");
        ensure!(
            std::time::Instant::now() < deadline,
            "Native simulation timed out: {caption}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    control(c, "Report", None)?;
    let report = ui(c, json!({"action":"inspect"}))?;
    let text = report["ui"]["surfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["name"] == "cam-report")
        .and_then(|v| v["text"].as_str())
        .context("CAM report missing")?;
    ensure!(
        text.contains("Target comparison")
            && text.contains("Effective voxel tolerance")
            && text.contains(" contacts"),
        "CAM report omitted verification details: {text}"
    );
    ensure!(
        text.contains("Requested tolerance: 0.250 mm"),
        "CAM report did not use the entered tolerance: {text}"
    );
    capture(c, out, "cam-simulation-report")?;
    control(c, "Close report", None)?;
    for (view, name) in [
        ("Stock", "stock"),
        ("Compare", "compare"),
        ("Model", "model"),
    ] {
        control(c, view, None)?;
        let inspected = ui(c, json!({"action":"inspect"}))?;
        ensure!(
            controls(&inspected).any(|v| v["label"] == view && v["selected"] == true),
            "CAM {view} was not selected"
        );
        capture(c, out, &format!("cam-simulation-{name}"))?;
    }
    control(c, "Show toolpaths", None)?;
    let inspected = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&inspected).any(|v| v["label"] == "Show toolpaths" && v["selected"] == false),
        "Paths remained visible after toggle"
    );
    control(c, "Show toolpaths", None)?;
    control(c, "Stock", None)?;
    control(c, "Start", None)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let inspected = ui(c, json!({"action":"inspect"}))?;
        let caption = inspected["ui"]["surfaces"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| v["name"] == "cam/view")
            .and_then(|v| v["text"].as_str())
            .unwrap_or("");
        if caption.starts_with("Paused") {
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "Playback start timed out: {caption}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    capture(c, out, "cam-playback-start")?;
    control(c, "Play", None)?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    control(c, "Pause", None)?;
    capture(c, out, "cam-playback-paused")?;
    let inspected = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&inspected).any(|v| v["label"] == "Play" && v["disabled"] == false),
        "Pause did not stop playback"
    );
    let target = (duration * 0.25 * 100.).round() / 100.;
    control(c, "Playback time", Some(&target.to_string()))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let inspected = ui(c, json!({"action":"inspect"}))?;
        let caption = inspected["ui"]["surfaces"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| v["name"] == "cam/view")
            .and_then(|v| v["text"].as_str())
            .unwrap_or("");
        if caption.starts_with("Paused")
            && caption
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<f64>().ok())
                .is_some_and(|t| (t - target).abs() <= 0.06)
        {
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "Playback seek timed out: {caption}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    capture(c, out, "cam-playback-seek")?;
    for (label, forward) in [("Next move", true), ("Previous move", false)] {
        control(c, label, None)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            let inspected = ui(c, json!({"action":"inspect"}))?;
            let time = controls(&inspected)
                .find(|v| v["label"] == "Playback time")
                .and_then(|v| {
                    v["value"]
                        .as_f64()
                        .or_else(|| v["value"].as_str()?.parse().ok())
                });
            if time.is_some_and(|t| {
                if forward {
                    t > target + 1e-6
                } else {
                    t <= target + 1e-6
                }
            }) {
                break;
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "{label} did not seek past the requested time: {inspected}"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    for (before_speed, after_speed) in [
        ("1", "2"),
        ("2", "5"),
        ("5", "10"),
        ("10", "0.25"),
        ("0.25", "0.5"),
        ("0.5", "1"),
    ] {
        control(c, &format!("Speed {before_speed}x"), None)?;
        let inspected = ui(c, json!({"action":"inspect"}))?;
        ensure!(
            controls(&inspected).any(|v| v["label"] == format!("Speed {after_speed}x")),
            "Playback speed {after_speed}x was not applied"
        );
    }
    control(c, "Cancel simulation", None)?;
    ensure!(
        c.call("cad_project_model", json!({}))? == before,
        "CAM simulation changed document intent"
    );
    Ok(())
}

fn post_review_caption(inspected: &Value) -> Option<&str> {
    inspected["ui"]["surfaces"]
        .as_array()?
        .iter()
        .find(|surface| surface["name"] == "cam-export")?["text"]
        .as_str()
}

fn wait_post_review(c: &mut Client) -> Result<Value> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        let inspected = ui(c, json!({"action":"inspect"}))?;
        if controls(&inspected)
            .any(|v| v["label"] == "Save NC…" || v["label"] == "Save post events…")
        {
            return Ok(inspected);
        }
        ensure!(
            post_review_caption(&inspected).is_some(),
            "Post review closed before output was prepared"
        );
        ensure!(
            std::time::Instant::now() < deadline,
            "Native post verification timed out: {inspected}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn check_post_review(c: &mut Client, out: &std::path::Path) -> Result<()> {
    let original = project_model(c)?;
    control(c, "Post NC", None)?;
    let inspected = ui(c, json!({"action":"inspect"}))?;
    for label in ["Program name", "Program number (optional)"] {
        ensure!(
            controls(&inspected).any(|v| v["label"] == label && v["role"] == "textbox"),
            "Native {label} is not an actual text field"
        );
    }
    ensure!(
        controls(&inspected).any(|v| v["label"] == "Sequence numbers" && v["role"] == "combobox"),
        "Sequence numbers is not a typed choice"
    );
    ensure!(
        controls(&inspected).any(|v| v["label"] == "Prepare and verify" && v["disabled"] == true),
        "Posting bypassed setup review"
    );
    control(c, "Reviewed setup and machine settings", None)?;
    rejected(c, "Prepare and verify")?;
    control(c, "Close Post", None)?;
    ensure!(
        project_model(c)? == original,
        "Rejected machine-less posting changed the model"
    );

    let before_machine = project_model(c)?;
    let before_cam = document(c)?;
    ensure!(
        before_cam["setups"][0]["machine"].is_null(),
        "Negative post check requires an unassigned machine"
    );
    control(c, "Setups", None)?;
    field(c, "Setup section", "machine")?;
    field(c, "Machine / controller", "starter:grbl")?;
    field(c, "Shop machine name", "Fixture GRBL / 3-axis")?;
    field(c, "Controller tool calls", "number")?;
    field(c, "Sequence numbers", "false")?;
    field(c, "Program number (optional)", "-1")?;
    rejected(c, "Apply")?;
    ensure!(
        project_model(c)? == before_machine,
        "Invalid native machine settings changed the project"
    );
    field(c, "Program number (optional)", "4321")?;
    capture(c, out, "cam-machine-editor")?;
    control(c, "Apply", None)?;
    let assigned = document(c)?;
    let machine = &assigned["setups"][0]["machine"];
    ensure!(
        machine["profile"]["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && id != "three-axis-starter")
            && machine["profile"]["name"] == "Fixture GRBL / 3-axis"
            && machine["profile"]["schema_version"] == 1
            && machine["profile"]["revision"] == 2
            && machine["profile"]["controller"]["family"] == "grbl"
            && machine["profile"]["controller"]["language"] == "grbl"
            && machine["mode"] == "fixed3_axis",
        "Native GRBL selection did not produce the requested shop snapshot"
    );
    ensure!(machine["profile"]["post"] == json!({"dialect":"grbl","program_number":4321,
        "sequence_numbers":false,"siemens_828d":null,"tool_call_mode":"number","machine_retract_z":null}),
        "Native machine fields did not preserve the selected post settings or unknown machine coordinate");
    ensure!(machine["profile"]["axes"].as_array().is_some_and(|axes| axes.len() == 3
        && axes.iter().all(|axis| axis["limits"].is_null())), "Starter invented verified axis travel");
    let mut expected_cam = before_cam.clone();
    expected_cam["setups"][0]["machine"] = machine.clone();
    ensure!(
        assigned == expected_cam,
        "Assigning a machine changed operations, tools, stock, WCS or generation evidence"
    );
    let after_machine = project_model(c)?;
    let mut expected_model = before_machine.clone();
    expected_model["cam"] = after_machine["cam"].clone();
    ensure!(
        after_machine == expected_model,
        "Native machine assignment changed non-CAM project data"
    );
    control(c, "Undo", None)?;
    ensure!(
        project_model(c)? == before_machine,
        "Native machine Undo did not restore the complete project"
    );
    field(c, "Machine / controller", "generic")?;
    control(c, "Redo", None)?;
    ensure!(
        project_model(c)? == after_machine,
        "Native machine Redo did not restore the complete project"
    );
    field(c, "Shop machine name", "Fixture GRBL / 3-axis")?;
    ensure!(
        project_model(c)? == after_machine,
        "Machine section navigation changed the model"
    );
    private_posts::check(c, out)?;
    field(c, "Setup section", "setup")?;
    control(c, "Toolpaths", None)?;
    field(c, "Operation section", "heights")?;
    field(c, "Bottom / target offset (mm)", "0")?;
    control(c, "Apply", None)?;
    field(c, "Operation section", "parameters")?;
    control(c, "Generate", None)?;
    current(c)?;
    let before = project_model(c)?;
    let cam = document(c)?;
    let setup_id = cam["setups"][0]["id"].clone();
    let mut config = cam["setups"][0]["machine"]["profile"]["post"].clone();
    config["program_number"] = json!(1234);
    config["sequence_numbers"] = json!(true);
    let expected_nc = c.call(
        "cam_post_setup",
        json!({"setup_id":setup_id,"post":config,"program_name":"Native review fixture"}),
    )?;
    let expected_events = c.call("cam_post_events", json!({"setup_id":setup_id}))?;
    for (open, expected, capture_name) in [
        ("Post NC", &expected_nc, "cam-post-nc-review"),
        ("Post events", &expected_events, "cam-post-events-review"),
    ] {
        control(c, open, None)?;
        if open == "Post NC" {
            control(c, "Program name", Some("Native review fixture"))?;
            control(c, "Program number (optional)", Some("1234"))?;
            control(c, "Sequence numbers", Some("true"))?;
        }
        control(c, "Reviewed setup and machine settings", None)?;
        control(c, "Prepare and verify", None)?;
        let inspected = wait_post_review(c)?;
        let caption = post_review_caption(&inspected).context("Prepared post caption missing")?;
        for warning in expected["warnings"]
            .as_array()
            .context("Shared post warnings missing")?
        {
            let warning = warning.as_str().context("Post warning was not text")?;
            ensure!(
                caption.contains(warning),
                "Native review omitted a shared warning: {warning}"
            );
        }
        if open == "Post NC" {
            let nc = expected["nc"].as_str().context("Shared NC missing")?;
            ensure!(
                caption.contains(&format!("{} NC lines", nc.lines().count())),
                "Native review line count differs from shared NC"
            );
            ensure!(
                caption.contains(&nc.lines().take(20).collect::<Vec<_>>().join("\n")),
                "Native preview differs from shared NC"
            );
            ensure!(
                caption.contains(&format!(
                    "{} operations",
                    expected["program"]["stats"]["operation_count"]
                )),
                "Native post operation count differs"
            );
        } else {
            let count = expected["events"]
                .as_array()
                .context("Shared post events missing")?
                .len();
            ensure!(
                caption.contains(&format!("{count} post events")),
                "Native post event count differs"
            );
        }
        let save_label = if open == "Post NC" {
            "Save NC…"
        } else {
            "Save post events…"
        };
        ensure!(
            controls(&inspected).any(|v| v["label"] == save_label && v["disabled"] == false),
            "Prepared output cannot be saved"
        );
        ensure!(
            !controls(&inspected).any(|v| v["label"] == "Reviewed post result and warnings"),
            "Native posting added a second review gate"
        );
        let prepared_caption = caption.to_owned();
        capture(c, out, capture_name)?;
        control(c, "Back to settings", None)?;
        let settings = ui(c, json!({"action":"inspect"}))?;
        ensure!(
            controls(&settings)
                .any(|v| v["label"] == "Prepare and verify" && v["disabled"] == true)
                && !controls(&settings).any(|v| v["label"] == save_label),
            "Back to settings retained old prepared output or bypassed machine review"
        );
        if open == "Post NC" {
            for (label, value) in [
                ("Program name", "Native review fixture"),
                ("Program number (optional)", "1234"),
                ("Sequence numbers", "true"),
            ] {
                ensure!(
                    controls(&settings).any(|v| v["label"] == label && v["value"] == value),
                    "Back to settings did not retain {label}"
                );
            }
        }
        control(c, "Reviewed setup and machine settings", None)?;
        control(c, "Prepare and verify", None)?;
        let prepared_again = wait_post_review(c)?;
        ensure!(
            post_review_caption(&prepared_again) == Some(prepared_caption.as_str()),
            "Back to settings lost the selected program settings"
        );
        control(c, "Close Post", None)?;
        ensure!(
            project_model(c)? == before,
            "Cancelled post review changed the model"
        );
    }

    control(c, "Post NC", None)?;
    control(c, "Reviewed setup and machine settings", None)?;
    control(c, "Prepare and verify", None)?;
    wait_post_review(c)?;
    let mut stale_cam = document(c)?;
    stale_cam["setups"][0]["name"] = json!("Changed during native post review");
    c.call("cam_set_document", stale_cam)?;
    let inspected = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        post_review_caption(&inspected).is_none()
            && !controls(&inspected)
                .any(|v| v["label"] == "Save NC…" || v["label"] == "Save post events…"),
        "A project edit left stale prepared output saveable"
    );
    control(c, "Undo", None)?;
    let restored = project_model(c)?;
    if restored != before {
        std::fs::write(
            out.join("post-stale-before.json"),
            serde_json::to_string_pretty(&before)?,
        )?;
        std::fs::write(
            out.join("post-stale-restored.json"),
            serde_json::to_string_pretty(&restored)?,
        )?;
        anyhow::bail!("Stale-output check did not restore the prior project exactly; before/restored evidence saved");
    }
    current(c)?;
    Ok(())
}
