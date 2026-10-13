//! Actual assembly inspector controls over known overlapping parametric stock.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
fn field(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    panel_field(
        c,
        label,
        value,
        "Scroll assembly up",
        "Scroll assembly down",
    )
}
fn assembly(c: &mut Client) -> Result<Value> {
    c.call("assembly_document", json!({}))
}
fn disabled(c: &mut Client, label: &str) -> Result<bool> {
    Ok(controls(&ui(c, json!({"action":"inspect"}))?)
        .any(|b| b["label"] == label && b["disabled"] == true))
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-inspect")?;
    let c = &mut fixture.client;
    let initial = ui(c, json!({"action":"inspect"}))?;
    if controls(&initial).any(|b| b["label"] == "Back to model browser") {
        control(c, "Back to model browser", None)?;
    }
    for i in 0..2 {
        begin_sketch(c, "XY")?;
        c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":i*5,"y":0},"p2":{"x":i*5+10,"y":10},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10}}))?;
    }
    let model = c.call("cad_project_model", json!({}))?;
    let mut model: Value = serde_json::from_str(model.as_str().context("Model JSON missing")?)?;
    model["assembly"]["motion_studies"] = json!([
        {"id":1,"name":"Stationary pair","duration_seconds":0.1,"playback_speed":1.,"looped":false,"drivers":[],"next_driver_id":1},
        {"id":2,"name":"Second sample","duration_seconds":0.2,"playback_speed":1.,"looped":false,"drivers":[],"next_driver_id":1}
    ]);
    model["assembly"]["next_motion_study_id"] = json!(3);
    c.call(
        "cad_load_project_model",
        json!({"model_json":model.to_string()}),
    )?;
    let source = c.call("solid_scene", json!({}))?;
    let before = assembly(c)?;
    control(c, "Assembly", None)?;
    control(c, "Assembly Inspect", None)?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    capture(c, &fixture.out, "inspect-initial")?;
    field(c, "Inspection clearance", Some("-1"))?;
    ensure!(
        disabled(c, "Check current interference")?,
        "Negative clearance accepted"
    );
    field(c, "Inspection clearance", Some("0.2 mm"))?;
    field(c, "Inspection sample rate", Some("NaN"))?;
    ensure!(
        disabled(c, "Check swept collisions")?,
        "Invalid sample rate accepted"
    );
    field(c, "Inspection sample rate", Some("10"))?;
    let checked = field(c, "Check current interference", None)?;
    ensure!(
        checked["value"]["exact"] == true,
        "Native inspection fell back to meshes"
    );
    let pair = &checked["value"]["pairs"][0];
    ensure!(
        pair["interfering"] == true
            && (pair["overlap_volume_mm3"].as_f64().unwrap() - 500.).abs() < 1e-5,
        "Exact intersection volume changed: {pair}"
    );
    ensure!(assembly(c)? == before, "Inspection changed the document");
    field(c, "Swept motion study", None)?;
    field(c, "Swept motion study: Second sample", None)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let choice = controls(&state)
        .find(|b| b["label"] == "Swept motion study")
        .context("Study chooser absent")?;
    ui(
        c,
        json!({"action":"key","target":choice["id"],"key":"Home"}),
    )?;
    field(c, "Stop at first collision", None)?;
    let swept = field(c, "Check swept collisions", None)?;
    ensure!(
        swept["value"]["exact"] == true && !swept["value"]["events"].as_array().unwrap().is_empty(),
        "Swept inspector missed known overlap"
    );
    ensure!(assembly(c)? == before, "Swept inspection changed model");
    capture(c, &fixture.out, "inspect-results")?;
    field(c, "Second contact body", Some("1:1"))?;
    ensure!(
        disabled(c, "Create physical stop")?,
        "Same-instance contact enabled"
    );
    field(c, "Second contact body", Some("2:2"))?;
    field(c, "Create physical stop", None)?;
    let made = assembly(c)?;
    ensure!(
        made["contact_sets"][0]["clearance_mm"] == 0.2,
        "Contact clearance lost"
    );
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == before, "Contact create Undo failed");
    control(c, "Redo", None)?;
    ensure!(assembly(c)? == made, "Contact create Redo failed");
    field(c, "Contact 1 name", Some("Safety stop"))?;
    field(c, "Contact 1 clearance", Some("-1"))?;
    field(c, "Contact 1 clearance", Some("0.04 cm"))?;
    field(c, "Apply contact 1", None)?;
    let edited = assembly(c)?;
    ensure!(
        edited["contact_sets"][0]["name"] == "Safety stop"
            && edited["contact_sets"][0]["clearance_mm"] == 0.4,
        "Typed contact edit lost units"
    );
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == made, "Contact edit Undo failed");
    control(c, "Redo", None)?;
    ensure!(assembly(c)? == edited, "Contact edit Redo failed");
    field(c, "Contact 1 enabled", None)?;
    ensure!(
        assembly(c)?["contact_sets"][0]["enabled"] == false,
        "Disable failed"
    );
    field(c, "Contact 1 enabled", None)?;
    field(c, "Contact 1 stops motion", None)?;
    ensure!(
        assembly(c)?["contact_sets"][0]["stop_motion"] == false,
        "Stop toggle failed"
    );
    field(c, "Contact 1 stops motion", None)?;
    field(c, "Delete contact 1", None)?;
    ensure!(
        assembly(c)?["contact_sets"].as_array().unwrap().is_empty(),
        "Delete failed"
    );
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == edited, "Undo delete failed");
    field(c, "Check current interference", None)?;
    field(c, "Contact 1 name", None)?;
    capture(c, &fixture.out, "inspect-contact-editor")?;
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "Inspection/contact changes altered source parts"
    );
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    std::fs::write(
        fixture.report,
        serde_json::to_string_pretty(
            &json!({"passed":true,"overlap_volume_mm3":500.,"exact":true,"contacts":edited["contact_sets"]}),
        )?,
    )?;
    println!("PASS native inspector: exact/swept checks, choices, units, invalid fields, contacts, toggles, delete, Undo/Redo, geometry isolation, capture and Save");
    Ok(())
}
