//! Run the same facade assertions natively and in a real browser's WASM runtime.
use super::WasmEngine;
use serde_json::{json, Value};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::*;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

fn unwrap(text: String) -> Value {
    let envelope: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(envelope["ok"], true, "{envelope}");
    envelope["value"].clone()
}
fn entity(sketch: &Value, id: &Value) -> Value {
    sketch["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == *id)
        .unwrap()
        .clone()
}
fn export(engine: &mut WasmEngine) -> String {
    unwrap(engine.project_export_model(None))
        .as_str()
        .unwrap()
        .into()
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn javascript_optional_export_arguments_remain_compatible() {
    use wasm_bindgen::{JsCast, JsValue};
    let engine = JsValue::from(WasmEngine::new());
    let method = js_sys::Reflect::get(&engine, &JsValue::from_str("project_export_model"))
        .unwrap()
        .dyn_into::<js_sys::Function>()
        .unwrap();
    let original = method.call0(&engine).unwrap();
    assert_eq!(
        method.call1(&engine, &JsValue::UNDEFINED).unwrap(),
        original
    );
    assert_eq!(method.call1(&engine, &JsValue::NULL).unwrap(), original);
    let free = js_sys::Reflect::get(&engine, &JsValue::from_str("free"))
        .unwrap()
        .dyn_into::<js_sys::Function>()
        .unwrap();
    free.call0(&engine).unwrap();
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn browser_facade_sketch_inference_drag_undo_redo_and_finish() {
    let mut engine = WasmEngine::new();
    assert_eq!(unwrap(engine.document())["settings"]["units"], "mm");
    let post=unwrap(engine.cam_analyze_nbpost(&json!({"file_name":"smoke.nbpost","source":"function onOpen(){} function onSection(){} function onRapid(){} function onClose(){}"}).to_string()));
    assert_eq!(post["source_kind"], "callback_javascript");
    assert_eq!(post["runnable"], false);
    let sketch =
        unwrap(engine.begin_sketch(&json!({"type":"origin_plane","plane":"xy"}).to_string()));
    assert_eq!(sketch["name"], "Sketch1");
    assert_eq!(sketch["basis"]["normal"], json!([0.0, 0.0, 1.0]));
    let doc = unwrap(engine.document());
    let folder = doc["browser"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "sketches_folder")
        .unwrap();
    assert!(folder["children"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "Sketch1"));
    let payload =
        json!({"from":{"x":0,"y":0},"to_raw":{"x":50,"y":1},"ctrl_held":false}).to_string();
    let preview = unwrap(engine.preview_segment(&payload));
    assert!(preview["inferences"]
        .as_array()
        .unwrap()
        .contains(&json!("horizontal")));
    assert_eq!(preview["snapped_to"]["y"].as_f64(), Some(0.));
    let l1 = unwrap(engine.add_line(&payload));
    assert_eq!(l1["created_constraints"][0]["type"], "horizontal");
    let l2 = unwrap(engine.add_line(
        &json!({"from":{"x":50,"y":0},"to_raw":{"x":51,"y":50},"ctrl_held":false}).to_string(),
    ));
    assert_eq!(l2["start_point_id"], l1["end_point_id"]);
    assert!(["perpendicular", "vertical"]
        .iter()
        .any(|kind| l2["created_constraints"][0]["type"] == *kind));
    let l3 = unwrap(engine.add_line(
        &json!({"from":{"x":50,"y":50},"to_raw":{"x":0.5,"y":0.4},"ctrl_held":false}).to_string(),
    ));
    assert_eq!(l3["end_point_id"], l1["start_point_id"]);
    assert!(l3["created_constraints"].as_array().unwrap().is_empty());
    let dragged=unwrap(engine.move_point(&json!({"point_id":l2["start_point_id"],"to_raw":{"x":80,"y":0},"ctrl_held":false,"phase":"single"}).to_string()));
    let first = entity(&dragged["sketch"], &l1["entity_id"]);
    let second = entity(&dragged["sketch"], &l2["entity_id"]);
    let at = |line: &Value, end: &str, axis: &str| line[end][axis].as_f64().unwrap();
    assert!((at(&first, "start", "y") - at(&first, "end", "y")).abs() < 1e-9);
    assert!((at(&second, "start", "x") - at(&second, "end", "x")).abs() < 1e-6);
    assert!(at(&first, "start", "x").abs() < 1e-6 && at(&first, "start", "y").abs() < 1e-6);
    assert!((at(&first, "end", "x") - 80.).abs() < 1e-6 && at(&first, "end", "y").abs() < 1e-6);
    unwrap(engine.undo());
    let restored = entity(&unwrap(engine.active_sketch()), &l1["entity_id"]);
    assert_eq!(at(&restored, "end", "x"), 50.);
    assert_eq!(at(&restored, "end", "y"), 0.);
    unwrap(engine.undo());
    unwrap(engine.undo());
    let empty = unwrap(engine.undo());
    assert!(empty["sketch"]["entities"].as_array().unwrap().is_empty());
    assert_eq!(
        unwrap(engine.redo())["sketch"]["entities"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(unwrap(engine.end_sketch())["document"]["name"], "Untitled");
    assert_eq!(unwrap(engine.active_sketch()), Value::Null);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn browser_facade_guarded_save_copies_adopts_and_rejects_stale_or_invalid_snapshots() {
    let mut engine = WasmEngine::new();
    assert_eq!(
        unwrap(engine.document_set_name(&json!("Original design").to_string()))["name"],
        "Original design"
    );
    let original = export(&mut engine);
    let mut expected: Value = serde_json::from_str(&original).unwrap();
    assert_eq!(expected["document"]["name"], "Original design");
    assert_eq!(
        unwrap(
            engine.project_export_model(Some(json!({"expected_model_json":original}).to_string()))
        ),
        json!(original)
    );
    let saved_name = "Saved copy Ω";
    expected["document"]["name"] = json!(saved_name);
    let saved = unwrap(engine.project_export_model(Some(
        json!({"expected_model_json":original,"save_name":format!("  {saved_name}  ")}).to_string(),
    )));
    assert_eq!(
        serde_json::from_str::<Value>(saved.as_str().unwrap()).unwrap(),
        expected
    );
    assert_eq!(export(&mut engine), original);
    assert_eq!(unwrap(engine.document())["name"], "Original design");
    assert_eq!(
        unwrap(engine.document_set_name(
            &json!({"name":saved_name,"expected_model_json":original}).to_string()
        ))["name"],
        saved_name
    );
    let adopted = export(&mut engine);
    assert_eq!(serde_json::from_str::<Value>(&adopted).unwrap(), expected);
    let responses = [
        engine.project_export_model(Some(
            json!({"expected_model_json":original,"save_name":"Wrong copy"}).to_string(),
        )),
        engine.document_set_name(
            &json!({"expected_model_json":original,"name":"Wrong live name"}).to_string(),
        ),
    ];
    for response in responses {
        let envelope: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(envelope["ok"], false);
        assert!(envelope["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("document changed"));
        assert_eq!(export(&mut engine), adopted);
        assert_eq!(unwrap(engine.document())["name"], saved_name);
    }
    for payload in [
        json!({"expected_model_json":adopted,"save_name":"   "}),
        json!({"save_name":"Unchecked copy"}),
        json!({"expected_model_json":"{","save_name":"Invalid copy"}),
    ] {
        let response: Value =
            serde_json::from_str(&engine.project_export_model(Some(payload.to_string()))).unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(export(&mut engine), adopted);
    }
}
