//! Disposable live fixture over an explicitly supplied blank native document.
//! Does not drive the OS chooser or claim physical keyboard/print validation.
use crate::native_fixture::{begin_sketch, capture, control, controls, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::fs;

fn model(c: &mut Client) -> Result<Value> {
    serde_json::from_str(
        c.call("cad_project_model", json!({}))?
            .as_str()
            .context("Model JSON")?,
    )
    .map_err(Into::into)
}
fn rows(text: &str) -> Result<Vec<Vec<(&str, &str)>>> {
    let lines: Vec<_> = text.lines().collect();
    ensure!(lines.len() % 2 == 0, "DXF has incomplete pairs");
    let pairs: Vec<_> = lines
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| (p[0], p[1]))
        .collect();
    let start = pairs
        .iter()
        .position(|p| *p == ("2", "ENTITIES"))
        .context("DXF entities")?;
    let mut rows: Vec<Vec<(&str, &str)>> = Vec::new();
    for &pair in &pairs[start + 1..] {
        if pair == ("0", "ENDSEC") {
            break;
        }
        if pair.0 == "0" {
            rows.push(Vec::new());
        }
        rows.last_mut().context("DXF entity start")?.push(pair);
    }
    Ok(rows)
}
fn value<'a>(row: &[(&str, &'a str)], code: &str) -> Result<&'a str> {
    row.iter()
        .find(|p| p.0 == code)
        .map(|p| p.1)
        .context("DXF field")
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut f = start(args, "native-profile-export")?;
    let c = &mut f.client;
    begin_sketch(c, "YZ")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":-20.,"y":10.},"p2":{"x":40.,"y":50.},"ctrl_held":true}),
    )?;
    c.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":10.,"y":30.},"p2":{"x":14.,"y":30.},"ctrl_held":true}))?;
    control(c, "Finish sketch", None)?;
    c.call(
        "solid_extrude",
        json!({"sketch_name":"Sketch1","profile_indices":[0],
            "extent":{"type":"distance","distance":6.}}),
    )?;
    let document = c.call("cad_document", json!({}))?;
    let extrude_id = document["features"]
        .as_array()
        .context("Features missing")?
        .iter()
        .find(|feature| feature["kind"] == "extrude")
        .context("Extrude feature missing")?["id"]
        .clone();
    ui(
        c,
        json!({"action":"file","command":"rename","name":"Profile export history check"}),
    )?;
    ui(
        c,
        json!({"action":"file","command":"save","path":f.project}),
    )?;
    let original = model(c)?;
    c.call(
        "solid_edit_extrude",
        json!({"feature_id":extrude_id,"extrude":{
            "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":8.},"taper_angle_deg":0.,
            "flip":false,"target_body_ids":[],
        }}),
    )?;
    let baseline = model(c)?;
    ensure!(
        baseline != original,
        "History fixture did not edit the solid"
    );
    let catalog = c.call("sketch_profiles", json!({}))?;
    let sketches = catalog.as_array().context("Profile catalog")?;
    ensure!(sketches.len() == 1, "Expected one real machining sketch");
    let sketch = &sketches[0];
    let profiles = sketch["profiles"].as_array().context("Profiles")?;
    ensure!(
        profiles.len() == 2,
        "Expected one outer region and one hole"
    );
    let outer = profiles
        .iter()
        .find(|p| p["nesting_depth"] == 0)
        .context("Material region")?;
    let hole = profiles
        .iter()
        .find(|p| p["nesting_depth"] == 1)
        .context("Hole wire")?;
    ensure!(
        hole["parent_index"] == outer["index"],
        "Wrong hole association"
    );
    control(c, "File", None)?;
    control(c, "Export 1:1 Manufacturing Profile DXF…", None)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let choice = controls(&state)
        .find(|v| v["label"] == "Manufacturing profile")
        .context("Visible profile selector")?;
    let key = format!("{}:{}", sketch["feature_id"], outer["index"]);
    ensure!(
        choice["options"]
            .as_array()
            .is_some_and(|v| v.len() == 1 && v[0]["value"] == key),
        "Picker includes hole wires or loses sketch identity"
    );
    control(c, "Manufacturing profile", Some(&key))?;
    capture(c, &f.out, "profile-selector")?;
    control(c, "Cancel", None)?;
    ensure!(
        model(c)? == baseline,
        "Selecting/cancelling profile export changes model"
    );
    let output = f.out.join("machining-profile.dxf");
    let command = json!({"action":"file","command":"export_profile_dxf","feature_id":sketch["feature_id"],"profile_index":outer["index"],"path":output});
    ui(c, command.clone())?;
    let text = fs::read_to_string(&output)?;
    ensure!(
        text.contains("$INSUNITS\n70\n4") || text.contains("$INSUNITS\r\n70\r\n4"),
        "Profile must remain millimetres"
    );
    let entities = rows(&text)?;
    ensure!(
        entities.len() == 5,
        "Expected four analytic lines and one exact circle"
    );
    let circle = entities
        .iter()
        .find(|r| value(r, "0").ok() == Some("CIRCLE"))
        .context("Analytic hole circle")?;
    ensure!(value(circle, "8")? == "PROFILE_HOLES", "Hole layer missing");
    for (code, expected) in [("10", 10.), ("20", 30.), ("40", 4.)] {
        ensure!(
            (value(circle, code)?.parse::<f64>()? - expected).abs() < 1e-8,
            "Wrong local circle coordinate/radius"
        );
    }
    let lines: Vec<_> = entities
        .iter()
        .filter(|r| value(r, "0").ok() == Some("LINE"))
        .collect();
    ensure!(lines.len() == 4, "Outer rectangle lost exact lines");
    for (codes, min, max) in [(["10", "11"], -20., 40.), (["20", "21"], 10., 50.)] {
        let mut values = Vec::new();
        for row in &lines {
            for code in codes {
                values.push(value(row, code)?.parse::<f64>()?);
            }
        }
        ensure!(
            values.iter().copied().fold(f64::INFINITY, f64::min) == min
                && values.iter().copied().fold(f64::NEG_INFINITY, f64::max) == max,
            "Export applied a paper/placement scale"
        );
    }
    ensure!(
        c.call("cad_interface", command).is_err(),
        "Unconfirmed overwrite succeeded"
    );
    ensure!(
        fs::read_to_string(&output)? == text,
        "Failed overwrite damaged reviewed output"
    );
    ensure!(c.call("cad_interface",json!({"action":"file","command":"export_profile_dxf","feature_id":sketch["feature_id"],"profile_index":hole["index"],"path":output,"overwrite":true})).is_err(),"Hole wire accepted as material");
    ensure!(
        fs::read_to_string(&output)? == text && model(c)? == baseline,
        "Rejected profile output changed intent/output"
    );
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == original,
        "Export inserted a history edit or renamed source"
    );
    control(c, "Redo", None)?;
    ensure!(
        model(c)? == baseline,
        "Export damaged existing redo history"
    );
    control(c, "Save", None)?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&f.project)?)?;
    let saved: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        saved == baseline,
        "Export replaced the project save destination or saved intent"
    );
    fs::write(
        &f.report,
        serde_json::to_vec_pretty(
            &json!({"status":"passed","session":f.session,"profile":key,"catalog":catalog,"path":output,"bytes":text.len(),"units":"mm","scale":1,"project":f.project,"exact_model_archive":true,"not_proven":["OS save picker","Physical keyboard","Print dialog"]}),
        )?,
    )?;
    println!("PASS native profile selection, exact local DXF, cancellation, overwrite rejection and unchanged project: {}",f.report.display());
    Ok(())
}
