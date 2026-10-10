//! Real drilled geometry; no manually forged hole metadata or fallback labels.
use super::*;
use crate::native_fixture::begin_sketch;

fn exports(
    c: &mut Client,
    out: &Path,
    stage: &str,
    expected: &Value,
    lines: &[&str],
) -> Result<Value> {
    let mut reports = Vec::new();
    for format in ["svg", "dxf"] {
        let path = out.join(format!("hole-{stage}.{format}"));
        let exported = c.call("drawing_export", json!({"sheet_id":1,"format":format}))?;
        ui(
            c,
            json!({"action":"file","command":format!("export_drawing_{format}"),"path":path}),
        )?;
        let text = std::fs::read_to_string(&path)?;
        ensure!(
            Some(text.as_str()) == exported["content"].as_str(),
            "Native {format} differs from shared hole export"
        );
        ensure!(text.contains("LEADER"), "Export omitted the hole leader");
        for line in lines {
            let exact = if format == "svg" {
                format!(">{line}</text>")
            } else {
                format!("\n1\n{line}\n")
            };
            ensure!(
                text.matches(&exact).count() == 1,
                "Missing/duplicate exact hole callout line {line} in {format}"
            );
        }
        ensure!(
            &model(c)? == expected,
            "Hole export changed the exact shared project"
        );
        reports
            .push(json!({"format":format,"path":path,"bytes":text.len(),"expected_lines":lines}));
    }
    Ok(json!(reports))
}

pub(super) fn open(c: &mut Client) -> Result<Value> {
    control(c, "More dimensions", None)?;
    control(c, "Hole Note", None)
}

