//! Exercise Move/Copy through the same controls and canvas as a person.
use crate::native_fixture::{
    begin_sketch, capture, control, controls, edit_feature, field, start, ui,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::fs;

fn definitions(client: &mut crate::replay::Client) -> Result<Value> {
    client.call("solid_body_feature_definitions", json!({}))
}

pub(super) fn project(camera: &Value, canvas: &Value, p: [f64; 3]) -> [f64; 2] {
    let read = |key: &str| std::array::from_fn::<_, 3, _>(|i| camera[key][i].as_f64().unwrap());
    let sub = |a: [f64; 3], b: [f64; 3]| std::array::from_fn::<_, 3, _>(|i| a[i] - b[i]);
    let dot = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| a[i] * b[i]).sum::<f64>();
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let norm = |a: [f64; 3]| {
        let n = dot(a, a).sqrt();
        a.map(|v| v / n)
    };
    let eye = read("position");
    let forward = norm(sub(read("target"), eye));
    let right = norm(cross(forward, read("up")));
    let up = cross(right, forward);
    let delta = sub(p, eye);
    let h = canvas["height"].as_f64().unwrap();
    let scale = h
        / (2.
            * dot(delta, forward)
            * (camera["verticalFovDegrees"].as_f64().unwrap().to_radians() * 0.5).tan());
    [
        canvas["x"].as_f64().unwrap()
            + canvas["width"].as_f64().unwrap() * 0.5
            + dot(delta, right) * scale,
        canvas["y"].as_f64().unwrap() + h * 0.5 - dot(delta, up) * scale,
    ]
}

