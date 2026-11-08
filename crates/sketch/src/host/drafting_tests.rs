use super::*;
use serde_json::{json, Value};

fn dispatch(manager: &mut SketchManager, command: Value) -> Value {
    serde_json::from_str(&handle(
        manager,
        "drawing_drafting_apply",
        &command.to_string(),
    ))
    .unwrap()
}
fn layer() -> Value {
    json!({"type":"create_layer","arguments":{
        "name":"Geometry","visible":true,"locked":false,"plottable":true,
        "style":{"line_type":null,"color_rgb":[0,0,0],"width_mm":0.25}
    }})
}
fn line(layer: u64) -> Value {
    json!({"type":"create_entity","arguments":{
        "space":{"kind":"model"},"layer":layer,"style":null,
        "content":{"kind":"curve","curve":{"kind":"line","start":[0,0],"end":[10,0]}}
    }})
}
#[test]
fn dispatch_allocates_ids_and_rejects_invalid_edits_without_mutation() {
    let mut manager = SketchManager::new();
    let created = dispatch(&mut manager, layer());
    assert_eq!(created["ok"], true, "{created}");
    assert_eq!(created["value"]["drafting"]["layers"][0]["id"], 1);
    let before = manager.export_project_model().unwrap();
    let bad = dispatch(&mut manager, line(99));
    assert_eq!(bad["ok"], false, "{bad}");
    assert_eq!(manager.export_project_model().unwrap(), before);
    let created = dispatch(&mut manager, line(1));
    assert_eq!(created["ok"], true, "{created}");
    assert_eq!(created["value"]["drafting"]["entities"][0]["id"], 2);
    let before = manager.export_project_model().unwrap();
    let bad = dispatch(
        &mut manager,
        json!({"type":"delete_layer","arguments":{"id":1}}),
    );
    assert_eq!(bad["ok"], false, "{bad}");
    assert_eq!(manager.export_project_model().unwrap(), before);
}
#[test]
fn malformed_wire_commands_cannot_create_a_graph_or_supply_engine_ids() {
    let mut manager = SketchManager::new();
    let before = manager.export_project_model().unwrap();
    let mut forged = layer();
    forged["arguments"]["id"] = json!(50);
    for command in [
        forged,
        json!({"type":"not_a_command","arguments":{}}),
        json!({"type":"create_layer","arguments":null}),
        json!({"type":"delete_entity","arguments":{"id":0}}),
    ] {
        let rejected = dispatch(&mut manager, command);
        assert_eq!(rejected["ok"], false, "{rejected}");
        assert_eq!(manager.export_project_model().unwrap(), before);
        assert!(manager.drawing_document().drafting.is_none());
    }
}
