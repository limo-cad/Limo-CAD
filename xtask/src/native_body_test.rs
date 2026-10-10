//! Live native body operations use the rendered selectors and shared commands.
use crate::native_fixture::{begin_sketch, control, controls, edit_feature, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};
mod patterns;
mod planes;

pub(super) fn run_patterns(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-pattern")?;
    let mut cases = Vec::new();
    for circular in [false, true] {
        if circular {
            let new = control(&mut fixture.client, "New design", None)?;
            fixture
                .client
                .call("cad_attach", json!({"session_id":new["active_session_id"]}))?;
        }
        cases.push(patterns::run(&mut fixture.client, &fixture.out, circular)?);
    }
    fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({"passed":true,"cases":cases}))?,
    )?;
    println!("PASS: saved {}", fixture.report.display());
    Ok(())
}

fn capture(client: &mut Client, out: &Path, name: &str) -> Result<()> {
    ui(
        client,
        json!({"action":"capture","path":out.join(format!("{name}.png"))}),
    )?;
    Ok(())
}
fn combine(client: &mut Client, out: &Path, operation: &str, keep: bool) -> Result<Value> {
    for (i, x) in [0., 20.].into_iter().enumerate() {
        begin_sketch(client, "XY")?;
        client.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+30.,"y":20.},"ctrl_held":true}))?;
        control(client, "Finish sketch", None)?;
        client.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    }
    control(client, "Isometric", None)?;
    let original = client.call("solid_scene", json!({}))?;
    let target = original["bodies"][0]["id"].clone();
    let tool = original["bodies"][1]["id"].clone();
    control(client, "Combine", None)?;
    let state = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&state).any(|c| c["label"] == "Apply Combine" && c["disabled"] == true),
        "Combine needs both body references"
    );
    ui(
        client,
        json!({"action":"viewport","gesture":"move","world":[5.,10.,10.]}),
    )?;
    ui(
        client,
        json!({"action":"viewport","gesture":"click","world":[5.,10.,10.]}),
    )?;
    control(client, "Click one or more tool bodies", None)?;
    for _ in 0..3 {
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[45.,10.,10.]}),
        )?;
    }
    control(client, "Operation: Add", None)?;
    control(
        client,
        match operation {
            "join" => "Add",
            "cut" => "Cut",
            _ => "Intersect",
        },
        None,
    )?;
    if keep {
        control(client, "Keep tool bodies", None)?;
    }
    let tag = format!("combine-{operation}{}", if keep { "-keep" } else { "" });
    capture(client, out, &format!("{tag}-form"))?;
    control(client, "Apply Combine", None)?;
    let definitions = client.call("solid_body_feature_definitions", json!({}))?;
    let d = &definitions[0];
    ensure!(
        d["target_body_id"] == target
            && d["tool_body_ids"] == json!([tool])
            && d["operation"] == operation
            && d["keep_tools"] == keep,
        "Wrong Combine references or options: {d}"
    );
    let created = client.call("solid_scene", json!({}))?;
    ensure!(
        created["errors"].as_array().is_some_and(Vec::is_empty),
        "Invalid Combine result"
    );
    ensure!(
        created["bodies"]
            .as_array()
            .is_some_and(|b| b.len() == if keep { 2 } else { 1 }),
        "Wrong retained tool count"
    );
    let body = created["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == target)
        .context("Target identity lost")?;
    let xs: Vec<_> = body["mesh"]["positions"]
        .as_array()
        .context("Mesh missing")?
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| p[0].as_f64().unwrap())
        .collect();
    let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (expected_min, expected_max) = match operation {
        "join" => (0., 50.),
        "cut" => (0., 20.),
        _ => (20., 30.),
    };
    ensure!(
        (min - expected_min).abs() < 1e-4 && (max - expected_max).abs() < 1e-4,
        "Wrong boolean geometry: {min}..{max}"
    );
    let name = d["name"].as_str().context("Combine name missing")?;
    let before = client.call("cad_document", json!({}))?;
    edit_feature(client, name, true)?;
    ensure!(
        client.call("cad_document", json!({}))? == before,
        "Opening Combine moved live history"
    );
    control(client, "Keep tool bodies", None)?;
    control(client, "Close Combine", None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))? == definitions,
        "Cancel changed Combine"
    );
    edit_feature(client, name, false)?;
    control(client, "Keep tool bodies", None)?;
    capture(client, out, &format!("{tag}-edit"))?;
    control(client, "Apply Combine", None)?;
    ensure!(
        client.call("solid_scene", json!({}))?["bodies"]
            .as_array()
            .is_some_and(|b| b.len() == if keep { 1 } else { 2 }),
        "Edited Keep tool bodies failed"
    );
    control(client, "Undo", None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))? == definitions,
        "Undo failed"
    );
    control(client, "Redo", None)?;
    ensure!(
        client.call("solid_body_feature_definitions", json!({}))?[0]["keep_tools"] == !keep,
        "Redo failed"
    );
    control(client, "Isometric", None)?;
    capture(client, out, &tag)?;
    let project = out.join(format!("{tag}.limo"));
    ui(
        client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    println!(
        "PASS: {tag} exact boolean bounds, picking, options, edit, Cancel, Undo/Redo and Save"
    );
    Ok(json!({"operation":operation,"keep_tools":!keep,"project":project}))
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-body")?;
    let mut cases = Vec::new();
    for (i, (operation, keep)) in [
        ("join", false),
        ("cut", false),
        ("intersect", false),
        ("cut", true),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            let new = control(&mut fixture.client, "New design", None)?;
            fixture
                .client
                .call("cad_attach", json!({"session_id":new["active_session_id"]}))?;
        }
        cases.push(combine(&mut fixture.client, &fixture.out, operation, keep)?);
    }
    for mirror in [true, false] {
        let new = control(&mut fixture.client, "New design", None)?;
        fixture
            .client
            .call("cad_attach", json!({"session_id":new["active_session_id"]}))?;
        cases.push(planes::run(&mut fixture.client, &fixture.out, mirror)?);
    }
    fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({"passed":true,"cases":cases}))?,
    )?;
    println!("PASS: saved {}", fixture.report.display());
    Ok(())
}
