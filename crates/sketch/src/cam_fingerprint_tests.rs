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

fn generated_rest_chain() -> SketchManager {
    let mut cam = project_tests::cam_roundtrip_fixture();
    for (setup_id, operation_id, source) in [(4, 8, Some(3)), (5, 9, Some(4)), (6, 10, None)] {
        let mut setup = cam.setups[0].clone();
        setup.id = setup_id;
        setup.name = format!("Setup {setup_id}");
        if let Some(source_setup_id) = source {
            setup.resolved_stock = CamResolvedStockDto::Rest { source_setup_id };
            setup.stock_spec = limo_cad_cam::CamStockSpecDto::RestFromSetup {
                setup_id: source_setup_id,
            };
        }
        if let CamOperationDto::Face {
            id, name, target_z, ..
        } = &mut setup.operations[0]
        {
            *id = operation_id;
            *name = format!("Face {operation_id}");
            *target_z = -(operation_id as f64 - 6.);
        }
        cam.setups.push(setup);
    }
    cam.next_setup_id = 7;
    cam.next_operation_id = 11;
    let mut manager = SketchManager::new();
    manager.set_cam_document(cam).unwrap();
    for id in [3, 4, 5, 6] {
        manager.cam_regenerate_setup(id).unwrap();
    }
    assert!(manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .all(|s| s.state == CamToolpathStateDto::Current));
    manager
}

#[test]
fn upstream_height_expression_edit_marks_transitive_rest_results_stale_before_regeneration() {
    let mut manager = generated_rest_chain();
    let generated = manager.cam_document();
    let mut edited = generated.clone();
    edited.height_expressions[0].bottom.as_mut().unwrap().offset = -0.5;
    manager.set_cam_document(edited.clone()).unwrap();
    for status in manager.cam_toolpath_statuses().unwrap() {
        assert_eq!(
            status.state,
            if status.setup_id == 6 {
                CamToolpathStateDto::Current
            } else {
                CamToolpathStateDto::Stale
            },
            "setup {}",
            status.setup_id
        );
    }
    assert_eq!(
        manager.cam_document().toolpath_generations,
        generated.toolpath_generations
    );
    assert!(manager
        .cam_regenerate_setup(5)
        .unwrap_err()
        .to_string()
        .contains("Regenerate the source setup"));
    assert_eq!(
        manager.cam_document(),
        edited,
        "Rejected regeneration must be atomic"
    );
    let project = manager.export_project_model().unwrap();
    let mut loaded = SketchManager::new();
    let plan = loaded.prepare_load_project(project).unwrap();
    loaded
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto::default(),
        })
        .unwrap();
    assert_eq!(
        loaded.cam_toolpath_statuses().unwrap(),
        manager.cam_toolpath_statuses().unwrap()
    );
    for id in [3, 4, 5] {
        loaded.cam_regenerate_setup(id).unwrap();
    }
    assert!(loaded
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .all(|s| s.state == CamToolpathStateDto::Current));
    manager.set_cam_document(generated).unwrap();
    assert!(manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .all(|s| s.state == CamToolpathStateDto::Current));
}

#[test]
fn every_upstream_height_role_and_removal_invalidates_the_rest_chain() {
    let mut manager = generated_rest_chain();
    let generated = manager.cam_document();
    for role in ["clearance", "retract", "feed", "top", "bottom", "remove"] {
        let mut edited = generated.clone();
        if role == "remove" {
            edited.height_expressions.clear();
        } else {
            let mut value = serde_json::to_value(&edited.height_expressions[0]).unwrap();
            value[role]["offset"] =
                serde_json::json!(value[role]["offset"].as_f64().unwrap() + 0.1);
            edited.height_expressions[0] = serde_json::from_value(value).unwrap();
        }
        manager.set_cam_document(edited).unwrap();
        for status in manager.cam_toolpath_statuses().unwrap() {
            assert_eq!(
                status.state,
                if status.setup_id == 6 {
                    CamToolpathStateDto::Current
                } else {
                    CamToolpathStateDto::Stale
                },
                "{role}: setup {}",
                status.setup_id
            );
            if [4, 5].contains(&status.setup_id) {
                assert!(status
                    .reasons
                    .iter()
                    .any(|r| r.contains("upstream rest-stock")));
            }
        }
    }
}

#[test]
fn legacy_rest_stamp_is_unproven_but_independent_generation_is_preserved() {
    let mut manager = generated_rest_chain();
    let mut cam = manager.cam_document();
    let mut source = cam.setups[0].clone();
    source.machine = None;
    let old_rest = stable_cam_fingerprint(&(
        "cam-upstream-dependencies",
        CAM_TOOLPATH_PLANNER_REVISION,
        vec![source],
        vec![&cam.tools[0]],
        Vec::<limo_cad_cam::CamLinkingDto>::new(),
    ))
    .unwrap();
    cam.toolpath_generations
        .iter_mut()
        .find(|g| g.operation_id == 8)
        .unwrap()
        .upstream_fingerprint = old_rest;
    let old_independent = stable_cam_fingerprint(&(
        "cam-upstream-dependencies",
        CAM_TOOLPATH_PLANNER_REVISION,
        Vec::<CamSetupDto>::new(),
        Vec::<limo_cad_cam::CamToolDto>::new(),
        Vec::<limo_cad_cam::CamLinkingDto>::new(),
    ))
    .unwrap();
    assert_eq!(
        cam.toolpath_generations
            .iter()
            .find(|g| g.operation_id == 10)
            .unwrap()
            .upstream_fingerprint,
        old_independent
    );
    manager.set_cam_document(cam.clone()).unwrap();
    assert_eq!(
        manager
            .cam_toolpath_statuses()
            .unwrap()
            .iter()
            .find(|s| s.operation_id == 8)
            .unwrap()
            .state,
        CamToolpathStateDto::Stale
    );
    assert_eq!(
        manager.cam_document().toolpath_generations,
        cam.toolpath_generations
    );
    manager.cam_regenerate_setup(4).unwrap();
    assert!(manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .all(|s| s.state == CamToolpathStateDto::Current));
}

#[test]
fn upstream_height_record_order_and_unrelated_intent_do_not_invalidate_rest_results() {
    let mut manager = generated_rest_chain();
    let mut cam = manager.cam_document();
    let mut second = cam.height_expressions[0].clone();
    second.operation_id = 8;
    second.bottom.as_mut().unwrap().offset = -2.;
    cam.height_expressions.push(second);
    manager.set_cam_document(cam).unwrap();
    manager.cam_regenerate_setup(4).unwrap();
    manager.cam_regenerate_setup(5).unwrap();
    let generated = manager.cam_document();
    let mut reordered = generated.clone();
    reordered.height_expressions.reverse();
    manager.set_cam_document(reordered).unwrap();
    assert!(manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .all(|s| s.state == CamToolpathStateDto::Current));
    let mut unrelated = manager.cam_document();
    let mut height = unrelated.height_expressions[0].clone();
    height.operation_id = 10;
    unrelated.height_expressions.push(height);
    manager.set_cam_document(unrelated).unwrap();
    for status in manager.cam_toolpath_statuses().unwrap() {
        assert_eq!(
            status.state,
            if status.setup_id == 6 {
                CamToolpathStateDto::Stale
            } else {
                CamToolpathStateDto::Current
            }
        );
    }
    assert_eq!(
        manager.cam_document().toolpath_generations,
        generated.toolpath_generations
    );
}
