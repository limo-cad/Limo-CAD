//! Live native sheet evidence: exact solid projections, paper-space dimensions,
//! a geometry edit, and the actual saved model. Captures still need visual review.
use crate::native_fixture::{begin_sketch, capture, control, start, ui};
use crate::replay::Client;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

fn drawing(client: &mut Client) -> Result<Value> {
    client
        .call("drawing_document", json!({}))
        .map(without_disclosure)
}

fn without_disclosure(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    value
}

fn scene(client: &mut Client) -> Result<Value> {
    client
        .call("solid_scene", json!({}))
        .map(without_disclosure)
}

fn projection(client: &mut Client, view: &Value) -> Result<Value> {
    client.call(
        "drawing_projection",
        json!({
            "direction":view["direction"],"up":view["up"],"include_hidden":true,
        }),
    )
}

fn anchor(row: &Value) -> Value {
    json!({"body_id":row["body_id"],"edge_id":row["edge_id"],"edge_key":row["edge_key"],
        "endpoint":row["endpoint"],"fallback_point":row["model_point"]})
}

/// Select actual topological endpoints, choosing the lower horizontal edge or
/// rightmost vertical edge so a positive offset lands outside the projection.
fn edge_pair(projected: &Value, axis: usize, length: f64) -> Result<[Value; 2]> {
    let rows = projected["anchors"]
        .as_array()
        .context("Projection anchors missing")?;
    let coordinate = |row: &Value, index| row["point"][index].as_f64().unwrap_or(f64::NAN);
    let mut pairs = Vec::new();
    for first in rows {
        for second in rows {
            if first["body_id"] == second["body_id"]
                && first["edge_id"] == second["edge_id"]
                && first["endpoint"] != second["endpoint"]
                && ((coordinate(first, axis) - coordinate(second, axis)).abs() - length).abs()
                    < 1e-7
                && (coordinate(first, 1 - axis) - coordinate(second, 1 - axis)).abs() < 1e-7
            {
                pairs.push([first.clone(), second.clone()]);
            }
        }
    }
    pairs.sort_by(|a, b| {
        let order = coordinate(&a[0], 1 - axis).total_cmp(&coordinate(&b[0], 1 - axis));
        if axis == 0 {
            order
        } else {
            order.reverse()
        }
    });
    pairs
        .into_iter()
        .next()
        .context("No exact projected edge has the expected length")
}

