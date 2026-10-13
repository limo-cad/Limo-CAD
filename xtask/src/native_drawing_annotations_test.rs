//! Existing annotated project preservation on actual OCCT solids. Pixel review
//! is required: authoritative JSON alone cannot prove these graphics rendered.
use crate::native_fixture::{begin_sketch, capture, control, start, ui};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

fn clean(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    value
}
/// The engine materializes omitted DTO defaults when loading a project. Every
/// explicitly saved intent value and collection member must survive unchanged.
fn preserves(actual: &Value, expected: &Value, path: &str) -> Result<()> {
    match expected {
        Value::Object(object) => {
            for (key, value) in object {
                preserves(&actual[key], value, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(values) => {
            ensure!(
                actual.as_array().is_some_and(|a| a.len() == values.len()),
                "Collection changed at {path}"
            );
            for (index, value) in values.iter().enumerate() {
                preserves(&actual[index], value, &format!("{path}[{index}]"))?;
            }
        }
        _ => ensure!(
            actual == expected,
            "Saved intent changed at {path}: {actual} != {expected}"
        ),
    }
    Ok(())
}
fn xy(row: &Value) -> [f64; 2] {
    [
        row["point"][0].as_f64().unwrap(),
        row["point"][1].as_f64().unwrap(),
    ]
}
fn anchor(row: &Value, projection: &Value) -> Value {
    json!({"body_id":row["body_id"],"edge_id":row["edge_id"],"edge_key":row["edge_key"],"occurrence_id":row["occurrence_id"],"endpoint":row["endpoint"],"fallback_point":row["model_point"],"topology_signature":projection["topology_signatures"][row["body_id"].as_u64().unwrap().to_string()]})
}
fn line(first: &Value, second: &Value, projection: &Value) -> Value {
    json!({"body_id":first["body_id"],"edge_id":first["edge_id"],"edge_key":first["edge_key"],"occurrence_id":first["occurrence_id"],"fallback_start":first["model_point"],"fallback_end":second["model_point"],"topology_signature":projection["topology_signatures"][first["body_id"].as_u64().unwrap().to_string()]})
}
fn circular(row: &Value, projection: &Value) -> Value {
    json!({"body_id":row["body_id"],"edge_id":row["edge_id"],"edge_key":row["edge_key"],"occurrence_id":row["occurrence_id"],"fallback_center":row["center_model"],"fallback_normal":row["normal_model"],"fallback_radius":row["radius"],"closed":row["closed"],"topology_signature":projection["topology_signatures"][row["body_id"].as_u64().unwrap().to_string()]})
}
struct References {
    a: Value,
    b: Value,
    c: Value,
    first_line: Value,
    second_line: Value,
    circles: Vec<Value>,
}
fn references(projection: &Value, base: &Value) -> Result<References> {
    let rows = projection["anchors"]
        .as_array()
        .context("Missing real projected anchors")?;
    let pair = |target_y: f64| -> Result<(&Value, &Value)> {
        for a in rows
            .iter()
            .filter(|a| &a["body_id"] == base && a["endpoint"] == "start")
        {
            let Some(b) = rows.iter().find(|b| {
                b["body_id"] == a["body_id"]
                    && b["edge_key"] == a["edge_key"]
                    && b["endpoint"] == "end"
            }) else {
                continue;
            };
            let p = xy(a);
            let q = xy(b);
            if (p[1] - target_y).abs() < 1e-5
                && (q[1] - target_y).abs() < 1e-5
                && ((p[0] - q[0]).abs() - 40.).abs() < 1e-5
            {
                return Ok(if p[0] < q[0] { (a, b) } else { (b, a) });
            }
        }
        anyhow::bail!("Real base has no 40 mm projected edge at y={target_y}")
    };
    let (a, b) = pair(0.)?;
    let (c, d) = pair(30.)?;
    let mut circles = Vec::new();
    let mut bodies = std::collections::BTreeSet::new();
    let mut candidates = projection["circles"]
        .as_array()
        .context("Missing real projected circles")?
        .clone();
    candidates.sort_by(|a, b| {
        b["center_model"][2]
            .as_f64()
            .unwrap()
            .total_cmp(&a["center_model"][2].as_f64().unwrap())
    });
    for circle in candidates {
        if circle["closed"] == true
            && circle["hidden"] == false
            && bodies.insert(circle["body_id"].as_u64().context("Circle body ID")?)
        {
            circles.push(circular(&circle, projection));
        }
    }
    ensure!(
        circles.len() == 3,
        "Expected three actual boss circles, got {}",
        circles.len()
    );
    Ok(References {
        a: anchor(a, projection),
        b: anchor(b, projection),
        c: anchor(c, projection),
        first_line: line(a, b, projection),
        second_line: line(c, d, projection),
        circles,
    })
}

fn variants(r: &References, position: [f64; 2]) -> Vec<(&'static str, Value)> {
    let a = &r.a;
    let b = &r.b;
    let c = &r.c;
    let first = &r.first_line;
    let second = &r.second_line;
    let circle = &r.circles[0];
    let attachment = json!({"type":"anchor","reference":a});
    let p = [position[0] + 30., position[1] + 8.];
    vec![
        (
            "Linear",
            json!({"kind":"linear_dimension","first":a,"second":b,"mode":"horizontal","offset":12.,"precision":2}),
        ),
        (
            "Line distance",
            json!({"kind":"line_dimension","first":first,"second":second,"mode":"distance","position":p}),
        ),
        (
            "Point-line",
            json!({"kind":"point_line_dimension","point":c,"line":first,"position":p}),
        ),
        (
            "Note",
            json!({"kind":"note","text":"Saved note\nCafé 零件","position":[position[0]+25.,position[1]-10.]}),
        ),
        (
            "Diameter",
            json!({"kind":"radial_dimension","feature":circle,"mode":"diameter","leader_angle_deg":25.,"offset":18.}),
        ),
        (
            "Angular",
            json!({"kind":"angular_dimension","vertex":a,"first":b,"second":c,"radius":12.}),
        ),
        (
            "Hole note",
            json!({"kind":"hole_note","feature":circle,"position":p,"quantity":3,"diameter":6.,"depth":8.,"hole_style":"counterbore","counterbore_diameter":10.,"counterbore_depth":2.,"note":"Saved callout"}),
        ),
        (
            "Chamfer",
            json!({"kind":"chamfer_note","first":a,"second":b,"position":p,"length":2.,"angle_deg":45.}),
        ),
        (
            "Center mark",
            json!({"kind":"center_mark","feature":circle,"extension":4.}),
        ),
        (
            "Center line",
            json!({"kind":"center_line","first":circle,"second":r.circles[1],"extension":4.}),
        ),
        (
            "Parallel center",
            json!({"kind":"center_line_between_edges","first":first,"second":second,"extension":4.}),
        ),
        (
            "Symmetry",
            json!({"kind":"automatic_symmetry_axis","axis":"both","extension":4.}),
        ),
        (
            "Bolt circle",
            json!({"kind":"bolt_circle_center_line","features":r.circles,"extension":4.}),
        ),
        (
            "Chain",
            json!({"kind":"chain_dimension","anchors":[a,b,c],"mode":"aligned","layout":"baseline","offset":12.,"spacing":8.}),
        ),
        (
            "Ordinate",
            json!({"kind":"ordinate_dimension","origin":a,"target":b,"axis":"both","offset":14.}),
        ),
        (
            "Arc length",
            json!({"kind":"arc_length_dimension","feature":circle,"first":a,"second":b,"offset":16.}),
        ),
        (
            "Jogged radius",
            json!({"kind":"jogged_radius_dimension","feature":circle,"jog":[position[0]+10.,position[1]+12.],"position":p}),
        ),
        (
            "Datum",
            json!({"kind":"datum_feature","attachment":attachment,"label":"A","position":p,"target_index":2}),
        ),
        (
            "GD&T",
            json!({"kind":"gdt_frame","attachment":attachment,"position":p,"characteristic":"position","tolerance":0.1,"diameter_zone":true,"material_condition":"maximum","datums":[{"label":"A"}]}),
        ),
        (
            "Surface texture",
            json!({"kind":"surface_texture","attachment":attachment,"position":p,"roughness_ra":1.6,"process":"Grind"}),
        ),
        (
            "Edge requirement",
            json!({"kind":"edge_requirement","attachment":first,"position":p,"upper_deviation":0.2,"lower_deviation":-0.1,"note":"Deburr"}),
        ),
        (
            "Weld",
            json!({"kind":"weld_symbol","attachment":first,"position":p,"weld_type":"fillet","side":"arrow","size":3.,"all_around":true,"field_weld":true,"tail":"W1"}),
        ),
        (
            "BOM balloon",
            json!({"kind":"item_balloon","attachment":attachment,"position":p,"bom_item_id":1}),
        ),
        (
            "Revision cloud",
            json!({"kind":"revision_cloud","revision":"B","points":[[position[0]-25.,position[1]-20.],[position[0]+25.,position[1]-20.],[position[0]+25.,position[1]+20.],[position[0]-25.,position[1]+20.]]}),
        ),
    ]
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    run_impl(args, false, false, false)
}
pub(super) fn run_authoring(args: impl Iterator<Item = String>) -> Result<()> {
    run_impl(args, false, false, true)
}
pub(super) fn run_navigation(args: impl Iterator<Item = String>) -> Result<()> {
    let mut input = false;
    let mut mcp_only = false;
    let mut authoring = false;
    let args: Vec<_> = args
        .filter(|arg| {
            if arg == "--desktop-input" {
                input = true;
                false
            } else if arg == "--mcp-only" {
                mcp_only = true;
                false
            } else if arg == "--authoring-input" {
                authoring = true;
                false
            } else {
                true
            }
        })
        .collect();
    ensure!(
        input != mcp_only && (!input || cfg!(target_os = "windows") || cfg!(target_os = "linux")),
        "Choose --desktop-input on an owned Windows window/private Linux Xvfb or --mcp-only; MCP-only does not prove OS gestures"
    );
    ensure!(
        !authoring || (input && cfg!(target_os = "linux")),
        "Physical annotation QA requires disposable Linux Xvfb desktop input"
    );
    run_impl(args.into_iter(), true, input, authoring)
}
fn run_impl(
    args: impl Iterator<Item = String>,
    navigation: bool,
    desktop_input: bool,
    authoring: bool,
) -> Result<()> {
    let mut fixture = start(
        args,
        if navigation {
            "native-drawing-navigation"
        } else if authoring {
            "native-drawing-authoring"
        } else {
            "native-drawing-annotations"
        },
    )?;
    let c = &mut fixture.client;
    if authoring && std::env::var("LIMO_CAD_NATIVE_CLOUD_ONLY").as_deref() == Ok("1") {
        let result = crate::native_drawing_authoring_test::exercise_cloud(
            c,
            &fixture.out,
            &fixture.server,
            desktop_input || std::env::var("LIMO_CAD_NATIVE_CLOUD_INPUT").as_deref() == Ok("1"),
        )?;
        std::fs::write(&fixture.report, serde_json::to_vec_pretty(&result)?)?;
        return Ok(());
    }
    if authoring && std::env::var("LIMO_CAD_NATIVE_CHAMFER_ONLY").as_deref() == Ok("1") {
        let result = crate::native_drawing_authoring_test::exercise_chamfer(
            c,
            &fixture.out,
            &fixture.server,
            desktop_input || std::env::var("LIMO_CAD_NATIVE_CHAMFER_INPUT").as_deref() == Ok("1"),
        )?;
        std::fs::write(&fixture.report, serde_json::to_vec_pretty(&result)?)?;
        return Ok(());
    }
    let drawing = clean(c.call("drawing_document", json!({}))?);
    ensure!(
        drawing["sheets"].as_array().is_some_and(Vec::is_empty),
        "Choose a blank document without sheets"
    );
    begin_sketch(c, "XY")?;
    c.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
    )?;
    control(c, "Finish sketch", None)?;
    c.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}))?;
    let base = clean(c.call("solid_scene", json!({}))?)["bodies"][0]["id"].clone();
    for center in [[10., 7.], [30., 7.], [20., 24.3205080757]] {
        begin_sketch(c, "XY")?;
        let active = c.call("sketch_active", json!({}))?;
        let name = active["name"]
            .as_str()
            .context("New boss sketch name")?
            .to_owned();
        c.call("sketch_add_circle",json!({"mode":"center_diameter","p1":{"x":center[0],"y":center[1]},"p2":{"x":center[0]+3.,"y":center[1]},"ctrl_held":true}))?;
        control(c, "Finish sketch", None)?;
        c.call("solid_extrude",json!({"sketch_name":name,"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}))?;
    }
    let solid = clean(c.call("solid_scene", json!({}))?);
    ensure!(
        solid["bodies"].as_array().is_some_and(|b| b.len() == 4),
        "Real four-body solid fixture missing"
    );
    let projection = clean(c.call(
        "drawing_projection",
        json!({"direction":[0.,0.,1.],"up":[0.,1.,0.],"include_hidden":true}),
    )?);
    let references = references(&projection, &base)?;
    let exported = c.call("cad_project_model", json!({}))?;
    let mut model: Value =
        serde_json::from_str(exported.as_str().context("Model export missing")?)?;
    let mut drawing = model["drawings"].clone();
    let mut sheets = Vec::new();
    let mut kinds = Vec::new();
    for page in 0..6 {
        let mut views = Vec::new();
        let mut annotations = Vec::new();
        for slot in 0..4 {
            let index = page * 4 + slot;
            let position = [
                if slot % 2 == 0 { 50. } else { 190. },
                if slot < 2 { 58. } else { 145. },
            ];
            let (name, mut annotation) = variants(&references, position).remove(index);
            let id = index + 1;
            annotation["id"] = json!(id);
            if annotation["kind"] != "note" && annotation["kind"] != "revision_cloud" {
                annotation["view_id"] = json!(id);
            }
            kinds.push(annotation["kind"].clone());
            views.push(json!({"id":id,"name":format!("{id:02} {name}"),"kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":position,"scale":1.,"show_hidden_lines":false}));
            annotations.push(annotation);
        }
        sheets.push(json!({"id":page+1,"name":format!("Annotations {}",page+1),"format":"a4","orientation":"landscape","views":views,"annotations":annotations,"bom":[{"id":page+1,"item_number":"7","part_number":"PART-7","description":"Existing base","quantity":1.}]}));
        for annotation in sheets.last_mut().unwrap()["annotations"]
            .as_array_mut()
            .unwrap()
        {
            if annotation["kind"] == "item_balloon" {
                annotation["bom_item_id"] = json!(page + 1);
            }
        }
    }
    let dense_segments: usize = projection["visible"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["points"].as_array().unwrap().len().saturating_sub(1))
        .sum::<usize>()
        * 20;
    if navigation {
        ensure!(
            dense_segments > 800,
            "Fixture must exceed the former projected-edge cap"
        );
        let views: Vec<_> = (0..20).map(|index| json!({
            "id":25+index,"name":format!("Complete view {}", index+1),"kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],
            "position":[40.+(index%5) as f64*80.,35.+(index/5) as f64*62.],"scale":1.,"show_hidden_lines":false
        })).collect();
        sheets.push(json!({"id":7,"name":"Complete dense projection","format":"a3","orientation":"landscape","views":views,
            "title_block":{"title":"Dense complete sheet","drawing_number":"EDGE-1160"}}));
    }
    drawing["sheets"] = json!(sheets);
    drawing["active_sheet_id"] = json!(1);
    drawing["next_sheet_id"] = json!(if navigation { 8 } else { 7 });
    drawing["next_view_id"] = json!(if navigation { 45 } else { 25 });
    drawing["next_annotation_id"] = json!(25);
    drawing["next_bom_item_id"] = json!(7);
    model["drawings"] = drawing.clone();
    std::fs::write(
        fixture.out.join("annotated-model-source.json"),
        serde_json::to_vec_pretty(&model)?,
    )?;
    c.call(
        "cad_load_project_model",
        json!({"model_json":serde_json::to_string(&model)?}),
    )?;
    ensure!(
        clean(c.call("solid_scene", json!({}))?) == solid,
        "Opening annotations changed the solid"
    );
    let loaded = clean(c.call("drawing_document", json!({}))?);
    preserves(&loaded, &drawing, "drawings")?;
    let drawing = loaded;
    control(c, "Switch workspace", None)?;
    control(c, "Drawing", None)?;
    for page in 1..=6 {
        c.call("drawing_select_sheet", json!({"sheet_id":page}))?;
        capture(c, &fixture.out, &format!("annotations-{page}"))?;
    }
    let navigation_result = if navigation {
        c.call("drawing_select_sheet", json!({"sheet_id":7}))?;
        Some(crate::native_drawing_navigation_test::exercise(
            c,
            &fixture.out,
            &fixture.session,
            &fixture.server,
            dense_segments,
            desktop_input,
        )?)
    } else {
        None
    };
    let authoring_result = if authoring {
        if navigation {
            c.call("drawing_select_sheet", json!({"sheet_id":6}))?;
        }
        let mut result =
            crate::native_drawing_authoring_test::exercise(c, &fixture.out, &fixture.server)?;
        if desktop_input {
            result["physical"] = crate::native_drawing_authoring_test::exercise_desktop(
                c,
                &fixture.out,
                &fixture.server,
            )?;
            result["not_proven"] = result["physical"]["not_proven"].clone();
        }
        Some(result)
    } else {
        None
    };
    ui(
        c,
        json!({"action":"file","command":"save","path":fixture.project}),
    )?;
    let mut saved = zip::ZipArchive::new(std::fs::File::open(&fixture.project)?)?;
    let saved_model: Value = serde_json::from_reader(saved.by_name("model.json")?)?;
    let current = c.call("cad_project_model", json!({}))?;
    let current: Value = serde_json::from_str(current.as_str().context("Model export missing")?)?;
    ensure!(
        saved_model == current,
        "Saved file lost existing drawing annotations"
    );
    for (actual, expected) in saved_model["drawings"]["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .zip(drawing["sheets"].as_array().unwrap())
    {
        ensure!(
            actual == expected,
            "Capture or sheet selection changed saved annotation content"
        );
    }
    std::fs::write(
        &fixture.report,
        serde_json::to_vec_pretty(
            &json!({"state_checks_passed":true,"pixel_review":"required","session":fixture.session,"annotation_kinds":kinds,"real_solid_bodies":4,"projection":projection,"navigation":navigation_result,"authoring":authoring_result,"captures":["annotations-1.png","annotations-2.png","annotations-3.png","annotations-4.png","annotations-5.png","annotations-6.png"],"expected_pixels":"Four named real-solid views per sheet, each with its saved annotation. Review all 24 variants, filled leaders, center dashes, text, frame/balloon masks, and scalloped revision cloud. No broken-association ! marks expected."}),
        )?,
    )?;
    println!(
        "Native drawing checks passed; review all live captures and the evidence report in {}",
        fixture.out.display()
    );
    Ok(())
}
