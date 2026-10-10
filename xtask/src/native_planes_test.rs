//! Construction tools through rendered fields, browser rows and canvas picking.
use crate::native_fixture::{
    begin_sketch, browser_select, control, controls, edit_feature, start, ui,
};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

fn definitions(client: &mut Client) -> Result<Value> {
    client.call("construction_plane_definitions", json!({}))
}
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-planes")?;
    let client = &mut fixture.client;
    control(client, "Top", None)?;
    control(client, "Offset Plane", None)?;
    browser_select(client, "Origin", "XY")?;
    control(client, "Offset distance", Some("1 in"))?;
    let empty = client.call("cad_document", json!({}))?;
    let state = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&state).any(|c| c["label"] == "Offset plane distance"),
        "Missing on-canvas distance field"
    );
    let canvas = state["ui"]["canvases"]
        .as_array()
        .and_then(|v| v.iter().find(|c| c["name"] == "viewport"))
        .context("Viewport bounds missing")?;
    let bounds = canvas;
    let x = bounds["x"].as_f64().unwrap() + bounds["width"].as_f64().unwrap() / 2.;
    let y = bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap() / 2.;
    ui(
        client,
        json!({"action":"viewport","gesture":"drag","world":[0.,0.,25.4],"to":[x,y-40.]}),
    )?;
    let state = ui(client, json!({"action":"inspect"}))?;
    let distance = controls(&state)
        .find(|c| c["label"] == "Offset distance")
        .context("Distance field missing")?["value"]
        .as_str()
        .context("Distance text missing")?
        .parse::<f64>()?;
    ensure!(
        (distance - 35.4).abs() < 0.02,
        "Offset drag did not follow the normal: {distance}"
    );
    ensure!(
        client.call("cad_document", json!({}))? == empty,
        "Dragging committed a feature"
    );
    control(client, "Offset plane distance", Some("NaN"))?;
    let state = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&state).any(|c| c["label"] == "Apply Offset Plane" && c["disabled"] == true),
        "Invalid offset accepted"
    );
    control(client, "Offset distance", Some("25"))?;
    ui(client, json!({"action":"view","view":"isometric"}))?;
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("offset-plane-form.png")}),
    )?;
    control(client, "Apply Offset Plane", None)?;
    let first = definitions(client)?[0].clone();
    ensure!(
        (first["basis"]["origin"][2].as_f64().unwrap() - 25.).abs() < 1e-8,
        "Wrong offset plane"
    );
    let name = first["name"]
        .as_str()
        .context("Plane name missing")?
        .to_owned();
    edit_feature(client, &name, true)?;
    control(client, "Offset distance", Some("30"))?;
    control(client, "Close Offset Plane", None)?;
    ensure!(definitions(client)?[0] == first, "Cancel changed the plane");
    edit_feature(client, &name, false)?;
    control(client, "Offset distance", Some("30"))?;
    control(client, "Apply Offset Plane", None)?;
    control(client, "Undo", None)?;
    ensure!(definitions(client)?[0] == first, "Undo failed");
    control(client, "Redo", None)?;
    ensure!(
        definitions(client)?[0]["source"]["distance"] == 30.,
        "Redo failed"
    );
    control(client, "Midplane", None)?;
    browser_select(client, "Origin", "XY")?;
    browser_select(client, "Origin", "XZ")?;
    let state = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&state).any(|c| c["label"] == "Apply Midplane" && c["disabled"] == true),
        "Perpendicular midplane accepted"
    );
    control(client, "XZ origin plane", None)?;
    browser_select(client, "Construction", &name)?;
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("midplane-form.png")}),
    )?;
    control(client, "Apply Midplane", None)?;
    let planes = definitions(client)?;
    let mid = planes.as_array().unwrap().last().unwrap();
    ensure!(
        (mid["basis"]["origin"][2].as_f64().unwrap() - 15.).abs() < 1e-8,
        "Wrong midplane"
    );
    let mid_name = mid["name"].as_str().unwrap().to_owned();
    edit_feature(client, &mid_name, false)?;
    control(client, "Close Midplane", None)?;
    begin_sketch(client, "XY")?;
    client.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":60.,"y":0.},"p2":{"x":100.,"y":20.},"ctrl_held":true}),
    )?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":20.}}))?;
    control(client, "Front", None)?;
    control(client, "Plane at Angle", None)?;
    browser_select(client, "Origin", "XY")?;
    ui(
        client,
        json!({"action":"viewport","gesture":"move","world":[80.,0.,0.]}),
    )?;
    ui(
        client,
        json!({"action":"viewport","gesture":"click","world":[80.,0.,0.]}),
    )?;
    control(client, "Angle", Some("30 deg"))?;
    ui(
        client,
        json!({"action":"view","view":"isometric","fit":true}),
    )?;
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("angle-plane-form.png")}),
    )?;
    control(client, "Apply Plane at Angle", None)?;
    let planes = definitions(client)?;
    let angle = planes.as_array().unwrap().last().unwrap();
    ensure!(
        (angle["basis"]["normal"][2].as_f64().unwrap() - 30f64.to_radians().cos()).abs() < 1e-8,
        "Wrong plane angle"
    );
    let angle_name = angle["name"].as_str().unwrap().to_owned();
    edit_feature(client, &angle_name, true)?;
    control(client, "Angle", Some("45"))?;
    control(client, "Apply Plane at Angle", None)?;
    control(client, "Undo", None)?;
    ensure!(definitions(client)? == planes, "Angle edit undo failed");
    control(client, "Redo", None)?;
    control(client, "Create Sketch", None)?;
    browser_select(client, "Construction", &angle_name)?;
    client.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":10.,"y":10.},"p2":{"x":20.,"y":10.},"ctrl_held":true}))?;
    control(client, "Finish sketch", None)?;
    control(client, "Isometric", None)?;
    ui(client, json!({"action":"capture","path":fixture.capture}))?;
    ui(
        client,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let final_scene = client.call("solid_scene", json!({}))?;
    ensure!(
        final_scene["errors"].as_array().is_some_and(Vec::is_empty),
        "Construction left invalid geometry"
    );
    std::fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"planes":definitions(client)?,"project":fixture.project,"capture":fixture.capture}),
        )?,
    )?;
    println!("PASS: offset drag and units, native plane references, all three forms, validation, edits, Cancel, Undo/Redo, dependent sketch and Save");
    Ok(())
}
