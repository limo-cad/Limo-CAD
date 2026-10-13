//! The first complete modeling workflow through the native controls, followed
//! by File close/reopen and an independent engine reading the actual archive.
use crate::{
    native_fixture::{begin_sketch, click, control, controls, edit_feature, field, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, process::Command, time::Duration};

fn model(client: &mut Client) -> Result<Value> {
    let value = client.call("cad_project_model", json!({}))?;
    serde_json::from_str(value.as_str().context("Project export is not JSON text")?)
        .context("Parse project model")
}

fn reattach(client: &mut Client, result: &Value) -> Result<String> {
    let session = result["active_session_id"]
        .as_str()
        .context("File transition did not identify its active document")?;
    client.call("cad_attach", json!({"session_id":session}))?;
    Ok(session.into())
}

fn solid(client: &mut Client, height: f64) -> Result<Value> {
    let scene = client.call("solid_scene", json!({}))?;
    ensure!(scene["errors"] == json!([]), "Model errors: {scene}");
    let bodies = scene["bodies"].as_array().context("Bodies missing")?;
    ensure!(
        bodies.len() == 1,
        "Expected one solid, got {}",
        bodies.len()
    );
    let positions = bodies[0]["mesh"]["positions"]
        .as_array()
        .context("Solid mesh missing")?;
    let maximum_z = positions
        .as_chunks::<3>()
        .0
        .iter()
        .map(|point| point[2].as_f64().unwrap_or(f64::NAN))
        .fold(f64::NEG_INFINITY, f64::max);
    ensure!(
        (maximum_z - height).abs() < 1e-3,
        "Expected {height} mm height, got {maximum_z}"
    );
    Ok(scene)
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-lifecycle")?;
    let client = &mut fixture.client;
    let original_session = fixture.session.clone();
    let created = control(client, "New design", None)?;
    let created_session = reattach(client, &created)?;
    ensure!(
        created_session != original_session,
        "New reused the original tab"
    );
    ui(
        client,
        json!({"action":"file","command":"rename","name":"Native lifecycle regression"}),
    )?;

    begin_sketch(client, "XY")?;
    control(client, "Rectangle", None)?;
    click(client, [-20., -12.], false)?;
    click(client, [20., 12.], false)?;
    control(client, "Finish sketch", None)?;
    let sketched = model(client)?;
    ensure!(
        client
            .call("sketch_finished", json!({}))?
            .as_array()
            .is_some_and(|s| s.len() == 1),
        "Rectangle sketch was not retained"
    );

    control(client, "Extrude", None)?;
    click(client, [0., 0.], false)?;
    field(client, "Distance (mm)", Some("unfinished+"))?;
    let invalid = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&invalid).any(|c| c["label"] == "Apply Extrude" && c["disabled"] == true),
        "Invalid distance did not block Apply"
    );
    field(client, "Distance (mm)", Some("2.5 cm"))?;
    ensure!(
        model(client)? == sketched,
        "Preview modified the live document"
    );
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("extrude-preview.png")}),
    )?;
    control(client, "Apply Extrude", None)?;
    solid(client, 25.)?;
    let created_model = model(client)?;
    let document = client.call("cad_document", json!({}))?;
    let name = document["features"]
        .as_array()
        .and_then(|f| f.last())
        .and_then(|f| f["name"].as_str())
        .context("Extrude history entry missing")?
        .to_owned();
    let feature_id = document["features"].as_array().unwrap().last().unwrap()["id"].clone();

    edit_feature(client, &name, false)?;
    field(client, "Distance (mm)", Some("35 mm"))?;
    ensure!(
        model(client)? == created_model,
        "Edit preview modified the live document"
    );
    control(client, "Close Extrude", None)?;
    ensure!(
        model(client)? == created_model,
        "Cancel changed the original extrusion"
    );
    solid(client, 25.)?;

    edit_feature(client, &name, true)?;
    field(client, "Distance (mm)", Some("35 mm"))?;
    control(client, "Apply Extrude", None)?;
    solid(client, 35.)?;
    let edited_document = client.call("cad_document", json!({}))?;
    ensure!(
        edited_document["features"]
            .as_array()
            .is_some_and(|f| f.len() == 2 && f[1]["id"] == feature_id),
        "Edit appended a feature instead of replacing its definition"
    );
    control(client, "Undo", None)?;
    solid(client, 25.)?;
    control(client, "Redo", None)?;
    let expected_scene = solid(client, 35.)?;
    let expected_model = model(client)?;
    control(client, "Isometric", None)?;
    ui(client, json!({"action":"capture","path":fixture.capture}))?;
    ui(
        client,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let bytes = fs::read(&fixture.project).context("Read native File Save output")?;
    let archived = crate::project_archive::model(&bytes)?;
    ensure!(
        serde_json::from_str::<Value>(&archived)? == expected_model,
        "File Save did not preserve the edited model"
    );

    let closed = ui(client, json!({"action":"file","command":"close"}))?;
    ensure!(
        closed["awaiting_input"] != true,
        "Saved tab still asks to discard changes"
    );
    ensure!(
        reattach(client, &closed)? == original_session,
        "Close did not return to the original blank tab"
    );
    let opened = ui(
        client,
        json!({"action":"file","command":"open","path":fixture.project}),
    )?;
    reattach(client, &opened)?;
    ensure!(
        model(client)? == expected_model,
        "Native File Open changed the saved model"
    );
    ensure!(
        solid(client, 35.)? == expected_scene,
        "Native File Open changed the solid"
    );
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("reopened.png")}),
    )?;

    let mut command = Command::new(&fixture.server);
    command.arg("--headless");
    let mut cold = Client::start_command(command, Some(Duration::from_secs(45)))?;
    cold.call("cad_load_project_model", json!({"model_json":archived}))?;
    ensure!(
        model(&mut cold)? == expected_model,
        "Cold reopen changed the saved model"
    );
    ensure!(
        solid(&mut cold, 35.)? == expected_scene,
        "Cold reopen changed the solid"
    );
    cold.finish(Duration::from_secs(10))?;

    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(&json!({
            "passed":true,"project":fixture.project,"capture":fixture.capture,
            "archive_bytes":bytes.len(),"feature_id":feature_id,"height_mm":35,
            "native_reopen":true,"cold_reopen":true,
            "steps":["new","sketch rectangle","invalid distance","preview","apply","edit preview","cancel","edit apply","undo","redo","save","close","open","cold reopen"]
        }))?,
    )?;
    println!("PASS: native New → sketch → Extrude preview/edit/Cancel/Apply → Undo/Redo → Save → close → reopen, with independent archive recomputation");
    Ok(())
}
