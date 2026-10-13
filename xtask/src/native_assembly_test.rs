//! Assembly structure and instance placement through actual native controls.
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
fn occurrences(a: &Value) -> Result<&Vec<Value>> {
    a["component_structure"]["occurrences"]
        .as_array()
        .context("Occurrences missing")
}
fn occurrence<'a>(a: &'a Value, name: &str) -> Result<&'a Value> {
    occurrences(a)?
        .iter()
        .find(|o| o["name"] == name)
        .context("Named component missing")
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-assembly")?;
    let c = &mut fixture.client;
    let initial = ui(c, json!({"action":"inspect"}))?;
    if controls(&initial).any(|b| b["label"] == "Back to model browser" && b["disabled"] == false) {
        control(c, "Back to model browser", None)?;
    }
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":10.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    let source = c.call("solid_scene", json!({}))?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    let picked = ui(
        c,
        json!({"action":"viewport","gesture":"click","world":[6.,4.,10.]}),
    )?;
    ensure!(
        picked["value"]["selection"]["bodies"] == json!([1]),
        "Canvas click failed to select stock: {}",
        picked["value"]
    );
    let removed = ui(
        c,
        json!({"action":"viewport","gesture":"click","world":[6.,4.,10.],"shift":true}),
    )?;
    ensure!(
        removed["value"]["selection"]["faces"] == json!([]),
        "Additive selection failed to toggle the face off"
    );
    ui(
        c,
        json!({"action":"viewport","gesture":"click","world":[6.,4.,10.]}),
    )?;
    control(c, "Assembly", None)?;
    capture(c, &fixture.out, "assembly-initial")?;
    field(c, "Make component", None)?;
    let made = assembly(c)?;
    ensure!(
        occurrences(&made)?.len() == 1,
        "Promoted body was not absorbed"
    );
    field(c, "Component Component", None)?;
    field(c, "Instance name", Some("Bracket!"))?;
    for key in ["End", "Backspace"] {
        let state = ui(c, json!({"action":"inspect"}))?;
        let name = controls(&state)
            .find(|b| b["label"] == "Instance name")
            .context("Name editor missing")?;
        ui(c, json!({"action":"key","target":name["id"],"key":key}))?;
    }
    let state = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&state).any(|b| b["label"] == "Instance name" && b["value"] == "Bracket"),
        "Keyboard did not edit the rendered text field"
    );
    field(c, "Rename instance", None)?;
    let renamed = assembly(c)?;
    let bracket = occurrence(&renamed, "Bracket")?["id"].clone();
    field(c, "Reusable definition", Some("Mounting bracket"))?;
    field(c, "Rename definition", None)?;
    field(c, "Create subassembly", None)?;
    let a = assembly(c)?;
    let parent = occurrence(&a, "Subassembly")?["id"].clone();
    field(c, "Component Bracket", None)?;
    field(c, "Parent coordinate system", Some(&parent.to_string()))?;
    ensure!(
        occurrence(&assembly(c)?, "Bracket")?["parent_occurrence_id"] == parent,
        "Parent selection was not committed"
    );
    for (key, expected) in [("Home", Value::Null), ("End", parent.clone())] {
        let state = ui(c, json!({"action":"inspect"}))?;
        let choice = controls(&state)
            .find(|b| b["label"] == "Parent coordinate system")
            .context("Parent choice missing")?;
        ensure!(
            choice["role"] == "combobox",
            "Parent choice has no keyboard semantics"
        );
        ui(c, json!({"action":"key","target":choice["id"],"key":key}))?;
        ensure!(
            occurrence(&assembly(c)?, "Bracket")?["parent_occurrence_id"] == expected,
            "Keyboard parent change failed"
        );
    }
    field(c, "Instance placement X translation", Some("NaN"))?;
    let mut seen = false;
    for _ in 0..12 {
        let state = ui(c, json!({"action":"inspect"}))?;
        if let Some(button) = controls(&state).find(|b| b["label"] == "Apply placement") {
            ensure!(button["disabled"] == true, "Invalid placement accepted");
            seen = true;
            break;
        }
        field(c, "Scroll assembly down", None)?;
    }
    ensure!(seen, "Apply placement was not rendered");
    for (label, value) in [
        ("Instance placement X translation", "2 cm"),
        ("Instance placement Z rotation", "90 deg"),
    ] {
        field(c, label, Some(value))?;
    }
    capture(c, &fixture.out, "assembly-placement-editor")?;
    let before = assembly(c)?;
    field(c, "Apply placement", None)?;
    let positioned = assembly(c)?;
    ensure!(
        occurrence(&positioned, "Bracket")?["local_pose"]["translation"][0] == 20.,
        "Unit-aware placement was lost"
    );
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == before, "Undo did not restore placement");
    control(c, "Redo", None)?;
    ensure!(assembly(c)? == positioned, "Redo did not restore placement");
    field(c, "Component Bracket", None)?;
    field(c, "Component coordinate system X translation", Some("5 mm"))?;
    field(c, "Apply component origin", None)?;
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "Component origin altered the authored body"
    );
    field(c, "Ground Bracket", None)?;
    ensure!(
        occurrence(&assembly(c)?, "Bracket")?["grounded"] == true,
        "Grounding failed"
    );
    field(c, "Release Bracket", None)?;
    field(c, "Hide Bracket", None)?;
    ensure!(
        occurrence(&assembly(c)?, "Bracket")?["visible"] == false,
        "Visibility failed"
    );
    field(c, "Show Bracket", None)?;
    field(c, "Duplicate Bracket", None)?;
    let copied = assembly(c)?;
    ensure!(
        occurrences(&copied)?.len() == 3,
        "Duplicate did not retain the assembly hierarchy"
    );
    let definition = occurrence(&copied, "Bracket")?["component_id"].to_string();
    field(c, "Reusable component definition", Some(&definition))?;
    field(c, "Component Subassembly", None)?;
    field(c, "Add child instance", None)?;
    let child_added = assembly(c)?;
    ensure!(
        occurrences(&child_added)?.last().unwrap()["parent_occurrence_id"] == parent,
        "Added instance lost its parent"
    );
    field(c, "Add root instance", None)?;
    ensure!(
        occurrences(&assembly(c)?)?.last().unwrap()["parent_occurrence_id"].is_null(),
        "Root instance was nested"
    );
    field(c, "Collapse Subassembly", None)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        !controls(&state).any(|b| b["label"] == "Component Bracket"),
        "Collapsed subtree remained interactive"
    );
    field(c, "Expand Subassembly", None)?;
    field(c, "Component Bracket", None)?;
    field(c, "Move Bracket", None)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&state).any(|b| b["label"] == "Component" && b["selected"] == true),
        "Assembly move opened in body mode"
    );
    control(c, "Cancel Move/Copy", None)?;
    field(c, "Component Subassembly", None)?;
    field(c, "Move Subassembly", None)?;
    crate::native_fixture::field(c, "Translation X", Some("30"))?;
    control(c, "Apply Move/Copy", None)?;
    ensure!(
        occurrence(&assembly(c)?, "Subassembly")?["local_pose"]["translation"][0] == 30.,
        "Subassembly move failed"
    );
    field(c, "Component Bracket", None)?;
    field(c, "Parent coordinate system", None)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        !controls(&state).any(|b| b["label"] == "Parent Bracket"),
        "Parent list permits a cycle"
    );
    field(c, "Parent coordinate system", None)?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":300}),
    )?;
    capture(c, &fixture.out, "assembly-final")?;
    field(c, "Back to model browser", None)?;
    ensure!(
        c.call("solid_scene", json!({}))? == source,
        "Assembly work changed source solids"
    );
    control(c, "Assembly", None)?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.out.join("native-assembly.limo")}),
    )?;
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(
            &json!({"passed":true,"source_body":source["bodies"][0]["id"],"bracket_occurrence":bracket,"assembly":assembly(c)?}),
        )?,
    )?;
    println!("PASS native assembly: components, hierarchy, typed placement, origin, grounding, visibility, copy, instance reuse, group Move/Copy, exact Undo/Redo and Save");
    Ok(())
}
