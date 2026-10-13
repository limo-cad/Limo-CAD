//! Solid form parity through the actual rendered widgets and canvas.
use crate::native_fixture::{click, control, controls, start, ui, Fixture};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::fs;

fn revolve(client: &mut Client) -> Result<Value> {
    client.call("solid_revolve_definitions", json!({}))
}
fn edit_feature(client: &mut Client, label: &str) -> Result<()> {
    let state = ui(client, json!({"action":"inspect"}))?;
    let target = controls(&state)
        .find(|c| c["label"] == label && c["surface"] == "document/history")
        .context("Feature history control missing")?;
    ui(
        client,
        json!({"action":"double_click","target":target["id"]}),
    )?;
    Ok(())
}

fn rib_case(client: &mut Client, out: &std::path::Path) -> Result<Value> {
    let project = out.join("native-rib.limo");
    let capture = out.join("native-rib.png");
    ensure!(
        !project.exists() && !capture.exists(),
        "Existing Rib evidence must be preserved"
    );
    let new = control(client, "New design", None)?;
    client.call("cad_attach", json!({"session_id":new["active_session_id"]}))?;
    crate::native_fixture::begin_sketch(client, "XY")?;
    control(client, "Line", None)?;
    click(client, [-20., 0.], false)?;
    click(client, [20., 0.], false)?;
    control(client, "Finish sketch", None)?;
    control(client, "Rib", None)?;
    click(client, [0., 0.], false)?;
    control(client, "Thickness (mm)", Some("0"))?;
    let invalid = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&invalid).any(|c| c["label"] == "Apply Rib" && c["disabled"] == true),
        "Zero Rib thickness should block Apply"
    );
    control(client, "Thickness (mm)", Some("0.3 cm"))?;
    control(client, "Depth (mm)", Some("12 mm"))?;
    control(client, "Extent: Distance", Some("to_next"))?;
    let next = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&next).any(|c| c["label"] == "Apply Rib" && c["disabled"] == true),
        "To Next needs a target body"
    );
    control(client, "Extent: To Next", Some("distance"))?;
    ui(
        client,
        json!({"action":"capture","path":out.join("rib-form.png")}),
    )?;
    control(client, "Apply Rib", None)?;
    let method = "solid_rib_definitions";
    let original = client.call(method, json!({}))?;
    ensure!(
        original.as_array().is_some_and(|a| a.len() == 1)
            && original[0]["thickness"] == 3.
            && original[0]["depth"] == 12.,
        "Rib dimensions incorrect: {original}"
    );
    let name = original[0]["name"].as_str().context("Rib name missing")?;
    edit_feature(client, name)?;
    control(client, "Depth (mm)", Some("18"))?;
    control(client, "Close Rib", None)?;
    ensure!(
        client.call(method, json!({}))? == original,
        "Cancel changed Rib"
    );
    edit_feature(client, name)?;
    control(client, "Depth (mm)", Some("18"))?;
    control(client, "Apply Rib", None)?;
    ensure!(
        client.call(method, json!({}))?[0]["depth"] == 18.,
        "Rib edit failed"
    );
    control(client, "Undo", None)?;
    ensure!(
        client.call(method, json!({}))?[0]["depth"] == 12.,
        "Rib Undo failed"
    );
    control(client, "Redo", None)?;
    ensure!(
        client.call(method, json!({}))?[0]["depth"] == 18.,
        "Rib Redo failed"
    );
    let scene = client.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"].as_array().is_some_and(Vec::is_empty)
            && scene["bodies"].as_array().is_some_and(|b| b.len() == 1),
        "Rib did not create one valid solid"
    );
    control(client, "Isometric", None)?;
    ui(client, json!({"action":"capture","path":capture}))?;
    ui(
        client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    println!("PASS: Rib native curve picking, units, invalid thickness/extent, edit, Cancel, Undo/Redo and Save");
    Ok(json!({"definitions":client.call(method,json!({}))?,"capture":capture,"project":project}))
}

