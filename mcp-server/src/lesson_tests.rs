use super::*;

#[test]
fn bracket_lessons_share_one_editable_build_and_localized_help() {
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
        json!([32., 40., 44.])
    );
    let lessons = &report["exports"]["lessons"];
    assert_eq!(lessons["sketch"].as_array().unwrap().len(), 5);
    assert_eq!(lessons["extrude"].as_array().unwrap().len(), 5);
    assert_eq!(lessons["cut"].as_array().unwrap().len(), 4);
    assert_eq!(
        lessons["sketch"][4]["profile"],
        lessons["extrude"][0]["profile"]
    );
    let stock = &lessons["extrude"][4]["scene"]["bodies"][0];
    let prepared = &lessons["cut"][0]["scene"]["bodies"][0];
    assert_eq!(stock["id"], prepared["id"]);
    assert_ne!(stock, prepared, "Cut starts after supporting setup");
    let cylinders = prepared["faces"].as_array().unwrap();
    for y in [8., 32.] {
        assert!(
            cylinders.iter().any(|face| {
                let cylinder = &face["cylinder"];
                cylinder["radius"] == 3.5
                    && cylinder["origin"]["x"] == 24.
                    && cylinder["origin"]["y"] == y
            }),
            "Both lower holes must exist before Cut at y={y}: {cylinders:?}"
        );
    }
    assert!(!cylinders
        .iter()
        .any(|face| face["cylinder"]["radius"] == 13.));
    assert!(cylinders
        .iter()
        .any(|face| face["cylinder"]["radius"] == 4.));
    assert_eq!(
        lessons["cut"][0]["profile"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entity| entity["kind"] == "circle")
            .count(),
        1
    );
    assert_ne!(lessons["cut"][0]["scene"], lessons["cut"][3]["scene"]);
    let body = &report["exports"]["final_scene"]["bodies"][0];
    assert!(body["faces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["cylinder"]["radius"]
            .as_f64()
            .is_some_and(|r| (r - 13.).abs() < 1e-6)));
    let openings: Vec<_> = body["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["circle"]["closed"] == true
                && e["circle"]["radius"]
                    .as_f64()
                    .is_some_and(|r| (r - 13.).abs() < 1e-6)
        })
        .map(|e| e["circle"]["center"]["x"].as_f64().unwrap())
        .collect();
    assert!(openings.iter().any(|x| x.abs() < 1e-6));
    assert!(openings.iter().any(|x| (*x - 5.).abs() < 1e-6));
    let saved = report["exports"]["final_model"].clone();
    assert_eq!(
        saved["body_appearances"][0]["preset_id"],
        "material.aluminum-6061-t6"
    );
    let mut reopened = CadServer::new().unwrap();
    reopened
        .call_tool(
            "cad_load_project_model",
            json!({"model_json":saved.to_string()}),
        )
        .unwrap();
    let replay = preview_script(recipe.source).unwrap();
    assert_eq!(
        report["exports"]["final_model"],
        replay["exports"]["final_model"]
    );
    for id in lessons::ALL {
        for language in lessons::LOCALES {
            let help = help_page_localized(id.help(), language).unwrap();
            assert_eq!(help["title"], id.title(language));
            assert_eq!(help["body"], lessons::page(language, id).body);
            assert_eq!(help["demonstrations"], json!([id]));
        }
    }
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
