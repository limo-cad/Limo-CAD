//! Real native camera requests, completion receipts and frame capture through MCP.
use crate::native_fixture::{begin_sketch, control, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn view(client: &mut Client, mut request: Value) -> Result<Value> {
    request["action"] = json!("view");
    let result = ui(client, request)?;
    let camera = result["value"]["camera"].clone();
    ensure!(
        camera.is_object(),
        "View completed without its final camera"
    );
    Ok(camera)
}
fn point(camera: &Value, key: &str) -> [f64; 3] {
    std::array::from_fn(|i| camera[key][i].as_f64().unwrap())
}
fn radius(camera: &Value) -> f64 {
    let p = point(camera, "position");
    let t = point(camera, "target");
    ((p[0] - t[0]).powi(2) + (p[1] - t[1]).powi(2) + (p[2] - t[2]).powi(2)).sqrt()
}
fn target(camera: &Value, expected: [f64; 3]) -> Result<()> {
    ensure!(
        point(camera, "target")
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-4),
        "Wrong framing target: {camera}"
    );
    Ok(())
}
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-view")?;
    let client = &mut fixture.client;
    begin_sketch(client, "XY")?;
    client.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":20.},"ctrl_held":true}),
    )?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    let scene = client.call("solid_scene", json!({}))?;
    let body = scene["bodies"][0]["id"].as_u64().context("Body missing")?;
    let original = client.call("cad_document", json!({}))?;
    let mut cameras = Vec::new();
    for (orientation, axis) in [
        ("front", [0., -1., 0.]),
        ("back", [0., 1., 0.]),
        ("left", [-1., 0., 0.]),
        ("right", [1., 0., 0.]),
        ("top", [0., 0., 1.]),
        ("bottom", [0., 0., -1.]),
    ] {
        let camera = view(
            client,
            json!({"view":orientation,"fit":true,"duration_ms":0}),
        )?;
        let p = point(&camera, "position");
        let t = point(&camera, "target");
        let distance = radius(&camera);
        ensure!(
            (0..3).all(|i| ((p[i] - t[i]) / distance - axis[i]).abs() < 1e-5),
            "Wrong {orientation} orientation"
        );
        cameras.push(json!({"view":orientation,"camera":camera}));
    }
    let started = Instant::now();
    let iso = view(
        client,
        json!({"view":"isometric","fit":true,"duration_ms":350}),
    )?;
    ensure!(
        started.elapsed() >= Duration::from_millis(340),
        "View receipt preceded camera completion"
    );
    target(&iso, [10., 10., 5.])?;
    let current = view(client, json!({"view":"current","fit":false}))?;
    ensure!(current == iso, "Current-view read changed the camera");
    let turned = view(
        client,
        json!({"view":"current","fit":false,"orbit_degrees":120,"duration_ms":450}),
    )?;
    ensure!(
        (radius(&turned) - radius(&iso)).abs() < 1e-3,
        "Orbit changed radius"
    );
    ensure!(
        (point(&turned, "position")[2] - point(&iso, "position")[2]).abs() < 1e-3,
        "Orbit changed elevation"
    );
    ensure!(turned["position"] != iso["position"], "Orbit did not move");
    let full = view(
        client,
        json!({"view":"current","fit":false,"orbit_degrees":360,"duration_ms":300}),
    )?;
    ensure!(
        full == turned,
        "Full turn did not return to its exact reference camera"
    );
    let invalid = client.call(
        "cad_interface",
        json!({"action":"view","body_id":u64::MAX,"duration_ms":0}),
    );
    ensure!(
        invalid.as_ref().is_err() || invalid.as_ref().is_ok_and(|v| v["status"] == "failed"),
        "Missing body was framed"
    );
    ensure!(
        view(client, json!({"view":"current","fit":false}))? == turned,
        "Rejected target moved the camera"
    );
    ui(client, json!({"action":"window","mode":"background"}))?;
    let background = view(
        client,
        json!({"view":"current","fit":false,"orbit_degrees":-120,"duration_ms":200}),
    )?;
    ensure!(
        (radius(&background) - radius(&iso)).abs() < 1e-3,
        "Background view command failed"
    );
    ui(client, json!({"action":"window","mode":"foreground"}))?;
    ensure!(
        client.call("cad_document", json!({}))? == original,
        "Camera operations changed the model"
    );
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("native-view-orbit.png")}),
    )?;

    let component = client.call(
        "assembly_create_component",
        json!({"name":"Camera target","body_ids":[body],"absorb_promoted_bodies":true}),
    )?;
    let component_id = component["id"].as_u64().context("Component missing")?;
    let assembly = client.call("assembly_document", json!({}))?;
    let occurrence = assembly["component_structure"]["occurrences"]
        .as_array()
        .context("Occurrences missing")?
        .iter()
        .find(|o| o["component_id"] == component_id)
        .context("Component occurrence missing")?["id"]
        .clone();
    client.call("assembly_set_occurrence_pose",json!({"occurrence_id":occurrence,"local_pose":{"translation":[100.,0.,0.],"rotation":[0.,0.,0.,1.]}}))?;
    let placed = view(
        client,
        json!({"view":"isometric","component_id":component_id,"duration_ms":250}),
    )?;
    target(&placed, [110., 10., 5.])?;
    let source = view(client, json!({"body_id":body,"duration_ms":0}))?;
    target(&source, [110., 10., 5.])?;
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("native-view-component.png")}),
    )?;

    begin_sketch(client, "XZ")?;
    client.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":80.,"y":30.},"p2":{"x":90.,"y":30.},"ctrl_held":true}))?;
    let sketch_before = client.call("sketch_active", json!({}))?;
    let focused = view(
        client,
        json!({"view":"front","target":"active_sketch","duration_ms":250}),
    )?;
    target(&focused, [80., 0., 30.])?;
    ensure!(
        client.call("sketch_active", json!({}))? == sketch_before,
        "Framing changed the active sketch"
    );
    ui(
        client,
        json!({"action":"capture","path":fixture.out.join("native-view-sketch.png")}),
    )?;
    control(client, "Finish sketch", None)?;
    control(client, "Isometric", None)?;
    ui(client, json!({"action":"capture","path":fixture.capture}))?;
    ui(
        client,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(
            &json!({"passed":true,"orientations":cameras,"component":component_id,"project":fixture.project}),
        )?,
    )?;
    println!("PASS: six orientations, ISO fit, timed/full-turn orbit, background/foreground, placed body/component and active-sketch framing, unchanged geometry, capture and Save");
    Ok(())
}
