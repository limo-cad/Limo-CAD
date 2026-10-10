//! Exercise the same published annotation controls on the established real
//! OCCT, 24-annotation fixture. This does not substitute MCP for OS drag proof.
use crate::native_fixture::{capture, control, controls, panel_field, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
mod centers;
mod chamfer;
mod cloud;
mod curved;
mod desktop;
mod hole;
mod presentation;
mod series;
mod straight;
mod tables;
pub(super) use chamfer::exercise_blank as exercise_chamfer;
pub(super) use cloud::exercise_blank as exercise_cloud;
pub(super) use desktop::exercise as exercise_desktop;
pub(super) use hole::exercise_blank as exercise_hole;

fn model(c: &mut Client) -> Result<Value> {
    let model = c.call("cad_project_model", json!({}))?;
    serde_json::from_str(model.as_str().context("Model JSON")?).map_err(Into::into)
}
fn field(c: &mut Client, label: &str, value: &str) -> Result<Value> {
    panel_field(c, label, Some(value), "Previous fields", "More fields")
}
fn history(c: &mut Client, before: &Value, after: &Value) -> Result<()> {
    control(c, "Undo", None)?;
    ensure!(
        &model(c)? == before,
        "Annotation Undo did not restore the exact model"
    );
    control(c, "Redo", None)?;
    ensure!(
        &model(c)? == after,
        "Annotation Redo did not restore the exact model"
    );
    Ok(())
}
fn annotations(model: &Value) -> Result<&Vec<Value>> {
    let active = &model["drawings"]["active_sheet_id"];
    model["drawings"]["sheets"]
        .as_array()
        .context("Drawing sheets")?
        .iter()
        .find(|s| &s["id"] == active)
        .context("Active sheet")?["annotations"]
        .as_array()
        .context("Annotations")
}
fn exact_one_added(before: &Value, after: &Value, out: &Path, stage: &str) -> Result<Value> {
    std::fs::write(
        out.join(format!("{stage}-before.json")),
        serde_json::to_vec_pretty(before)?,
    )?;
    std::fs::write(
        out.join(format!("{stage}-after.json")),
        serde_json::to_vec_pretty(after)?,
    )?;
    let old = annotations(before)?;
    let new = annotations(after)?;
    let changed = old.iter().zip(new).enumerate().find(|(_, (a, b))| a != b);
    ensure!(
        new.len() == old.len() + 1 && changed.is_none(),
        "{stage}: expected one new annotation with exact saved records; before count {}, after count {}, first changed record {changed:?}. Full models saved under {}",
        old.len(),
        new.len(),
        out.display()
    );
    let created = new.last().unwrap().clone();
    let mut expected = before.clone();
    let active = before["drawings"]["active_sheet_id"].clone();
    let sheet = expected["drawings"]["sheets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["id"] == active)
        .unwrap();
    sheet["annotations"]
        .as_array_mut()
        .unwrap()
        .push(created.clone());
    if sheet["release"]["status"] == "released" {
        sheet["release"]["status"] = json!("draft");
    }
    expected["drawings"]["next_annotation_id"] =
        json!(before["drawings"]["next_annotation_id"].as_u64().unwrap() + 1);
    ensure!(
        &expected == after,
        "{stage}: annotation creation changed unrelated model intent; exact models saved under {}",
        out.display()
    );
    Ok(created)
}
fn replace_expected(before: &Value, id: u64, change: impl FnOnce(&mut Value)) -> Value {
    let mut expected = before.clone();
    let active = before["drawings"]["active_sheet_id"].clone();
    let sheet = expected["drawings"]["sheets"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["id"] == active)
        .unwrap();
    let row = sheet["annotations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["id"] == id)
        .unwrap();
    change(row);
    if sheet["release"]["status"] == "released" {
        sheet["release"]["status"] = json!("draft");
    }
    expected
}
fn pair(c: &mut Client) -> Result<[String; 2]> {
    let state = ui(c, json!({"action":"inspect"}))?;
    let anchors: Vec<_> = controls(&state)
        .filter(|c| c["surface"] == "drawing/anchors" && c["disabled"] == false)
        .collect();
    let xy = |c: &Value| -> Result<[f64; 2]> {
        Ok([
            c["bounds"]["x"].as_f64().context("Anchor x")?
                + c["bounds"]["width"].as_f64().context("Anchor width")? / 2.,
            c["bounds"]["y"].as_f64().context("Anchor y")?
                + c["bounds"]["height"].as_f64().context("Anchor height")? / 2.,
        ])
    };
    let view = |c: &Value| {
        c["label"]
            .as_str()
            .unwrap_or("")
            .split(" anchor ")
            .next()
            .unwrap_or("")
            .to_owned()
    };
    let first_view = anchors
        .first()
        .context("No actual projected anchor controls")?;
    for a in &anchors {
        for b in &anchors {
            if view(a) != view(first_view) || view(a) != view(b) {
                continue;
            }
            let p = xy(a)?;
            let q = xy(b)?;
            if (p[1] - q[1]).abs() < 0.02 && q[0] - p[0] > 60. {
                return Ok([
                    a["label"].as_str().unwrap().into(),
                    b["label"].as_str().unwrap().into(),
                ]);
            }
        }
    }
    anyhow::bail!("The real top view has no visible horizontal anchor pair: {anchors:?}")
}

pub(super) fn exercise(c: &mut Client, out: &Path, server: &str) -> Result<Value> {
    let baseline = model(c)?;
    if std::env::var("LIMO_CAD_NATIVE_CENTERS_ONLY").as_deref() == Ok("1") {
        let mut result = centers::exercise(c, out, &baseline)?;
        if std::env::var("LIMO_CAD_NATIVE_CENTERS_INPUT").as_deref() == Ok("1") {
            result["physical"] = desktop::exercise_centers(c, out, server, &baseline)?;
            result["not_proven"] = result["physical"]["not_proven"].clone();
        }
        return Ok(result);
    }
    let note_text = "Caf\u{e9} \u{96f6}\u{4ef6}\nNative note";
    control(c, "Note", None)?;
    field(c, "Note text", note_text)?;
    field(c, "Paper X (mm)", "110")?;
    field(c, "Paper Y (mm)", "22")?;
    ensure!(
        model(c)? == baseline,
        "A pending note mutated the shared model before placement"
    );
    capture(c, out, "author-note-placement")?;
    control(c, "Place note", None)?;
    let created = model(c)?;
    let note = exact_one_added(&baseline, &created, out, "author-note-created")?;
    ensure!(
        note["kind"] == "note"
            && note["text"] == note_text
            && note["position"] == json!([110., 22.]),
        "Native note placement lost text or paper position"
    );
    let id = note["id"].as_u64().unwrap();
    history(c, &baseline, &created)?;
    control(c, &format!("Edit annotation {id}"), None)?;
    field(c, "Note text", "Edited \u{96f6}\u{4ef6}\nSecond line")?;
    field(c, "Paper X (mm)", "118")?;
    field(c, "Paper Y (mm)", "26")?;
    ensure!(
        model(c)? == created,
        "Editing note fields committed before Apply"
    );
    control(c, "Apply annotation", None)?;
    let edited = model(c)?;
    let expected = replace_expected(&created, id, |a| {
        a["text"] = json!("Edited \u{96f6}\u{4ef6}\nSecond line");
        a["position"] = json!([118., 26.]);
    });
    ensure!(
        edited == expected,
        "Note edit changed unrelated drawing intent"
    );
    history(c, &created, &edited)?;
    control(c, &format!("Edit annotation {id}"), None)?;
    capture(c, out, "author-note-edited")?;
    control(c, "Delete annotation", None)?;
    let deleted = model(c)?;
    ensure!(
        !annotations(&deleted)?.iter().any(|a| a["id"] == id),
        "Delete retained the note"
    );
    history(c, &edited, &deleted)?;
    control(c, "Undo", None)?;
    control(c, "Undo", None)?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == baseline,
        "Note editing did not return to the exact 24-annotation baseline"
    );
    control(c, "Linear dimension", None)?;
    let labels = pair(c)?;
    capture(c, out, "author-linear-anchors")?;
    control(c, &labels[0], None)?;
    ensure!(model(c)? == baseline, "First anchor mutated the drawing");
    control(c, &labels[0], None)?;
    ensure!(
        model(c)? == baseline,
        "Repeated anchor created a zero-span dimension"
    );
    control(c, &labels[1], None)?;
    let dimensioned = model(c)?;
    let dimension = exact_one_added(&baseline, &dimensioned, out, "author-linear-created")?;
    ensure!(
        dimension["kind"] == "linear_dimension"
            && dimension["mode"] == "aligned"
            && dimension["offset"] == 12.,
        "Two projected anchors did not create the shared dimension"
    );
    ensure!(
        dimension["first"]["topology_signature"].is_string()
            && dimension["second"]["topology_signature"].is_string(),
        "Created anchors lost exact topology signatures"
    );
    for side in ["first", "second"] {
        ensure!(
            dimension[side]["fallback_point"][2] == 6.,
            "Native pick used a rear/hidden base endpoint: {}",
            dimension[side]
        );
    }
    history(c, &baseline, &dimensioned)?;
    let dim_id = dimension["id"].as_u64().unwrap();
    control(c, &format!("Edit annotation {dim_id}"), None)?;
    field(c, "Dimension mode", "horizontal")?;
    field(c, "Offset (paper mm)", "20")?;
    field(c, "Precision", "3")?;
    field(c, "Prefix", "LIMIT ")?;
    field(c, "Suffix", " mm")?;
    control(c, "Apply annotation", None)?;
    let edited_dimension = model(c)?;
    let expected = replace_expected(&dimensioned, dim_id, |a| {
        a["mode"] = json!("horizontal");
        a["offset"] = json!(20.);
        a["precision"] = json!(3);
        a["prefix"] = json!("LIMIT ");
        a["suffix"] = json!(" mm");
    });
    ensure!(
        edited_dimension == expected,
        "Dimension edit lost shared anchors/presentation or changed another annotation"
    );
    history(c, &dimensioned, &edited_dimension)?;
    control(c, &format!("Edit annotation {dim_id}"), None)?;
    capture(c, out, "author-linear-edited")?;
    presentation::exercise(c, out, dim_id, &edited_dimension)?;
    control(c, "Delete annotation", None)?;
    let deleted_dimension = model(c)?;
    history(c, &edited_dimension, &deleted_dimension)?;
    control(c, "Undo", None)?;
    control(c, "Undo", None)?;
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == baseline,
        "Native annotation authoring did not restore all 24 saved variants exactly"
    );
    let curved = curved::exercise(c, out, &baseline)?;
    let centers = centers::exercise(c, out, &baseline)?;
    let series = series::exercise(c, out, &baseline)?;
    let straight = straight::exercise(c, out, &baseline)?;
    let tables = tables::exercise(c, out, &baseline)?;
    Ok(
        json!({"center_annotations":centers,"straight_dimensions":straight,"tables":tables,"series_ordinate_dimensions":series,"curved_dimensions":curved,"shared_document_controls_passed":true,"exact_history_passed":true,"dimension_presentation_controls_passed":true,"dimension_presentation_archive_passed":true,"all_24_saved_variants_preserved":true,"frontmost_topology_signatures_preserved":true,
        "captures":["author-note-placement.png","author-note-edited.png","author-linear-anchors.png","author-linear-edited.png","author-presentation-symmetric.png","author-presentation-deviation.png","author-presentation-limits.png","author-presentation-none.png","author-presentation-dual.png"],
        "not_proven":["Actual OS note placement click","Actual OS annotation drag","IME composition in note text","macOS/Linux pointer gestures"]}),
    )
}