fn path_case(client: &mut Client, out: &std::path::Path, kind: &str) -> Result<Value> {
    let project = out.join(format!("native-{}.limo", kind.to_lowercase()));
    let capture = out.join(format!("native-{}.png", kind.to_lowercase()));
    ensure!(
        !project.exists() && !capture.exists(),
        "Existing {kind} evidence must be preserved"
    );
    let new = control(client, "New design", None)?;
    let session = new["active_session_id"]
        .as_str()
        .context("New document session missing")?;
    client.call("cad_attach", json!({"session_id":session}))?;
    crate::native_fixture::begin_sketch(client, "XY")?;
    control(client, "Rectangle", None)?;
    click(client, [-8., -6.], false)?;
    click(client, [8., 6.], false)?;
    control(client, "Finish sketch", None)?;
    if kind == "Sweep" {
        crate::native_fixture::begin_sketch(client, "XZ")?;
        control(client, "Line", None)?;
        for world in [[0., 0., 0.], [0., 0., 30.]] {
            ui(
                client,
                json!({"action":"viewport","gesture":"click","world":world}),
            )?;
        }
        control(client, "Finish sketch", None)?;
    } else {
        let datum=client.call("construction_plane_offset",json!({"name":"Upper section","reference":{"type":"origin_plane","plane":"xy"},"distance":30.}))?;
        let datum_id = datum["planes"]
            .as_array()
            .and_then(|p| p.last())
            .context("New datum missing")?["datum_id"]
            .clone();
        ensure!(datum_id.is_number(), "Datum id missing: {datum}");
        control(client, "Create Sketch", None)?;
        crate::native_fixture::browser_select(client, "Construction", "Upper section")?;
        ensure!(
            crate::native_fixture::sketch(client)?["plane"]["datum_id"] == datum_id,
            "Loft sketch used the wrong support"
        );
        client.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":-5.,"y":-4.},"p2":{"x":5.,"y":4.},"ctrl_held":true}))?;
        client.call("sketch_finish", json!({}))?;
    }
    control(client, "Isometric", None)?;
    control(client, kind, None)?;
    ui(
        client,
        json!({"action":"viewport","gesture":"click","world":[2.,1.,0.]}),
    )?;
    if kind == "Sweep" {
        control(client, "Select path curves", None)?;
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[0.,0.,15.]}),
        )?;
    } else {
        ui(
            client,
            json!({"action":"viewport","gesture":"click","world":[1.,1.,30.]}),
        )?;
    }
    control(client, "Use guide rail", None)?;
    let blocked = ui(client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&blocked).any(|c| c["label"] == format!("Apply {kind}") && c["disabled"] == true),
        "Empty guide should block Apply"
    );
    control(client, "Use guide rail", None)?;
    ui(
        client,
        json!({"action":"capture","path":out.join(format!("{}-form.png",kind.to_lowercase()))}),
    )?;
    control(client, &format!("Apply {kind}"), None)?;
    let method = format!("solid_{}_definitions", kind.to_lowercase());
    let before = client.call(&method, json!({}))?;
    ensure!(
        before.as_array().is_some_and(|d| d.len() == 1),
        "Expected a single {kind} feature: {before}"
    );
    let feature = before[0]["name"].as_str().context("Feature name missing")?;
    let field = if kind == "Sweep" {
        "Force C1 continuity"
    } else {
        "Ruled surfaces"
    };
    let prop = if kind == "Sweep" { "force_c1" } else { "ruled" };
    edit_feature(client, feature)?;
    control(client, field, None)?;
    control(client, &format!("Close {kind}"), None)?;
    ensure!(
        client.call(&method, json!({}))? == before,
        "Cancel changed {kind}"
    );
    edit_feature(client, feature)?;
    control(client, field, None)?;
    control(client, &format!("Apply {kind}"), None)?;
    ensure!(
        client.call(&method, json!({}))?[0][prop] == true,
        "Edit did not update {kind}"
    );
    control(client, "Undo", None)?;
    ensure!(
        client.call(&method, json!({}))?[0][prop] == false,
        "Undo failed for {kind}"
    );
    control(client, "Redo", None)?;
    ensure!(
        client.call(&method, json!({}))?[0][prop] == true,
        "Redo failed for {kind}"
    );
    let scene = client.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"].as_array().is_some_and(Vec::is_empty)
            && scene["bodies"].as_array().is_some_and(|b| b.len() == 1),
        "Invalid {kind} solid: {scene}"
    );
    control(client, "Isometric", None)?;
    ui(client, json!({"action":"capture","path":capture}))?;
    ui(
        client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    println!("PASS: {kind} native reference picking, validation, Apply, edit, Cancel, Undo/Redo and Save");
    Ok(
        json!({"feature":kind,"definitions":client.call(&method,json!({}))?,"capture":capture,"project":project}),
    )
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let Fixture {
        mut client,
        server,
        session,
        out,
        project,
        capture,
        report,
    } = start(args, "native-build")?;
    crate::native_fixture::begin_sketch(&mut client, "XY")?;
    control(&mut client, "Rectangle", None)?;
    click(&mut client, [10., 0.], false)?;
    click(&mut client, [30., 20.], false)?;
    control(&mut client, "Finish sketch", None)?;
    control(&mut client, "Revolve", None)?;
    click(&mut client, [20., 10.], false)?;
    control(&mut client, "Select axis line", None)?;
    click(&mut client, [10., 10.], false)?;
    let selected = ui(&mut client, json!({"action":"inspect"}))?;
    ensure!(
        controls(&selected).any(|c| c["label"]
            .as_str()
            .is_some_and(|label| label.starts_with("Axis: Sketch1 · line "))),
        "Axis pick did not resolve a stable line"
    );
    control(&mut client, "Sketch Y axis", None)?;
    control(&mut client, "Angle (degrees)", Some("360"))?;
    ui(
        &mut client,
        json!({"action":"capture","path":out.join("revolve-form.png")}),
    )?;
    ensure!(
        client.call("solid_scene", json!({}))?["bodies"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Reference preview created geometry"
    );
    control(&mut client, "Apply Revolve", None)?;
    let definitions = revolve(&mut client)?;
    ensure!(
        definitions.as_array().is_some_and(|d| d.len() == 1),
        "Expected one parametric Revolve"
    );
    ensure!(
        definitions[0]["angle_deg"] == 360.,
        "Revolve angle did not persist"
    );
    let feature = definitions[0]["name"]
        .as_str()
        .context("Revolve name missing")?
        .to_owned();
    let body = client.call("solid_scene", json!({}))?;
    ensure!(
        body["errors"].as_array().is_some_and(Vec::is_empty)
            && body["bodies"].as_array().is_some_and(|b| b.len() == 1),
        "Revolve produced invalid solid: {}",
        body["errors"]
    );
    println!("PASS: profile and axis picking, native Revolve fields and exact solid");
    edit_feature(&mut client, &feature)?;
    control(&mut client, "Angle (degrees)", Some("180"))?;
    control(&mut client, "Close Revolve", None)?;
    ensure!(
        revolve(&mut client)? == definitions,
        "Closing the form changed the feature"
    );
    edit_feature(&mut client, &feature)?;
    control(&mut client, "Angle (degrees)", Some("180"))?;
    control(&mut client, "Apply Revolve", None)?;
    ensure!(
        revolve(&mut client)?[0]["angle_deg"] == 180.,
        "Revolve edit failed"
    );
    control(&mut client, "Undo", None)?;
    ensure!(
        revolve(&mut client)?[0]["angle_deg"] == 360.,
        "Undo did not restore full revolution"
    );
    control(&mut client, "Redo", None)?;
    ensure!(
        revolve(&mut client)?[0]["angle_deg"] == 180.,
        "Redo did not restore half revolution"
    );
    println!("PASS: Revolve history edit, close/cancel, Undo and Redo");
    control(&mut client, "Isometric", None)?;
    ui(&mut client, json!({"action":"capture","path":capture}))?;
    ui(
        &mut client,
        json!({"action":"file","command":"save","path":project}),
    )?;
    let revolve = revolve(&mut client)?;
    let sweep = path_case(&mut client, &out, "Sweep")?;
    let loft = path_case(&mut client, &out, "Loft")?;
    let rib = rib_case(&mut client, &out)?;
    fs::write(
        &report,
        serde_json::to_vec_pretty(&json!({"status":"passed","server":server,"session":session,
        "checks":["profile_pick","axis_line_pick","axis_preset","revolve_apply","history_edit","close_cancel","undo_redo","render_capture","save"],
        "definitions":revolve,"capture":capture,"project":project,"sweep":sweep,"loft":loft,"rib":rib}))?,
    )?;
    println!("PASS: native build saved; report {}", report.display());
    Ok(())
}
