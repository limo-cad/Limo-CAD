//! Native forms edit the existing drawing DTO, with real-solid capture and
//! exact model/history/archive checks. OS input is exercised by native-platform.
use crate::native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

fn clean(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    value
}
fn drawing(client: &mut Client) -> Result<Value> {
    client.call("drawing_document", json!({})).map(clean)
}
fn model(client: &mut Client) -> Result<Value> {
    let value = client.call("cad_project_model", json!({}))?;
    Ok(serde_json::from_str(
        value.as_str().context("Project model JSON text")?,
    )?)
}
fn field(client: &mut Client, label: &str, value: &str) -> Result<Value> {
    panel_field(client, label, Some(value), "Previous fields", "More fields")
}
fn rejected(client: &mut Client, label: &str, value: Option<&str>, expected: &str) -> Result<()> {
    let state = ui(client, json!({"action":"inspect"}))?;
    let found = controls(&state)
        .find(|c| c["label"] == label && c["disabled"] == false)
        .context("Expected enabled control for rejected action")?;
    let request = if let Some(value) = value {
        json!({"action":"set_value","target":found["id"],"value":value})
    } else {
        json!({"action":"click","target":found["id"]})
    };
    let result = client.rpc(
        "tools/call",
        json!({"name":"cad_interface","arguments":request}),
    )?;
    let text = result["content"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["type"] == "text"))
        .and_then(|item| item["text"].as_str())
        .context("Expected rejection must contain its structured interface result")?;
    let response: Value = serde_json::from_str(text)?;
    ensure!(
        response["status"] == "failed"
            && response["error"]
                .as_str()
                .is_some_and(|error| error.contains(expected)),
        "Expected drawing rejection '{expected}', received: {response}"
    );
    Ok(())
}
fn history(client: &mut Client, before: &Value, after: &Value) -> Result<()> {
    control(client, "Undo", None)?;
    ensure!(
        &model(client)? == before,
        "Drawing Undo did not restore the exact model"
    );
    control(client, "Redo", None)?;
    ensure!(
        &model(client)? == after,
        "Drawing Redo did not restore the exact model"
    );
    Ok(())
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-drawing-editor")?;
    let client = &mut fixture.client;
    ensure!(
        drawing(client)?["sheets"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Choose a blank document without existing sheets"
    );
    begin_sketch(client, "XY")?;
    client.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
    )?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}))?;
    let solid = clean(client.call("solid_scene", json!({}))?);
    control(client, "Switch workspace", None)?;
    control(client, "Drawing", None)?;
    for _ in 0..8 {
        control(client, "New Sheet", None)?;
    }
    let initial = drawing(client)?;
    ensure!(
        initial["sheets"].as_array().is_some_and(|s| s.len() == 8),
        "Native sheet creation did not retain all eight sheets"
    );
    let seventh = initial["sheets"][6]["id"]
        .as_u64()
        .context("Seventh sheet missing")?;
    control(client, "Sheet", Some(&seventh.to_string()))?;
    ensure!(
        drawing(client)?["active_sheet_id"] == seventh,
        "Could not select sheet7 beyond the old ribbon limit"
    );
    capture(client, &fixture.out, "drawing-sheet-seven")?;
    let before = model(client)?;
    field(client, "Standard", "ansi")?;
    field(client, "Sheet name", "Production sheet")?;
    rejected(
        client,
        "Sheet",
        Some(&initial["sheets"][0]["id"].to_string()),
        "Apply or reset the drawing edit first",
    )?;
    rejected(
        client,
        "New Sheet",
        None,
        "Apply or reset the drawing edit first",
    )?;
    ensure!(
        model(client)? == before,
        "Changing a draft or rejected selection mutated the drawing"
    );
    field(client, "Format", "ansi_b")?;
    field(client, "Orientation", "portrait")?;
    field(client, "Projection", "first_angle")?;
    field(client, "General tolerance", "custom")?;
    field(client, "Tolerance note", "Company tolerance 0.25 mm")?;
    for (label, value) in [
        ("Title", "Native Café 零件"),
        ("Drawing number", "DWG-007"),
        ("Revision", "B"),
        ("Drawn by", "Engineer"),
        ("Checked by", "Checker"),
        ("Approved by", "Approver"),
        ("Company", "Existing drawing workflow"),
        ("Material", "Aluminium"),
        ("Finish", "Deburr"),
    ] {
        field(client, label, value)?;
    }
    control(client, "Apply", None)?;
    let edited = drawing(client)?;
    let sheet = &edited["sheets"][6];
    ensure!(
        sheet["name"] == "Production sheet"
            && sheet["format"] == "ansi_b"
            && sheet["orientation"] == "portrait"
            && sheet["standard"] == "ansi"
            && sheet["projection_method"] == "first_angle",
        "Native sheet setup was not applied: {sheet}"
    );
    ensure!(
        sheet["tolerance_note"] == json!({"preset":"custom","custom":"Company tolerance 0.25 mm"}),
        "Custom tolerance was not retained"
    );
    ensure!(
        sheet["title_block"]["title"] == "Native Café 零件"
            && sheet["title_block"]["approved_by"] == "Approver"
            && sheet["title_block"]["finish"] == "Deburr",
        "Title-block edits were not retained"
    );
    for index in [0, 1, 2, 3, 4, 5, 7] {
        ensure!(
            edited["sheets"][index] == initial["sheets"][index],
            "Editing sheet7 changed sheet{}",
            index + 1
        );
    }
    let after = model(client)?;
    history(client, &before, &after)?;
    capture(client, &fixture.out, "drawing-sheet-setup")?;

    control(client, "Auto-layout", None)?;
    let arranged = drawing(client)?;
    let views = arranged["sheets"][6]["views"]
        .as_array()
        .context("Auto-layout views missing")?;
    ensure!(
        views.len() == 4
            && views[0]["kind"] == "front"
            && views[1]["kind"] == "top"
            && views[2]["kind"] == "right"
            && views[3]["kind"] == "isometric",
        "Native auto-layout did not create the conventional group"
    );
    ensure!(
        views[1]["position"][1].as_f64().unwrap() > views[0]["position"][1].as_f64().unwrap()
            && views[2]["position"][0].as_f64().unwrap()
                < views[0]["position"][0].as_f64().unwrap(),
        "First-angle layout convention was not retained"
    );
    let before = model(client)?;
    control(client, "View", Some(&views[1]["id"].to_string()))?;
    field(client, "Scale (paper mm / model mm)", "0.5")?;
    control(client, "Apply", None)?;
    let scaled = drawing(client)?;
    ensure!(
        scaled["sheets"][6]["views"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["scale"] == 0.5),
        "Editing a projected view did not rescale the complete group"
    );
    let after = model(client)?;
    history(client, &before, &after)?;
    control(client, "View", Some(&views[0]["id"].to_string()))?;
    field(client, "View name", "Front production")?;
    field(client, "Paper X (mm)", "140")?;
    field(client, "Paper Y (mm)", "190")?;
    control(client, "Apply", None)?;
    let moved = drawing(client)?;
    let moved_views = moved["sheets"][6]["views"].as_array().unwrap();
    ensure!(
        moved_views[0]["position"] == json!([140., 190.])
            && moved_views[1]["position"][0] == 140.
            && moved_views[2]["position"][1] == 190.,
        "Native placement broke projected alignment"
    );
    ensure!(
        moved_views[3] == scaled["sheets"][6]["views"][3],
        "Moving a base view moved the freely placed isometric view"
    );
    let before_invalid = model(client)?;
    field(client, "Scale (paper mm / model mm)", "0")?;
    rejected(client, "Apply", None, "Scale must be greater than zero")?;
    ensure!(
        model(client)? == before_invalid,
        "Invalid scale changed the model"
    );
    control(client, "Reset", None)?;
    capture(client, &fixture.out, "drawing-view-edit")?;
    ensure!(
        clean(client.call("solid_scene", json!({}))?) == solid,
        "Drawing editing changed the real solid"
    );
    let mut with_tables = model(client)?;
    let sheet = &mut with_tables["drawings"]["sheets"][6];
    sheet["revision_table_position"] = json!([12., 20.]);
    sheet["bom_table_position"] = json!([12., 55.]);
    sheet["revisions"] = json!([
        {"id":1,"revision":"A","description":"Initial","date":"2026-09-25","author":"Engineer","checked_by":"Checker","approved_by":"QA","change_order":"ECO-1","status":"released"},
        {"id":2,"revision":"B","description":"","date":"2026-09-26","author":"Engineer","checked_by":"Checker","approved_by":"","change_order":"ECO-7","status":"draft"}
    ]);
    sheet["bom"] = json!([
        {"id":1,"item_number":"1","body_id":null,"part_number":"P-7","description":"Plate \u{96f6}\u{4ef6}","quantity":2.5,"material":"Al","finish":"Deburr"},
        {"id":2,"item_number":"2","body_id":null,"part_number":"PIN-1","description":"Dowel pin","quantity":10.,"material":"Steel","finish":""}
    ]);
    with_tables["drawings"]["next_revision_id"] = json!(3);
    with_tables["drawings"]["next_bom_item_id"] = json!(3);
    client.call(
        "cad_load_project_model",
        json!({"model_json":serde_json::to_string(&with_tables)?}),
    )?;
    ensure!(
        model(client)? == with_tables,
        "Loading saved table intent changed the project"
    );
    control(client, "Switch workspace", None)?;
    control(client, "Drawing", None)?;
    control(client, "Sheet setup", None)?;
    capture(client, &fixture.out, "drawing-title-revision-bom")?;
    ensure!(
        model(client)? == with_tables,
        "Rendering sheet metadata changed saved intent"
    );
    let full_title = "\u{96f6}".repeat(300);
    field(client, "Title", &full_title)?;
    control(client, "Apply", None)?;
    ensure!(
        model(client)?["drawings"]["sheets"][6]["title_block"]["title"] == full_title,
        "Overflowing title text was silently truncated"
    );
    capture(client, &fixture.out, "drawing-title-overflow")?;
    control(client, "Undo", None)?;
    ensure!(
        model(client)? == with_tables,
        "Undo did not restore complete sheet table intent"
    );
    ensure!(
        clean(client.call("solid_scene", json!({}))?) == solid,
        "Loading sheet tables changed the real solid"
    );
    ui(
        client,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        saved == model(client)?,
        "Save did not retain the exact drawing project"
    );
    std::fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"state_checks_passed":true,"pixel_review":"required","session":fixture.session,"drawing":with_tables["drawings"],"captures":["drawing-sheet-seven.png","drawing-sheet-setup.png","drawing-view-edit.png","drawing-title-revision-bom.png","drawing-title-overflow.png"],"checks":["native eight-sheet creation and selection","metadata/title-block preservation","dirty navigation rejection","first-angle auto-layout","group scale and aligned placement","invalid scale rejection","exact Undo/Redo and Save","real solid unchanged","saved title/revision/BOM frame","overflow marker preserves complete title"]}),
        )?,
    )?;
    println!(
        "Native drawing editor state/history/save checks passed; review five captures in {}",
        fixture.out.display()
    );
    Ok(())
}
