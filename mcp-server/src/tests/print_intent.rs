use super::*;

fn data(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_disclosure");
    }
    value
}

#[test]
fn print_intent_mcp_persists_sources_and_rejects_stale_edits_atomically() {
    let (mut server, initial) = mcp_box();
    let body = initial["scene"]["bodies"][0]["id"].clone();
    let before_geometry = data(server.call_tool("solid_scene", json!({})).unwrap());
    let before_assembly = data(server.call_tool("assembly_document", json!({})).unwrap());
    let before_appearance = data(server.call_tool("body_appearances", json!({})).unwrap());
    let before = server.manager.export_project_model().unwrap();
    let mut document = data(server.call_tool("print_intent_get", json!({})).unwrap());
    assert_eq!(document["source_document_id"], Value::Null);
    document["defaults"]["wall_count"] = json!(2);
    document["defaults"]["infill_density_percent"] = json!(15);
    server
        .call_tool(
            "print_intent_set_document",
            json!({"document":document,"expected_model_json":before}),
        )
        .unwrap();
    let current = server.manager.export_project_model().unwrap();
    let result = server.call_tool("print_intent_set_part", json!({
        "body_id":body,"settings":{"wall_count":0,"infill_density_percent":40,"infill_pattern":"gyroid"},"expected_model_json":current
    })).unwrap();
    let namespace = result["source_document_id"].clone();
    assert!(namespace.as_str().is_some());
    let saved = server.manager.export_project_model().unwrap();
    assert!(server
        .call_tool(
            "print_intent_reset_part",
            json!({"body_id":body,"expected_model_json":current})
        )
        .is_err());
    assert_eq!(server.manager.export_project_model().unwrap(), saved);
    let effective = server
        .call_tool(
            "print_intent_effective",
            json!({"target":"portable","body_ids":[body]}),
        )
        .unwrap();
    assert_eq!(effective["parts"][0]["settings"]["wall_count"], 0);
    assert_eq!(effective["parts"][0]["sources"]["wall_count"], "part");
    assert!(!effective["parts"][0]["unsupported"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        data(server.call_tool("solid_scene", json!({})).unwrap()),
        before_geometry
    );
    assert_eq!(
        data(server.call_tool("assembly_document", json!({})).unwrap()),
        before_assembly
    );
    assert_eq!(
        data(server.call_tool("body_appearances", json!({})).unwrap()),
        before_appearance
    );
    server
        .call_tool("cad_load_project_model", json!({"model_json":saved}))
        .unwrap();
    assert_eq!(
        server.call_tool("print_intent_get", json!({})).unwrap()["source_document_id"],
        namespace
    );
    server.call_tool("print_intent_reset_part", json!({"body_id":body,"expected_model_json":server.manager.export_project_model().unwrap()})).unwrap();
    let effective = server
        .call_tool("print_intent_effective", json!({"target":"bambu_studio"}))
        .unwrap();
    assert_eq!(effective["parts"][0]["settings"]["wall_count"], 2);
    assert_eq!(
        effective["parts"][0]["sources"]["wall_count"],
        "project_default"
    );
}

#[test]
fn print_intent_discovery_is_typed_owned_and_not_a_geometry_script() {
    let tools = tool_specs();
    for spec in print_intent_tools::specs()
        .into_iter()
        .chain(print_modifier_tools::specs())
        .chain(print_height_tools::specs())
    {
        assert_eq!(
            interface::group_for(spec.name),
            Some("document/print-intent")
        );
        assert!(!records_in_script(spec.name));
        assert_eq!(tags_for_tool(spec.name).0, FocusPack::Print);
        if matches!(
            spec.name,
            "print_intent_get"
                | "print_intent_effective"
                | "print_modifier_effective"
                | "print_intent_height_binding"
        ) {
            assert!(limo_cad_mcp_mutate::is_live_engine_query(
                spec.engine_method
            ));
            assert!(!is_modeling_mutate(spec.name));
        } else {
            assert!(is_modeling_mutate(spec.name));
            assert!(spec.input_schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("expected_model_json")));
        }
        assert!(tools.iter().any(|tool| tool.name == spec.name));
    }
    let schema = print_intent_tools::specs()
        .into_iter()
        .find(|tool| tool.name == "print_intent_set_part")
        .unwrap()
        .input_schema;
    assert_eq!(
        schema["properties"]["settings"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["properties"]["settings"]["properties"]["infill_density_percent"]["maximum"],
        100
    );
}

#[test]
fn print_modifier_mcp_guarded_roundtrip_keeps_physical_scene_appearance_and_script() {
    let (mut server, initial) = mcp_box();
    let body = initial["scene"]["bodies"][0]["id"].clone();
    let scene = data(server.call_tool("solid_scene", json!({})).unwrap());
    let appearance = data(server.call_tool("body_appearances", json!({})).unwrap());
    let before = server.manager.export_project_model().unwrap();
    let modifier = json!({"id":"01234567-89ab-4cde-8123-456789abcdef","name":"Drive local shell","body_id":body,"enabled":true,
        "primitive":{"kind":"cylinder","radius_mm":2,"height_mm":5},"local_pose":{"translation_mm":[0,0,2.5],"rotation":[0,0,0,1]},"settings":{"wall_count":6,"infill_density_percent":80}});
    server
        .call_tool(
            "print_modifier_create",
            json!({"modifier":modifier,"expected_model_json":before}),
        )
        .unwrap();
    let saved = server.manager.export_project_model().unwrap();
    assert!(server
        .call_tool(
            "print_modifier_reset",
            json!({"id":modifier["id"],"expected_model_json":before})
        )
        .is_err());
    assert_eq!(server.manager.export_project_model().unwrap(), saved);
    let effective = data(
        server
            .call_tool("print_modifier_effective", json!({"target":"portable"}))
            .unwrap(),
    );
    assert_eq!(effective["modifiers"][0]["binding"], "live");
    assert_eq!(effective["modifiers"][0]["settings"]["wall_count"], 6);
    assert_eq!(
        effective["modifiers"][0]["sources"]["wall_count"],
        "modifier"
    );
    assert_eq!(
        effective["modifiers"][0]["unsupported"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    server
        .call_tool("cad_load_project_model", json!({"model_json":saved}))
        .unwrap();
    let normalized_modifier = serde_json::to_value(
        serde_json::from_value::<limo_cad_core::PrintModifierDto>(modifier.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        server.call_tool("print_intent_get", json!({})).unwrap()["modifiers"][0],
        normalized_modifier
    );
    assert_eq!(
        data(server.call_tool("solid_scene", json!({})).unwrap()),
        scene
    );
    assert_eq!(
        data(server.call_tool("body_appearances", json!({})).unwrap()),
        appearance
    );
    server.call_tool("print_modifier_reset",json!({"id":modifier["id"],"expected_model_json":server.manager.export_project_model().unwrap()})).unwrap();
    let reset = server.call_tool("print_intent_get", json!({})).unwrap()["modifiers"][0].clone();
    assert_eq!(reset["primitive"], normalized_modifier["primitive"]);
    assert_eq!(reset["local_pose"], normalized_modifier["local_pose"]);
    assert!(reset["settings"]
        .as_object()
        .unwrap()
        .values()
        .all(Value::is_null));
}

#[test]
fn print_height_mcp_real_kernel_capture_guards_roundtrip_and_explicit_remove() {
    let (mut server, initial) = mcp_box();
    let body = initial["scene"]["bodies"][0]["id"].clone();
    let scene = data(server.call_tool("solid_scene", json!({})).unwrap());
    let appearance = data(server.call_tool("body_appearances", json!({})).unwrap());
    let before = server.manager.export_project_model().unwrap();
    let binding = data(
        server
            .call_tool(
                "print_intent_height_binding",
                json!({"body_id":body,"layout":{"kind":"assembly"}}),
            )
            .unwrap(),
    );
    assert_eq!(binding["occurrences"].as_array().unwrap().len(), 1);
    assert_eq!(binding["occurrences"][0]["min_z_mm"], 0.);
    assert_eq!(binding["occurrences"][0]["max_z_mm"], 10.);
    assert_eq!(
        server.manager.export_project_model().unwrap(),
        before,
        "capture is read-only"
    );
    let range = json!({"name":"Actual print Z band","body_id":body,"enabled":true,"coordinate":"object_bottom","min_z_mm":2,"max_z_mm":7,"layout":{"kind":"assembly"},"settings":{"wall_count":6,"infill_density_percent":80},"speeds":{"outer_wall_mm_s":12}});
    assert!(server
        .call_tool("print_intent_upsert_height_range", json!({"range":range}))
        .is_err());
    let result = server
        .call_tool(
            "print_intent_upsert_height_range",
            json!({"range":range,"expected_model_json":before}),
        )
        .unwrap();
    let saved = server.manager.export_project_model().unwrap();
    let id = result["height_ranges"][0]["id"].clone();
    assert!(server
        .call_tool(
            "print_intent_remove_height",
            json!({"id":id,"expected_model_json":before})
        )
        .is_err());
    assert_eq!(server.manager.export_project_model().unwrap(), saved);
    let effective = server
        .call_tool("print_intent_effective", json!({"target":"portable"}))
        .unwrap();
    assert_eq!(effective["height_ranges"][0]["binding_current"], true);
    assert_eq!(effective["height_ranges"][0]["settings"]["wall_count"], 6);
    assert_eq!(
        effective["height_ranges"][0]["sources"]["wall_count"],
        "height_range"
    );
    assert!(!effective["height_ranges"][0]["unsupported_speeds"]
        .as_array()
        .unwrap()
        .is_empty());
    let qualified = server
        .call_tool("print_intent_effective", json!({"target":"bambu_studio"}))
        .unwrap();
    assert!(qualified["height_ranges"][0]["unsupported"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(qualified["height_ranges"][0]["unsupported_speeds"]
        .as_array()
        .unwrap()
        .is_empty());
    let orca = server
        .call_tool("print_intent_effective", json!({"target":"orca_slicer"}))
        .unwrap();
    assert!(!orca["height_ranges"][0]["unsupported_speeds"]
        .as_array()
        .unwrap()
        .is_empty());
    server
        .call_tool("cad_load_project_model", json!({"model_json":saved}))
        .unwrap();
    assert_eq!(
        server.call_tool("print_intent_get", json!({})).unwrap()["height_ranges"],
        result["height_ranges"]
    );
    assert_eq!(
        data(server.call_tool("solid_scene", json!({})).unwrap()),
        scene
    );
    assert_eq!(
        data(server.call_tool("body_appearances", json!({})).unwrap()),
        appearance
    );
    server
        .call_tool(
            "print_intent_remove_height",
            json!({"id":id,"expected_model_json":server.manager.export_project_model().unwrap()}),
        )
        .unwrap();
    assert!(
        server.call_tool("print_intent_get", json!({})).unwrap()["height_ranges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn bambu_mcp_rejects_stale_and_untyped_requests_without_changing_portable_export() {
    let (mut server, initial) = mcp_box();
    let body = initial["scene"]["bodies"][0]["id"].clone();
    let before = server.manager.export_project_model().unwrap();
    let authored = server
        .call_tool(
            "print_intent_set_part",
            json!({
                "body_id":body,"settings":{"wall_count":6},"expected_model_json":before
            }),
        )
        .unwrap();
    let current = server.manager.export_project_model().unwrap();
    let mut request = json!({
        "export":{"scope":"assembly","expected_model_json":before},
        "project":{"source_document_id":authored["source_document_id"]},
        "template_base64":"deliberately-invalid-template"
    });
    let stale = server
        .call_tool("bambu_project_preview", request.clone())
        .unwrap_err();
    assert!(
        stale.contains("model") || stale.contains("document"),
        "stale model precondition must reject"
    );
    assert!(
        !stale.contains("base64"),
        "stale ownership must reject before template decoding"
    );
    request["export"]["expected_model_json"] = json!(current);
    request["project"]["arbitrary_slicer_overrides"] = json!({"nozzle_temperature":300});
    assert!(server
        .call_tool("solid_export_bambu_project", request)
        .unwrap_err()
        .contains("unknown field"));
    let portable = data(
        server
            .call_tool("solid_export_3mf", json!({"expected_model_json":current}))
            .unwrap(),
    );
    let bytes = BASE64
        .decode(portable["bytes_base64"].as_str().unwrap())
        .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(archive.by_name("3D/3dmodel.model").is_ok());
    assert!(archive.by_name("Metadata/project_settings.config").is_err());
    assert!(archive.by_name("Metadata/model_settings.config").is_err());
    assert_eq!(server.manager.export_project_model().unwrap(), current);
}
