//! Refine the rendered model and edit original topology through live MCP.
use crate::native_fixture::{begin_sketch, control, controls, edit_feature, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn case(client: &mut Client, out: &Path, kind: &str) -> Result<Value> {
    begin_sketch(client, "XY")?;
    client.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":30.,"y":20.},"ctrl_held":true}),
    )?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    control(client, "Isometric", None)?;
    let scene = client.call("solid_scene", json!({}))?;
    let body = &scene["bodies"][0];
    let edge = body["edges"]
        .as_array()
        .context("No edges")?
        .iter()
        .find(|e| {
            e["refinable"] == true
                && e["points"].as_array().is_some_and(|p| {
                    p.len() >= 2
                        && p.iter().all(|p| {
                            p["z"].as_f64().is_some_and(|z| (z - 10.).abs() < 1e-6)
                                && p["y"].as_f64().is_some_and(|y| y.abs() < 1e-6)
                        })
                })
        })
        .context("Visible upper front edge missing")?;
    let points = edge["points"].as_array().unwrap();
    let world: Vec<_> = if kind == "Shell" {
        vec![15., 10., 10.]
    } else {
        ["x", "y", "z"]
            .iter()
            .map(|k| {
                (points.first().unwrap()[*k].as_f64().unwrap()
                    + points.last().unwrap()[*k].as_f64().unwrap())
                    * 0.5
            })
            .collect()
    };
    control(client, kind, None)?;
    ui(
        client,
        json!({"action":"viewport","gesture":"move","world":world}),
    )?;
    ui(
        client,
        json!({"action":"viewport","gesture":"click","world":world}),
    )?;
    let size = if kind == "Shell" {
        "Wall thickness"
    } else if kind == "Fillet" {
        "Radius"
    } else {
        "Distance"
    };
    let property = if kind == "Shell" {
        "thickness"
    } else if kind == "Fillet" {
        "radius"
    } else {
        "distance"
    };
    control(client, size, Some("0"))?;
    let inspected = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&inspected)
            .any(|c| c["label"] == format!("Apply {kind}") && c["disabled"] == true),
        "Invalid size did not block Apply"
    );
    ensure!(
        controls(&inspected)
            .filter(|c| c["label"] == size || c["label"] == format!("Apply {kind}"))
            .all(|c| c["surface"] == "solid/refine"),
        "Refinement fields escaped the shared Refine group"
    );
    control(client, size, Some("0.1 cm"))?;
    if kind == "Shell" {
        for _ in 0..2 {
            ui(
                client,
                json!({"action":"viewport","gesture":"click","world":[15.,0.,5.]}),
            )?;
        }
        control(client, "Offset walls inward", None)?;
        control(client, "Offset walls inward", None)?;
    } else {
        control(client, "Tangent chain", None)?;
    }
    ui(
        client,
        json!({"action":"capture","path":out.join(format!("{}-form.png",kind.to_lowercase()))}),
    )?;
    control(client, &format!("Apply {kind}"), None)?;
    let method = if kind == "Shell" {
        "solid_body_feature_definitions".to_owned()
    } else {
        format!("solid_{}_definitions", kind.to_lowercase())
    };
    let original = client.call(&method, json!({}))?;
    ensure!(
        original[0][property] == 1.
            && (if kind == "Shell" {
                original[0]["face_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.len() == 1)
                    && original[0]["inward"] == true
            } else {
                original[0]["edge_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.contains(&edge["id"]))
            }),
        "Wrong edge or size: {original}"
    );
    let name = original[0]["name"]
        .as_str()
        .context("Feature name missing")?;
    let before = client.call("cad_document", json!({}))?;
    edit_feature(client, name, true)?;
    ensure!(
        client.call("cad_document", json!({}))? == before,
        "Opening edit changed live history"
    );
    control(client, size, Some("2 mm"))?;
    control(client, &format!("Close {kind}"), None)?;
    ensure!(
        client.call(&method, json!({}))? == original,
        "Cancel changed the feature"
    );
    edit_feature(client, name, false)?;
    control(client, size, Some("2 mm"))?;
    ui(
        client,
        json!({"action":"capture","path":out.join(format!("{}-edit.png",kind.to_lowercase()))}),
    )?;
    control(client, &format!("Apply {kind}"), None)?;
    ensure!(
        client.call(&method, json!({}))?[0][property] == 2.,
        "Edit failed"
    );
    control(client, "Undo", None)?;
    ensure!(
        client.call(&method, json!({}))? == original,
        "Undo did not restore original feature"
    );
    control(client, "Redo", None)?;
    ensure!(
        client.call(&method, json!({}))?[0][property] == 2.,
        "Redo failed"
    );
    let final_scene = client.call("solid_scene", json!({}))?;
    ensure!(
        final_scene["bodies"]
            .as_array()
            .is_some_and(|b| b.len() == 1)
            && final_scene["errors"].as_array().is_some_and(Vec::is_empty),
        "Refine produced invalid geometry"
    );
    if kind == "Shell" {
        ensure!(
            final_scene["bodies"][0]["faces"]
                .as_array()
                .is_some_and(|faces| faces.iter().any(|face| face["plane"]["normal"][2]
                    .as_f64()
                    .is_some_and(|z| z > 0.999)
                    && face["plane"]["origin"][2]
                        .as_f64()
                        .is_some_and(|z| (z - 2.).abs() < 1e-6))),
            "Shell must leave the edited 2 mm floor beneath the selected top opening"
        );
        control(client, "Top", None)?;
        ui(
            client,
            json!({"action":"capture","path":out.join("shell-top.png")}),
        )?;
    }
    control(client, "Isometric", None)?;
    let capture = out.join(format!("native-{}.png", kind.to_lowercase()));
    let project = out.join(format!("native-{}.limo", kind.to_lowercase()));
    ensure!(
        !capture.exists() && !project.exists(),
        "Preserve existing evidence"
    );
    ui(client, json!({"action":"capture","path":capture}))?;
    ui(
        client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    println!("PASS: {kind} live reference picking, typed sizes, validation, history edit, Cancel, Undo/Redo and Save");
    Ok(json!({"project":project,"capture":capture,"definitions":client.call(&method,json!({}))?}))
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-refine")?;
    let mut cases = Vec::new();
    cases.push(case(&mut fixture.client, &fixture.out, "Fillet")?);
    let new = control(&mut fixture.client, "New design", None)?;
    fixture
        .client
        .call("cad_attach", json!({"session_id":new["active_session_id"]}))?;
    cases.push(case(&mut fixture.client, &fixture.out, "Chamfer")?);
    let new = control(&mut fixture.client, "New design", None)?;
    fixture
        .client
        .call("cad_attach", json!({"session_id":new["active_session_id"]}))?;
    cases.push(case(&mut fixture.client, &fixture.out, "Shell")?);
    fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({"passed":true,"cases":cases}))?,
    )?;
    println!("PASS: saved {}", fixture.report.display());
    Ok(())
}
