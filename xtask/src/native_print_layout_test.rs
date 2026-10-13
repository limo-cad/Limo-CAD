//! Live Bevy named-view and manufacturing handoff over an owned blank document.
use crate::{
    native_fixture::{begin_sketch, capture, control, controls, owned_config, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn field(client: &mut Client, label: &str, value: Option<&str>) -> Result<Value> {
    for pass in 0..2 {
        if pass == 1 {
            for _ in 0..64 {
                let state = ui(client, json!({"action":"inspect"}))?;
                if !controls(&state)
                    .any(|c| c["label"] == "Previous named-view fields" && c["disabled"] == false)
                {
                    break;
                }
                control(client, "Previous named-view fields", None)?;
            }
        }
        for _ in 0..64 {
            let state = ui(client, json!({"action":"inspect"}))?;
            let found: Vec<_> = controls(&state)
                .filter(|c| {
                    c["surface"] == "document/views"
                        && c["disabled"] == false
                        && c["label"]
                            .as_str()
                            .is_some_and(|s| s == label || s.starts_with(&format!("{label} (")))
                })
                .collect();
            ensure!(found.len() <= 1, "Ambiguous named-view control {label}");
            if let Some(found) = found.first() {
                return control(
                    client,
                    found["label"].as_str().context("Control label")?,
                    value,
                );
            }
            if !controls(&state)
                .any(|c| c["label"] == "More named-view fields" && c["disabled"] == false)
            {
                break;
            }
            control(client, "More named-view fields", None)?;
        }
    }
    anyhow::bail!("Missing enabled named-view control {label}")
}

fn model(client: &mut Client) -> Result<Value> {
    let raw = client.call("cad_project_model", json!({}))?;
    serde_json::from_str(raw.as_str().context("Project model text")?).map_err(Into::into)
}

fn saved(client: &mut Client, name: &str) -> Result<Value> {
    let views = client.call("named_views", json!({}))?;
    views["views"]
        .as_array()
        .context("Named views")?
        .iter()
        .find(|v| v["name"] == name)
        .cloned()
        .context("Saved view missing")
}

fn rejected(client: &mut Client, request: Value, path: &Path) -> Result<()> {
    let before = model(client)?;
    let result = client.call("cad_interface", request);
    ensure!(
        result.is_err() || result.is_ok_and(|v| v["status"] == "failed"),
        "Unsafe export unexpectedly accepted"
    );
    ensure!(!path.exists(), "Rejected export wrote output");
    ensure!(
        model(client)? == before,
        "Rejected export changed the model"
    );
    Ok(())
}

fn export(client: &mut Client, path: &Path, allow: bool) -> Result<Value> {
    ui(
        client,
        json!({"action":"file","command":"export_3mf","path":path,
        "scope":"assembly","allow_layout_issues":allow,"slicer_target":"bambu_studio"}),
    )
}

fn bounds(vertices: &[[f64; 3]]) -> [[f64; 3]; 2] {
    [
        std::array::from_fn(|i| vertices.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min)),
        std::array::from_fn(|i| {
            vertices
                .iter()
                .map(|p| p[i])
                .fold(f64::NEG_INFINITY, f64::max)
        }),
    ]
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-print-layout")?;
    owned_config(&fixture.out)?;
    let c = &mut fixture.client;
    ensure!(
        c.call("named_views", json!({}))?["views"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Choose a blank document without existing named-view intent"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":10.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    let source = c.call("solid_scene", json!({}))?;
    let body = source["bodies"][0]["id"].clone();
    let component = c.call(
        "assembly_create_component",
        json!({"name":"Printed bracket","body_ids":[body],"absorb_promoted_bodies":true}),
    )?;
    let assembly = c.call("assembly_document", json!({}))?;
    let root = assembly["component_structure"]["occurrences"]
        .as_array()
        .context("Occurrences")?
        .iter()
        .find(|o| o["component_id"] == component["id"])
        .context("Root occurrence")?["id"]
        .clone();
    c.call("assembly_create_occurrence",json!({"component_id":component["id"],"name":"Intentional repeated bracket",
        "parent_occurrence_id":root,"local_pose":{"translation":[40.,0.,0.],"rotation":[0.,0.,0.,1.]}}))?;
    let assembled = c.call("assembly_document", json!({}))?;
    let original = model(c)?;
    ui(
        c,
        json!({"action":"view","view":"isometric","fit":true,"duration_ms":0}),
    )?;
    let catalog = c.call("printer_catalog", json!({}))?;
    let profile = &catalog["profiles"][0];
    let printer = format!("{}:dual", profile["id"].as_str().context("Printer ID")?);
    control(c, "Named Views", None)?;
    field(c, "Create named view", None)?;
    field(c, "View name", Some("Bracket print"))?;
    field(c, "Capture current camera and visibility", None)?;
    field(c, "View purpose", Some("true"))?;
    field(c, "Printer bed", Some(&printer))?;
    field(
        c,
        "CAD occurrence or body",
        Some(&format!("occurrence:{root}")),
    )?;
    for (label, value) in [
        ("View offset X", "5 mm"),
        ("View offset Y", "5 mm"),
        ("View offset Z", "-2 mm"),
        ("View rotation Z", "90 deg"),
    ] {
        field(c, label, Some(value))?;
    }
    field(c, "Preview draft", None)?;
    ensure!(
        model(c)? == original,
        "Draft preview changed persisted intent"
    );
    capture(c, &fixture.out, "draft-layout")?;
    let draft_output = fixture.out.join("must-not-export-preview.3mf");
    rejected(
        c,
        json!({"action":"file","command":"export_3mf","scope":"assembly","path":draft_output,"allow_layout_issues":true}),
        &draft_output,
    )?;
    let checked = field(c, "Check print layout", None)?;
    let report = checked["value"]["report"].clone();
    ensure!(
        report["printable_instances"] == 2
            && report["printable_groups"] == 1
            && report["excluded_instances"] == 0,
        "Hierarchy/repeats lost: {report}"
    );
    ensure!(
        report["issues"]
            .as_array()
            .is_some_and(|v| v.iter().any(|i| i["code"] == "below_bed")
                && v.iter().any(|i| i["code"] == "outside_bed")),
        "Expected bed diagnostics: {report}"
    );
    ensure!(
        report["proposal_fits"] == true
            && report["proposed_translations"]
                .as_array()
                .is_some_and(|v| v.len() == 1),
        "Whole group correction unavailable: {report}"
    );
    field(c, "Save view", None)?;
    let first = saved(c, "Bracket print")?;
    ensure!(
        first["print_layout"] == true && first["print_bed"] == profile["dual"],
        "Purpose/profile not retained"
    );
    field(c, "Recall saved view", None)?;
    ensure!(
        c.call("named_views", json!({}))?["active"] == "Bracket print",
        "Recall did not activate view"
    );
    let warning_model = model(c)?;
    let blocked = fixture.out.join("must-not-export-warning.3mf");
    rejected(
        c,
        json!({"action":"file","command":"export_3mf","scope":"assembly","path":blocked}),
        &blocked,
    )?;
    control(c, "File", None)?;
    control(c, "Export All Bodies as 3MF…", None)?;
    control(c, "3MF view", Some("saved:Bracket print"))?;
    control(c, "Printer bed for layout checks", Some(&printer))?;
    control(c, "Export despite layout issues", None)?;
    let allowed = ui(c, json!({"action":"inspect"}))?;
    ensure!(
        controls(&allowed)
            .any(|v| v["label"] == "Export despite layout issues" && v["selected"] == true),
        "Deliberate issue acknowledgement did not update the retained control"
    );
    capture(c, &fixture.out, "deliberate-export-dialog")?;
    control(c, "Cancel", None)?;
    let warning_output = fixture.out.join("deliberate-warning.3mf");
    export(c, &warning_output, true)?;
    ensure!(
        model(c)? == warning_model,
        "Deliberate export changed intent"
    );
    let decoded = limo_cad_export::test_reader::read_package(&fs::read(&warning_output)?)
        .map_err(anyhow::Error::msg)?;
    ensure!(
        decoded.len() == 2 && decoded.iter().all(|m| m.build_item == 0),
        "Actual 3MF lost multipart repeated instances"
    );
    let mut actual: Vec<_> = decoded.iter().map(|m| bounds(&m.vertices)).collect();
    for expected in [
        [[-5., 5., -2.], [5., 25., 8.]],
        [[-5., 45., -2.], [5., 65., 8.]],
    ] {
        let index = actual
            .iter()
            .position(|a| (0..2).all(|j| (0..3).all(|i| (a[j][i] - expected[j][i]).abs() < 1e-4)))
            .context("Actual ZIP coordinates differ from the chosen translated/rotated layout")?;
        actual.remove(index);
    }
    ensure!(actual.is_empty(), "Unexpected print instances");
    field(c, "Check print layout", None)?;
    field(c, "Apply proposed group corrections to draft", None)?;
    ensure!(
        saved(c, "Bracket print")? == first && model(c)? == warning_model,
        "Corrections modified saved view before Save"
    );
    field(c, "Check print layout", None)?;
    field(c, "Save view", None)?;
    let corrected = saved(c, "Bracket print")?;
    ensure!(
        corrected["occurrence_offsets"][0]["rotation"]
            == first["occurrence_offsets"][0]["rotation"],
        "Corrections changed saved rotation"
    );
    field(c, "Recall saved view", None)?;
    let corrected_report = field(c, "Check print layout", None)?["value"]["report"].clone();
    ensure!(
        corrected_report["issues"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Corrected multipart layout remains invalid: {corrected_report}"
    );
    let corrected_output = fixture.out.join("corrected.3mf");
    export(c, &corrected_output, false)?;
    let corrected_meshes =
        limo_cad_export::test_reader::read_package(&fs::read(&corrected_output)?)
            .map_err(anyhow::Error::msg)?;
    ensure!(
        corrected_meshes.len() == 2 && corrected_meshes.iter().all(|m| m.build_item == 0),
        "Corrections changed repeated multipart grouping"
    );
    let mut corrected_bounds: Vec<_> = corrected_meshes
        .iter()
        .map(|m| bounds(&m.vertices))
        .collect();
    corrected_bounds.sort_by(|a, b| a[0][1].total_cmp(&b[0][1]));
    for expected in [[20.5, 0., 0.], [20.5, 40., 0.]]
        .into_iter()
        .zip(&corrected_bounds)
    {
        ensure!((0..3).all(|i|(expected.0[i]-expected.1[0][i]).abs()<1e-4),
            "Correction changed relative alignment or usable dual-nozzle origin: {corrected_bounds:?}");
        ensure!(
            (0..3)
                .all(|i| ((expected.1[1][i] - expected.1[0][i]) - [10., 20., 10.][i]).abs() < 1e-4),
            "Correction changed rotation or part size"
        );
    }
    field(c, "View purpose", Some("false"))?;
    field(c, "Save view", None)?;
    field(c, "Recall saved view", None)?;
    let presentation_output = fixture.out.join("presentation-view.3mf");
    export(c, &presentation_output, false)?;
    ensure!(
        fs::read(&presentation_output)? == fs::read(&corrected_output)?,
        "Print-purpose opt-in changes export geometry"
    );
    field(c, "View name", Some("Reviewed bracket"))?;
    field(c, "Rename saved view", None)?;
    let renamed = saved(c, "Reviewed bracket")?;
    ensure!(
        renamed["occurrence_offsets"] == corrected["occurrence_offsets"],
        "Rename lost layout placement"
    );
    field(c, "Reset to assembled placement", None)?;
    ensure!(
        c.call("named_views", json!({}))?["active"].is_null(),
        "Reset retained active layout"
    );
    field(c, "Create named view", None)?;
    field(c, "View name", Some("Disposable view"))?;
    field(c, "Save view", None)?;
    field(c, "Delete saved view", None)?;
    let views = c.call("named_views", json!({}))?;
    ensure!(
        views["views"]
            .as_array()
            .is_some_and(|v| v.len() == 1 && v[0]["name"] == "Reviewed bracket"),
        "Delete affected another view"
    );
    ensure!(
        c.call("solid_scene", json!({}))? == source
            && c.call("assembly_document", json!({}))? == assembled,
        "View workflow changed authored solid/assembly intent"
    );
    control(c, "Close named views", None)?;
    control(c, "Isometric", None)?;
    ui(c, json!({"action":"capture","path":fixture.capture}))?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let expected_model = model(c)?;
    let archived = crate::project_archive::model(&fs::read(&fixture.project)?)?;
    ensure!(
        serde_json::from_str::<Value>(&archived)? == expected_model,
        "File save lost view intent"
    );
    let closed = ui(c, json!({"action":"file","command":"close"}))?;
    c.call(
        "cad_attach",
        json!({"session_id":closed["active_session_id"].as_str().context("Closed session")?}),
    )?;
    let opened = ui(
        c,
        json!({"action":"file","command":"open","path":fixture.project}),
    )?;
    c.call(
        "cad_attach",
        json!({"session_id":opened["active_session_id"].as_str().context("Reopened session")?}),
    )?;
    ensure!(
        model(c)? == expected_model && saved(c, "Reviewed bracket")? == renamed,
        "Save/reopen changed named layout"
    );
    capture(c, &fixture.out, "reopened-layout")?;
    fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(&json!({"passed":true,"project":fixture.project,
        "printer":printer,"initial_diagnostics":report,"corrected_diagnostics":corrected_report,
        "source_instances":2,"multipart_groups":1,"native_reopen":true,
        "exports":[warning_output,corrected_output,presentation_output],"not_proven":["OS file chooser","physical keyboard"]}))?,
    )?;
    println!("PASS native named-view capture/preview/save/recall, printer layout checks, draft corrections, deliberate export guards, repeated multipart ZIP coordinates, presentation export, rename/delete/reset and Save/reopen: {}",fixture.report.display());
    Ok(())
}
