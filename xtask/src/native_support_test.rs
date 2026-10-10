//! Sketch support and coordinate-origin workflow through rendered native UI.
use crate::native_fixture::{begin_sketch, browser_select, control, controls, sketch, start, ui};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::fs;

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-support")?;
    let client = &mut fixture.client;
    control(client, "Create Sketch", None)?;
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("pick-plane.png")}),
    )?;
    let inspected = ui(client, json!({"action":"inspect"}))?;
    let create = controls(&inspected)
        .find(|c| c["label"] == "Create Sketch")
        .context("Create Sketch missing")?;
    ui(
        client,
        json!({"action":"key","key":"Escape","target":create["id"]}),
    )?;
    ensure!(
        sketch(client)?.is_null(),
        "Cancel placement created a sketch"
    );
    for (label, normal) in [("XZ", [0., -1., 0.]), ("YZ", [1., 0., 0.])] {
        begin_sketch(client, label)?;
        ensure!(
            sketch(client)?["basis"]["normal"] == json!(normal),
            "Wrong {label} support"
        );
        client.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":4.,"y":0.},"ctrl_held":true}))?;
        control(client, "Finish sketch", None)?;
    }
    begin_sketch(client, "XY")?;
    client.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":10.,"y":5.},"p2":{"x":30.,"y":25.},"ctrl_held":true}),
    )?;
    let source = sketch(client)?["name"]
        .as_str()
        .context("Sketch name missing")?
        .to_owned();
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":source,"profile_indices":[0],"extent":{"type":"distance","distance":12.}}))?;
    control(client, "Top", None)?;
    for origin in ["Center of selected face", "Project the global origin"] {
        control(client, "Clear selection", None)?;
        control(client, "Top", None)?;
        control(client, "Create Sketch", None)?;
        ui(
            client,
            json!({"action":"viewport","gesture":"move","world":[20.,15.,12.]}),
        )?;
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[20.,15.,12.]}),
        )?;
        control(client, origin, None)?;
        let state = ui(client, json!({"action":"inspect"}))?;
        ensure!(
            controls(&state).any(|c| c["label"] == origin && c["selected"] == true),
            "Origin radio selection missing"
        );
        ui(
            client,
            json!({"action":"capture","path":fixture.out.join(if origin.starts_with("Center") {"face-origin-center.png"} else {"face-origin-projected.png"})}),
        )?;
        if origin.starts_with("Center") {
            control(client, "Close sketch origin", None)?;
            ensure!(sketch(client)?.is_null(), "Close committed the face sketch");
            control(client, "Create Sketch", None)?;
        } else {
            let state = ui(client, json!({"action":"inspect"}))?;
            let choice = controls(&state)
                .find(|c| c["label"] == origin)
                .context("Origin radio missing")?;
            ui(
                client,
                json!({"action":"key","key":"Escape","target":choice["id"]}),
            )?;
            ensure!(
                sketch(client)?.is_null(),
                "Escape committed the face sketch"
            );
            control(client, "Create Sketch", None)?;
            control(client, origin, None)?;
        }
        control(client, "Confirm sketch origin", None)?;
        let active = sketch(client)?;
        ensure!(
            active["plane"]["type"] == "planar_face",
            "Wrong support: {active}"
        );
        let expected = if origin.starts_with("Center") {
            [20., 15., 12.]
        } else {
            [0., 0., 12.]
        };
        let actual = active["basis"]["origin"]
            .as_array()
            .context("No sketch basis")?;
        for (value, expected) in actual.iter().zip(expected) {
            ensure!(
                (value.as_f64().unwrap() - expected).abs() < 1e-6,
                "Wrong coordinate zero: {active}"
            );
        }
        client.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":3.,"y":0.},"ctrl_held":true}))?;
        control(client, "Finish sketch", None)?;
    }
    let datum=client.call("construction_plane_offset",json!({"name":"Raised support","reference":{"type":"origin_plane","plane":"xy"},"distance":24.}))?;
    let datum_id = datum["planes"]
        .as_array()
        .and_then(|p| p.last())
        .context("Datum missing")?["datum_id"]
        .clone();
    control(client, "Clear selection", None)?;
    control(client, "Create Sketch", None)?;
    browser_select(client, "Construction", "Raised support")?;
    ensure!(
        sketch(client)?["plane"] == json!({"type":"datum_plane","datum_id":datum_id}),
        "Wrong datum support"
    );
    client.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":5.,"y":0.},"ctrl_held":true}))?;
    control(client, "Finish sketch", None)?;
    control(client, "Isometric", None)?;
    control(client, "Create Sketch", None)?;
    ui(
        client,
        json!({"action":"viewport","gesture":"click","world":[2.,2.,24.]}),
    )?;
    ensure!(
        sketch(client)?["plane"] == json!({"type":"datum_plane","datum_id":datum_id}),
        "Canvas datum pick failed"
    );
    client.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":6.,"y":0.},"ctrl_held":true}))?;
    control(client, "Finish sketch", None)?;
    control(client, "Isometric", None)?;
    ui(client, json!({"action":"capture","path":fixture.capture}))?;
    ui(
        client,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let scene = client.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"].as_array().is_some_and(Vec::is_empty),
        "Support sketches damaged solid: {scene}"
    );
    fs::write(
        &fixture.report,
        serde_json::to_string_pretty(
            &json!({"passed":true,"project":fixture.project,"capture":fixture.capture,"scene":scene,"document":client.call("cad_document",json!({}))?}),
        )?,
    )?;
    println!("PASS: native origin, planar-face and datum support selection; coordinate zero; Cancel; saved {}",fixture.report.display());
    Ok(())
}
