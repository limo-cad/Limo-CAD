use super::*;
use limo_cad_core::OriginPlane;
use limo_cad_solid::KernelSceneDto;

fn rectangle(manager: &mut SketchManager, x: f64) {
    manager
        .add_rectangle(RectangleRequest {
            mode: crate::dto::RectangleMode::TwoPoint,
            p1: crate::Vec2::new(x, 0.),
            p2: crate::Vec2::new(x + 10., 10.),
            ctrl_held: true,
        })
        .unwrap();
}

#[test]
fn cam_geometry_freshness_ignores_local_sketch_history_but_not_geometry_changes() {
    let mut manager = SketchManager::new();
    manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    rectangle(&mut manager, 0.);
    manager.end_sketch().unwrap();
    assert!(manager.finished_sketches()[0].can_undo);
    manager
        .set_cam_document(project_tests::cam_roundtrip_fixture())
        .unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let generated = manager.cam_document();

    let mut loaded = SketchManager::new();
    let plan = loaded
        .prepare_load_project(manager.export_project_model().unwrap())
        .unwrap();
    assert!(plan.jobs.is_empty());
    loaded
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto::default(),
        })
        .unwrap();
    assert!(!loaded.finished_sketches()[0].can_undo);
    assert_eq!(
        loaded.cam_document().toolpath_generations,
        generated.toolpath_generations
    );
    assert_eq!(
        loaded.cam_toolpath_statuses().unwrap()[0].state,
        CamToolpathStateDto::Current
    );

    loaded.edit_sketch("Sketch1").unwrap();
    rectangle(&mut loaded, 30.);
    loaded.undo().unwrap();
    loaded.end_sketch().unwrap();
    assert!(loaded.finished_sketches()[0].can_redo);
    assert_eq!(
        loaded.cam_toolpath_statuses().unwrap()[0].state,
        CamToolpathStateDto::Current,
        "An undone edit returns to the same geometry even though redo is now available"
    );

    loaded.edit_sketch("Sketch1").unwrap();
    loaded.redo().unwrap();
    loaded.end_sketch().unwrap();
    let status = &loaded.cam_toolpath_statuses().unwrap()[0];
    assert_eq!(status.state, CamToolpathStateDto::Stale);
    assert!(status
        .reasons
        .iter()
        .any(|reason| reason.contains("CAD model or sketch geometry")));
    assert_eq!(
        loaded.cam_document().toolpath_generations,
        generated.toolpath_generations,
        "Geometry changes never earn a generation stamp without explicit regeneration"
    );
}

#[test]
fn old_cam_stamp_with_transient_sketch_flags_is_stale_until_regenerated() {
    let mut manager = SketchManager::new();
    manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    rectangle(&mut manager, 0.);
    manager.end_sketch().unwrap();
    manager
        .set_cam_document(project_tests::cam_roundtrip_fixture())
        .unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let legacy = stable_cam_fingerprint(&(
        "cam-model-dependencies",
        CAM_TOOLPATH_PLANNER_REVISION,
        BTreeSet::<BodyId>::new(),
        Vec::<limo_cad_solid::BodyDto>::new(),
        manager.finished_sketches(),
    ))
    .unwrap();
    let mut cam = manager.cam_document();
    assert_ne!(legacy, cam.toolpath_generations[0].model_fingerprint);
    cam.toolpath_generations[0].model_fingerprint = legacy;
    manager.set_cam_document(cam).unwrap();
    assert_eq!(
        manager.cam_toolpath_statuses().unwrap()[0].state,
        CamToolpathStateDto::Stale
    );
    manager.cam_regenerate_setup(3).unwrap();
    assert_eq!(
        manager.cam_toolpath_statuses().unwrap()[0].state,
        CamToolpathStateDto::Current
    );
}
