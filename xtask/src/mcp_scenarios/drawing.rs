use super::*;
pub(super) fn run(s: &mut Scenario) -> Result<()> {
    s.attach_or_launch()?;
    ensure!(
        array(&s.call("solid_scene", json!({}))?, "bodies")?.is_empty(),
        "Drawing lesson requires an empty document"
    );
    s.call(
        "sketch_begin",
        json!({"plane":{"type":"origin_plane","plane":"xy"}}),
    )?;
    s.call("sketch_add_rectangle_locked",json!({"mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":40,"y":25},"width_mm":40,"height_mm":25,"ctrl_held":true}))?;
    s.call("sketch_finish", json!({}))?;
    let sketch = last(&s.model()?, "sketches")?["name"].clone();
    let extrude = |height| json!({"sketch_name":sketch,"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":height},"taper_angle_deg":0,"flip":false,"target_body_ids":[]});
    s.call("solid_extrude", extrude(6))?;
    let stock = s.call("cad_document", json!({}))?;
    let doc=s.call("drawing_create_sheet",json!({"name":"Bracket stock — fabrication","format":"a4","orientation":"landscape","title_block":{"title":"Parametric bracket stock","drawing_number":"GOLDEN-001","revision":"A"}}))?;
    let sheet = doc["active_sheet_id"].clone();
    for (name, kind, direction, up, position) in [
        ("Top", "top", [0, 0, 1], [0, 1, 0], [80, 65]),
        ("Front", "front", [0, -1, 0], [0, 0, 1], [80, 125]),
        ("Isometric", "isometric", [1, -1, 1], [0, 0, 1], [185, 80]),
    ] {
        let result=s.call("drawing_add_view",json!({"sheet_id":sheet,"view":{"name":name,"kind":kind,"direction":direction,"up":up,"position":position,"scale":2,"show_hidden_lines":true}}))?;
        ensure!(result["active_sheet_id"] == sheet, "Active sheet changed");
        let projected = s.call(
            "drawing_projection",
            json!({"direction":direction,"up":up,"include_hidden":true}),
        )?;
        ensure!(
            !array(&projected, "visible")?.is_empty() && !array(&projected, "anchors")?.is_empty(),
            "Exact projection lacks linework/topology"
        );
        if kind == "top" {
            ensure!(
                (number(&projected["bounds"][2])? - number(&projected["bounds"][0])? - 40.).abs()
                    < 0.01
                    && (number(&projected["bounds"][3])? - number(&projected["bounds"][1])? - 25.)
                        .abs()
                        < 0.01,
                "Stock projection bounds changed"
            );
        }
    }
    let drawing = s.call("drawing_document", json!({}))?;
    let front = array(&drawing["sheets"][0], "views")?
        .iter()
        .find(|v| v["kind"] == "front")
        .context("Front view missing")?;
    let projection_args =
        json!({"direction":front["direction"],"up":front["up"],"include_hidden":true});
    let projected = s.call("drawing_projection", projection_args.clone())?;
    let rows = array(&projected, "anchors")?;
    let mut pair = None;
    'find: for a in rows {
        for b in rows {
            if a["body_id"] == b["body_id"]
                && a["edge_id"] == b["edge_id"]
                && a["endpoint"] != b["endpoint"]
                && (number(&a["model_point"][2])? - number(&b["model_point"][2])?).abs() > 5.99
            {
                pair = Some((a, b));
                break 'find;
            }
        }
    }
    let (a, b) = pair.context("Find thickness edge from actual topology")?;
    let reference = |a: &Value| json!({"body_id":a["body_id"],"edge_id":a["edge_id"],"edge_key":a["edge_key"],"endpoint":a["endpoint"],"fallback_point":a["model_point"]});
    let dimension = json!({"sheet_id":sheet,"view_id":front["id"],"first":reference(a),"second":reference(b),"mode":"vertical","offset":12,"presentation":{"tolerance":{"mode":"symmetric","upper":0.2,"lower":0.2}}});
    let untouched = s.call("drawing_document", json!({}))?;
    for (pointer, value) in [
        ("/view_id", json!(999)),
        ("/first/edge_key", json!("stale")),
        ("/second", {
            let mut r = dimension["first"].clone();
            r["fallback_point"] = json!([99, 99, 99]);
            r
        }),
        ("/precision", json!(7)),
        ("/first/body_id", json!(999)),
        ("/first/circle_center", json!(true)),
    ] {
        let mut bad = dimension.clone();
        if let Some(target) = bad.pointer_mut(pointer) {
            *target = value;
        } else if pointer == "/precision" {
            bad["precision"] = value;
        } else {
            bad["first"]["circle_center"] = value;
        }
        let result = s.client.rpc(
            "tools/call",
            json!({"name":"drawing_add_linear_dimension","arguments":bad}),
        )?;
        ensure!(
            result["isError"] == true,
            "Invalid dimension accepted: {bad}"
        );
        ensure!(
            s.call("drawing_document", json!({}))? == untouched,
            "Rejected dimension changed IDs/history"
        );
    }
    s.call("drawing_add_linear_dimension", dimension)?;
    let feature = array(&stock, "features")?
        .iter()
        .find(|f| f["kind"] == "extrude")
        .context("Extrusion feature missing")?;
    s.call(
        "solid_edit_extrude",
        json!({"feature_id":feature["id"],"extrude":extrude(8)}),
    )?;
    let changed = s.call("drawing_projection", projection_args)?;
    let doc = s.call("drawing_document", json!({}))?;
    let annotation = &doc["sheets"][0]["annotations"][0];
    ensure!(
        annotation["kind"] == "linear_dimension"
            && annotation["presentation"]["tolerance"]["upper"] == 0.2,
        "Dimension/tolerance changed"
    );
    let resolve = |r: &Value| -> Result<&Value> {
        array(&changed, "anchors")?
            .iter()
            .find(|a| {
                a["body_id"] == r["body_id"]
                    && a["edge_key"] == r["edge_key"]
                    && a["endpoint"] == r["endpoint"]
            })
            .context("Annotation lost live topology")
    };
    ensure!(
        ((number(&resolve(&annotation["first"])?["model_point"][2])?
            - number(&resolve(&annotation["second"])?["model_point"][2])?)
        .abs()
            - 8.)
            .abs()
            < 1e-7,
        "Dimension did not follow edited thickness"
    );
    s.call(
        "drawing_add_note",
        json!({"sheet_id":sheet,"text":"DEBURR ALL EDGES. DIMENSIONS IN mm.","position":[25,175]}),
    )?;
    let before = s.call("drawing_document", json!({}))?;
    let invalid=s.client.rpc("tools/call",json!({"name":"drawing_add_view","arguments":{"sheet_id":sheet,"view":{"name":"Invalid","kind":"top","direction":[0,0,1],"up":[0,1,0],"position":[0,0],"scale":0}}}))?;
    ensure!(
        invalid["isError"] == true && s.call("drawing_document", json!({}))? == before,
        "Invalid scale changed document/IDs"
    );
    let scratch = s.call(
        "drawing_create_sheet",
        json!({"name":"Scratch","format":"a4","orientation":"portrait"}),
    )?;
    s.call("drawing_select_sheet", json!({"sheet_id":sheet}))?;
    s.call(
        "drawing_delete_sheet",
        json!({"sheet_id":scratch["active_sheet_id"]}),
    )?;
    let final_doc = s.call("drawing_document", json!({}))?;
    ensure!(
        array(&final_doc, "sheets")?.len() == 1
            && array(&final_doc["sheets"][0], "views")?.len() == 3
            && array(&final_doc["sheets"][0], "annotations")?.len() == 2,
        "Final drawing inventory changed"
    );
    if s.session.is_some() {
        let state = s.ui(json!({"action":"inspect"}))?;
        ensure!(
            array(&state["ui"], "canvases")?
                .iter()
                .any(|c| c["name"] == "drawing"),
            "Live renderer lacks drawing canvas"
        );
        if let Some(path) = s.options.get("--save").cloned() {
            s.ui(json!({"action":"file","command":"save","path":path}))?;
            s.ui(json!({"action":"file","command":"open","path":path}))?;
            ensure!(
                s.call("drawing_document", json!({}))? == final_doc,
                "Native reopen changed dimensions/tolerances"
            );
        }
    }
    let persisted = s.call("cad_project_model", json!({}))?;
    let mut restored = Client::start_command(
        Client::worker_command(&s.options["--server"]),
        Some(Duration::from_secs(60)),
    )?;
    restored.call("cad_load_project_model", json!({"model_json":persisted}))?;
    ensure!(
        restored.call("drawing_document", json!({}))? == s.call("drawing_document", json!({}))?,
        "Fresh-process restore changed drawings"
    );
    s.check("drawing: exact projections, thickness dimension 6 to 8 mm, rejected edits, sheet selection/deletion, fresh-process restore");
    Ok(())
}
