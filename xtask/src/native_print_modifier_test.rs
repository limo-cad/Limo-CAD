//! Owned Bevy controls qualify local zones without adding source solids.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, owned_config, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, process::Command, time::Duration};

fn field(c: &mut Client, label: &str, value: Option<&str>, kind: &str) -> Result<Value> {
    let (up, down) = if kind == "view" {
        ("Previous named-view fields", "More named-view fields")
    } else {
        (
            "Previous print settings fields",
            "More print settings fields",
        )
    };
    for pass in 0..2 {
        if pass == 1 {
            for _ in 0..128 {
                let state = ui(c, json!({"action":"inspect"}))?;
                if !controls(&state).any(|v| v["label"] == up && v["disabled"] == false) {
                    break;
                }
                control(c, up, None)?;
            }
        }
        for _ in 0..128 {
            let state = ui(c, json!({"action":"inspect"}))?;
            let found: Vec<_> = controls(&state)
                .filter(|v| {
                    v["disabled"] == false
                        && v["label"]
                            .as_str()
                            .is_some_and(|s| s == label || s.starts_with(&format!("{label} (")))
                })
                .collect();
            ensure!(found.len() <= 1, "Ambiguous retained field {label}");
            if let Some(found) = found.first() {
                return control(c, found["label"].as_str().context("Field label")?, value);
            }
            if !controls(&state).any(|v| v["label"] == down && v["disabled"] == false) {
                break;
            }
            control(c, down, None)?;
        }
    }
    anyhow::bail!("Missing enabled retained field {label}")
}
fn print(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    field(c, label, value, "print")
}
fn view(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    field(c, label, value, "view")
}
fn model(c: &mut Client) -> Result<Value> {
    serde_json::from_str(
        c.call("cad_project_model", json!({}))?
            .as_str()
            .context("Model JSON")?,
    )
    .map_err(Into::into)
}
fn attach(c: &mut Client, value: &Value) -> Result<()> {
    c.call(
        "cad_attach",
        json!({"session_id":value["active_session_id"].as_str().context("Transition session")?}),
    )?;
    Ok(())
}
fn modifiers(c: &mut Client) -> Result<Vec<Value>> {
    Ok(c.call("print_intent_get", json!({}))?["modifiers"]
        .as_array()
        .context("Saved zones")?
        .clone())
}
fn shape(c: &mut Client, label: &str, x: &str, y: &str, z: &str) -> Result<()> {
    print(c, "Local modifier name", Some(label))?;
    for (axis, value) in [("X", x), ("Y", y), ("Z", z)] {
        print(c, &format!("Modifier local offset {axis}"), Some(value))?;
    }
    Ok(())
}
fn exported(c: &mut Client, path: &std::path::Path, format: &str) -> Result<Vec<u8>> {
    ui(
        c,
        json!({"action":"file","command":format!("export_{format}"),"scope":"assembly","allow_layout_issues":true,"path":path}),
    )?;
    fs::read(path).map_err(Into::into)
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-print-modifier")?;
    owned_config(&fixture.out)?;
    let c = &mut fixture.client;
    let new = control(c, "New design", None)?;
    attach(c, &new)?;
    for i in 0..2 {
        begin_sketch(c, "XY")?;
        let x = i as f64 * 100.;
        c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+30.,"y":20.},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":20.}}))?;
    }
    let source = c.call("solid_scene", json!({}))?;
    let ids: Vec<_> = source["bodies"]
        .as_array()
        .context("Bodies")?
        .iter()
        .map(|b| b["id"].clone())
        .collect();
    ensure!(ids.len() == 2, "Use two independent source solids");
    let component = c.call(
        "assembly_create_component",
        json!({"name":"Repeated zoned part","body_ids":[ids[0]],"absorb_promoted_bodies":true}),
    )?;
    let assembly = c.call("assembly_document", json!({}))?;
    let root = assembly["component_structure"]["occurrences"]
        .as_array()
        .context("Occurrences")?
        .iter()
        .find(|o| o["component_id"] == component["id"])
        .context("Root")?["id"]
        .clone();
    c.call("assembly_create_occurrence",json!({"component_id":component["id"],"parent_occurrence_id":root,"name":"Intentional repeated zoned part","local_pose":{"translation":[50.,0.,0.],"rotation":[0.,0.,0.,1.]}}))?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    control(c, "Named Views", None)?;
    view(c, "Create named view", None)?;
    view(c, "View name", Some("Modifier rotated layout"))?;
    view(c, "Capture current camera and visibility", None)?;
    view(
        c,
        "CAD occurrence or body",
        Some(&format!("occurrence:{root}")),
    )?;
    view(c, "View offset X", Some("25 mm"))?;
    view(c, "View rotation Z", Some("90 deg"))?;
    view(c, "Save view", None)?;
    view(c, "Recall saved view", None)?;
    view(
        c,
        "CAD occurrence or body",
        Some(&format!("body:{}", ids[0])),
    )?;
    view(c, "Part print settings", None)?;
    print(c, "Settings scope", Some("project"))?;
    print(c, "Requested walls", Some("2"))?;
    print(c, "Requested infill (%)", Some("15"))?;
    print(c, "Apply print settings", None)?;
    print(c, "Close print settings", None)?;
    view(c, "Close named views", None)?;
    ui(
        c,
        json!({"action":"view","view":"top","fit":true,"duration_ms":0}),
    )?;
    let before = model(c)?;
    let portable_before = exported(c, &fixture.out.join("before.3mf"), "3mf")?;
    let stl_before = exported(c, &fixture.out.join("before.stl"), "stl")?;
    control(c, "Named Views", None)?;
    view(
        c,
        "CAD occurrence or body",
        Some(&format!("body:{}", ids[0])),
    )?;
    view(c, "Part print settings", None)?;
    print(c, "Settings scope", Some("modifier"))?;
    print(c, "Create local print modifier", None)?;
    shape(c, "Drive", "10 mm", "10 mm", "10 mm")?;
    for axis in ["X", "Y", "Z"] {
        print(c, &format!("Modifier box size {axis}"), Some("6 mm"))?;
    }
    print(c, "Modifier local rotation Z", Some("45 deg"))?;
    print(c, "Requested walls", Some("6"))?;
    print(c, "Requested infill (%)", Some("80"))?;
    print(c, "Requested infill pattern", Some("gyroid"))?;
    let rejected = c.call(
        "cad_interface",
        json!({"action":"file","command":"save","path":fixture.out.join("unapplied.limo")}),
    );
    ensure!(
        rejected.is_err() || rejected.is_ok_and(|v| v["status"] == "failed"),
        "Unsaved local draft allowed Save"
    );
    ensure!(
        !fixture.out.join("unapplied.limo").exists(),
        "Rejected Save wrote output"
    );
    print(c, "Apply print settings", None)?;
    let box_zone = modifiers(c)?.pop().context("Saved box zone")?;
    ensure!(
        box_zone["settings"]["wall_count"] == 6 && box_zone["body_id"] == ids[0],
        "Box requests missing"
    );
    let portable = c.call(
        "print_modifier_effective",
        json!({"body_ids":[ids[0]],"target":"portable"}),
    )?;
    ensure!(
        portable["modifiers"][0]["occurrence_ids"]
            .as_array()
            .is_some_and(|v| v.len() == 2),
        "Zone must inherit both intentional repeats"
    );
    ensure!(
        portable["modifiers"][0]["warnings"]
            .as_array()
            .is_some_and(|v| !v.is_empty()),
        "Portable omission must be reported"
    );
    print(c, "Local modifier inherited occurrences", None)?;
    capture(c, &fixture.out, "repeated-rotated-box-overlays")?;
    print(c, "Local modifier enabled", Some("false"))?;
    print(c, "Apply print settings", None)?;
    ensure!(
        modifiers(c)?[0]["enabled"] == false,
        "Disable not persisted"
    );
    print(c, "Local modifier enabled", Some("true"))?;
    print(c, "Apply print settings", None)?;
    print(c, "Reset print settings to inheritance", None)?;
    let reset = modifiers(c)?[0].clone();
    ensure!(
        reset["id"] == box_zone["id"]
            && reset["primitive"] == box_zone["primitive"]
            && reset["settings"]["wall_count"].is_null(),
        "Reset must retain zone identity/shape and inherit"
    );
    control(c, "Undo", None)?;
    ensure!(
        modifiers(c)?[0] == box_zone,
        "Undo did not restore exact zone requests"
    );
    control(c, "Redo", None)?;
    ensure!(
        modifiers(c)?[0] == reset,
        "Redo did not restore inherited zone"
    );
    print(c, "Copy modifier to part", Some(&ids[1].to_string()))?;
    print(c, "Copy local print modifier", None)?;
    let copy = modifiers(c)?
        .into_iter()
        .find(|m| m["body_id"] == ids[1])
        .context("Explicit copied zone")?;
    ensure!(
        copy["id"] != box_zone["id"] && copy["primitive"] == box_zone["primitive"],
        "Copy must allocate independent zone identity"
    );
    print(c, "Delete local print modifier", None)?;
    ensure!(modifiers(c)?.len() == 1, "Delete copied zone failed");
    print(c, "Print settings part", Some(&ids[0].to_string()))?;
    print(c, "Create local print modifier", None)?;
    print(c, "Local modifier shape", Some("cylinder"))?;
    shape(c, "Sleeve", "22 mm", "10 mm", "10 mm")?;
    print(c, "Modifier cylinder radius", Some("2 mm"))?;
    print(c, "Modifier cylinder height", Some("8 mm"))?;
    print(c, "Modifier local rotation Y", Some("30 deg"))?;
    print(c, "Requested walls", Some("0"))?;
    print(c, "Apply print settings", None)?;
    let zones = modifiers(c)?;
    ensure!(
        zones.len() == 2
            && zones
                .iter()
                .any(|m| m["primitive"]["kind"] == "cylinder" && m["settings"]["wall_count"] == 0),
        "Cylinder/explicit zero missing"
    );
    print(c, "Local modifier name", None)?;
    capture(c, &fixture.out, "box-cylinder-local-overlays")?;
    print(c, "Show local modifier overlays", Some("false"))?;
    capture(c, &fixture.out, "modifier-overlays-hidden")?;
    print(c, "Show local modifier overlays", Some("true"))?;
    print(c, "Close print settings", None)?;
    view(c, "Close named views", None)?;
    let expected = model(c)?;
    let mut source_before = before.clone();
    source_before
        .as_object_mut()
        .unwrap()
        .remove("print_intent");
    let mut source_after = expected.clone();
    source_after.as_object_mut().unwrap().remove("print_intent");
    ensure!(
        source_before == source_after,
        "Local modifiers changed source geometry, grouping or saved layout"
    );
    ensure!(
        c.call("named_views", json!({}))?["active"] == "Modifier rotated layout",
        "Print edits reset recalled layout"
    );
    let portable_after = exported(c, &fixture.out.join("after.3mf"), "3mf")?;
    let stl_after = exported(c, &fixture.out.join("after.stl"), "stl")?;
    let package = |bytes: &[u8]| {
        limo_cad_export::test_reader::read_package_text(bytes, "3D/3dmodel.model")
            .map_err(anyhow::Error::msg)
    };
    ensure!(
        package(&portable_before)? == package(&portable_after)? && stl_before == stl_after,
        "Print-only zones changed portable 3MF/STL physical meshes"
    );
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let archived = crate::project_archive::model(&fs::read(&fixture.project)?)?;
    let closed = ui(c, json!({"action":"file","command":"close"}))?;
    attach(c, &closed)?;
    let opened = ui(
        c,
        json!({"action":"file","command":"open","path":fixture.project}),
    )?;
    attach(c, &opened)?;
    ensure!(
        model(c)? == expected && modifiers(c)? == zones,
        "Reopen changed exact zones/source/layout"
    );
    let mut command = Command::new(&fixture.server);
    command.arg("--headless");
    let mut cold = Client::start_command(command, Some(Duration::from_secs(45)))?;
    cold.call("cad_load_project_model", json!({"model_json":archived}))?;
    ensure!(
        model(&mut cold)? == expected && modifiers(&mut cold)? == zones,
        "Cold recomputation changed saved modifier intent"
    );
    cold.finish(Duration::from_secs(10))?;
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"body_ids":ids,"zones":zones,"portable_effective":portable,"project":fixture.project,"cold_load":true,"portable_and_stl_meshes_unchanged":true,"not_proven":["native slicer GUI","physical strength","OS file chooser"]}),
        )?,
    )?;
    println!("PASS native modifier controls, local repeated/rotated overlays, copy/reset/disable/delete, metadata Undo/Redo, Save/cold reopen and portable/STL omission: {}",fixture.report.display());
    Ok(())
}
