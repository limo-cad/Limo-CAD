//! Component dragging through the rendered canvas and shared joint solver.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
fn assembly(c: &mut Client) -> Result<Value> {
    c.call("assembly_document", json!({}))
}
fn frame(c: &mut Client, body: u64) -> Result<[f64; 2]> {
    ui(
        c,
        json!({"action":"view","view":"front","body_id":body,"fit":true,"duration_ms":0}),
    )?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let b = state["ui"]["canvases"]
        .as_array()
        .context("Canvas missing")?
        .iter()
        .find(|c| c["name"] == "viewport")
        .context("Viewport missing")?;
    Ok([
        b["x"].as_f64().unwrap() + b["width"].as_f64().unwrap() / 2.,
        b["y"].as_f64().unwrap() + b["height"].as_f64().unwrap() / 2.,
    ])
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut f = start(args, "native-mechanism")?;
    let c = &mut f.client;
    if controls(&ui(c, json!({"action":"inspect"}))?).any(|v| v["label"] == "Back to model browser")
    {
        control(c, "Back to model browser", None)?;
    }
    for i in 0..2 {
        begin_sketch(c, "XY")?;
        c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":i*40,"y":0},"p2":{"x":i*40+20,"y":10},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    }
    crate::native_joint_test::open(c, "slider")?;
    panel_field(
        c,
        "Limit Slide",
        None,
        "Scroll joint up",
        "Scroll joint down",
    )?;
    panel_field(
        c,
        "Slide Minimum",
        Some("-5"),
        "Scroll joint up",
        "Scroll joint down",
    )?;
    panel_field(
        c,
        "Slide Maximum",
        Some("20"),
        "Scroll joint up",
        "Scroll joint down",
    )?;
    control(c, "Apply joint", None)?;
    let source = c.call("solid_scene", json!({}))?;
    let before = assembly(c)?;
    let point = frame(c, 2)?;
    ui(
        c,
        json!({"action":"viewport","gesture":"drag","point":point,"to":[point[0],point[1]-60.]}),
    )?;
    let moved = assembly(c)?;
    let offset = moved["joints"][0]["linear_offset_mm"]
        .as_f64()
        .context("Joint coordinate missing")?;
    ensure!(
        offset > 0.05 && offset <= 20.,
        "Drag did not move the joint within its limits: {offset}"
    );
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "Drag modified source geometry"
    );
    control(c, "Undo", None)?;
    ensure!(
        assembly(c)? == before,
        "Drag Undo did not restore original coordinates"
    );
    control(c, "Redo", None)?;
    ensure!(
        assembly(c)? == moved,
        "Drag Redo did not restore exact coordinates"
    );
    let point = frame(c, 2)?;
    ui(
        c,
        json!({"action":"viewport","gesture":"drag","point":point,"to":[point[0],point[1]+30.]}),
    )?;
    let second = assembly(c)?;
    ensure!(
        second["joints"][0]["linear_offset_mm"].as_f64().unwrap() < offset,
        "Consecutive drag did not follow the displayed pose"
    );
    control(c, "Undo", None)?;
    ensure!(
        assembly(c)? == moved,
        "Consecutive drag created multiple history entries"
    );
    let point = frame(c, 1)?;
    let _ = c.call(
        "cad_interface",
        json!({"action":"viewport","gesture":"drag","point":point,"to":[point[0]+40.,point[1]]}),
    );
    ensure!(assembly(c)? == moved, "Grounded component moved");
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    capture(c, &f.out, "mechanism-final")?;
    ui(
        c,
        json!({"action":"file","command":"save","path":f.project}),
    )?;
    std::fs::write(
        &f.report,
        serde_json::to_string_pretty(
            &json!({"passed":true,"first_travel_mm":offset,"assembly":assembly(c)?}),
        )?,
    )?;
    println!("PASS native mechanism dragging: shared solver, grounded rejection, joint limits, consecutive poses, unchanged source geometry, exact single-step Undo/Redo");
    Ok(())
}
