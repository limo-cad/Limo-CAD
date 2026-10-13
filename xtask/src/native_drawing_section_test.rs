//! Exact solid/sheet preservation plus live section graphics for pixel review.
use crate::{
    native_fixture::{begin_sketch, capture, control, start, ui},
    replay::Client,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

fn clean(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    value
}
fn model(client: &mut Client) -> Result<Value> {
    let value = client.call("cad_project_model", json!({}))?;
    Ok(serde_json::from_str(
        value.as_str().context("Saved model JSON")?,
    )?)
}
fn preserves(actual: &Value, expected: &Value) -> Result<()> {
    match expected {
        Value::Object(values) => {
            for (key, value) in values {
                preserves(&actual[key], value)?;
            }
        }
        Value::Array(values) => {
            ensure!(
                actual.as_array().is_some_and(|a| a.len() == values.len()),
                "Saved section collection changed"
            );
            for (index, value) in values.iter().enumerate() {
                preserves(&actual[index], value)?;
            }
        }
        _ => ensure!(
            actual == expected,
            "Saved section intent changed: {actual} != {expected}"
        ),
    }
    Ok(())
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut fixture = start(args, "native-drawing-sections")?;
    let client = &mut fixture.client;
    ensure!(
        clean(client.call("drawing_document", json!({}))?)["sheets"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "Choose a blank document without sheets"
    );
    begin_sketch(client, "XY")?;
    client.call("sketch_add_rectangle",json!({"mode":"two_point","p1":{"x":-20.,"y":-15.},"p2":{"x":20.,"y":15.},"ctrl_held":true}))?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":20.}}))?;
    let body = clean(client.call("solid_scene", json!({}))?)["bodies"][0]["id"].clone();
    begin_sketch(client, "XY")?;
    client.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":6.,"y":0.},"ctrl_held":true}))?;
    control(client, "Finish sketch", None)?;
    client.call("solid_extrude",json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"cut","target_body_ids":[body],"extent":{"type":"distance","distance":20.}}))?;
    let solid = clean(client.call("solid_scene", json!({}))?);
    ensure!(
        solid["bodies"].as_array().is_some_and(|b| b.len() == 1),
        "Hollow real solid missing"
    );
    let projection = clean(client.call(
        "drawing_projection",
        json!({"direction":[0.,0.,1.],"up":[0.,1.,0.],"include_hidden":true}),
    )?);
    let circle = projection["circles"]
        .as_array()
        .context("Real circle projections")?
        .iter()
        .find(|circle| circle["closed"] == true && circle["hidden"] == false)
        .context("Visible through-hole circle")?;
    let z = circle["center_model"][2]
        .as_f64()
        .context("Circle model height")?;
    let corner = projection["anchors"]
        .as_array()
        .context("Real topology anchors")?
        .iter()
        .find(|anchor| {
            let point = &anchor["model_point"];
            point[0].as_f64().is_some_and(|v| (v - 20.).abs() < 1e-6)
                && point[1].as_f64().is_some_and(|v| (v - 15.).abs() < 1e-6)
                && point[2].as_f64().is_some_and(|v| (v - z).abs() < 1e-6)
        })
        .context("Through-hole plane corner")?;
    let signature =
        &projection["topology_signatures"][body.as_u64().context("Body ID")?.to_string()];
    let first = json!({"body_id":body,"edge_id":circle["edge_id"],"edge_key":circle["edge_key"],"endpoint":"start",
        "circle_center":true,"topology_signature":signature,"fallback_point":[900000.,900000.,900000.]});
    let second = json!({"body_id":body,"edge_id":corner["edge_id"],"edge_key":corner["edge_key"],"endpoint":corner["endpoint"],
        "topology_signature":signature,"fallback_point":[800000.,800000.,800000.]});
    let anchors = projection["anchors"]
        .as_array()
        .context("Projected endpoints")?;
    let line = anchors
        .iter()
        .find(|a| {
            a["endpoint"] == "start"
                && anchors.iter().any(|b| {
                    a["body_id"] == b["body_id"]
                        && a["edge_id"] == b["edge_id"]
                        && a["edge_key"] == b["edge_key"]
                        && b["endpoint"] == "end"
                        && (0..2).any(|i| {
                            (a["point"][i].as_f64().unwrap() - b["point"][i].as_f64().unwrap())
                                .abs()
                                > 1.
                        })
                })
        })
        .context("Projected straight edge for auxiliary view")?;
    let reference = json!({"body_id":body,"edge_id":line["edge_id"],"edge_key":line["edge_key"],
        "topology_signature":signature,"fallback_start":[900000.,900000.,900000.],"fallback_end":[800000.,800000.,800000.]});
    client.call(
        "drawing_create_sheet",
        json!({"name":"Sections","format":"a4","orientation":"landscape"}),
    )?;
    let mut saved = model(client)?;
    let template = saved["drawings"]["sheets"][0].clone();
    let mut sheets = Vec::new();
    for (index, kind, depth, scale) in [
        (0, "section", None, 1.5),
        (1, "removed_section", None, 1.25),
        (2, "section", Some(4.), 1.5),
        (3, "section", None, 1.5),
        (4, "detail", None, 1.5),
        (5, "broken", None, 1.5),
        (6, "broken", None, 1.5),
        (7, "auxiliary", None, 1.5),
        (8, "auxiliary", None, 1.5),
    ] {
        let parent_id = index * 2 + 1;
        let child_id = parent_id + 1;
        let title = if index == 4 {
            "Clipped corner detail"
        } else if index == 5 {
            "Horizontal broken view"
        } else if index == 6 {
            "Vertical break with minimum gap"
        } else if index == 7 {
            "Auxiliary edge view"
        } else if index == 8 {
            "Flipped auxiliary edge view"
        } else if index == 3 {
            "Section with custom hatch dashes"
        } else if depth.is_some() {
            "Finite section depth 4 mm"
        } else if kind == "removed_section" {
            "Removed section"
        } else {
            "Section through a real hole"
        };
        let label = if index == 0 {
            "A-A"
        } else if index == 1 {
            "B-B"
        } else if index == 2 {
            "C-C"
        } else {
            "D-D"
        };
        let mut derivation = match kind {
            "detail" => {
                json!({"type":kind,"parent_view_id":parent_id,"center":second,"radius":12.,"label":"DETAIL E"})
            }
            "broken" => {
                json!({"type":kind,"parent_view_id":parent_id,"axis":if index==5 {"horizontal"} else {"vertical"},"first":-11.,"second":14.,"gap_mm":if index==5 {7.} else {1.}})
            }
            "auxiliary" => {
                json!({"type":kind,"parent_view_id":parent_id,"reference":reference,"label":if index==7 {"F"} else {"G"},"flipped":index==8})
            }
            _ => {
                json!({"type":kind,"parent_view_id":parent_id,"first":first,"second":second,"label":label,"hatch_angle_deg":17.,"hatch_spacing_mm":2.})
            }
        };
        if let Some(depth) = depth {
            derivation["depth"] = json!(depth);
        }
        let child = json!({"id":child_id,"name":title,"kind":kind,"body_ids":[body],"direction":[0.6,-0.8,0.],"up":[0.,0.,1.],"position":[185.,70.],"scale":scale,"show_hidden_lines":true,"derivation":derivation});
        let parent = json!({"id":parent_id,"name":"Parent top view","kind":"top","body_ids":[body],"direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[70.,70.],"scale":1.});
        let mut sheet = template.clone();
        sheet["id"] = json!(index + 1);
        sheet["name"] = json!(title);
        sheet["views"] = json!([child, parent]);
        sheet["style"]["hatch_spacing_mm"] = json!(5.);
        if index == 3 {
            sheet["style"]["hatch"]["dash_mm"] = json!([1.5, 0.7, 2.]);
            sheet["style"]["visible"]["dash_mm"] = json!([5., 2., 1.]);
            sheet["style"]["dimension"]["dash_mm"] = json!([3., 1.]);
            sheet["revision_table_position"] = json!([10., 140.]);
            sheet["bom_table_position"] = json!([150., 140.]);
            sheet["revisions"] = json!([{"id":1,"revision":"A","date":"2026-09-27","description":"Dash styles","approved_by":"QA"}]);
            sheet["bom"] = json!([{"id":1,"item_number":"1","body_id":body,"part_number":"P-1","description":"Hollow block","quantity":1.,"material":"Al"}]);
        }
        let note = if index < 4 {
            "Saved screen hatch: 17 degrees from vertical, 5 mm paper spacing.\nThe real through-hole must remain clear. Source arrows use current topology."
        } else {
            "Detail geometry stops at its circular boundary. Broken views retain their centered paper gap.\nAuxiliary arrows follow the selected edge and flip direction. Source marks use current topology."
        };
        sheet["annotations"] =
            json!([{"kind":"note","id":index+1,"text":note,"position":[30.,125.]}]);
        sheet["title_block"]["title"] = json!(title);
        sheets.push(sheet);
    }
    saved["drawings"]["sheets"] = json!(sheets);
    saved["drawings"]["active_sheet_id"] = json!(1);
    saved["drawings"]["next_sheet_id"] = json!(10);
    saved["drawings"]["next_view_id"] = json!(19);
    saved["drawings"]["next_annotation_id"] = json!(10);
    saved["drawings"]["next_revision_id"] = json!(2);
    saved["drawings"]["next_bom_item_id"] = json!(2);
    std::fs::write(
        fixture.out.join("section-model-source.json"),
        serde_json::to_vec_pretty(&saved)?,
    )?;
    client.call(
        "cad_load_project_model",
        json!({"model_json":serde_json::to_string(&saved)?}),
    )?;
    let mut baseline = model(client)?;
    preserves(&baseline["drawings"], &saved["drawings"])?;
    ensure!(
        clean(client.call("solid_scene", json!({}))?) == solid,
        "Opening section intent changed the real solid"
    );
    control(client, "Switch workspace", None)?;
    control(client, "Drawing", None)?;
    let mut captures = Vec::new();
    for page in 1..=9 {
        client.call("drawing_select_sheet", json!({"sheet_id":page}))?;
        baseline["drawings"]["active_sheet_id"] = json!(page);
        control(client, "Fit sheet", None)?;
        let name = format!("section-{page}-fit");
        capture(client, &fixture.out, &name)?;
        captures.push(name);
        for _ in 0..4 {
            control(client, "Zoom drawing in", None)?;
        }
        let name = format!("section-{page}-zoom");
        capture(client, &fixture.out, &name)?;
        captures.push(name);
        control(client, "Fit sheet", None)?;
        ensure!(
            model(client)? == baseline,
            "Section rendering/navigation changed saved model or history counters"
        );
    }
    ui(
        client,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let archived: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        archived == model(client)? && archived == baseline,
        "Section archive did not preserve exact project intent"
    );
    ensure!(
        clean(client.call("solid_scene", json!({}))?) == solid,
        "Section presentation changed the real solid"
    );
    std::fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"state_checks_passed":true,"pixel_review":"required","server":fixture.server,"session":fixture.session,"captures":captures,"body":body,"expected_pixels":"Real hole clear in angled 5 mm hatching; custom hatch, outer-frame and title/revision/BOM grid dashes on sheet 4; purple source lines/arrows/labels; full/removed/finite-depth sections; circular detail clipping; both broken axes and minimum gap; normal/flipped auxiliary views. Child before parent, varied scale, Fit/button zoom; exact topology overrides poisoned fallback points.","not_proven":["Physical paper gestures","DPI transitions"]}),
        )?,
    )?;
    println!(
        "PASS native real-solid derived-view preservation and archive; review eighteen captures"
    );
    Ok(())
}
