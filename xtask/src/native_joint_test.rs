//! Joint creation/editing through rendered Bevy controls and real connector picks.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, panel_field, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
fn field(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    panel_field(c, label, value, "Scroll joint up", "Scroll joint down")
}
fn browser(c: &mut Client, label: &str) -> Result<Value> {
    panel_field(c, label, None, "Scroll assembly up", "Scroll assembly down")
}
fn assembly(c: &mut Client) -> Result<Value> {
    c.call("assembly_document", json!({}))
}
pub(super) fn open(c: &mut Client, kind: &str) -> Result<()> {
    control(c, "Joint", None)?;
    field(c, "Joint name", Some(&format!("Native {kind}")))?;
    field(c, "Joint type", Some(kind))?;
    field(c, "Flip joint direction", None)?;
    for (body, view, point) in [(1, "top", [10., 5., 10.]), (2, "bottom", [50., 5., 0.])] {
        ui(
            c,
            json!({"action":"view","view":view,"body_id":body,"duration_ms":0}),
        )?;
        ui(
            c,
            json!({"action":"viewport","gesture":"move","world":point}),
        )?;
        ui(
            c,
            json!({"action":"viewport","gesture":"click","world":point}),
        )?;
    }
    Ok(())
}
fn body_center(c: &mut Client) -> Result<[f64; 3]> {
    let value = ui(
        c,
        json!({"action":"view","view":"isometric","body_id":2,"duration_ms":0}),
    )?;
    let target = value["value"]["camera"]["target"]
        .as_array()
        .context("Camera target missing")?;
    Ok(std::array::from_fn(|i| target[i].as_f64().unwrap()))
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-joint")?;
    let c = &mut fixture.client;
    let mut passed = vec![];
    for kind in [
        "rigid",
        "revolute",
        "slider",
        "cylindrical",
        "planar",
        "ball",
        "pin_slot",
        "screw",
        "universal",
    ] {
        if !passed.is_empty() {
            let new = control(c, "New design", None)?;
            let id = new["active_session_id"]
                .as_str()
                .context("Blank session missing")?;
            c.call("cad_attach", json!({"session_id":id}))?;
        }
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state).any(|b| b["label"] == "Back to model browser") {
            control(c, "Back to model browser", None)?;
        }
        for i in 0..2 {
            begin_sketch(c, "XY")?;
            c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":i as f64*40.,"y":0.},"p2":{"x":i as f64*40.+20.,"y":10.},"ctrl_held":true}))?;
            control(c, "Finish sketch", None)?;
            c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
        }
        let before = assembly(c)?;
        let source = c.call("solid_scene", json!({}))?;
        open(c, kind)?;
        ensure!(assembly(c)? == before, "Joint preview mutated the model");
        if kind == "rigid" {
            let target = body_center(c)?;
            ensure!(
                (target[0] - 50.).abs() > 10.,
                "Preview did not move the rendered component: {target:?}"
            );
            capture(c, &fixture.out, "joint-rigid-preview")?;
            control(c, "Cancel joint", None)?;
            let target = body_center(c)?;
            ensure!(
                target
                    .iter()
                    .zip([50., 5., 5.])
                    .all(|(a, b)| (a - b).abs() < 0.001),
                "Cancel did not restore rendered component placement: {target:?}"
            );
            ensure!(assembly(c)? == before, "Cancel changed the assembly");
            open(c, kind)?;
        }
        let mut axes = vec![];
        if matches!(
            kind,
            "revolute" | "cylindrical" | "planar" | "ball" | "pin_slot" | "screw" | "universal"
        ) {
            axes.push(if kind == "screw" {
                "Rotation travel"
            } else {
                "Primary rotation"
            });
        }
        if matches!(kind, "slider" | "cylindrical" | "planar" | "pin_slot") {
            axes.push(if matches!(kind, "planar" | "pin_slot") {
                "X slide"
            } else {
                "Slide"
            });
        }
        if matches!(kind, "ball" | "universal") {
            axes.push("Secondary rotation");
        }
        if kind == "ball" {
            axes.push("Tertiary rotation");
        }
        if kind == "planar" {
            axes.push("Y slide");
        }
        for axis in &axes {
            let angular = axis.contains("rotation") || *axis == "Rotation travel";
            field(
                c,
                &format!("{axis} offset"),
                Some(if angular { "15 deg" } else { "2 mm" }),
            )?;
            field(c, &format!("Limit {axis}"), None)?;
            field(
                c,
                &format!("{axis} Minimum"),
                Some(if angular { "-30 deg" } else { "-5 mm" }),
            )?;
            field(
                c,
                &format!("{axis} Maximum"),
                Some(if angular { "30 deg" } else { "5 mm" }),
            )?;
        }
        if kind == "screw" {
            field(c, "Screw pitch", Some("-1"))?;
            let state = ui(c, json!({"action":"inspect"}))?;
            ensure!(
                controls(&state).any(|b| b["label"] == "Apply joint" && b["disabled"] == true),
                "Nonpositive screw pitch accepted"
            );
            field(c, "Screw pitch", Some("4 mm"))?;
        }
        field(c, "Connector orientation", None)?;
        field(c, "Connector A twist", Some("10 deg"))?;
        field(c, "Connector B twist", Some("5 deg"))?;
        ensure!(
            assembly(c)? == before,
            "Typed joint preview changed saved assembly intent"
        );
        ui(
            c,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
        )?;
        capture(c, &fixture.out, &format!("joint-{kind}-form"))?;
        control(c, "Apply joint", None)?;
        let made = assembly(c)?;
        ensure!(
            made["joints"].as_array().is_some_and(|j| j.len() == 1),
            "Joint not created"
        );
        ensure!(made["joints"][0]["kind"] == kind, "Joint type changed");
        ensure!(
            c.call("assembly_solution", json!({}))?["solved"] == true,
            "Joint failed to solve"
        );
        ensure!(
            c.call("solid_scene", json!({}))? == source,
            "Joint changed source features"
        );
        control(c, "Undo", None)?;
        ensure!(
            assembly(c)? == before,
            "Undo did not remove the joint exactly"
        );
        control(c, "Redo", None)?;
        ensure!(
            assembly(c)? == made,
            "Redo did not restore the joint exactly"
        );
        motion(c, kind, &axes, &made, &fixture.out)?;
        browser(c, &format!("Edit joint Native {kind}"))?;
        field(c, "Joint name", Some("Cancelled name"))?;
        control(c, "Cancel joint", None)?;
        ensure!(assembly(c)? == made, "Cancel altered the edited joint");
        browser(c, &format!("Edit joint Native {kind}"))?;
        field(c, "Joint name", Some(&format!("Edited {kind}")))?;
        control(c, "Apply joint", None)?;
        let edited = assembly(c)?;
        control(c, "Undo", None)?;
        ensure!(assembly(c)? == made, "Undo edit lost joint coordinates");
        control(c, "Redo", None)?;
        ensure!(assembly(c)? == edited, "Redo edit lost joint coordinates");
        browser(c, &format!("Suppress joint Edited {kind}"))?;
        ensure!(
            assembly(c)?["joints"][0]["enabled"] == false,
            "Suppress did not disable joint"
        );
        browser(c, &format!("Unsuppress joint Edited {kind}"))?;
        browser(c, &format!("Delete joint Edited {kind}"))?;
        ensure!(
            assembly(c)?["joints"].as_array().unwrap().is_empty(),
            "Delete did not remove joint"
        );
        control(c, "Undo", None)?;
        ensure!(
            assembly(c)? == edited,
            "Undo delete did not restore exact joint"
        );
        capture(c, &fixture.out, &format!("joint-{kind}-result"))?;
        ui(
            c,
            json!({"action":"file","command":"save","path":fixture.out.join(format!("joint-{kind}.limo"))}),
        )?;
        passed.push(kind);
        println!("PASS native joint {kind}: picks, preview, validation, controls, create/edit, Cancel, Undo/Redo, suppression, delete, source geometry and Save");
        std::fs::write(
            &fixture.report,
            serde_json::to_string_pretty(&json!({"passed":passed}))?,
        )?;
    }
    Ok(())
}