fn measured(projected: &Value, annotation: &Value) -> Result<f64> {
    let resolve = |reference: &Value| -> Result<&Value> {
        projected["anchors"]
            .as_array()
            .context("Projection anchors missing")?
            .iter()
            .find(|row| {
                row["body_id"] == reference["body_id"]
                    && row["edge_key"] == reference["edge_key"]
                    && row["endpoint"] == reference["endpoint"]
            })
            .context("Saved dimension no longer resolves to live topology")
    };
    let first = resolve(&annotation["first"])?;
    let second = resolve(&annotation["second"])?;
    let axis = if annotation["mode"] == "horizontal" {
        0
    } else {
        1
    };
    Ok((first["point"][axis]
        .as_f64()
        .context("First coordinate missing")?
        - second["point"][axis]
            .as_f64()
            .context("Second coordinate missing")?)
    .abs())
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-drawing")?;
    let c = &mut fixture.client;
    ensure!(
        drawing(c)?["sheets"].as_array().is_some_and(Vec::is_empty),
        "Choose a blank document without existing sheets"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},
        "p2":{"x":40.,"y":25.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call(
        "solid_extrude",
        json!({"sketch_name":"Sketch1","profile_indices":[0],
        "extent":{"type":"distance","distance":6.}}),
    )?;
    let solid = scene(c)?;
    ensure!(
        solid["bodies"]
            .as_array()
            .is_some_and(|rows| rows.len() == 1),
        "Real solid missing"
    );
    let document = c.call("cad_document", json!({}))?;
    let extrude_id = document["features"]
        .as_array()
        .context("Features missing")?
        .iter()
        .find(|feature| feature["kind"] == "extrude")
        .context("Extrude feature missing")?["id"]
        .clone();
    control(c, "Switch workspace", None)?;
    control(c, "Drawing", None)?;
    control(c, "New Sheet", None)?;
    let sheet_id = drawing(c)?["active_sheet_id"]
        .as_u64()
        .context("New Sheet missing")?;
    for (name, kind, direction, up, position, scale) in [
        (
            "Front 2:1",
            "front",
            [0., -1., 0.],
            [0., 0., 1.],
            [80., 125.],
            2.,
        ),
        ("Top 1:1", "top", [0., 0., 1.], [0., 1., 0.], [80., 55.], 1.),
        (
            "Isometric 1:1",
            "isometric",
            [1., -1., 1.],
            [0., 0., 1.],
            [205., 65.],
            1.,
        ),
    ] {
        c.call(
            "drawing_add_view",
            json!({"sheet_id":sheet_id,"view":{
                "name":name,"kind":kind,"direction":direction,"up":up,"position":position,
                "scale":scale,"show_hidden_lines":true,
            }}),
        )?;
    }
    let initial = drawing(c)?;
    let front = initial["sheets"][0]["views"][0].clone();
    let top = initial["sheets"][0]["views"][1].clone();
    let front_projection = projection(c, &front)?;
    let top_projection = projection(c, &top)?;
    ensure!(
        front_projection["visible"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "Exact front projection has no graphics"
    );
    capture(c, &fixture.out, "drawing-before-dimensions")?;
    for (view, projected, mode, axis, length, offset) in [
        (&front, &front_projection, "horizontal", 0, 40., 12.),
        (&front, &front_projection, "vertical", 1, 6., 12.),
        (&top, &top_projection, "horizontal", 0, 40., 8.),
    ] {
        let pair = edge_pair(projected, axis, length)?;
        c.call(
            "drawing_add_linear_dimension",
            json!({"sheet_id":sheet_id,"view_id":view["id"],
                "first":anchor(&pair[0]),"second":anchor(&pair[1]),"mode":mode,"offset":offset,
                "precision":2,
            }),
        )?;
    }
    let dimensioned = drawing(c)?;
    ensure!(
        dimensioned["sheets"][0]["annotations"]
            .as_array()
            .is_some_and(|rows| rows.len() == 3),
        "Dimensions missing from authoritative drawing"
    );
    ensure!(scene(c)? == solid, "Drawing changed the solid");
    capture(c, &fixture.out, "drawing-dimensions-6mm")?;
    c.call(
        "solid_edit_extrude",
        json!({"feature_id":extrude_id,"extrude":{
            "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":8.},"taper_angle_deg":0.,
            "flip":false,"target_body_ids":[],
        }}),
    )?;
    let edited_projection = projection(c, &front)?;
    let after_edit = drawing(c)?;
    ensure!(
        after_edit == dimensioned,
        "Solid edit changed saved drawing intent"
    );
    let annotations = after_edit["sheets"][0]["annotations"].as_array().unwrap();
    ensure!(
        (measured(&edited_projection, &annotations[0])? - 40.).abs() < 1e-7,
        "Associative width lost its real 40 mm span"
    );
    ensure!(
        (measured(&edited_projection, &annotations[1])? - 8.).abs() < 1e-7,
        "Associative thickness did not follow the solid from 6 to 8 mm"
    );
    for (annotation, offset) in annotations.iter().zip([12., 12., 8.]) {
        ensure!(
            annotation["offset"] == offset,
            "Paper-millimetre offset changed with view scale"
        );
    }
    c.call(
        "drawing_add_note",
        json!({"sheet_id":sheet_id,
        "text":"DIMENSIONS IN mm. FRONT SCALE 2:1.","position":[20.,175.]}),
    )?;
    control(c, "New Sheet", None)?;
    let scratch = drawing(c)?;
    let scratch_name = scratch["sheets"][1]["name"]
        .as_str()
        .context("Scratch sheet missing")?;
    control(c, "Sheet 1", None)?;
    ensure!(
        drawing(c)?["active_sheet_id"] == sheet_id,
        "Sheet selection failed"
    );
    control(c, scratch_name, None)?;
    control(c, "Delete sheet", None)?;
    let final_drawing = drawing(c)?;
    ensure!(
        final_drawing["sheets"]
            .as_array()
            .is_some_and(|rows| rows.len() == 1),
        "Scratch sheet was not deleted"
    );
    ensure!(
        final_drawing["active_sheet_id"] == sheet_id,
        "Original sheet was not restored"
    );
    ui(c, json!({"action":"capture","path":fixture.capture}))?;
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut saved = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let saved_model: Value = serde_json::from_reader(saved.by_name("model.json")?)?;
    let exported = c.call("cad_project_model", json!({}))?;
    let current_model: Value =
        serde_json::from_str(exported.as_str().context("Model export missing")?)?;
    ensure!(
        saved_model == current_model,
        "Saved file does not contain the authoritative model"
    );
    ensure!(
        saved_model["drawings"] == final_drawing,
        "Save lost drawing intent"
    );
    control(c, "File", None)?;
    let menu = ui(c, json!({"action":"inspect"}))?;
    for label in [
        "Export Active Drawing as DXF…",
        "Export Active Drawing as SVG…",
    ] {
        ensure!(
            crate::native_fixture::controls(&menu)
                .any(|item| item["label"] == label && item["disabled"] == false),
            "Missing enabled drawing File command: {label}"
        );
    }
    capture(c, &fixture.out, "drawing-file-output-menu")?;
    control(c, "File", None)?;
    let mut drawing_exports = Vec::new();
    for format in ["svg", "dxf"] {
        let path = fixture.out.join(format!("real-solid-sheet.{format}"));
        let expected = c.call(
            "drawing_export",
            json!({"sheet_id":sheet_id,"format":format}),
        )?;
        ui(
            c,
            json!({"action":"file","command":format!("export_drawing_{format}"),"path":path}),
        )?;
        let content = std::fs::read_to_string(&path)?;
        ensure!(
            Some(content.as_str()) == expected["content"].as_str(),
            "Native {format} output differs from the shared engine export"
        );
        ensure!(
            c.call("cad_project_model", json!({}))? == exported,
            "Drawing export changed project or history"
        );
        drawing_exports.push(json!({"format":format,"path":path,"bytes":content.len()}));
    }
    control(c, "File", None)?;
    control(c, "Save", None)?;
    let mut saved_again = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let saved_again: Value = serde_json::from_reader(saved_again.by_name("model.json")?)?;
    ensure!(
        saved_again == current_model,
        "Drawing export replaced the project save destination"
    );
    for name in [
        "drawing-before-dimensions.png",
        "drawing-dimensions-6mm.png",
        "native-drawing.png",
    ] {
        let bytes = std::fs::read(fixture.out.join(name))?;
        ensure!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() > 1024,
            "Live window capture {name} is missing or empty"
        );
    }
    std::fs::write(
        &fixture.report,
        serde_json::to_string_pretty(&json!({
            "state_checks_passed":true,"pixel_review":"required",
            "session":fixture.session,"drawing":final_drawing,"front_projection":edited_projection,
            "drawing_exports":drawing_exports,"save_dialog_os_input":"not exercised",
            "expected_pixels":{
                "drawing-before-dimensions.png":"Three real solid projections; no dimension labels.",
                "drawing-dimensions-6mm.png":"Front 2:1 labels 40.00 mm and 6.00 mm; Top 1:1 label 40.00 mm. Dimensions appear without a solid change.",
                "native-drawing.png":"Front 2:1 labels 40.00 mm and 8.00 mm; Top 1:1 label 40.00 mm. Front extension offsets are 12 paper mm; Top offset is 8 paper mm. Note and projections are legible with no clipped dimension strokes.",
                "drawing-file-output-menu.png":"Active drawing SVG/DXF commands are visible and enabled; the File menu and footer fit the window.",
            },
        }))?,
    )?;
    println!("PASS native drawing state/save/capture; review the three PNGs before marking pixel validation passed");
    Ok(())
}