fn exercise_handles(client: &mut crate::replay::Client, out: &std::path::Path) -> Result<()> {
    let view = ui(
        client,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    let camera = &view["value"]["camera"];
    let state = ui(client, json!({"action":"inspect"}))?;
    let canvas = state["ui"]["canvases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "viewport")
        .unwrap();
    let pivot = [10., 5., 5.];
    let eye: [f64; 3] = std::array::from_fn(|i| camera["position"][i].as_f64().unwrap());
    let depth = (0..3)
        .map(|i| (eye[i] - pivot[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    let length =
        (2. * depth * (camera["verticalFovDegrees"].as_f64().unwrap().to_radians() * 0.5).tan()
            / canvas["height"].as_f64().unwrap()
            * 96.)
            .max(6.);
    let before = client.call("cad_document", json!({}))?;
    for axis in 0..3 {
        let mut start = pivot;
        start[axis] += length * 0.8;
        let mut end = start;
        end[axis] += 2.;
        ui(
            client,
            json!({"action":"viewport","gesture":"drag","world":start,"to":project(camera,canvas,end)}),
        )?;
        let state = ui(client, json!({"action":"inspect"}))?;
        let label = format!("Translation {}", ["X", "Y", "Z"][axis]);
        let value = controls(&state)
            .find(|c| c["label"] == label)
            .context("Translation field missing")?["value"]
            .as_str()
            .unwrap()
            .parse::<f64>()?;
        ensure!(
            (value - 2.).abs() < 0.15,
            "Translation handle {axis} did not follow its rendered axis: {value}"
        );
        field(client, &label, Some("0"))?;
    }
    for axis in 0..3 {
        let mut radial = [std::f64::consts::FRAC_1_SQRT_2; 3];
        radial[axis] = 0.;
        let bead = std::array::from_fn::<_, 3, _>(|i| pivot[i] + radial[i] * length * 0.62);
        let tangent = match axis {
            0 => [0., -radial[2], radial[1]],
            1 => [radial[2], 0., -radial[0]],
            _ => [-radial[1], radial[0], 0.],
        };
        let screen = project(camera, canvas, bead);
        let projected = project(
            camera,
            canvas,
            std::array::from_fn(|i| bead[i] + tangent[i]),
        );
        let distance = (projected[0] - screen[0]).hypot(projected[1] - screen[1]);
        let pixels = distance * length * 0.62 * std::f64::consts::PI / 180.;
        let end = std::array::from_fn::<_, 2, _>(|i| {
            screen[i] + (projected[i] - screen[i]) / distance * pixels * 20.
        });
        ui(
            client,
            json!({"action":"viewport","gesture":"drag","world":bead,"to":end}),
        )?;
        let state = ui(client, json!({"action":"inspect"}))?;
        let label = format!("Rotation {}", ["X", "Y", "Z"][axis]);
        let value = controls(&state)
            .find(|c| c["label"] == label)
            .context("Rotation field missing")?["value"]
            .as_str()
            .unwrap()
            .parse::<f64>()?;
        ensure!(
            (value - 20.).abs() < 0.2,
            "Rotation handle {axis} did not follow its rendered tangent: {value}"
        );
        for key in ["Rotation X", "Rotation Y", "Rotation Z"] {
            field(client, key, Some("0"))?;
        }
    }
    ensure!(
        client.call("cad_document", json!({}))? == before,
        "A gizmo drag committed geometry"
    );
    capture(client, out, "move-six-handles")?;
    Ok(())
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-move")?;
    let mut cases = vec![];
    for (index, (mode, copy)) in [
        ("free", false),
        ("free", true),
        ("translate", false),
        ("rotate", false),
        ("point_to_point", false),
    ]
    .into_iter()
    .enumerate()
    {
        let tag = format!("{mode}-{}", if copy { "copy" } else { "move" });
        let client = &mut fixture.client;
        if index > 0 {
            let next = control(client, "New design", None)?;
            client.call(
                "cad_attach",
                json!({"session_id":next["active_session_id"]}),
            )?;
        }
        begin_sketch(client, "XY")?;
        client.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":10.},"ctrl_held":true}))?;
        control(client, "Finish sketch", None)?;
        client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
        ui(
            client,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
        )?;
        control(client, "Move/Copy", None)?;
        capture(client, &fixture.out, &format!("move-{tag}-empty"))?;
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[6.,4.,10.]}),
        )?;
        let original = client.call("cad_document", json!({}))?;
        if index == 0 {
            exercise_handles(client, &fixture.out)?;
        }
        field(client, "Move type", None)?;
        let label = match mode {
            "free" => "Free move — XYZ + XYZ rotation",
            "translate" => "Translate — direction and distance",
            "rotate" => "Rotate — axis and angle",
            _ => "Point to point",
        };
        field(client, label, None)?;
        if copy {
            field(client, "Create copy", None)?;
        }
        match mode {
            "free" => {
                field(client, "Translation X", Some("NaN"))?;
                let s = ui(client, json!({"action":"inspect"}))?;
                ensure!(
                    controls(&s).any(|c| c["label"] == "Apply Move/Copy" && c["disabled"] == true),
                    "Nonfinite move accepted"
                );
                field(client, "Translation X", Some("3 cm"))?;
                field(client, "Translation Y", Some("2"))?;
                field(client, "Translation Z", Some("3"))?;
                field(client, "Rotation Z", Some("90 deg"))?;
                for key in ["Rotation pivot X", "Rotation pivot Y", "Rotation pivot Z"] {
                    field(client, key, Some("0"))?;
                }
            }
            "translate" => {
                ui(
                    client,
                    json!({"action":"viewport","gesture":"click","world":[10.,0.,10.]}),
                )?;
                field(client, "Direction X", Some("0"))?;
                field(client, "Direction Y", Some("2"))?;
                field(client, "Direction Z", Some("0"))?;
                field(client, "Distance", Some("30 mm"))?;
            }
            "rotate" => {
                field(client, "Angle (deg)", Some("90 deg"))?;
                for key in ["Rotation pivot X", "Rotation pivot Y", "Rotation pivot Z"] {
                    field(client, key, Some("0"))?;
                }
            }
            _ => {
                ui(
                    client,
                    json!({"action":"viewport","gesture":"move","world":[0.,0.,10.]}),
                )?;
                ui(
                    client,
                    json!({"action":"viewport","gesture":"click","world":[0.,0.,10.]}),
                )?;
                ui(
                    client,
                    json!({"action":"viewport","gesture":"click","world":[0.,10.,10.]}),
                )?;
                for (key, value) in [
                    ("From point X", "10"),
                    ("From point Y", "6"),
                    ("From point Z", "5"),
                    ("To point X", "50"),
                    ("To point Y", "20"),
                    ("To point Z", "5"),
                ] {
                    field(client, key, Some(value))?;
                }
            }
        }
        ensure!(
            client.call("cad_document", json!({}))? == original,
            "Preview changed the document"
        );
        ui(
            client,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":300}),
        )?;
        capture(client, &fixture.out, &format!("move-{tag}-form"))?;
        control(client, "Apply Move/Copy", None)?;
        let def = definitions(client)?;
        let movement = def
            .as_array()
            .context("Definitions missing")?
            .iter()
            .find(|d| d["type"] == "move_copy")
            .context("Move feature missing")?;
        ensure!(movement["copy"] == copy, "Copy choice lost");
        let scene = client.call("solid_scene", json!({}))?;
        ensure!(
            scene["errors"].as_array().is_some_and(Vec::is_empty),
            "Move geometry failed"
        );
        ensure!(
            scene["bodies"].as_array().context("Bodies missing")?.len() == if copy { 2 } else { 1 },
            "Wrong body count"
        );
        let name = movement["name"].as_str().context("Name missing")?;
        edit_feature(client, name, true)?;
        field(client, "Translation X", Some("55"))?;
        control(client, "Close Move/Copy", None)?;
        ensure!(definitions(client)? == def, "Cancel changed the move");
        edit_feature(client, name, false)?;
        field(client, "Translation X", Some("55"))?;
        capture(client, &fixture.out, &format!("move-{tag}-edit"))?;
        control(client, "Apply Move/Copy", None)?;
        let edited = definitions(client)?;
        ensure!(edited != def, "Edit was not applied");
        control(client, "Undo", None)?;
        ensure!(definitions(client)? == def, "Undo lost placement");
        control(client, "Redo", None)?;
        ensure!(definitions(client)? == edited, "Redo lost placement");
        ui(
            client,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":300}),
        )?;
        capture(client, &fixture.out, &format!("move-{tag}"))?;
        ui(
            client,
            json!({"action":"file","command":"save","path":fixture.out.join(format!("move-{tag}.limo"))}),
        )?;
        cases.push(json!({"mode":mode,"copy":copy,"passed":true}));
        println!("PASS native Move/Copy {tag}: real selectors, validation, preview, edit, Cancel, Undo/Redo and Save");
    }
    for copy in [false, true] {
        let client = &mut fixture.client;
        let tag = if copy {
            "component-copy"
        } else {
            "component-move"
        };
        let next = control(client, "New design", None)?;
        client.call(
            "cad_attach",
            json!({"session_id":next["active_session_id"]}),
        )?;
        begin_sketch(client, "XY")?;
        client.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":10.},"ctrl_held":true}))?;
        control(client, "Finish sketch", None)?;
        client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
        let a = client.call("assembly_document", json!({}))?;
        let child = a["component_structure"]["occurrences"][0]["id"].clone();
        client.call(
            "assembly_create_component",
            json!({"name":"Carrier","body_ids":[]}),
        )?;
        let a = client.call("assembly_document", json!({}))?;
        let component = a["component_structure"]["definitions"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["id"]
            .clone();
        let r = std::f64::consts::FRAC_1_SQRT_2;
        client.call("assembly_create_occurrence",json!({"component_id":component,"name":"Rotated parent","local_pose":{"translation":[100.,0.,0.],"rotation":[0.,0.,r,r]}}))?;
        let a = client.call("assembly_document", json!({}))?;
        let parent = a["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["id"]
            .clone();
        client.call("assembly_update_occurrence",json!({"occurrence":{"id":child,"parent_occurrence_id":parent,"local_pose":{"translation":[10.,0.,0.],"rotation":[0.,0.,0.,1.]}}}))?;
        let before = client.call("assembly_document", json!({}))?;
        let original_scene = client.call("solid_scene", json!({}))?;
        let source_history = definitions(client)?;
        for cancel in [true, false] {
            ui(
                client,
                json!({"action":"view","view":"isometric","fit":true,"duration_ms":300}),
            )?;
            control(client, "Move/Copy", None)?;
            field(client, "Component", None)?;
            ui(
                client,
                json!({"action":"viewport","gesture":"move","world":[96.,16.,10.]}),
            )?;
            ui(
                client,
                json!({"action":"viewport","gesture":"click","world":[96.,16.,10.]}),
            )?;
            for (key, value) in [
                ("Translation X", "30"),
                ("Translation Y", "2"),
                ("Translation Z", "3"),
                ("Rotation Z", "90"),
                ("Rotation pivot X", "0"),
                ("Rotation pivot Y", "0"),
                ("Rotation pivot Z", "0"),
            ] {
                field(client, key, Some(value))?;
            }
            if copy {
                field(client, "Create copy", None)?;
            }
            ensure!(
                client.call("assembly_document", json!({}))? == before,
                "A component preview mutated assembly intent"
            );
            capture(
                client,
                &fixture.out,
                &format!(
                    "move-{tag}-{}-form",
                    if cancel { "cancel" } else { "apply" }
                ),
            )?;
            control(
                client,
                if cancel {
                    "Cancel Move/Copy"
                } else {
                    "Apply Move/Copy"
                },
                None,
            )?;
            if cancel {
                ensure!(
                    client.call("assembly_document", json!({}))? == before,
                    "Cancel changed component placement"
                );
            }
        }
        let after = client.call("assembly_document", json!({}))?;
        ensure!(after != before, "Component transform was not saved");
        ensure!(
            definitions(client)? == source_history,
            "Component move changed source feature history"
        );
        ensure!(
            client.call("solid_scene", json!({}))? == original_scene,
            "Component move changed source geometry"
        );
        let solved = client.call("assembly_solution", json!({}))?;
        ensure!(solved["solved"] == true, "Moved assembly is not solved");
        let instances = solved["instance_body_poses"]
            .as_array()
            .context("Instance placements missing")?;
        ensure!(
            instances.len() == if copy { 2 } else { 1 },
            "Wrong component copy count"
        );
        let moved = instances
            .iter()
            .find(|p| {
                if copy {
                    p["occurrence_id"] != child
                } else {
                    p["occurrence_id"] == child
                }
            })
            .context("Moved instance missing")?;
        for (i, expected) in [20., 102., 3.].into_iter().enumerate() {
            ensure!(
                (moved["translation"][i]
                    .as_f64()
                    .context("Placement missing")?
                    - expected)
                    .abs()
                    < 1e-8,
                "Nested placement is incorrect: {moved}"
            );
        }
        control(client, "Undo", None)?;
        ensure!(
            client.call("assembly_document", json!({}))? == before,
            "Undo lost assembly placement"
        );
        control(client, "Redo", None)?;
        ensure!(
            client.call("assembly_document", json!({}))? == after,
            "Redo lost assembly placement"
        );
        ui(
            client,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":300}),
        )?;
        capture(client, &fixture.out, &format!("move-{tag}"))?;
        ui(
            client,
            json!({"action":"file","command":"save","path":fixture.out.join(format!("move-{tag}.limo"))}),
        )?;
        cases.push(json!({"mode":"component","copy":copy,"passed":true}));
        println!("PASS native Move/Copy {tag}: actual instance picking, rotated parent, exact preview, Cancel, source identity and Undo/Redo");
    }
    fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({"passed":true,"cases":cases}))?,
    )?;
    println!("PASS native Move/Copy: saved {}", fixture.report.display());
    Ok(())
}
