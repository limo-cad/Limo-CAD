//! Exercise saved presentation metadata against real OCCT replay and exports.
#![cfg(feature = "native-occt")]

use limo_cad_core::{OriginPlane, PlaneRef};
use limo_cad_export::MeshExportRequest;
use limo_cad_occt::OcctKernel;
use limo_cad_sketch::{host, SketchManager};
use limo_cad_solid::{CommitKernelRequest, RecomputePlanDto};

fn value(json: String) -> serde_json::Value {
    let envelope: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(envelope["ok"], true, "{envelope}");
    envelope["value"].clone()
}

fn replay(manager: &mut SketchManager, kernel: &mut OcctKernel, plan: RecomputePlanDto) {
    assert!(plan.errors.is_empty());
    let scene = kernel.recompute(&plan).unwrap();
    assert!(scene.errors.is_empty());
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene,
        })
        .unwrap();
}

#[test]
fn named_views_preserve_native_geometry_and_migrate_schema_nine() {
    let mut manager = SketchManager::new();
    let mut kernel = OcctKernel::new().unwrap();
    manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    value(host::handle(
        &mut manager,
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":10},"ctrl_held":false}"#,
    ));
    manager.end_sketch().unwrap();
    let request = serde_json::from_str(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}"#).unwrap();
    let plan = manager.prepare_extrude(request).unwrap();
    replay(&mut manager, &mut kernel, plan);
    let geometry = serde_json::to_value(manager.solid_scene()).unwrap();
    let mut legacy: serde_json::Value =
        serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
    legacy["schema_version"] = serde_json::json!(9);
    legacy["format"] = serde_json::json!("nbcad-project");
    legacy.as_object_mut().unwrap().remove("views");
    legacy.as_object_mut().unwrap().remove("print_intent");
    let plan = manager.prepare_load_project(legacy.to_string()).unwrap();
    replay(&mut manager, &mut kernel, plan);
    let migrated: serde_json::Value =
        serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
    assert_eq!(
        migrated["schema_version"],
        limo_cad_sketch::PROJECT_SCHEMA_VERSION
    );
    assert_eq!(migrated["views"], serde_json::json!([]));
    assert_eq!(
        serde_json::to_value(manager.solid_scene()).unwrap(),
        geometry
    );
    let body_id = manager.solid_scene().bodies[0].id.0;
    let views = serde_json::json!({"views":[{"name":"exploded","camera":{"position":[80,-40,30],"target":[0,0,8],"up":[0,0,1]},"visible_body_ids":[],"part_offsets":[{"body_id":body_id,"translation":[0,14,0]}]}]});
    let stored = value(host::handle(
        &mut manager,
        "set_named_views",
        &views.to_string(),
    ));
    let physical_mesh = kernel.export_stl(&MeshExportRequest::default()).unwrap();
    let recall = value(host::handle(
        &mut manager,
        "recall_named_view",
        r#"{"name":"exploded"}"#,
    ));
    assert_eq!(
        recall["visibility"]["hidden_body_ids"],
        serde_json::json!([body_id])
    );
    assert_eq!(
        serde_json::to_value(manager.solid_scene()).unwrap(),
        geometry
    );
    assert_eq!(
        kernel.export_stl(&MeshExportRequest::default()).unwrap(),
        physical_mesh
    );
    let saved = manager.export_project_model().unwrap();
    let plan = manager.prepare_load_project(saved).unwrap();
    replay(&mut manager, &mut kernel, plan);
    assert_eq!(
        value(host::handle(&mut manager, "named_views", ""))["views"],
        stored["views"]
    );
    value(host::handle(
        &mut manager,
        "recall_named_view",
        r#"{"name":"exploded"}"#,
    ));
    value(host::handle(&mut manager, "clear_named_view", ""));
    assert!(manager.named_views().active.is_none());
    assert_eq!(
        serde_json::to_value(manager.project_visibility()).unwrap(),
        recall["visibility"]
    );
    assert_eq!(
        serde_json::to_value(manager.solid_scene()).unwrap(),
        geometry
    );
}