fn motion(
    c: &mut Client,
    kind: &str,
    axes: &[&str],
    saved: &Value,
    out: &std::path::Path,
) -> Result<()> {
    browser(c, &format!("Joint Native {kind}"))?;
    if axes.is_empty() {
        ensure!(
            !controls(&ui(c, json!({"action":"inspect"}))?).any(|v| v["label"] == "Demo motion"),
            "Rigid joint offered motion"
        );
        return Ok(());
    }
    let field = |c: &mut Client, label: &str, value: Option<&str>| {
        panel_field(
            c,
            label,
            value,
            "Scroll assembly up",
            "Scroll assembly down",
        )
    };
    let source = c.call("solid_scene", json!({}))?;
    for axis in axes {
        let label = format!("{axis} slider");
        field(c, &label, Some("0"))?;
        let inspected = ui(c, json!({"action":"inspect"}))?;
        let range = controls(&inspected)
            .find(|v| v["label"] == label)
            .context("Native motion slider missing")?;
        ensure!(range["role"] == "slider", "Motion used a fake slider");
        let id = range["id"].clone();
        ui(c, json!({"action":"key","target":id,"key":"End"}))?;
        let fresh = ui(c, json!({"action":"inspect"}))?;
        let id = controls(&fresh)
            .find(|v| v["label"] == label)
            .context("Motion slider lost after End")?["id"]
            .clone();
        ui(c, json!({"action":"key","target":id,"key":"Home"}))?;
        field(
            c,
            &format!("{axis} position"),
            Some(if axis.contains("rotation") || *axis == "Rotation travel" {
                "20 deg"
            } else {
                "3 mm"
            }),
        )?;
        ensure!(
            assembly(c)? == *saved,
            "{kind}: preview persisted coordinates"
        );
    }
    capture(c, out, &format!("joint-{kind}-motion"))?;
    field(c, "Revert joint position", None)?;
    ensure!(
        assembly(c)? == *saved,
        "{kind}: Revert changed saved position"
    );
    field(c, &format!("{} slider", axes[0]), Some("0"))?;
    field(c, "Save joint position", None)?;
    let moved = assembly(c)?;
    ensure!(moved != *saved, "{kind}: Save did not persist coordinates");
    control(c, "Undo", None)?;
    ensure!(assembly(c)? == *saved, "{kind}: motion Undo failed");
    control(c, "Redo", None)?;
    ensure!(assembly(c)? == moved, "{kind}: motion Redo failed");
    control(c, "Undo", None)?;
    browser(c, &format!("Joint Native {kind}"))?;
    field(c, "Demo motion", None)?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if controls(&state).any(|v| v["label"] == "Demo motion") {
            break;
        }
        ensure!(
            std::time::Instant::now() < until,
            "{kind}: demo did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    ensure!(
        assembly(c)? == *saved && c.call("solid_scene", json!({}))? == source,
        "{kind}: motion demo changed geometry or intent"
    );
    Ok(())
}
