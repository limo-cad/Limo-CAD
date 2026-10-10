//! Each tooling study restores the blank checkpoint before making its own stock.
use super::*;

fn reset(s: &mut Scenario, empty: &str) -> Result<()> {
    s.call("cad_load_project_model", json!({"model_json":empty}))?;
    Ok(())
}
fn begin(s: &mut Scenario, empty: &str) -> Result<()> {
    reset(s, empty)?;
    s.call(
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"xy"}}),
    )?;
    s.call("sketch_set_grid_snap", json!({"enabled":false}))?;
    Ok(())
}
fn active(s: &mut Scenario) -> Result<Value> {
    s.call("sketch_active", json!({}))
}
fn line(s: &mut Scenario, a: [f64; 2], b: [f64; 2]) -> Result<Value> {
    s.call(
        "sketch_add_line",
        json!({"from":{"x":a[0],"y":a[1]},"to_raw":{"x":b[0],"y":b[1]},"ctrl_held":true}),
    )?;
    array(&active(s)?, "entities")?
        .iter()
        .rev()
        .find(|e| e["kind"] == "line")
        .map(|e| e["id"].clone())
        .context("Line missing")
}
fn circle(s: &mut Scenario) -> Result<Value> {
    s.call(
        "sketch_add_circle",
        json!({"mode":"center_diameter","p1":{"x":0,"y":0},"p2":{"x":10,"y":0},"ctrl_held":true}),
    )?;
    array(&active(s)?, "entities")?
        .iter()
        .find(|e| e["kind"] == "circle")
        .map(|e| e["id"].clone())
        .context("Circle missing")
}
fn stock(s: &mut Scenario, empty: &str) -> Result<Value> {
    reset(s, empty)?;
    s.call(
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"xy"}}),
    )?;
    s.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":-10,"y":-10},"p2":{"x":10,"y":10},"ctrl_held":true}),
    )?;
    s.call("sketch_finish", json!({}))?;
    s.call("solid_extrude",json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":20},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}))?;
    array(&s.call("solid_scene", json!({}))?, "bodies")?
        .first()
        .cloned()
        .context("Stock body missing")
}
fn feature(s: &mut Scenario, name: &str, args: Value, body_count: usize) -> Result<()> {
    s.call(name, args.clone())?;
    let made = s.model()?["document"]["history"].clone();
    let edit = name.replacen("solid_", "solid_edit_", 1);
    let schema = s
        .tools
        .iter()
        .find(|t| t["name"] == edit)
        .context("Edit tool missing")?["inputSchema"]
        .clone();
    let fields: Vec<_> = array(&schema, "required")?
        .iter()
        .filter(|k| *k != "feature_id")
        .collect();
    ensure!(fields.len() == 1, "Expected one edit payload: {schema}");
    let mut payload = json!({"feature_id":last(&made,"features")?["id"]});
    payload[fields[0].as_str().context("Edit payload key missing")?] = args;
    s.call(&edit, payload)?;
    ensure!(
        array(&s.model()?["document"]["history"], "features")?.len()
            == array(&made, "features")?.len(),
        "Editing appended history"
    );
    let scene = s.call("solid_scene", json!({}))?;
    ensure!(
        array(&scene, "errors")?.is_empty() && array(&scene, "bodies")?.len() == body_count,
        "Wrong edited geometry: {scene}"
    );
    ensure!(
        array(&scene, "bodies")?
            .iter()
            .all(|b| b["mesh"]["positions"]
                .as_array()
                .is_some_and(|p| !p.is_empty())),
        "Edited body mesh missing"
    );
    Ok(())
}

