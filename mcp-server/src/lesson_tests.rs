use super::*;

#[test]
fn extrude_demo_replays_real_geometry_and_shares_help() {
    let recipe = limo_cad_recipes::find("extrude-basics").unwrap();
    let mut server = CadServer::new().unwrap();
    let report = server
        .call_tool(
            "cad_interface",
            json!({"action":"script","recipe":recipe.id,"mode":"fast"}),
        )
        .unwrap();
    assert_eq!(
        report["summary"]["bodies"][0]["size"],
        json!([40., 24., 12.])
    );
    let frames = report["exports"]["preview_frames"].as_array().unwrap();
    assert_eq!(frames.len(), 7);
    assert!(frames[0]["scene"]["bodies"].as_array().unwrap().is_empty());
    assert!(frames[0]["profile"]["entities"].as_array().unwrap().len() >= 4);
    for (frame, depth) in frames[2..6].iter().zip([3., 6., 9., 12.]) {
        let positions = frame["scene"]["bodies"][0]["mesh"]["positions"]
            .as_array()
            .unwrap();
        let top = positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[2].as_f64().unwrap())
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((top - depth).abs() < 1e-6);
    }
    let replay = preview_script(recipe.source).unwrap();
    assert_eq!(
        report["exports"]["final_model"],
        replay["exports"]["final_model"]
    );
    let help = help_page("concepts.extrude-basics").unwrap();
    assert!(help["related_recipes"]
        .as_array()
        .unwrap()
        .contains(&json!(recipe.id)));
    assert!(help["body"].as_str().unwrap().contains("12 mm"));
}

#[test]
fn component_recovery_lesson_replays_previews_and_reopens_editable_shared_geometry() {
    let recipe = limo_cad_recipes::find("component-edit-recovery").unwrap();
    let mut first = CadServer::new().unwrap();
    let result = first.call_tool(
        "cad_interface",
        json!({"action":"script","recipe":recipe.id,"mode":"fast"}),
    );
    let report = result.unwrap_or_else(|error| {
        panic!(
            "{error}; assembly={:?}; solution={:?}",
            first.call_tool("assembly_document", json!({})),
            first.call_tool("assembly_solution", json!({}))
        )
    });
    assert_eq!(
        report["summary"]["bodies"][0]["size"],
        json!([30., 10., 3.])
    );
    let frames = report["exports"]["preview_frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2);
    assert_ne!(frames[0]["scene"], frames[1]["scene"]);
    assert!(frames
        .iter()
        .all(|frame| frame["caption"].as_str().unwrap().len() > 10));
    let second = preview_script(recipe.source).unwrap();
    for name in [
        "final_scene",
        "final_sketches",
        "final_solution",
        "preview_frames",
    ] {
        assert_eq!(report["exports"][name], second["exports"][name], "{name}");
    }
    let saved = first.call_tool("cad_project_model", json!({})).unwrap();
    let mut reopened = CadServer::new().unwrap();
    reopened
        .call_tool("cad_load_project_model", json!({"model_json":saved}))
        .unwrap();
    let occurrence = report["exports"]["edited_occurrence"].clone();
    let sketch = reopened
        .call_tool(
            "sketch_edit",
            json!({"name":"Shared footprint","occurrence_id":occurrence}),
        )
        .unwrap();
    assert_eq!(sketch["edit_occurrence_id"], occurrence);
    assert!(sketch["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|dimension| dimension["value"] == 30.));
    let help = first
        .call_tool(
            "cad_help",
            json!({"action":"get","id":"concepts.component-edit-recovery"}),
        )
        .unwrap();
    assert!(help["related_recipes"]
        .as_array()
        .unwrap()
        .contains(&json!(recipe.id)));
}
