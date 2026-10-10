//! Owned retained controls qualify bound print-Z metadata and reviewed group copies.
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

fn intent(c: &mut Client) -> Result<Value> {
    c.call("print_intent_get", json!({}))
}
fn save(c: &mut Client) -> Result<()> {
    print(c, "Apply print settings", None)?;
    Ok(())
}
fn open_part(c: &mut Client, id: &Value) -> Result<()> {
    control(c, "Named Views", None)?;
    view(c, "CAD occurrence or body", Some(&format!("body:{id}")))?;
    view(c, "Part print settings", None)?;
    Ok(())
}
fn close_part(c: &mut Client) -> Result<()> {
    print(c, "Close print settings", None)?;
    view(c, "Close named views", None)?;
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
    let mut fixture = start(args, "native-print-height")?;
    owned_config(&fixture.out)?;
    let c = &mut fixture.client;
    let new = control(c, "New design", None)?;
    attach(c, &new)?;
    for i in 0..2 {
        begin_sketch(c, "XY")?;
        let x = i as f64 * 40.;
        c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+30.,"y":10.},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":20.}}))?;
    }
    let scene = c.call("solid_scene", json!({}))?;
    let bodies: Vec<_> = scene["bodies"]
        .as_array()
        .context("Bodies")?
        .iter()
        .map(|b| b["id"].clone())
        .collect();
    ensure!(bodies.len() == 2, "Two source definitions");
    let component = c.call(
        "assembly_create_component",
        json!({"name":"Multipart height test","body_ids":bodies,"absorb_promoted_bodies":true}),
    )?;
    let assembly = c.call("assembly_document", json!({}))?;
    let root = assembly["component_structure"]["occurrences"]
        .as_array()
        .context("Occurrences")?
        .iter()
        .find(|o| o["component_id"] == component["id"])
        .context("Root")?["id"]
        .clone();
    let repeat=c.call("assembly_create_occurrence",json!({"component_id":component["id"],"name":"Intentional repeated group","local_pose":{"translation":[100.,0.,0.],"rotation":[0.,0.,0.,1.]}}))?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    control(c, "Named Views", None)?;
    view(c, "Create named view", None)?;
    view(c, "View name", Some("Height layout"))?;
    view(c, "Capture current camera and visibility", None)?;
    view(c, "Save view", None)?;
    view(c, "Recall saved view", None)?;
    view(c, "Close named views", None)?;
    let views = c.call("named_views", json!({}))?;
    let layout_id = views["views"]
        .as_array()
        .context("Views")?
        .iter()
        .find(|v| v["name"] == "Height layout")
        .context("Saved view")?["id"]
        .as_str()
        .context("Stable view ID")?
        .to_string();
    let before = model(c)?;
    let portable_before = exported(c, &fixture.out.join("before.3mf"), "3mf")?;
    let stl_before = exported(c, &fixture.out.join("before.stl"), "stl")?;
    open_part(c, &bodies[0])?;
    print(c, "Settings scope", Some("height_range"))?;
    print(c, "Height request layout", Some(&layout_id))?;
    print(c, "Create print height range", None)?;
    print(c, "Height request name", Some("Load-bearing band"))?;
    print(c, "Height range minimum Z", Some("3 mm"))?;
    print(c, "Height range maximum Z", Some("12 mm"))?;
    print(c, "Requested walls", Some("6"))?;
    print(c, "Requested infill (%)", Some("30"))?;
    print(c, "Requested infill pattern", Some("gyroid"))?;
    print(c, "Requested outer wall speed (mm/s)", Some("12"))?;
    print(c, "Requested inner wall speed (mm/s)", Some("14"))?;
    print(c, "Requested infill speed (mm/s)", Some("16"))?;
    let blocked = c.call(
        "cad_interface",
        json!({"action":"file","command":"save","path":fixture.out.join("unapplied.limo")}),
    );
    ensure!(
        blocked.is_err() || blocked.is_ok_and(|v| v["status"] == "failed"),
        "Dirty height draft allowed Save"
    );
    ensure!(
        !fixture.out.join("unapplied.limo").exists(),
        "Rejected Save wrote output"
    );
    save(c)?;
    let band = intent(c)?["height_ranges"][0].clone();
    ensure!(
        band["binding"]["occurrences"]
            .as_array()
            .is_some_and(|v| v.len() == 2),
        "Every intentional repeat must be captured"
    );
    print(c, "Review copies to printable group members", None)?;
    print(c, "Reviewed group members", None)?;
    capture(c, &fixture.out, "reviewed-multipart-height-copy")?;
    print(c, "Apply reviewed group copies", None)?;
    let with_copies = intent(c)?;
    ensure!(
        with_copies["height_ranges"]
            .as_array()
            .is_some_and(|v| v.len() == 2),
        "Reviewed group copy omitted sibling definition"
    );
    let copy = with_copies["height_ranges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["body_id"] == bodies[1])
        .context("Sibling copy")?
        .clone();
    ensure!(copy["id"] != band["id"], "Copies require independent UUID");
    control(c, "Undo", None)?;
    ensure!(
        intent(c)?["height_ranges"] == json!([band.clone()]),
        "Atomic Undo did not remove entire group copy"
    );
    control(c, "Redo", None)?;
    ensure!(
        intent(c)?["height_ranges"] == with_copies["height_ranges"],
        "Redo changed group-copy identities"
    );
    print(
        c,
        "Group copy target replacement",
        Some(copy["id"].as_str().context("Explicit replacement UUID")?),
    )?;
    print(c, "Toggle explicit target replacement", None)?;
    print(c, "Review copies to printable group members", None)?;
    print(c, "Apply reviewed group copies", None)?;
    let replaced = intent(c)?;
    ensure!(
        replaced["height_ranges"]
            .as_array()
            .is_some_and(|rs| rs.len() == 2 && rs.iter().all(|r| r["id"] != copy["id"])),
        "Explicit UUID replacement retained stale target or duplicated requests"
    );
    print(c, "Print settings part", Some(&bodies[1].to_string()))?;
    print(c, "Delete height request", None)?;
    print(c, "Print settings part", Some(&bodies[0].to_string()))?;
    close_part(c)?;
    control(c, "Named Views", None)?;
    for occurrence in [root.clone(), repeat["id"].clone()] {
        view(
            c,
            "CAD occurrence or body",
            Some(&format!("occurrence:{occurrence}")),
        )?;
        view(c, "View rotation Y", Some("90 deg"))?;
        view(c, "View offset Z", Some("150 mm"))?;
    }
    view(c, "Save view", None)?;
    view(c, "Recall saved view", None)?;
    view(c, "Close named views", None)?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    let stale = c.call(
        "print_intent_effective",
        json!({"body_ids":[bodies[0]],"target":"bambu_studio"}),
    )?;
    ensure!(
        stale["height_ranges"][0]["binding_current"] == false,
        "Rotation failed to mark old print-Z binding stale"
    );
    open_part(c, &bodies[0])?;
    print(c, "Settings scope", Some("height_range"))?;
    print(c, "Height range maximum Z", Some("25 mm"))?;
    print(c, "Review height rebind", None)?;
    print(c, "Reviewed height change", None)?;
    capture(c, &fixture.out, "reviewed-rotated-height-rebind")?;
    print(c, "Apply reviewed height rebind", None)?;
    let rebound = intent(c)?["height_ranges"][0].clone();
    ensure!(
        rebound["id"] == band["id"]
            && rebound["max_z_mm"] == 25.
            && rebound["settings"] == band["settings"],
        "Rebind lost identity/settings/corrected interval"
    );
    print(c, "Review copies to printable group members", None)?;
    print(c, "Apply reviewed group copies", None)?;
    print(c, "Settings scope", Some("layer_profile"))?;
    print(c, "Height request layout", Some(&layout_id))?;
    print(c, "Create variable layer profile", None)?;
    print(
        c,
        "Height request name",
        Some("Explicit upright-to-horizontal schedule"),
    )?;
    print(c, "Variable layer sample", Some("0"))?;
    print(c, "Sample layer height", Some("0.15 mm"))?;
    print(c, "Add layer sample midpoint", None)?;
    print(c, "Sample layer height", Some("0.12 mm"))?;
    save(c)?;
    print(c, "Review copies to printable group members", None)?;
    print(c, "Apply reviewed group copies", None)?;
    let final_intent = intent(c)?;
    ensure!(
        final_intent["layer_height_profiles"]
            .as_array()
            .is_some_and(|v| v.len() == 2),
        "Variable profile group copies missing"
    );
    print(c, "Height binding status", None)?;
    capture(
        c,
        &fixture.out,
        "qualified-height-range-and-variable-profile",
    )?;
    close_part(c)?;
    let expected = model(c)?;
    let mut physical_before = before.clone();
    let mut physical_after = expected.clone();
    physical_before
        .as_object_mut()
        .unwrap()
        .remove("print_intent");
    physical_after
        .as_object_mut()
        .unwrap()
        .remove("print_intent");
    physical_before.as_object_mut().unwrap().remove("views");
    physical_after.as_object_mut().unwrap().remove("views");
    ensure!(
        physical_before == physical_after,
        "Height edits changed CAD geometry/hierarchy"
    );
    let portable_after = exported(c, &fixture.out.join("after.3mf"), "3mf")?;
    let stl_after = exported(c, &fixture.out.join("after.stl"), "stl")?;
    // Selected layout legitimately changed orientation; compare omission against a fresh copy without intent in the same final pose.
    let mut stripped = expected.clone();
    stripped["print_intent"] = json!(limo_cad_core::PrintIntentDocumentDto::default());
    let mut cmd = Command::new(&fixture.server);
    cmd.arg("--headless");
    let mut reference = Client::start_command(cmd, Some(Duration::from_secs(45)))?;
    reference.call(
        "cad_load_project_model",
        json!({"model_json":stripped.to_string()}),
    )?;
    let source_bytes = reference.call(
        "solid_export_3mf",
        json!({"scope":"assembly","named_view":"Height layout"}),
    )?;
    use base64::Engine;
    let reference_bytes = base64::engine::general_purpose::STANDARD.decode(
        source_bytes["bytes_base64"]
            .as_str()
            .context("Portable bytes")?,
    )?;
    let reference_stl = reference.call(
        "solid_export_stl",
        json!({"scope":"assembly","named_view":"Height layout"}),
    )?;
    let reference_stl = base64::engine::general_purpose::STANDARD.decode(
        reference_stl["bytes_base64"]
            .as_str()
            .context("STL bytes")?,
    )?;
    ensure!(
        stl_after == reference_stl,
        "Height metadata leaked into physical STL triangles"
    );
    let package = |bytes: &[u8]| {
        limo_cad_export::test_reader::read_package_text(bytes, "3D/3dmodel.model")
            .map_err(anyhow::Error::msg)
    };
    ensure!(
        package(&portable_after)? == package(&reference_bytes)?,
        "Height records leaked print-only geometry into portable 3MF"
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
        model(c)? == expected && intent(c)? == final_intent,
        "Save/reopen lost exact height metadata"
    );
    let mut cmd = Command::new(&fixture.server);
    cmd.arg("--headless");
    let mut cold = Client::start_command(cmd, Some(Duration::from_secs(45)))?;
    cold.call("cad_load_project_model", json!({"model_json":archived}))?;
    ensure!(
        model(&mut cold)? == expected && intent(&mut cold)? == final_intent,
        "Cold reload changed height bindings/settings/UUIDs"
    );
    fs::write(
        fixture.out.join("native-print-height.json"),
        serde_json::to_vec_pretty(
            &json!({"status":"passed","band":band,"rebound":rebound,"intent":final_intent,"source_geometry_unchanged":true,"portable_omission_same_final_pose":true,"before_portable_bytes":portable_before.len(),"before_stl_bytes":stl_before.len(),"after_stl_bytes":stl_after.len(),"native_toolpaths":"separate backend qualification","physical_qualification":"not_run"}),
        )?,
    )?;
    println!("PASS native-print-height {}", fixture.out.display());
    Ok(())
}