pub(super) fn run(s: &mut Scenario, empty: &str) -> Result<()> {
    begin(s, empty)?;
    for (name, args) in [
        ("sketch_set_grid_step", json!({"step_mm":5})),
        ("sketch_set_dimension_style", json!({"style":"iso"})),
        ("sketch_eval_expression", json!({"text":"1200 / 5"})),
        (
            "sketch_preview_line",
            json!({"from":{"x":0,"y":0},"to_raw":{"x":20,"y":0},"ctrl_held":true}),
        ),
        (
            "sketch_preview_line_locked",
            json!({"from":{"x":0,"y":0},"to_hint":{"x":20,"y":0},"length_mm":20,"ctrl_held":true}),
        ),
        (
            "sketch_add_line_locked",
            json!({"from":{"x":0,"y":0},"to_hint":{"x":20,"y":0},"length_mm":20,"ctrl_held":true}),
        ),
        (
            "sketch_add_midpoint_line",
            json!({"mid_raw":{"x":50,"y":0},"end_raw":{"x":60,"y":0},"ctrl_held":true}),
        ),
        ("sketch_add_point", json!({"position":{"x":80,"y":0}})),
    ] {
        s.call(name, args)?;
    }
    let point = array(&active(s)?, "entities")?
        .iter()
        .rev()
        .find(|e| e["kind"] == "point")
        .context("Point missing")?["id"]
        .clone();
    s.call(
        "sketch_move_point",
        json!({"point_id":point,"to_raw":{"x":85,"y":5},"ctrl_held":true,"phase":"single"}),
    )?;
    for _ in 0..2 {
        s.call("sketch_toggle_fix", json!({"entity_ids":[point]}))?;
    }
    s.call("sketch_delete_entities", json!({"entity_ids":[point]}))?;
    s.call("sketch_undo", json!({}))?;
    s.call("sketch_redo", json!({}))?;
    ensure!(
        !array(&active(s)?, "entities")?
            .iter()
            .any(|e| e["id"] == point),
        "Redo did not remove point"
    );
    for (name, args) in [
        (
            "sketch_add_rectangle",
            json!({"mode":"center","p1":{"x":30,"y":40},"p2":{"x":40,"y":50},"ctrl_held":true}),
        ),
        (
            "sketch_add_rectangle_locked",
            json!({"mode":"two_point","anchor":{"x":60,"y":30},"corner_hint":{"x":80,"y":50},"width_mm":20,"height_mm":20,"ctrl_held":true}),
        ),
        (
            "sketch_add_circle",
            json!({"mode":"two_point","p1":{"x":100,"y":30},"p2":{"x":120,"y":30},"ctrl_held":true}),
        ),
        (
            "sketch_add_circle_locked",
            json!({"mode":"center_diameter","anchor":{"x":150,"y":40},"edge_hint":{"x":160,"y":40},"diameter_mm":20,"ctrl_held":true}),
        ),
        (
            "sketch_add_arc_3pt",
            json!({"p1":{"x":0,"y":80},"p2":{"x":20,"y":80},"p3":{"x":10,"y":90},"ctrl_held":true}),
        ),
        (
            "sketch_add_arc_center",
            json!({"center":{"x":50,"y":80},"start":{"x":60,"y":80},"sweep":{"x":50,"y":90},"ctrl_held":true}),
        ),
    ] {
        s.call(name, args)?;
    }
    for (i, mode) in ["center_to_center", "overall", "center_point"]
        .into_iter()
        .enumerate()
    {
        s.call("sketch_add_slot",json!({"mode":mode,"p1":{"x":90+i*40,"y":80},"p2":{"x":110+i*40,"y":80},"cursor":{"x":100+i*40,"y":85},"width_mm":10}))?;
    }
    s.call(
        "sketch_add_spline",
        json!({"points":[{"x":0,"y":120},{"x":20,"y":135},{"x":40,"y":120}]}),
    )?;
    s.call("sketch_polygon",json!({"center":{"x":80,"y":120},"edge_count":6,"radius_text":"12","rotation_deg":0,"mode":"inscribed"}))?;
    ensure!(
        array(&active(s)?, "entities")?.len() > 30,
        "Primitive geometry missing"
    );
    s.call("sketch_finish", json!({}))?;
    s.call("sketch_finished", json!({}))?;
    s.call("sketch_profiles", json!({}))?;
    s.call("sketch_edit", json!({"name":"Sketch1"}))?;
    s.call("sketch_finish", json!({}))?;
    s.check("sketch primitives and dimensioned layout");

    begin(s, empty)?;
    let id = line(s, [0., 0.], [20., 0.])?;
    s.call(
        "sketch_add_constraint",
        json!({"type":"horizontal","entity":id}),
    )?;
    let second = line(s, [0., 20.], [20., 20.])?;
    s.call("sketch_add_constraints",json!({"constraints":[{"type":"horizontal","entity":second},{"type":"equal","a":id,"b":second}]}))?;
    s.call(
        "sketch_add_dimension",
        json!({"entities":[id],"text_pos":{"x":10,"y":-10},"value_text":"20"}),
    )?;
    let dimension = last(&active(s)?, "dimensions")?["constraint_id"].clone();
    s.call(
        "sketch_edit_dimension",
        json!({"constraint_id":dimension,"text":"25"}),
    )?;
    s.call(
        "sketch_move_dimension",
        json!({"constraint_id":dimension,"text_pos":{"x":12,"y":-15}}),
    )?;
    let state = active(s)?;
    let measured = array(&state, "entities")?
        .iter()
        .find(|e| e["id"] == id)
        .context("Dimensioned line missing")?;
    let dx = number(&measured["end"]["x"])? - number(&measured["start"]["x"])?;
    let dy = number(&measured["end"]["y"])? - number(&measured["start"]["y"])?;
    ensure!(
        (dx.hypot(dy) - 25.).abs() < 1e-5,
        "Dimension did not drive length"
    );
    s.call(
        "sketch_delete_dimension",
        json!({"constraint_id":dimension}),
    )?;
    s.check("driving dimensions and geometric constraints");
    for operation in ["fillet", "chamfer"] {
        begin(s, empty)?;
        let l1 = line(s, [0., 0.], [30., 0.])?;
        let l2 = line(s, [0., 0.], [0., 30.])?;
        if operation == "fillet" {
            s.call(
                "sketch_preview_fillet",
                json!({"l1":l1,"l2":l2,"radius_text":"4"}),
            )?;
        }
        s.call(
            &format!("sketch_{operation}"),
            if operation == "fillet" {
                json!({"l1":l1,"l2":l2,"radius_text":"4"})
            } else {
                json!({"l1":l1,"l2":l2,"distance_text":"4"})
            },
        )?;
        ensure!(
            array(&active(s)?, "entities")?
                .iter()
                .filter(|e| e["kind"] != "point")
                .count()
                >= 3,
            "Corner treatment geometry missing"
        );
        s.check(&format!("sketch {operation} joinery corner"));
    }
    begin(s, empty)?;
    let id = circle(s)?;
    for name in ["sketch_preview_offset", "sketch_offset"] {
        s.call(
            name,
            json!({"entity":id,"distance_text":"2","cursor":{"x":15,"y":0}}),
        )?;
    }
    let axis = line(s, [30., -30.], [30., 30.])?;
    for (name, args) in [
        ("sketch_mirror", json!({"entity_ids":[id],"axis_line":axis})),
        (
            "sketch_move_copy",
            json!({"entity_ids":[id],"dx":0,"dy":40,"copy":true}),
        ),
        (
            "sketch_scale",
            json!({"entity_ids":[id],"origin":{"x":0,"y":0},"factor_text":"1.2"}),
        ),
        (
            "sketch_rectangular_pattern",
            json!({"entity_ids":[id],"direction":{"x":1,"y":0},"spacing":50,"count":3}),
        ),
        (
            "sketch_circular_pattern",
            json!({"entity_ids":[id],"center":{"x":0,"y":100},"count":3,"total_angle_deg":180}),
        ),
    ] {
        s.call(name, args)?;
    }
    ensure!(
        array(&active(s)?, "entities")?
            .iter()
            .filter(|e| e["kind"] == "circle")
            .count()
            >= 7,
        "Transformed circles missing"
    );
    s.check("sketch offset and transforms");
    begin(s, empty)?;
    let id = line(s, [0., 0.], [30., 0.])?;
    line(s, [10., -10.], [10., 10.])?;
    line(s, [20., -10.], [20., 10.])?;
    for name in ["sketch_preview_trim", "sketch_trim"] {
        s.call(name, json!({"entity":id,"click":{"x":15,"y":0}}))?;
    }
    begin(s, empty)?;
    let short = line(s, [0., 0.], [10., 0.])?;
    line(s, [20., -10.], [20., 10.])?;
    s.call(
        "sketch_extend",
        json!({"entity":short,"click":{"x":9,"y":0}}),
    )?;
    s.call("sketch_break", json!({"entity":short,"at":{"x":5,"y":0}}))?;
    ensure!(
        array(&active(s)?, "entities")?
            .iter()
            .filter(|e| e["kind"] == "line")
            .count()
            == 3,
        "Break/extend geometry changed"
    );
    s.check("sketch trim, extend and break");

    for operation in [
        "shell",
        "move_copy",
        "mirror",
        "rectangular_pattern",
        "circular_pattern",
        "split_body",
        "fillet",
        "chamfer",
        "hole",
    ] {
        let b = stock(s, empty)?;
        let (args, count) = match operation {
            "shell" => (
                json!({"body_id":b["id"],"face_ids":[top_face(&b)?["id"]],"thickness":2,"inward":true}),
                1,
            ),
            "move_copy" => (
                json!({"body_ids":[b["id"]],"translation":{"x":30,"y":0,"z":0},"rotation":[0,0,0,1],"pivot":{"x":0,"y":0,"z":0},"copy":true}),
                2,
            ),
            "mirror" => (
                json!({"body_ids":[b["id"]],"plane":{"type":"origin_plane","plane":"yz"}}),
                2,
            ),
            "rectangular_pattern" => (
                json!({"body_ids":[b["id"]],"direction":{"x":1,"y":0,"z":0},"spacing":30,"count":3,"second_direction":null,"second_spacing":0,"second_count":1}),
                3,
            ),
            "circular_pattern" => (
                json!({"body_ids":[b["id"]],"axis_origin":{"x":50,"y":0,"z":0},"axis_direction":{"x":0,"y":0,"z":1},"count":4,"total_angle_deg":360}),
                4,
            ),
            "split_body" => {
                s.call(
                    "construction_plane_offset",
                    json!({"reference":{"type":"origin_plane","plane":"xy"},"distance":10}),
                )?;
                let planes = s.call("construction_plane_definitions", json!({}))?;
                (
                    json!({"body_id":b["id"],"plane":{"type":"datum_plane","datum_id":planes[0]["datum_id"]}}),
                    2,
                )
            }
            "fillet" => (
                json!({"body_id":b["id"],"edge_ids":[b["edges"][0]["id"]],"tangent_chain":false,"radius":2}),
                1,
            ),
            "chamfer" => (
                json!({"body_id":b["id"],"edge_ids":[b["edges"][0]["id"]],"tangent_chain":false,"distance":2}),
                1,
            ),
            "hole" => {
                let face = top_face(&b)?;
                let mut delta = [0.; 3];
                for i in 0..3 {
                    delta[i] = [0., 0., 20.][i] - number(&face["plane"]["origin"][i])?;
                }
                let dot = |axis: &str| -> Result<f64> {
                    (0..3)
                        .map(|i| Ok(number(&face["plane"][axis][i])? * delta[i]))
                        .sum()
                };
                (
                    json!({"body_id":b["id"],"face_id":face["id"],"position":{"x":dot("u")?,"y":dot("v")?},"diameter":5,"extent":{"type":"through_all"},"bottom_style":"flat","drill_point_angle_deg":118,"flip":false}),
                    1,
                )
            }
            _ => unreachable!(),
        };
        feature(s, &format!("solid_{operation}"), args, count)?;
        s.check(&format!("solid {operation} and edit"));
    }
    for operation in ["join", "cut", "intersect"] {
        let b = stock(s, empty)?;
        s.call("solid_move_copy",json!({"body_ids":[b["id"]],"translation":{"x":10,"y":0,"z":0},"pivot":{"x":0,"y":0,"z":0},"copy":true}))?;
        let scene = s.call("solid_scene", json!({}))?;
        let other = array(&scene, "bodies")?
            .iter()
            .find(|p| p["id"] != b["id"])
            .context("Copied stock missing")?["id"]
            .clone();
        feature(
            s,
            "solid_combine",
            json!({"target_body_id":b["id"],"tool_body_ids":[other],"operation":operation,"keep_tools":false}),
            1,
        )?;
        s.check(&format!("boolean {operation} and edit"));
    }
    stock(s, empty)?;
    let id = last(&s.model()?["document"]["history"], "features")?["id"].clone();
    s.call("solid_edit_extrude",json!({"feature_id":id,"extrude":{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":25},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}}))?;
    let scene = s.call("solid_scene", json!({}))?;
    let positions = array(&scene["bodies"][0]["mesh"], "positions")?;
    let zs: Vec<_> = positions
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| number(&p[2]))
        .collect::<Result<_>>()?;
    ensure!(
        (zs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - zs.iter().copied().fold(f64::INFINITY, f64::min)
            - 25.)
            .abs()
            < 1e-6,
        "Extrude edit did not change height"
    );
    s.check("extrude edit changes stock height");
    begin(s, empty)?;
    s.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":10,"y":0},"p2":{"x":20,"y":15},"ctrl_held":true}),
    )?;
    s.call("sketch_finish", json!({}))?;
    feature(
        s,
        "solid_revolve",
        json!({"sketch_name":"Sketch1","profile_indices":[0],"axis_origin":{"x":0,"y":0},"axis_direction":{"x":0,"y":1},"angle_deg":360,"flip":false,"operation":"new_body","target_body_ids":[]}),
        1,
    )?;
    s.check("revolved bench hardware and edit");
    begin(s, empty)?;
    s.call("sketch_add_arc_center",json!({"center":{"x":0,"y":0},"start":{"x":20,"y":0},"sweep":{"x":0,"y":20},"ctrl_held":true}))?;
    let arc = arc_id(s)?;
    s.call("sketch_finish", json!({}))?;
    feature(
        s,
        "solid_rib",
        json!({"sketch_name":"Sketch1","line_entity_ids":[arc],"thickness":2,"depth":5,"extent":{"type":"distance","depth":5},"symmetric":false,"flip":false,"operation":"new_body","target_body_ids":[]}),
        1,
    )?;
    s.check("curved support rib and edit");
    begin(s, empty)?;
    s.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":-2,"y":-2},"p2":{"x":2,"y":2},"ctrl_held":true}),
    )?;
    s.call("sketch_finish", json!({}))?;
    s.call(
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"yz"}}),
    )?;
    s.call("sketch_add_arc_center",json!({"center":{"x":-20,"y":0},"start":{"x":0,"y":0},"sweep":{"x":-20,"y":20},"ctrl_held":true}))?;
    let arc = arc_id(s)?;
    s.call("sketch_finish", json!({}))?;
    feature(
        s,
        "solid_sweep",
        json!({"profile":{"sketch_name":"Sketch1","profile_index":0},"path_sketch_name":"Sketch2","path_entity_ids":[arc],"operation":"new_body","target_body_ids":[],"guide_rail":null,"orientation":"corrected_frenet","transition":"round_corner","force_c1":true}),
        1,
    )?;
    s.check("swept handrail and edit");
    begin(s, empty)?;
    s.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":-10,"y":-10},"p2":{"x":10,"y":10},"ctrl_held":true}),
    )?;
    s.call("sketch_finish", json!({}))?;
    s.call(
        "construction_plane_offset",
        json!({"reference":{"type":"origin_plane","plane":"xy"},"distance":30}),
    )?;
    let planes = s.call("construction_plane_definitions", json!({}))?;
    s.call(
        "sketch_begin",
        json!({"plane":{"type":"datum_plane","datum_id":planes[0]["datum_id"]}}),
    )?;
    s.call(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":-6,"y":-6},"p2":{"x":6,"y":6},"ctrl_held":true}),
    )?;
    s.call("sketch_finish", json!({}))?;
    feature(
        s,
        "solid_loft",
        json!({"sections":[{"sketch_name":"Sketch1","profile_index":0},{"sketch_name":"Sketch2","profile_index":0}],"ruled":false,"operation":"new_body","target_body_ids":[]}),
        1,
    )?;
    s.check("lofted foot pad and edit");
    stock(s, empty)?;
    let exported = s.call("solid_export_step", json!({}))?;
    reset(s, empty)?;
    feature(
        s,
        "solid_import_step",
        json!({"file_name":"joinery-reference.step","data_base64":exported["bytes_base64"]}),
        1,
    )?;
    s.check("reference STEP import and replacement");
    reset(s, empty)
}
fn top_face(b: &Value) -> Result<&Value> {
    array(b, "faces")?
        .iter()
        .find(|f| f["plane"]["normal"][2].as_f64().is_some_and(|n| n > 0.99))
        .context("Top face missing")
}
fn arc_id(s: &mut Scenario) -> Result<Value> {
    array(&active(s)?, "entities")?
        .iter()
        .find(|e| e["kind"] == "arc")
        .map(|e| e["id"].clone())
        .context("Arc missing")
}
