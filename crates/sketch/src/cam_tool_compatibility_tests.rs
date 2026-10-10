use super::*;
use limo_cad_cam::CamToolKind;
use limo_cad_solid::KernelSceneDto;

fn job() -> CamDocumentDto {
    let mut doc = super::project_tests::cam_roundtrip_fixture();
    doc.height_expressions.clear();
    let mut mill = doc.tools[0].clone();
    mill.id = 6;
    mill.number = Some(2);
    doc.tools.push(mill);
    doc.setups[0].operations.push(
        serde_json::from_value(serde_json::json!({
            "kind":"contour2d","id":8,"name":"Contour","enabled":true,"tool_id":6,
            "path":[{"x":0.,"y":0.},{"x":30.,"y":0.},{"x":30.,"y":20.},{"x":0.,"y":20.}],
            "closed":true,"top_z":-1.,"bottom_z":-2.,"step_down":1.,
            "compensation":"outside","compensation_mode":"in_software",
            "lead_in":1.,"lead_out":1.,"lead_arc_radius":0.5,"direction":"climb",
            "clearance_z":8.,"retract_z":2.,"feed_height_z":1.,
            "cutting":{"spindle_rpm":8000,"feed_xy":600.,"feed_z":100.,"coolant":"flood"}
        }))
        .unwrap(),
    );
    doc.next_tool_id = 7;
    doc.next_operation_id = 9;
    doc
}

#[test]
fn unsupported_library_edit_is_invalid_not_suppressed_or_silently_regenerated() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let current = manager.cam_document();
    let stamps = current.toolpath_generations.clone();
    for kind in [CamToolKind::Drill, CamToolKind::ChamferMill] {
        let mut edited = current.clone();
        edited.tools[1].kind = kind;
        edited.tools[1].point_angle_degrees = Some(if kind == CamToolKind::Drill {
            118.
        } else {
            90.
        });
        manager.set_cam_document(edited).unwrap();
        assert!(manager.cam_document().setups[0].operations[1].enabled());
        let statuses = manager.cam_toolpath_statuses().unwrap();
        assert_eq!(
            statuses[0].state,
            CamToolpathStateDto::Current,
            "unrelated preceding face stays current"
        );
        assert_eq!(statuses[1].state, CamToolpathStateDto::Invalid);
        assert!(!statuses[1].reasons.is_empty());
        assert_eq!(manager.cam_document().toolpath_generations, stamps);
        assert!(manager.cam_plan(3).is_err());
        assert!(
            manager.cam_plan_through(3, 7).is_ok(),
            "earlier preview stays visible"
        );
        assert!(manager.cam_regenerate_operation(8).is_err());
        assert!(manager.cam_regenerate_setup(3).is_err());
        assert!(manager
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None
            })
            .unwrap_err()
            .to_string()
            .contains("Invalid toolpath"));
        assert_eq!(manager.cam_document().toolpath_generations, stamps);

        let saved = manager.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let transaction = loaded.prepare_load_project(saved).unwrap();
        loaded
            .commit_solid(CommitKernelRequest {
                transaction_id: transaction.transaction_id,
                scene: KernelSceneDto::default(),
            })
            .unwrap();
        assert!(loaded.cam_document().setups[0].operations[1].enabled());
        assert_eq!(
            loaded.cam_toolpath_statuses().unwrap()[1].state,
            CamToolpathStateDto::Invalid
        );
        assert!(loaded
            .cam_post(CamPostRequestDto {
                setup_id: 3,
                post: None,
                program_name: None
            })
            .is_err());
    }
    let mut supported = current;
    supported.tools[1].kind = CamToolKind::BullNoseEndMill;
    supported.tools[1].corner_radius = Some(0.5);
    manager.set_cam_document(supported).unwrap();
    assert_eq!(
        manager.cam_toolpath_statuses().unwrap()[1].state,
        CamToolpathStateDto::Stale
    );
    manager.cam_regenerate_operation(8).unwrap();
    assert!(manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .all(|s| s.state == CamToolpathStateDto::Current));
}

#[test]
fn editing_stays_strict_for_bad_tool_geometry_and_malformed_operation_numbers() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    let original = manager.cam_document();
    let mut bad = original.clone();
    bad.tools[0].diameter = -1.;
    assert!(manager.set_cam_document(bad).is_err());
    let mut bad = original.clone();
    if let CamOperationDto::Contour2d { step_down, .. } = &mut bad.setups[0].operations[1] {
        *step_down = -1.;
    }
    assert!(manager.set_cam_document(bad).is_err());
    assert_eq!(manager.cam_document(), original);

    let mut smaller = original;
    smaller.tools[0].diameter = 2.;
    manager.set_cam_document(smaller).unwrap();
    let face = &manager.cam_toolpath_statuses().unwrap()[0];
    assert_eq!(face.state, CamToolpathStateDto::Invalid);
    assert!(face.reasons[0].contains("stepover"));
}

#[test]
fn high_speed_library_kind_changes_are_reported_even_before_first_generation() {
    let mut manager = SketchManager::new();
    let mut doc = job();
    doc.setups[0].operations[1] = serde_json::from_value(serde_json::json!({
        "kind":"adaptive3d","id":8,"name":"Roughing","enabled":true,"tool_id":6,
        "top_z":0.,"bottom_z":-2.,"clearance_z":8.,"retract_z":2.,"feed_height_z":1.,
        "cutting":{"spindle_rpm":8000,"feed_xy":600.,"feed_z":100.,"coolant":"flood"},
        "parameters":{"optimal_load":0.6,"maximum_stepdown":1.,"minimum_cutting_radius":0.6,
            "radial_stock_to_leave":0.1,"axial_stock_to_leave":0.1,"tolerance":0.2,
            "ramp_angle_degrees":3.,"maximum_ramp_stepdown":0.5,"ramp_feed":100.,
            "linking_feed":600.,"stay_down_distance":8.,"machine_cavities":true}
    }))
    .unwrap();
    for kind in [
        CamToolKind::FlatEndMill,
        CamToolKind::BullNoseEndMill,
        CamToolKind::ChamferMill,
        CamToolKind::Drill,
    ] {
        doc.tools[1].kind = kind;
        doc.tools[1].corner_radius = (kind == CamToolKind::BullNoseEndMill).then_some(0.5);
        doc.tools[1].point_angle_degrees = match kind {
            CamToolKind::Drill => Some(118.),
            CamToolKind::ChamferMill => Some(90.),
            _ => None,
        };
        manager.set_cam_document(doc.clone()).unwrap();
        assert_eq!(
            manager.cam_toolpath_statuses().unwrap()[1].state,
            if matches!(kind, CamToolKind::Drill | CamToolKind::ChamferMill) {
                CamToolpathStateDto::Invalid
            } else {
                CamToolpathStateDto::NeverGenerated
            }
        );
    }
}
