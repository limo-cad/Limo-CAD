//! A disposable real OCCT bevel, separate from manually labeled loaded notes.
use super::*;
use crate::native_fixture::begin_sketch;

fn open(c: &mut Client) -> Result<Value> {
    control(c, "More dimensions", None)?;
    control(c, "Chamfer Note", None)
}
pub(super) fn pick(c: &mut Client) -> Result<String> {
    open(c)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let mut labels: Vec<_> = controls(&state)
        .filter(|r| {
            r["disabled"] == false
                && r["label"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("View 1 chamfer edge "))
        })
        .map(|r| r["label"].as_str().unwrap().to_owned())
        .collect();
    labels.sort();
    labels.dedup();
    ensure!(
        labels.len() == 1,
        "Expected one actual visible bevel target: {labels:?}"
    );
    Ok(labels.remove(0))
}
pub(super) fn check_note(p: &Value, a: &Value) -> Result<()> {
    ensure!(
        a["kind"] == "chamfer_note",
        "Wrong created annotation family"
    );
    curved::endpoint_ref(p, &a["first"])?;
    curved::endpoint_ref(p, &a["second"])?;
    for key in [
        "body_id",
        "edge_id",
        "edge_key",
        "occurrence_id",
        "topology_signature",
    ] {
        ensure!(
            a["first"][key] == a["second"][key],
            "Chamfer endpoint identity differs: {key}"
        );
    }
    ensure!(
        a["first"]["endpoint"] != a["second"]["endpoint"]
            && a["first"]["circle_center"] == false
            && a["second"]["circle_center"] == false,
        "Chamfer needs opposite real endpoints"
    );
    let mut length2 = 0.;
    for i in 0..3 {
        length2 += (a["first"]["fallback_point"][i].as_f64().unwrap()
            - a["second"]["fallback_point"][i].as_f64().unwrap())
        .powi(2);
    }
    ensure!(
        (length2.sqrt() - 8_f64.sqrt()).abs() < 1e-7,
        "Selected edge is not the modeled 2 mm bevel hypotenuse"
    );
    ensure!(
        (a["length"].as_f64().context("Chamfer setback")? - 2.).abs() < 1e-7
            && (a["angle_deg"].as_f64().context("Chamfer angle")? - 45.).abs() < 1e-7,
        "Chamfer note must use setback 2 and angle 45, not hypotenuse"
    );
    Ok(())
}
pub(in super::super) fn exercise_blank(
    c: &mut Client,
    out: &Path,
    server: &str,
    physical: bool,
) -> Result<Value> {
    let incoming = model(c)?;
    ensure!(
        incoming["drawings"]["sheets"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Chamfer fixture requires a blank drawing"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    let solid = c.call("solid_scene", json!({}))?;
    let body = &solid["bodies"][0];
    let edge = body["edges"]
        .as_array()
        .context("Real box edges")?
        .iter()
        .find(|e| {
            e["points"].as_array().is_some_and(|p| {
                p.len() >= 2
                    && p.iter().all(|p| {
                        (p["x"].as_f64().unwrap() - 40.).abs() < 1e-7
                            && (p["y"].as_f64().unwrap() - 30.).abs() < 1e-7
                    })
                    && (p[0]["z"].as_f64().unwrap() - p.last().unwrap()["z"].as_f64().unwrap())
                        .abs()
                        > 9.9
            })
        })
        .context("Exact vertical corner edge")?;
    c.call(
        "solid_chamfer",
        json!({"body_id":body["id"],"edge_ids":[edge["id"]],"distance":2.,"tangent_chain":false}),
    )?;
    let projection = curved::projection(c)?;
    std::fs::write(
        out.join("chamfer-real-projection.json"),
        serde_json::to_vec_pretty(&projection)?,
    )?;
    let mut prepared = model(c)?;
    let mut drawings = prepared["drawings"].clone();
    drawings["sheets"] = json!([
        {"id":1,"name":"Real chamfer","format":"a4","orientation":"landscape","release":{"status":"released","released_revision":"A","released_at":"2026-09-27"},
        "views":[{"id":1,"name":"Top bevel","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[150.,100.],"scale":2.,"show_hidden_lines":false}],
        "annotations":[{"kind":"note","id":1,"text":"Saved Ω note","position":[30.,30.]}]},
        {"id":2,"name":"Preserved sheet","format":"a4","orientation":"landscape","annotations":[{"kind":"note","id":2,"text":"Unrelated intent","position":[40.,30.]}]}]);
    drawings["active_sheet_id"] = json!(1);
    drawings["next_sheet_id"] = json!(3);
    drawings["next_view_id"] = json!(2);
    drawings["next_annotation_id"] = json!(3);
    prepared["drawings"] = drawings;
    c.call(
        "cad_load_project_model",
        json!({"model_json":serde_json::to_string(&prepared)?}),
    )?;
    control(c, "Switch workspace", None)?;
    control(c, "Drawing", None)?;
    control(c, "Fit sheet", None)?;
    let baseline = model(c)?;
    std::fs::write(
        out.join("chamfer-baseline-model.json"),
        serde_json::to_vec_pretty(&baseline)?,
    )?;
    let label = pick(c)?;
    capture(c, out, "chamfer-candidates")?;
    control(c, &label, None)?;
    ensure!(
        model(c)? == baseline,
        "Chamfer selection committed before placement"
    );
    capture(c, out, "chamfer-staged")?;
    control(c, "Reset annotation", None)?;
    ensure!(
        model(c)? == baseline,
        "Reset consumed annotation ID or changed document"
    );
    control(c, &label, None)?;
    control(c, "Place note", None)?;
    let created = model(c)?;
    let a = exact_one_added(&baseline, &created, out, "chamfer-created")?;
    check_note(&projection, &a)?;
    let id = a["id"].as_u64().context("Chamfer annotation ID")?;
    history(c, &baseline, &created)?;
    capture(c, out, "chamfer-created")?;
    control(c, &format!("Edit annotation {id}"), None)?;
    for (name, value) in [
        ("Chamfer setback (mm)", "2.5"),
        ("Chamfer angle (degrees)", "37.125"),
        ("Prefix", "2X Ω "),
        ("Paper X (mm)", "202"),
        ("Paper Y (mm)", "63"),
    ] {
        field(c, name, value)?;
    }
    ensure!(
        model(c)? == created,
        "Chamfer field draft changed the document"
    );
    control(c, "Apply annotation", None)?;
    let edited = model(c)?;
    let expected = replace_expected(&created, id, |a| {
        a["length"] = json!(2.5);
        a["angle_deg"] = json!(37.125);
        a["prefix"] = json!("2X Ω ");
        a["position"] = json!([202., 63.]);
    });
    ensure!(
        edited == expected,
        "Chamfer edit changed endpoint identity or unrelated intent"
    );
    history(c, &created, &edited)?;
    control(c, &format!("Edit annotation {id}"), None)?;
    capture(c, out, "chamfer-edited")?;
    curved::save_exact(c, out, "chamfer-edited", &edited)?;
    control(c, "Apply annotation", None)?;
    ensure!(model(c)? == edited, "No-op chamfer Apply changed the model");
    control(c, "Undo", None)?;
    ensure!(model(c)? == created, "No-op chamfer Apply inserted history");
    control(c, "Redo", None)?;
    curved::delete_and_restore(c, id, &created, &edited, &baseline)?;
    let os = if physical {
        Some(desktop::exercise_chamfer(
            c,
            out,
            server,
            &baseline,
            &projection,
        )?)
    } else {
        None
    };
    ensure!(
        model(c)? == baseline,
        "Chamfer proof did not restore the complete fixture model"
    );
    curved::save_exact(c, out, "chamfer-restored", &baseline)?;
    Ok(
        json!({"status":"passed","real_occt_chamfer_distance_mm":2.,"setback_not_hypotenuse":true,"exact_reference_edit_history_archive":true,
        "released_receipt_preserved":true,"physical":os,"captures":["chamfer-candidates.png","chamfer-staged.png","chamfer-created.png","chamfer-edited.png"],
        "not_proven":["Native shared sheet export","macOS pointer gestures","IME composition"]}),
    )
}