pub(super) fn entry_label(projection: &Value, state: &Value) -> Result<String> {
    let mut visible: Vec<_> = projection["circles"]
        .as_array()
        .context("Projected circles")?
        .iter()
        .filter(|p| p["hidden"] == false && p["closed"] == true)
        .collect();
    visible.sort_by_key(|p| {
        (
            p["occurrence_id"].as_u64(),
            p["body_id"].as_u64(),
            p["edge_id"].as_u64(),
        )
    });
    let entry = visible
        .iter()
        .position(|p| {
            p["center_model"][0]
                .as_f64()
                .is_some_and(|x| (x - 20.).abs() < 1e-6)
                && p["center_model"][1]
                    .as_f64()
                    .is_some_and(|y| (y - 20.).abs() < 1e-6)
                && p["center_model"][2]
                    .as_f64()
                    .is_some_and(|z| (z - 10.).abs() < 1e-6)
        })
        .context("Exact entry pick")?;
    let picks: Vec<_> = controls(state)
        .filter(|p| p["surface"] == "drawing/circles" && p["disabled"] == false)
        .collect();
    ensure!(
        picks.len() == visible.len() && picks.len() <= 4,
        "Native circular controls do not match the bounded real drill projection"
    );
    let label = format!("View 1 circular edge {}", entry + 1);
    ensure!(
        picks.iter().any(|p| p["label"] == label),
        "Exact drilled entry control is absent"
    );
    Ok(label)
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
        "Hole fixture needs a blank drawing"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":60.,"y":40.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}))?;
    let solid = c.call("solid_scene", json!({}))?;
    let bodies = solid["bodies"].as_array().context("Real extruded stock")?;
    ensure!(bodies.len() == 1, "Expected one real stock body");
    let body = &bodies[0];
    let faces: Vec<_> = body["faces"]
        .as_array()
        .context("Stock faces")?
        .iter()
        .filter(|f| {
            f["plane"]["normal"][2].as_f64().is_some_and(|z| z > 0.999)
                && f["plane"]["origin"][2]
                    .as_f64()
                    .is_some_and(|z| (z - 10.).abs() < 1e-6)
        })
        .collect();
    ensure!(faces.len() == 1, "Expected the exact top support face");
    let basis = &faces[0]["plane"];
    let positions: Vec<_> = [20., 40.]
        .into_iter()
        .map(|x| {
            let delta = [
                x - basis["origin"][0].as_f64().unwrap(),
                20. - basis["origin"][1].as_f64().unwrap(),
                10. - basis["origin"][2].as_f64().unwrap(),
            ];
            let dot = |axis: &str| {
                (0..3)
                    .map(|i| basis[axis][i].as_f64().unwrap() * delta[i])
                    .sum::<f64>()
            };
            json!({"position":{"x":dot("u"),"y":dot("v")}})
        })
        .collect();
    c.call("solid_hole",json!({"body_id":body["id"],"face_id":faces[0]["id"],
        "position":positions[0]["position"],"positions":positions,"diameter":6.,"extent":{"type":"through_all"},
        "bottom_style":"flat","flip":false}))?;
    let definitions = c.call("solid_hole_definitions", json!({}))?;
    let definitions = definitions
        .as_array()
        .context("Canonical hole definitions")?;
    ensure!(
        definitions.len() == 1,
        "Fixture must have one modeled two-position hole feature"
    );
    let definition = definitions[0].clone();
    ensure!(
        definition["diameter"] == 6.
            && definition["extent"]["type"] == "through_all"
            && definition["positions"].as_array().is_some_and(
                |p| p.len() == 2 && p.iter().all(|p| p["position_reference"].is_null())
            )
            && definition["face_basis"].is_object(),
        "Hole catalog lost authored numeric positions/extent"
    );
    let projection = curved::projection(c)?;
    let circles: Vec<_> = projection["circles"]
        .as_array()
        .context("Real drill projection")?
        .iter()
        .filter(|circle| {
            circle["hidden"] == false
                && circle["closed"] == true
                && circle["radius"]
                    .as_f64()
                    .is_some_and(|r| (r - 3.).abs() < 1e-6)
                && circle["center_model"][2]
                    .as_f64()
                    .is_some_and(|z| (z - 10.).abs() < 1e-6)
        })
        .collect();
    ensure!(
        circles.len() == 2,
        "Expected exactly two real visible 6 mm hole entry circles"
    );
    for x in [20., 40.] {
        ensure!(
            circles.iter().any(
                |p| (p["center_model"][0].as_f64().unwrap() - x).abs() < 1e-6
                    && (p["center_model"][1].as_f64().unwrap() - 20.).abs() < 1e-6
            ),
            "Missing exact drilled entry ({x},20,10)"
        );
    }
    std::fs::write(
        out.join("hole-real-geometry.json"),
        serde_json::to_vec_pretty(
            &json!({"projection":projection,"definition":definition,"solid":c.call("solid_scene",json!({}))?}),
        )?,
    )?;
    let mut prepared = model(c)?;
    let mut drawings = prepared["drawings"].clone();
    drawings["sheets"] = json!([
        {"id":1,"name":"Drilled holes","format":"a4","orientation":"landscape","standard":"iso",
         "release":{"status":"released","released_revision":"A","released_at":"2026-09-27"},
         "views":[{"id":1,"name":"Top drilled stock","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[130.,100.],"scale":2.,"show_hidden_lines":false}],
         "annotations":[{"kind":"note","id":1,"text":"Saved Ω note","position":[30.,30.]}]},
        {"id":2,"name":"Preserved sheet","format":"a4","orientation":"landscape",
         "annotations":[{"kind":"note","id":2,"text":"Unrelated intent","position":[30.,30.]}]}
    ]);
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
    open(c)?;
    let state = ui(c, json!({"action":"inspect"}))?;
    let label = entry_label(&projection, &state)?;
    capture(c, out, "hole-targets")?;
    ensure!(
        model(c)? == baseline,
        "Choosing Hole Note mutated the project"
    );
    control(c, &label, None)?;
    let created = model(c)?;
    let note = exact_one_added(&baseline, &created, out, "hole-created")?;
    curved::circular_ref(&projection, &note["feature"])?;
    ensure!(
        note["kind"] == "hole_note"
            && note["quantity"] == 2
            && note["diameter"] == 6.
            && note["depth"].is_null()
            && note["through_all"] == true
            && note["note"] == "THRU"
            && note["source_feature_id"] == definition["feature_id"]
            && note["feature_name"] == definition["name"]
            && note["pattern_note"] == "2 HOLES"
            && note["hole_style"] == "simple",
        "Native hole callout lost exact canonical metadata: {note}"
    );
    let center = &note["feature"]["fallback_center"];
    ensure!(
        (center[0].as_f64().context("Hole X")? - 20.).abs() < 1e-6
            && (center[1].as_f64().context("Hole Y")? - 20.).abs() < 1e-6
            && (center[2].as_f64().context("Hole Z")? - 10.).abs() < 1e-6,
        "Native pick selected a different/bottom circle"
    );
    let id = note["id"].as_u64().context("Hole note ID")?;
    history(c, &baseline, &created)?;
    control(c, &format!("Edit annotation {id}"), None)?;
    capture(c, out, "hole-created")?;
    let initial_exports = exports(c, out, "created", &created, &["2× ⌀6 THRU"])?;
    field(c, "Depth (mm, optional)", "8")?;
    field(c, "Through hole", "true")?;
    ensure!(
        model(c)? == created,
        "Extent draft mutated the document before Apply"
    );
    control(c, "Reset annotation", None)?;
    ensure!(model(c)? == created, "Reset changed hole intent");
    field(c, "Additional note", "Deburr\nInspect holes")?;
    field(c, "Paper X (mm)", "188")?;
    field(c, "Paper Y (mm)", "66")?;
    ensure!(
        model(c)? == created,
        "Hole note draft committed before Apply"
    );
    control(c, "Apply annotation", None)?;
    let edited = model(c)?;
    let expected = replace_expected(&created, id, |a| {
        a["note"] = json!("Deburr\nInspect holes");
        a["position"] = json!([188., 66.]);
    });
    ensure!(
        edited == expected,
        "Hole edit changed source/extent or unrelated shared intent"
    );
    history(c, &created, &edited)?;
    control(c, &format!("Edit annotation {id}"), None)?;
    capture(c, out, "hole-edited")?;
    curved::save_exact(c, out, "hole-edited", &edited)?;
    let edited_exports = exports(
        c,
        out,
        "edited",
        &edited,
        &["2× ⌀6 THRU", "Deburr", "Inspect holes"],
    )?;
    control(c, "Apply annotation", None)?;
    ensure!(model(c)? == edited, "No-op hole Apply changed the model");
    control(c, "Undo", None)?;
    ensure!(
        model(c)? == created,
        "Export/save/no-op introduced an extra history edit"
    );
    control(c, "Redo", None)?;
    ensure!(model(c)? == edited, "Redo failed after native export/save");
    curved::delete_and_restore(c, id, &created, &edited, &baseline)?;
    let physical_result = if physical {
        Some(desktop::exercise_hole(
            c,
            out,
            server,
            &baseline,
            &projection,
            &definition,
        )?)
    } else {
        None
    };
    ensure!(
        model(c)? == baseline,
        "Hole proof did not restore the complete fixture model"
    );
    curved::save_exact(c, out, "hole-restored", &baseline)?;
    let mut not_proven = vec!["Candidate popup/IME", "Monitor DPI transitions"];
    if physical_result.is_none() {
        not_proven.insert(0, "Physical OS circle picking/leader drag");
    } else {
        not_proven.extend([
            "Windows or macOS hole pointer input",
            "Wayland",
            "Physical touchscreen",
        ]);
    }
    let captures: Vec<&str> = if physical_result.is_some() {
        vec![
            "hole-targets.png",
            "hole-created.png",
            "hole-edited.png",
            "hole-os-targets.png",
            "hole-os-created.png",
            "hole-os-dragged.png",
        ]
    } else {
        vec!["hole-targets.png", "hole-created.png", "hole-edited.png"]
    };
    Ok(
        json!({"status":"passed","canonical_definition":definition,"exact_circle_reference":note["feature"],
        "expected_created_label":"2× ⌀6 THRU","expected_edited_label":"2× ⌀6 THRU\nDeburr\nInspect holes",
        "exact_history_and_archives":true,"initial_exports":initial_exports,"edited_exports":edited_exports,
        "physical":physical_result,
        "captures":captures,"pixel_review":"required",
        "not_proven":not_proven}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hole_pointer_entry_label_is_the_front_drilled_circle() {
        let projection = json!({"circles":[
            {"hidden":false,"closed":true,"occurrence_id":1,"body_id":1,"edge_id":2,"center_model":[20.,20.,0.]},
            {"hidden":true,"closed":true,"occurrence_id":1,"body_id":1,"edge_id":1,"center_model":[20.,20.,10.]},
            {"hidden":false,"closed":true,"occurrence_id":1,"body_id":1,"edge_id":9,"center_model":[40.,20.,10.]},
            {"hidden":false,"closed":false,"occurrence_id":1,"body_id":1,"edge_id":3,"center_model":[20.,20.,10.]},
            {"hidden":false,"closed":true,"occurrence_id":1,"body_id":1,"edge_id":4,"center_model":[20.,20.,10.]}
        ]});
        let state = json!({"ui":{"surfaces":[{"controls":[
            {"surface":"drawing/circles","disabled":false,"label":"View 1 circular edge 1"},
            {"surface":"drawing/circles","disabled":false,"label":"View 1 circular edge 2"},
            {"surface":"drawing/circles","disabled":false,"label":"View 1 circular edge 3"},
            {"surface":"drawing/circles","disabled":true,"label":"View 1 circular edge 4"}
        ]}]}});
        assert_eq!(
            entry_label(&projection, &state).unwrap(),
            "View 1 circular edge 2"
        );
    }
}
