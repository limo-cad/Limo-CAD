//! Hole parity through one existing native window's rendered controls.
use crate::native_fixture::{
    begin_sketch, capture, control, controls, edit_feature, field, start, ui,
};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::fs;

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-hole")?;
    let mut cases = vec![];
    for (index, mode) in [
        "simple",
        "counterbore",
        "countersink",
        "iso_metric",
        "custom_trapezoidal",
    ]
    .into_iter()
    .enumerate()
    {
        let client = &mut fixture.client;
        if index > 0 {
            let next = control(client, "New design", None)?;
            client.call(
                "cad_attach",
                json!({"session_id":next["active_session_id"]}),
            )?;
        }
        begin_sketch(client, "XY")?;
        client.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":60.,"y":40.},"ctrl_held":true}))?;
        for x in [15., 45.] {
            client.call(
                "sketch_add_point",
                json!({"position":{"x":x,"y":20.},"ctrl_held":true}),
            )?;
        }
        control(client, "Finish sketch", None)?;
        client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":20.}}))?;
        control(client, "Expand Sketches", None)?;
        let state = ui(client, json!({"action":"inspect"}))?;
        if controls(&state)
            .any(|c| c["label"] == "Show Sketch1" && !c["disabled"].as_bool().unwrap_or(true))
        {
            control(client, "Show Sketch1", None)?;
        }
        ui(
            client,
            json!({"action":"view","view":"top","fit":true,"duration_ms":0}),
        )?;
        control(client, "Hole", None)?;
        capture(client, &fixture.out, &format!("hole-{mode}-empty"))?;
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[15.,20.,20.]}),
        )?;
        for x in [15., 45.] {
            ui(
                client,
                json!({"action":"viewport","gesture":"move","world":[x,20.,0.]}),
            )?;
            ui(
                client,
                json!({"action":"viewport","gesture":"click","world":[x,20.,0.]}),
            )?;
        }
        let before = client.call("cad_document", json!({}))?;
        field(client, "Diameter", Some("NaN"))?;
        let state = ui(client, json!({"action":"inspect"}))?;
        ensure!(
            controls(&state).any(|c| c["label"] == "Apply Hole" && c["disabled"] == true),
            "Invalid diameter was accepted"
        );
        ensure!(
            client.call("cad_document", json!({}))? == before,
            "Invalid input changed geometry"
        );
        field(client, "Diameter", Some("0.5 cm"))?;
        if mode != "simple" {
            field(client, "Extent", Some("distance"))?;
            field(client, "Depth", Some("8 mm"))?;
            field(client, "Hole bottom", Some("flat"))?;
        }
        if matches!(mode, "counterbore" | "countersink") {
            field(client, "Hole style", None)?;
            field(
                client,
                if mode == "counterbore" {
                    "Counterbore"
                } else {
                    "Countersink"
                },
                None,
            )?;
            let size = if mode == "counterbore" {
                "Counterbore diameter"
            } else {
                "Countersink diameter"
            };
            field(client, size, Some("4"))?;
            let state = ui(client, json!({"action":"inspect"}))?;
            ensure!(
                controls(&state).any(|c| c["label"] == "Apply Hole" && c["disabled"] == true),
                "An undersized head was accepted"
            );
            field(client, size, Some("9"))?;
        } else if mode != "simple" {
            field(client, "Threaded hole", None)?;
            if mode == "custom_trapezoidal" {
                field(client, "Thread standard", Some(mode))?;
            }
            field(client, "Thread full cylindrical hole depth", None)?;
            field(client, "Thread depth", Some("4 mm"))?;
        }
        ui(
            client,
            json!({"action":"view","view":"isometric","fit":true,"duration_ms":300}),
        )?;
        capture(client, &fixture.out, &format!("hole-{mode}-form"))?;
        control(client, "Apply Hole", None)?;
        let definitions = client.call("solid_hole_definitions", json!({}))?;
        let hole = &definitions.as_array().context("Hole definitions missing")?[0];
        ensure!(
            hole["positions"].as_array().is_some_and(|p| p.len() == 2
                && p.iter()
                    .all(|p| p["position_reference"]["sketch_name"] == "Sketch1")),
            "Associative positions were lost: {hole}"
        );
        ensure!(
            client.call("solid_scene", json!({}))?["errors"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "Hole geometry failed"
        );
        capture(client, &fixture.out, &format!("hole-{mode}"))?;
        let name = hole["name"].as_str().context("Hole feature name missing")?;
        let original = client.call("cad_document", json!({}))?;
        edit_feature(client, name, true)?;
        let edit_field = if mode == "simple" {
            "Diameter"
        } else {
            "Depth"
        };
        field(client, edit_field, Some("6"))?;
        control(client, "Close Hole", None)?;
        ensure!(
            client.call("cad_document", json!({}))? == original,
            "Cancel changed the hole"
        );
        edit_feature(client, name, false)?;
        field(client, edit_field, Some("6"))?;
        capture(client, &fixture.out, &format!("hole-{mode}-edit"))?;
        control(client, "Apply Hole", None)?;
        let edited = client.call("solid_hole_definitions", json!({}))?;
        ensure!(edited != definitions, "Hole edit was lost");
        control(client, "Undo", None)?;
        ensure!(
            client.call("solid_hole_definitions", json!({}))? == definitions,
            "Hole Undo failed"
        );
        control(client, "Redo", None)?;
        ensure!(
            client.call("solid_hole_definitions", json!({}))? == edited,
            "Hole Redo failed"
        );
        ensure!(
            client.call("solid_scene", json!({}))?["errors"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "Edited hole geometry failed"
        );
        ui(
            client,
            json!({"action":"file","command":"save","path":fixture.out.join(format!("hole-{mode}.limo"))}),
        )?;
        cases.push(json!({"mode":mode,"passed":true}));
        println!("PASS native Hole {mode}: associative points, validation, history edit, Cancel, Undo/Redo, capture and Save");
    }
    fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({"passed":true,"cases":cases}))?,
    )?;
    println!("PASS native holes: saved {}", fixture.report.display());
    Ok(())
}
