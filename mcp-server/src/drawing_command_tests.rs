use super::*;

#[test]
#[ignore = "Complete bench construction and native drafting; invoke explicitly when changing the package"]
fn bench_drawing_package_replays_and_preserves_current_exports_after_reopen() {
    let recipe = limo_cad_recipes::find("garden-bench").unwrap();
    let replay = run_script(recipe.source, None, None, "fast", 1.).unwrap();
    let exports = &replay["exports"];
    let drawing = &exports["drawing_document"];
    assert_eq!(drawing["sheets"].as_array().unwrap().len(), 22);
    let mut server = CadServer::new().unwrap();
    server
        .call_tool(
            "cad_load_project_model",
            json!({"model_json":serde_json::to_string(&exports["final_model"]).unwrap()}),
        )
        .unwrap();
    let reopened = tool(&mut server, "drawing_document", json!({})).unwrap();
    assert_eq!(reopened["sheets"], drawing["sheets"]);
    for record in exports["drawings"].as_array().unwrap() {
        for format in ["svg", "dxf"] {
            let output = tool(
                &mut server,
                "drawing_export",
                json!({"sheet_id":record["sheet_id"],"format":format}),
            )
            .unwrap();
            assert_eq!(
                output["content"], record[format],
                "{} {format} changed on reopen",
                record["part"]
            );
        }
        if let Some(directory) = std::env::var_os("LIMO_CAD_DRAWING_REPRO_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                directory.join(format!("bench-{}.svg", record["part"].as_str().unwrap())),
                record["svg"].as_str().unwrap(),
            )
            .unwrap();
        }
    }
    let parts = exports["verification_inputs"]["parts"].as_array().unwrap();
    let picket = parts
        .iter()
        .find(|p| p["stock_mm"] == json!([85, 25, 340]))
        .unwrap();
    let extrude = exports["final_model"]["extrudes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["feature_id"] == picket["feature_id"])
        .unwrap();
    let mut changed = extrude.clone();
    changed["extent"]["distance"] = json!(460.);
    changed.as_object_mut().unwrap().remove("feature_id");
    server
        .call_tool(
            "solid_edit_extrude",
            json!({"feature_id":picket["feature_id"],"extrude":changed}),
        )
        .unwrap();
    let record = exports["drawings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["part"] == "picket")
        .unwrap();
    let updated = tool(
        &mut server,
        "drawing_export",
        json!({"sheet_id":record["sheet_id"],"format":"svg"}),
    )
    .unwrap();
    assert!(updated["content"].as_str().unwrap().contains("460.0 mm"));
    assert_ne!(updated["content"], record["svg"]);
}

fn tool(server: &mut CadServer, name: &str, arguments: Value) -> Result<Value, String> {
    if name.starts_with("drawing_") {
        let spec = tool_specs().iter().find(|tool| tool.name == name).unwrap();
        crate::tests::schema_accepts(&spec.input_schema, &arguments)
            .map_err(|error| format!("{name} schema: {error}"))?;
    }
    let mut result = server.call_tool(name, arguments)?;
    if let Some(object) = result.as_object_mut() {
        object.remove("_disclosure");
    }
    Ok(result)
}
fn fixture() -> (CadServer, Value, Value, Value) {
    let mut server = CadServer::new().unwrap();
    tool(
        &mut server,
        "cad_interface",
        json!({"action":"script","recipe":"mounting-plate","mode":"fast"}),
    )
    .unwrap();
    tool(&mut server, "drawing_create_sheet", json!({"name":"Machining","format":"a4","orientation":"landscape","title_block":{"drawing_number":"PLATE-1"}})).unwrap();
    tool(&mut server, "drawing_add_view", json!({"sheet_id":1,"view":{"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,80.],"scale":1.}})).unwrap();
    let projection = tool(
        &mut server,
        "drawing_projection",
        json!({"direction":[0.,0.,1.],"up":[0.,1.,0.]}),
    )
    .unwrap();
    let circle = &projection["circles"][0];
    let signature = &projection["topology_signatures"][circle["body_id"].to_string()];
    let feature = json!({"body_id":circle["body_id"],"edge_id":circle["edge_id"],"edge_key":circle["edge_key"],"topology_signature":signature,
        "fallback_center":circle["center_model"],"fallback_normal":circle["normal_model"],"fallback_radius":circle["radius"],"closed":circle["closed"]});
    let scene = tool(&mut server, "solid_scene", json!({})).unwrap();
    let edge = scene["bodies"][0]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["circle"].is_null() && e["points"].as_array().unwrap().len() == 2)
        .unwrap();
    let point = |index| {
        let p = &edge["points"][index];
        json!([p["x"], p["y"], p["z"]])
    };
    let line = json!({"body_id":scene["bodies"][0]["id"],"edge_id":edge["id"],"edge_key":edge["key"],"topology_signature":signature,"fallback_start":point(0),"fallback_end":point(1)});
    (server, feature, line, projection)
}

#[test]
fn specialized_drawing_commands_preserve_exports_and_reject_stale_edits_atomically() {
    let (mut server, feature, line, _) = fixture();
    for annotation in [
        json!({"kind":"center_mark","view_id":1,"feature":feature,"extension":4.}),
        json!({"kind":"hole_note","view_id":1,"feature":feature,"position":[160.,60.],"quantity":4,"diameter":6.,"through_all":true,"note":"Inspect hole locations"}),
        json!({"kind":"gdt_frame","view_id":1,"attachment":{"type":"line","reference":line},"position":[155.,100.],"characteristic":"straightness","tolerance":0.2}),
        json!({"kind":"weld_symbol","view_id":1,"attachment":line,"position":[160.,120.],"weld_type":"fillet","side":"arrow","size":3.,"tail":"EXAMPLE ONLY"}),
        json!({"kind":"radial_dimension","view_id":1,"feature":feature,"mode":"diameter","leader_angle_deg":45.,"offset":15.,"presentation":{"dual_units":{"unit":"inch","precision":3,"placement":"bracketed"}}}),
    ] {
        tool(
            &mut server,
            "drawing_add_annotation",
            json!({"sheet_id":1,"annotation":annotation}),
        )
        .unwrap();
    }
    let before = tool(&mut server, "drawing_document", json!({})).unwrap();
    let mut annotation = before["sheets"][0]["annotations"][1].clone();
    annotation["position"] = json!([175., 65.]);
    let updated = tool(
        &mut server,
        "drawing_update_annotation",
        json!({"sheet_id":1,"annotation":annotation}),
    )
    .unwrap();
    assert_eq!(updated["next_annotation_id"], before["next_annotation_id"]);
    assert_eq!(
        updated["sheets"][0]["annotations"][1]["position"],
        json!([175., 65.])
    );
    for format in ["svg", "dxf"] {
        let exported = tool(
            &mut server,
            "drawing_export",
            json!({"sheet_id":1,"format":format}),
        )
        .unwrap();
        assert!(exported["content"].as_str().unwrap().len() > 500);
        let model = tool(&mut server, "cad_project_model", json!({})).unwrap();
        let mut restored = CadServer::new().unwrap();
        restored
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        let reopened = restored
            .call_tool("drawing_export", json!({"sheet_id":1,"format":format}))
            .unwrap();
        assert_eq!(exported["content"], reopened["content"]);
    }
    for stale in [
        json!({"topology_signature":"old topology"}),
        json!({"edge_key":"missing edge"}),
    ] {
        let mut annotation = updated["sheets"][0]["annotations"][0].clone();
        for (key, value) in stale.as_object().unwrap() {
            annotation["feature"][key] = value.clone();
        }
        assert!(tool(
            &mut server,
            "drawing_update_annotation",
            json!({"sheet_id":1,"annotation":annotation})
        )
        .is_err());
        assert_eq!(
            updated,
            tool(&mut server, "drawing_document", json!({})).unwrap()
        );
    }
    let mut view = updated["sheets"][0]["views"][0].clone();
    view["position"] = json!([110., 85.]);
    let moved = tool(
        &mut server,
        "drawing_update_view",
        json!({"sheet_id":1,"view":view}),
    )
    .unwrap();
    assert_eq!(
        moved["sheets"][0]["annotations"],
        updated["sheets"][0]["annotations"]
    );
    tool(&mut server, "drawing_add_view", json!({"sheet_id":1,"view":{"name":"Related","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[110.,140.],"scale":1.,"parent_view_id":1,"alignment":"vertical"}})).unwrap();
    let dependent = tool(&mut server, "drawing_document", json!({})).unwrap();
    let mut root = dependent["sheets"][0]["views"][0].clone();
    root["position"] = json!([125., 95.]);
    root["scale"] = json!(1.5);
    let dependent = tool(
        &mut server,
        "drawing_update_view",
        json!({"sheet_id":1,"view":root,"rescale_group":true}),
    )
    .unwrap();
    assert_eq!(
        dependent["sheets"][0]["views"][1]["position"],
        json!([125., 140.])
    );
    assert_eq!(dependent["sheets"][0]["views"][1]["scale"], json!(1.5));
    assert!(tool(
        &mut server,
        "drawing_delete_view",
        json!({"sheet_id":1,"view_id":1})
    )
    .is_err());
    assert_eq!(
        dependent,
        tool(&mut server, "drawing_document", json!({})).unwrap()
    );
    let deleted = tool(
        &mut server,
        "drawing_delete_view",
        json!({"sheet_id":1,"view_id":1,"cascade":true}),
    )
    .unwrap();
    assert_eq!(deleted["sheets"][0]["views"], json!([]));
    assert_eq!(deleted["sheets"][0]["annotations"], json!([]));
}

#[test]
fn drawing_templates_are_copied_and_content_edits_revoke_explicit_release() {
    let (mut server, _, _, _) = fixture();
    tool(
        &mut server,
        "drawing_create_template",
        json!({"sheet_id":1,"name":"Workshop"}),
    )
    .unwrap();
    tool(
        &mut server,
        "drawing_create_sheet",
        json!({"name":"Second","format":"a3","orientation":"landscape"}),
    )
    .unwrap();
    tool(
        &mut server,
        "drawing_apply_template",
        json!({"sheet_id":2,"template_id":1}),
    )
    .unwrap();
    let before = tool(&mut server, "drawing_document", json!({})).unwrap();
    assert_eq!(
        before["sheets"][1]["title_block"]["drawing_number"],
        "PLATE-1"
    );
    let deleted = tool(
        &mut server,
        "drawing_delete_template",
        json!({"template_id":1}),
    )
    .unwrap();
    assert_eq!(deleted["sheets"], before["sheets"]);
    assert!(tool(&mut server, "drawing_set_release", json!({"sheet_id":1,"release":{"status":"released","released_revision":"A","released_at":"2026-10-06"}})).is_err());
    tool(&mut server, "drawing_add_revision", json!({"sheet_id":1,"revision":{"revision":"A","date":"2026-10-06","description":"Digital review fixture","status":"released"},"position":[10.,10.]})).unwrap();
    let issued = tool(&mut server, "drawing_document", json!({})).unwrap();
    assert_eq!(issued["sheets"][0]["release"]["status"], "released");
    tool(&mut server, "drawing_select_sheet", json!({"sheet_id":2})).unwrap();
    assert_eq!(
        tool(&mut server, "drawing_document", json!({})).unwrap()["sheets"][0]["release"],
        issued["sheets"][0]["release"]
    );
    let draft = tool(
        &mut server,
        "drawing_add_note",
        json!({"sheet_id":1,"text":"Changed after issue","position":[10.,120.]}),
    )
    .unwrap();
    assert_eq!(draft["sheets"][0]["release"]["status"], "draft");
    assert_eq!(
        draft["sheets"][0]["revisions"],
        issued["sheets"][0]["revisions"]
    );
    let removed = tool(
        &mut server,
        "drawing_delete_annotation",
        json!({"sheet_id":1,"annotation_id":draft["sheets"][0]["annotations"][0]["id"]}),
    )
    .unwrap();
    assert_eq!(removed["sheets"][0]["annotations"], json!([]));
    tool(
        &mut server,
        "drawing_set_release",
        json!({"sheet_id":1,"release":issued["sheets"][0]["release"]}),
    )
    .unwrap();
    let reviewed = tool(&mut server,"drawing_add_revision",json!({"sheet_id":1,"revision":{"revision":"B","date":"2026-10-07","description":"Next review","status":"in_review"}})).unwrap();
    assert_eq!(reviewed["sheets"][0]["release"]["status"], "in_review");
    assert_eq!(reviewed["sheets"][0]["release"]["released_revision"], "A");
    assert_eq!(reviewed["sheets"][0]["title_block"]["revision"], "B");
    assert_eq!(
        reviewed["sheets"][0]["revisions"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn issued_drawing_survives_reopen_and_recompute_but_geometry_edit_revokes_release() {
    let (mut server, _, _, _) = fixture();
    tool(&mut server,"drawing_add_revision",json!({"sheet_id":1,"revision":{"revision":"A","date":"2026-10-06","description":"Digital review","status":"released"}})).unwrap();
    let issued = tool(&mut server, "drawing_document", json!({})).unwrap();
    let model = tool(&mut server, "cad_project_model", json!({})).unwrap();
    let mut restored = CadServer::new().unwrap();
    restored
        .call_tool("cad_load_project_model", json!({"model_json":model}))
        .unwrap();
    assert_eq!(
        tool(&mut restored, "drawing_document", json!({})).unwrap(),
        issued
    );
    restored.call_tool("solid_recompute", json!({})).unwrap();
    assert_eq!(
        tool(&mut restored, "drawing_document", json!({})).unwrap(),
        issued
    );
    let model: Value = serde_json::from_str(model.as_str().unwrap()).unwrap();
    let definition = &model["extrudes"][0];
    let mut extrude = definition.clone();
    extrude["extent"]["distance"] = json!(definition["extent"]["distance"].as_f64().unwrap() + 1.);
    extrude.as_object_mut().unwrap().remove("feature_id");
    restored
        .call_tool(
            "solid_edit_extrude",
            json!({"feature_id":definition["feature_id"],"extrude":extrude}),
        )
        .unwrap();
    let changed = tool(&mut restored, "drawing_document", json!({})).unwrap();
    assert_eq!(changed["sheets"][0]["release"]["status"], "draft");
    assert_eq!(changed["sheets"][0]["release"]["released_revision"], "A");
    assert_eq!(
        changed["sheets"][0]["revisions"],
        issued["sheets"][0]["revisions"]
    );
}

#[test]
fn occurrence_pose_edits_revoke_assembly_sheet_release_but_keep_definition_sheets_issued() {
    let (mut server, _, _, _) = fixture();
    let drawing = tool(&mut server, "drawing_document", json!({})).unwrap();
    let mut placed_view = drawing["sheets"][0]["views"][0].clone();
    placed_view["scope"] = json!("assembly");
    tool(
        &mut server,
        "drawing_update_view",
        json!({"sheet_id":1,"view":placed_view}),
    )
    .unwrap();
    let assembly = tool(&mut server, "assembly_document", json!({})).unwrap();
    let occurrence = &assembly["component_structure"]["occurrences"][0];
    let scene = tool(&mut server, "solid_scene", json!({})).unwrap();
    let body_ids: Vec<Value> = scene["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| body["id"].clone())
        .collect();
    tool(&mut server, "drawing_create_sheet", json!({"name":"Definition","format":"a4","orientation":"landscape","title_block":{"drawing_number":"PLATE-2"}})).unwrap();
    tool(&mut server, "drawing_add_view", json!({"sheet_id":2,"view":{"name":"Definition top","kind":"top","scope":"definition","body_ids":body_ids,"direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,80.],"scale":1.}})).unwrap();
    for sheet_id in [1, 2] {
        tool(&mut server,"drawing_add_revision",json!({"sheet_id":sheet_id,"revision":{"revision":"A","date":"2026-10-06","description":"Digital review","status":"released"}})).unwrap();
    }
    let before = tool(&mut server, "drawing_document", json!({})).unwrap();
    tool(&mut server,"assembly_set_occurrence_pose",json!({"occurrence_id":occurrence["id"],"local_pose":{"translation":[10.,0.,0.],"rotation":[0.,0.,0.,1.]}})).unwrap();
    let after = tool(&mut server, "drawing_document", json!({})).unwrap();
    assert_eq!(after["sheets"][0]["release"]["status"], "draft");
    assert_eq!(after["sheets"][1], before["sheets"][1]);
    assert_eq!(
        after["sheets"][0]["revisions"],
        before["sheets"][0]["revisions"]
    );
    assert_eq!(after["sheets"][0]["release"]["released_revision"], "A");
    tool(
        &mut server,
        "assembly_update_occurrence",
        json!({"occurrence":{"id":occurrence["id"],"visible":false}}),
    )
    .unwrap();
    let before_reissue = tool(&mut server, "drawing_document", json!({})).unwrap();
    let error = tool(
        &mut server,
        "drawing_set_release",
        json!({"sheet_id":1,"release":before["sheets"][0]["release"]}),
    )
    .unwrap_err();
    assert!(error.contains("visible body occurrence"), "{error}");
    assert_eq!(
        tool(&mut server, "drawing_document", json!({})).unwrap(),
        before_reissue
    );
}
