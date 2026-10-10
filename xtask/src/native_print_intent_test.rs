//! Retained Bevy controls qualify the five-part manufacturing-intent workflow.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, owned_config, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, process::Command, time::Duration};

fn field(
    c: &mut Client,
    label: &str,
    value: Option<&str>,
    surface: &str,
    up: &str,
    down: &str,
) -> Result<Value> {
    for pass in 0..2 {
        if pass == 1 {
            for _ in 0..64 {
                let state = ui(c, json!({"action":"inspect"}))?;
                if !controls(&state).any(|v| v["label"] == up && v["disabled"] == false) {
                    break;
                }
                control(c, up, None)?;
            }
        }
        for _ in 0..64 {
            let state = ui(c, json!({"action":"inspect"}))?;
            if controls(&state)
                .any(|v| v["surface"] == surface && v["label"] == label && v["disabled"] == false)
            {
                return control(c, label, value);
            }
            if !controls(&state).any(|v| v["label"] == down && v["disabled"] == false) {
                break;
            }
            control(c, down, None)?;
        }
    }
    anyhow::bail!("Missing enabled {surface} field {label}")
}
fn print(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    field(
        c,
        label,
        value,
        "body/print-intent",
        "Previous print settings fields",
        "More print settings fields",
    )
}
fn view(c: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    field(
        c,
        label,
        value,
        "document/views",
        "Previous named-view fields",
        "More named-view fields",
    )
}
fn model(c: &mut Client) -> Result<Value> {
    let model = c.call("cad_project_model", json!({}))?;
    serde_json::from_str(model.as_str().context("Model JSON")?).map_err(Into::into)
}
fn part(document: &Value, id: &Value) -> Result<Value> {
    document["parts"]
        .as_array()
        .context("Print parts")?
        .iter()
        .find(|p| p["body_id"] == *id)
        .map(|p| p["settings"].clone())
        .context("Part requests missing")
}
fn attach(c: &mut Client, transition: &Value) -> Result<()> {
    c.call("cad_attach",json!({"session_id":transition["active_session_id"].as_str().context("Transition session")?}))?;
    Ok(())
}
pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-print-intent")?;
    owned_config(&fixture.out)?;
    let c = &mut fixture.client;
    ensure!(
        c.call("print_intent_get", json!({}))?["parts"] == json!([]),
        "Use a blank document without manufacturing intent"
    );
    let new = control(c, "New design", None)?;
    attach(c, &new)?;
    for i in 0..5 {
        begin_sketch(c, "XY")?;
        let x = i as f64 * 30.;
        c.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+20.,"y":12.},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}))?;
    }
    let scene = c.call("solid_scene", json!({}))?;
    let ids: Vec<Value> = scene["bodies"]
        .as_array()
        .context("Bodies")?
        .iter()
        .map(|b| b["id"].clone())
        .collect();
    ensure!(ids.len() == 5, "Fixture requires five source solids");
    let component=c.call("assembly_create_component",json!({"name":"Housing print group","body_ids":[ids[0],ids[1]],"absorb_promoted_bodies":true}))?;
    c.call("assembly_create_occurrence",json!({"component_id":component["id"],"name":"Intentional repeated housing","local_pose":{"translation":[0.,35.,0.],"rotation":[0.,0.,0.,1.]}}))?;
    let mut geometry = model(c)?;
    geometry
        .as_object_mut()
        .context("Model object")?
        .remove("print_intent");
    control(c, "Named Views", None)?;
    view(
        c,
        "CAD occurrence or body",
        Some(&format!("body:{}", ids[0])),
    )?;
    view(c, "Part print settings", None)?;
    print(c, "Settings scope", Some("project"))?;
    print(c, "Requested walls", Some("2"))?;
    print(c, "Requested infill (%)", Some("15"))?;
    print(c, "Apply print settings", None)?;
    print(c, "Settings scope", Some("part"))?;
    for (label, value) in [
        ("Requested walls", "6"),
        ("Requested infill (%)", "30"),
        ("Requested infill pattern", "gyroid"),
        ("Requested top shell layers", "6"),
        ("Requested bottom shell layers", "6"),
        ("Print preset name", "PETG housing"),
    ] {
        print(c, label, Some(value))?;
    }
    print(c, "Save print preset", None)?;
    print(c, "Apply print settings", None)?;
    let first = c.call("print_intent_get", json!({}))?;
    ensure!(
        part(&first, &ids[0])?["wall_count"] == 6,
        "Housing walls not persisted"
    );
    ensure!(
        first["defaults"]["wall_count"] == 2
            && first["defaults"]["infill_density_percent"].as_f64() == Some(15.),
        "Project defaults missing"
    );
    let invalid = print(c, "Requested walls", Some("1.5"))?;
    ensure!(
        invalid["value"]["valid"] == false,
        "Fractional wall count accepted"
    );
    let rejected = c.call(
        "cad_interface",
        json!({"action":"file","command":"save","path":fixture.out.join("unapplied.limo")}),
    );
    ensure!(
        rejected.is_err() || rejected.is_ok_and(|v| v["status"] == "failed"),
        "Unapplied draft allowed File Save"
    );
    ensure!(
        !fixture.out.join("unapplied.limo").exists(),
        "Rejected draft wrote a file"
    );
    let before_replacement = c.call("cad_project_model", json!({}))?;
    for (operation, arguments) in [
        ("cad_new_project", json!({})),
        (
            "cad_load_project_model",
            json!({"model_json":before_replacement}),
        ),
    ] {
        let rejected = c.call(operation, arguments);
        ensure!(
            rejected.is_err(),
            "Dirty print draft allowed incoming {operation}"
        );
        ensure!(
            c.call("cad_project_model", json!({}))? == before_replacement,
            "Rejected {operation} changed the document"
        );
        let state = ui(c, json!({"action":"inspect"}))?;
        ensure!(
            controls(&state).any(|v| v["surface"] == "body/print-intent"),
            "Rejected {operation} discarded the Print Settings card"
        );
    }
    ensure!(
        c.call(
            "print_intent_set_part",
            json!({"body_id":ids[0],
        "settings":{"wall_count":9},"expected_model_json":"{}"})
        )
        .is_err(),
        "Incoming print write bypassed the exact model precondition"
    );
    ensure!(
        c.call("cad_project_model", json!({}))? == before_replacement,
        "Rejected stale print write changed the document"
    );
    print(c, "Discard print settings draft", None)?;
    print(c, "Print settings part", Some(&ids[1].to_string()))?;
    print(c, "Print preset", Some("PETG housing"))?;
    print(c, "Apply print settings", None)?;
    print(c, "Print settings part", Some(&ids[2].to_string()))?;
    print(c, "Print preset", Some("PETG housing"))?;
    print(c, "Requested infill (%)", Some("40"))?;
    print(c, "Apply print settings", None)?;
    print(c, "Print settings part", Some(&ids[3].to_string()))?;
    print(c, "Requested walls", Some("6"))?;
    print(c, "Requested infill (%)", Some("100"))?;
    print(c, "Apply print settings", None)?;
    print(c, "Print settings part", Some(&ids[4].to_string()))?;
    print(c, "Copy requests from part", Some(&ids[0].to_string()))?;
    print(c, "Copy part requests", None)?;
    ensure!(
        part(&c.call("print_intent_get", json!({}))?, &ids[4])?["wall_count"] == 6,
        "Copy did not use source definition"
    );
    print(c, "Reset print settings to inheritance", None)?;
    control(c, "Undo", None)?;
    ensure!(
        part(&c.call("print_intent_get", json!({}))?, &ids[4])?["wall_count"] == 6,
        "Undo did not restore copied requests"
    );
    control(c, "Redo", None)?;
    let effective = c.call(
        "print_intent_effective",
        json!({"body_ids":[ids[4]],"target":"bambu_studio"}),
    )?;
    ensure!(
        effective["parts"][0]["settings"]["wall_count"] == 2
            && effective["parts"][0]["settings"]["infill_density_percent"].as_f64() == Some(15.),
        "Sleeve failed project inheritance"
    );
    ensure!(
        effective["parts"][0]["sources"]["wall_count"] == "project_default",
        "Inheritance source missing"
    );
    print(c, "Requested walls", Some("0"))?;
    print(c, "Apply print settings", None)?;
    ensure!(
        part(&c.call("print_intent_get", json!({}))?, &ids[4])?["wall_count"] == 0,
        "Explicit zero became inheritance"
    );
    print(c, "Reset print settings to inheritance", None)?;
    print(c, "Print capability target", Some("portable"))?;
    capture(c, &fixture.out, "print-intent-portable-capabilities")?;
    print(c, "Print capability target", Some("bambu_studio"))?;
    print(c, "Close print settings", None)?;
    view(c, "Close named views", None)?;
    let intent = c.call("print_intent_get", json!({}))?;
    ensure!(
        part(&intent, &ids[1])? == part(&intent, &ids[0])?,
        "Housing preset changed requests"
    );
    ensure!(
        part(&intent, &ids[2])?["infill_density_percent"].as_f64() == Some(40.)
            && part(&intent, &ids[3])?["infill_density_percent"].as_f64() == Some(100.),
        "Auger/adapter requests lost"
    );
    let expected = model(c)?;
    let mut comparable = expected.clone();
    comparable
        .as_object_mut()
        .context("Model object")?
        .remove("print_intent");
    ensure!(
        comparable == geometry,
        "Print metadata changed source geometry or assembly intent"
    );
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let archived = crate::project_archive::model(&fs::read(&fixture.project)?)?;
    ensure!(
        serde_json::from_str::<Value>(&archived)? == expected,
        "Archive did not persist print intent"
    );
    let closed = ui(c, json!({"action":"file","command":"close"}))?;
    attach(c, &closed)?;
    let opened = ui(
        c,
        json!({"action":"file","command":"open","path":fixture.project}),
    )?;
    attach(c, &opened)?;
    ensure!(
        c.call("print_intent_get", json!({}))? == intent,
        "Reopen changed print requests"
    );
    ensure!(model(c)? == expected, "Reopen changed complete model");
    let mut command = Command::new(&fixture.server);
    command.arg("--headless");
    let mut cold = Client::start_command(command, Some(Duration::from_secs(45)))?;
    cold.call("cad_load_project_model", json!({"model_json":archived}))?;
    ensure!(
        cold.call("print_intent_get", json!({}))? == intent,
        "Independent cold engine changed saved manufacturing intent"
    );
    ensure!(
        model(&mut cold)? == expected,
        "Independent cold recomputation changed complete model"
    );
    ensure!(
        cold.call(
            "print_intent_effective",
            json!({"body_ids":[ids[4]],"target":"bambu_studio"})
        )? == effective,
        "Cold recomputation changed inherited values or sources"
    );
    cold.finish(Duration::from_secs(10))?;
    capture(c, &fixture.out, "print-intent-reopened")?;
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"passed":true,"body_ids":ids,"intent":intent,"sleeve_effective":effective,"project":fixture.project,"cold_recompute":true,"evidence":"CAD requested settings and persistent UI controls; slicer slicing is qualified separately","not_proven":["physical print strength","physical keyboard","OS file chooser"]}),
        )?,
    )?;
    println!("PASS native Print Settings requested values/inheritance/preset/copy/reset/zero, invalid draft guards, Undo/Redo and Save/reopen: {}",fixture.report.display());
    Ok(())
}
