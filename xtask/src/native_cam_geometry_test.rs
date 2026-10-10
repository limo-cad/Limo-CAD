//! Geometry coverage through actual retained CAM controls. CAD fixture geometry
//! uses the shared sketch/extrude commands; no CAM document is seeded by API.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
mod hole_pick;
mod linking;
mod viewport_pick;

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
fn model(c: &mut Client) -> Result<Value> {
    let value = c.call("cad_project_model", json!({}))?;
    serde_json::from_str(value.as_str().context("Project export was not JSON text")?)
        .context("Parse complete project model")
}
fn field(c: &mut Client, label: &str, value: &str) -> Result<()> {
    panel_field(c, label, Some(value), "Previous fields", "More fields")?;
    Ok(())
}
fn section(c: &mut Client, value: &str) -> Result<()> {
    field(c, "Operation section", value)
}
fn operation(cam: &Value) -> Result<&Value> {
    let operations = cam["setups"][0]["operations"]
        .as_array()
        .context("CAM operations missing")?;
    ensure!(
        operations.len() == 1,
        "Expected one isolated test operation"
    );
    Ok(&operations[0])
}
fn history(c: &mut Client, before: &Value, after: &Value) -> Result<()> {
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == before,
        "Geometry Undo changed unrelated project data"
    );
    control(c, "Redo", None)?;
    ensure!(
        &model(c)? == after,
        "Geometry Redo did not restore the complete project"
    );
    Ok(())
}
fn generate(c: &mut Client) -> Result<Value> {
    control(c, "Generate", None)?;
    let statuses = c.call("cam_toolpath_statuses", json!({}))?;
    ensure!(
        statuses
            .as_array()
            .is_some_and(|rows| rows.len() == 1 && rows[0]["state"] == "current"),
        "Shared regeneration did not earn current status: {statuses}"
    );
    let cam = document(c)?;
    let program = c.call("cam_plan_setup", json!({"setup_id":cam["setups"][0]["id"]}))?;
    ensure!(
        program["commands"]
            .as_array()
            .is_some_and(|commands| !commands.is_empty()),
        "Generated operation has no shared motion commands"
    );
    Ok(cam)
}
fn save(c: &mut Client, path: &Path) -> Result<Value> {
    ui(c, json!({"action":"file","command":"save","path":path}))?;
    let expected = model(c)?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let actual: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        actual == expected,
        "Native Save lost geometry or CAM intent: {}",
        path.display()
    );
    Ok(expected)
}
fn create_tool(c: &mut Client, kind: &str, name: &str, diameter: &str) -> Result<u64> {
    control(c, "Project tools", None)?;
    control(c, "New project tool", None)?;
    for (label, value) in [
        ("Name", name),
        ("Cutter type", kind),
        ("Diameter (mm)", diameter),
        ("Flute length (mm)", "25"),
        ("Overall length (mm)", "50"),
        ("Default spindle (rpm)", "6000"),
        ("Default cutting feed (mm/min)", "600"),
        ("Default plunge feed (mm/min)", "150"),
    ] {
        field(c, label, value)?;
    }
    control(c, "Create", None)?;
    let cam = document(c)?;
    let tool = cam["tools"]
        .as_array()
        .context("Tool library missing")?
        .iter()
        .find(|tool| tool["name"] == name)
        .context("Native tool creation missing")?;
    ensure!(
        tool["kind"] == kind,
        "Native creation chose a different cutter type"
    );
    tool["id"].as_u64().context("Created tool ID missing")
}
fn new_operation(c: &mut Client, kind: &str, setup: &str, tool: u64) -> Result<()> {
    control(c, "Toolpaths", None)?;
    control(c, "New toolpath", None)?;
    field(c, "Toolpath type", kind)?;
    field(c, "Setup · click to cycle", setup)?;
    field(c, "Tool · click to cycle", &tool.to_string())?;
    field(c, "Name", &format!("Native {kind} geometry"))
}
fn edge_loop(c: &mut Client, key: &str) -> Result<()> {
    field(c, "Geometry source", "model")?;
    field(c, "Edge selection", "closed")?;
    field(c, "Edge 1", key)?;
    let inspected = ui(c, json!({"action":"inspect"}))?;
    let picker = controls(&inspected)
        .find(|row| row["label"] == "Edge 1")
        .context("Actual edge chooser missing")?;
    ensure!(
        picker["role"] == "combobox"
            && picker["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["value"] == key
                    && option["disabled"] == false
                    && option["label"]
                        .as_str()
                        .is_some_and(|label| !label.is_empty()))),
        "A real named model edge was not offered by the native chooser"
    );
    Ok(())
}
fn picked_hole(c: &mut Client, face: &str) -> Result<()> {
    section(c, "geometry")?;
    field(c, "Hole count", "1")?;
    field(c, "Hole source", "face")?;
    field(c, "Cylindrical face", face)
}
fn shallow_bottom(c: &mut Client) -> Result<()> {
    section(c, "heights")?;
    field(c, "Bottom / target reference", "model_top")?;
    field(c, "Bottom / target offset (mm)", "-2")
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-cam-geometry")?;
    let c = &mut fixture.client;
    ensure!(
        document(c)?["setups"].as_array().is_some_and(Vec::is_empty),
        "Use a blank CAM document"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":8.}}))?;
    let uncut = scene(c)?;
    let body_id = uncut["bodies"][0]["id"].clone();
    ensure!(body_id.is_u64(), "Box fixture body missing");
    begin_sketch(c, "XY")?;
    c.call("sketch_add_circle", json!({"mode":"center_diameter","p1":{"x":20.,"y":15.},"p2":{"x":26.,"y":15.},"ctrl_held":true}))?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude", json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"cut","extent":{"type":"distance","distance":8.},"target_body_ids":[body_id]}))?;
    let solid = scene(c)?;
    ensure!(
        solid["errors"].as_array().is_some_and(Vec::is_empty),
        "Fixture cut failed"
    );
    let body = &solid["bodies"][0];
    let edges = body["edges"]
        .as_array()
        .context("Real model edges missing")?;
    let top_edge = |closed: bool| -> Result<String> {
        let edge = edges
            .iter()
            .find(|edge| {
                edge["circle"]["closed"].as_bool().unwrap_or(false) == closed
                    && edge["points"].as_array().is_some_and(|points| {
                        points.len() >= 2
                            && points
                                .iter()
                                .all(|p| p["z"].as_f64().is_some_and(|z| (z - 8.).abs() < 1e-5))
                    })
            })
            .context("Expected top outer edge and top circular hole edge")?;
        Ok(format!(
            "edge:{}:{}",
            body["id"],
            edge["key"].as_str().context("Edge key missing")?
        ))
    };
    let outer_edge = top_edge(false)?;
    let circle_edge = top_edge(true)?;
    let cylinder = body["faces"]
        .as_array()
        .context("Model faces missing")?
        .iter()
        .find(|face| face["cylinder"].is_object())
        .context("Cut did not produce a real cylindrical face")?;
    let face_key = format!("{}:{}", body["id"], cylinder["id"]);
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    capture(c, &fixture.out, "geometry-real-cut-hole")?;
    control(c, "Switch workspace", None)?;
    control(c, "Manufacture", None)?;
    control(c, "New setup", None)?;
    field(c, "Model body · click to cycle", &body_id.to_string())?;
    field(c, "Name", "Geometry fixture setup")?;
    control(c, "Create", None)?;
    let setup = document(c)?["setups"][0]["id"].to_string();
    control(c, "Project tools", None)?;
    crate::native_cam_test::verify_tool_library_isolation(c, &fixture.out)?;
    let flat = create_tool(c, "flat_end_mill", "Geometry 4 mm flat", "4")?;
    let chamfer = create_tool(c, "chamfer_mill", "Geometry 6 mm chamfer", "6")?;
    let drill = create_tool(c, "drill", "Geometry 4 mm drill", "4")?;
    let thread = create_tool(c, "thread_mill", "Geometry 3 mm thread", "3")?;
    let mut cases = Vec::new();
    let mut adaptive_rejection = Value::Null;
    for (kind, tool) in [
        ("contour2d", flat),
        ("pocket2d", flat),
        ("chamfer2d", chamfer),
        ("drill", drill),
        ("thread", thread),
        ("adaptive3d", flat),
    ] {
        new_operation(c, kind, &setup, tool)
            .with_context(|| format!("Open native {kind} creation"))?;
        match kind {
            "contour2d" | "pocket2d" => {
                shallow_bottom(c)?;
                section(c, "geometry")?;
                edge_loop(
                    c,
                    if kind == "contour2d" {
                        &outer_edge
                    } else {
                        &circle_edge
                    },
                )?;
            }
            "chamfer2d" => {
                section(c, "heights")?;
                field(c, "Height programming", "absolute")?;
                section(c, "geometry")?;
                edge_loop(c, &outer_edge)?;
                field(c, "Chain material wall side", "inside")?;
                field(c, "Chamfer chain count", "2")?;
                edge_loop(c, &circle_edge)?;
                field(c, "Chain material wall side", "outside")?;
                field(c, "Chain top Z (mm)", "-1.25")?;
                field(c, "Chain chamfer width (mm)", "0.3")?;
            }
            "drill" => picked_hole(c, &face_key)?,
            "thread" => {
                for (label, value) in [
                    ("Thread pitch (mm)", "1"),
                    ("Major diameter (mm)", "12"),
                    ("Minor diameter (mm)", "10"),
                ] {
                    field(c, label, value)?;
                }
                picked_hole(c, &face_key)?;
            }
            "adaptive3d" => {
                shallow_bottom(c)?;
                section(c, "parameters")?;
                field(c, "Maximum step down (mm)", "2")?;
                field(c, "Stock-envelope tolerance (mm)", "0.5")?;
            }
            _ => unreachable!(),
        }
        capture(c, &fixture.out, &format!("geometry-{kind}-create"))?;
        control(c, "Create", None).with_context(|| format!("Create native {kind}"))?;
        let created = generate(c).with_context(|| format!("Generate created {kind}"))?;
        let created_operation = operation(&created)?.clone();
        ensure!(
            created_operation["kind"] == kind && created_operation["tool_id"] == tool,
            "Creation changed explicit operation/tool selection"
        );
        match kind {
            "contour2d" | "pocket2d" => ensure!(
                created_operation["chain_ref"]["source"] == "model"
                    && created_operation["chain_ref"]["keys"]
                        .as_array()
                        .is_some_and(|keys| !keys.is_empty()),
                "Model association lost"
            ),
            "chamfer2d" => ensure!(
                created_operation["additional_chains"][0]["top_z"] == -1.25
                    && created_operation["additional_chains"][0]["chamfer_width"] == 0.3,
                "Independent chamfer defaults were overwritten"
            ),
            "drill" | "thread" => ensure!(
                created_operation["holes"][0]["face_key"] == face_key
                    && created_operation["holes"][0]["top_z"] == -1.
                    && created_operation["holes"][0]["bottom_z"] == -9.,
                "Hole association did not resolve the real cylinder span"
            ),
            "adaptive3d" => ensure!(
                created_operation["geometry"]["targets"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty()),
                "Adaptive regeneration did not snapshot the real model"
            ),
            _ => unreachable!(),
        }
        let viewport_pick_evidence =
            if std::env::var("LIMO_CAD_NATIVE_CAM_PICK_INPUT").as_deref() == Ok("1") {
                match kind {
                    "contour2d" | "pocket2d" => Some(viewport_pick::exercise(
                        c,
                        &fixture.out,
                        &fixture.server,
                        kind,
                    )?),
                    "drill" | "thread" => {
                        Some(hole_pick::exercise(c, &fixture.out, &fixture.server, kind)?)
                    }
                    _ => None,
                }
            } else {
                None
            };
        let linking_evidence = if kind == "contour2d" {
            Some(linking::check(c, &fixture.out, &created, &fixture.server)?)
        } else {
            None
        };
        let before_edit = model(c)?;
        match kind {
            "contour2d" => {
                section(c, "geometry")?;
                field(c, "Geometry source", "manual")?;
                field(c, "Path point number", "1")?;
                let x = created_operation["path"][0]["x"]
                    .as_f64()
                    .context("Contour point missing")?
                    + 0.25;
                field(c, "Path point 1 X (mm)", &x.to_string())?;
                field(c, "Path point number", "2")?;
                field(c, "Path point number", "1")?;
            }
            "pocket2d" => {
                section(c, "geometry")?;
                field(c, "Reverse chain", "true")?;
            }
            "chamfer2d" => {
                section(c, "geometry")?;
                field(c, "Chamfer chain number", "1")?;
                field(c, "Chain chamfer width (mm)", "0.4")?;
                field(c, "Chamfer chain number", "2")?;
                field(c, "Chamfer chain number", "1")?;
            }
            "drill" => {
                section(c, "parameters")?;
                field(c, "Holemaking cycle", "chip_breaking")?;
                field(c, "Peck depth (optional) (mm)", "2")?;
            }
            "thread" => {
                section(c, "parameters")?;
                field(c, "Thread pitch (mm)", "1.25")?;
            }
            "adaptive3d" => {
                section(c, "parameters")?;
                field(c, "Optimal load (mm)", "0.6")?;
            }
            _ => unreachable!(),
        }
        capture(c, &fixture.out, &format!("geometry-{kind}-edit"))?;
        control(c, "Apply", None).with_context(|| format!("Apply native {kind} edit"))?;
        let edited = document(c)?;
        let edited_operation = operation(&edited)?;
        match kind {
            "contour2d" => {
                let mut expected = created_operation["path"].clone();
                expected[0]["x"] = json!(expected[0]["x"].as_f64().unwrap() + 0.25);
                ensure!(
                    edited_operation["path"] == expected && edited_operation["chain_ref"].is_null(),
                    "Lazy point edit changed another row or kept a stale model association"
                );
            }
            "pocket2d" => ensure!(
                edited_operation["chain_ref"]["reversed"] == true
                    && edited_operation["chain_ref"]["keys"]
                        == created_operation["chain_ref"]["keys"],
                "Pocket reversal lost canonical edge keys"
            ),
            "chamfer2d" => ensure!(
                edited_operation["chamfer_width"] == 0.4
                    && edited_operation["additional_chains"]
                        == created_operation["additional_chains"],
                "First-chain edit rewrote another chamfer chain"
            ),
            "drill" | "thread" => ensure!(
                edited_operation["holes"] == created_operation["holes"],
                "Parameter edit changed a face association or its hole span"
            ),
            "adaptive3d" => ensure!(
                edited_operation["parameters"]["optimal_load"] == 0.6
                    && edited_operation["geometry"] == created_operation["geometry"],
                "Adaptive parameter edit lost its model snapshot"
            ),
            _ => unreachable!(),
        }
        ensure!(
            edited["tools"] == created["tools"]
                && edited["toolpath_generations"] == created["toolpath_generations"],
            "Editing geometry changed the tool library or fabricated generation stamps"
        );
        let after_edit = model(c)?;
        history(c, &before_edit, &after_edit)?;
        if kind == "adaptive3d" {
            let error = control(c, "Generate", None)
                .err()
                .context("The undersized adaptive load must retain the shared memory guard")?
                .to_string();
            ensure!(
                error.contains("patch frontier exceeds its memory budget"),
                "Expected the shared adaptive memory guard, got: {error}"
            );
            ensure!(
                model(c)? == after_edit,
                "Rejected adaptive generation changed the project or generation evidence"
            );
            capture(c, &fixture.out, "geometry-adaptive3d-budget-rejected")?;
            section(c, "parameters")?;
            field(c, "Optimal load (mm)", "1")?;
            control(c, "Apply", None)?;
            let mut expected = edited.clone();
            expected["setups"][0]["operations"][0]["parameters"]["optimal_load"] = json!(1.);
            ensure!(
                document(c)? == expected,
                "Adaptive retry must change only the explicitly entered optimal load"
            );
            let retry = model(c)?;
            history(c, &after_edit, &retry)?;
            adaptive_rejection = json!({
                "error":"High Speed Roughing patch frontier exceeds its memory budget",
                "rejected_optimal_load":0.6,"retry_optimal_load":1.,
                "rejection_preserved_complete_project":true,"retry":expected
            });
        }
        let generated = generate(c).with_context(|| format!("Regenerate edited {kind}"))?;
        capture(c, &fixture.out, &format!("geometry-{kind}-generated"))?;
        let saved = save(c, &fixture.out.join(format!("geometry-{kind}.limo")))?;
        cases.push(json!({"kind":kind,"created":created,"edited":edited,"regenerated":generated,"saved_model":saved,"linking_edit":linking_evidence,"os_viewport_pick":viewport_pick_evidence}));
        let before_delete = model(c)?;
        control(c, "Delete", None)?;
        let after_delete = model(c)?;
        ensure!(
            document(c)?["setups"][0]["operations"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "Delete retained {kind}"
        );
        history(c, &before_delete, &after_delete)?;
        println!("PASS native {kind}: explicit geometry, edit, shared regeneration, exact history and saved archive");
    }
    control(c, "Undo", None)?;
    ensure!(
        operation(&document(c)?)?["kind"] == "adaptive3d",
        "Final history restoration lost the saved case"
    );
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    capture(c, &fixture.out, "native-cam-geometry")?;
    let final_model = save(c, &fixture.project)?;
    ensure!(scene(c)? == solid, "CAM actions changed the CAD solid");
    let mut captures = vec![
        "geometry-real-cut-hole".to_owned(),
        "geometry-linking-points".to_owned(),
        "geometry-adaptive3d-budget-rejected".to_owned(),
        "native-cam-geometry".to_owned(),
    ];
    for kind in [
        "contour2d",
        "pocket2d",
        "chamfer2d",
        "drill",
        "thread",
        "adaptive3d",
    ] {
        for stage in ["create", "edit", "generated"] {
            captures.push(format!("geometry-{kind}-{stage}"));
        }
    }
    for name in &captures {
        let png = std::fs::read(fixture.out.join(format!("{name}.png")))?;
        ensure!(
            png.starts_with(b"\x89PNG\r\n\x1a\n") && png.len() > 1024,
            "Missing capture {name}"
        );
    }
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required","session":fixture.session,"server":fixture.server,
            "provenance":{"cad_fixture":"shared sketch rectangle/circle and real extrusion/cut commands",
                "cam":"native setup/tool/operation fields and Create/Apply/Generate/Delete/Undo/Redo controls; no CAM seeding",
                "model_edges":[outer_edge,circle_edge],"cylindrical_face":face_key,
                "scope":"generation and persistence checks; no post or geometric machining-verification claim"},
            "captures":captures,"cases":cases,"adaptive_budget_rejection":adaptive_rejection,"final_model":final_model
        }))?,
    )?;
    println!(
        "PASS native CAM geometry fixture; pixel review required: {}",
        fixture.report.display()
    );
    Ok(())
}
