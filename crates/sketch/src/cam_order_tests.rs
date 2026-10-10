use super::*;
use limo_cad_cam::{CamCommandDto, CamOperationDependencyKind};
use limo_cad_solid::KernelSceneDto;

fn job() -> CamDocumentDto {
    let mut doc = super::project_tests::cam_roundtrip_fixture();
    doc.height_expressions.clear();
    let mut drill = doc.tools[0].clone();
    drill.id = 6;
    drill.number = Some(2);
    drill.kind = limo_cad_cam::CamToolKind::Drill;
    drill.diameter = 8.0;
    drill.point_angle_degrees = Some(118.0);
    doc.tools.push(drill);
    doc.setups[0].operations.push(
        serde_json::from_value(serde_json::json!({
            "kind":"drill","id":8,"name":"Drill holes","enabled":true,"tool_id":6,
            "points":[{"x":15.0,"y":10.0}],"top_z":0.0,"bottom_z":-8.0,
            "clearance_z":8.0,"retract_z":2.0,"feed_height_z":1.0,
            "cutting":{"spindle_rpm":3000,"feed_xy":300.0,"feed_z":100.0,"coolant":"flood"}
        }))
        .unwrap(),
    );
    for id in [9, 10] {
        doc.setups[0].operations.push(serde_json::from_value(serde_json::json!({
            "kind":"contour2d","id":id,"name":format!("Contour {id}"),"enabled":true,"tool_id":5,
            "path":[{"x":0.0,"y":0.0},{"x":30.0,"y":0.0},{"x":30.0,"y":20.0},{"x":0.0,"y":20.0}],
            "closed":true,"top_z":-1.0,"bottom_z":-2.0,"step_down":1.0,
            "compensation":"outside","compensation_mode":"in_software",
            "lead_in":1.0,"lead_out":1.0,"lead_arc_radius":0.5,"direction":"climb",
            "clearance_z":8.0,"retract_z":2.0,"feed_height_z":1.0,
            "cutting":{"spindle_rpm":8000,"feed_xy":600.0,"feed_z":100.0,"coolant":"flood"}
        })).unwrap());
    }
    doc.next_tool_id = 7;
    doc.next_operation_id = 11;
    doc
}

fn reorder(doc: &mut CamDocumentDto, ids: &[u64]) {
    let previous = doc.setups[0].operations.clone();
    doc.setups[0].operations = ids
        .iter()
        .map(|id| previous.iter().find(|o| o.id() == *id).unwrap().clone())
        .collect();
}

fn statuses(manager: &SketchManager) -> BTreeMap<u64, CamToolpathStateDto> {
    manager
        .cam_toolpath_statuses()
        .unwrap()
        .into_iter()
        .map(|s| (s.operation_id, s.state))
        .collect()
}

fn motion(program: &CamProgramDto, id: u64) -> Vec<CamCommandDto> {
    let mut active = false;
    program
        .commands
        .iter()
        .filter_map(|c| {
            if let CamCommandDto::SectionStart { operation_id, .. } = c {
                active = *operation_id == id;
            }
            (active && c.motion_kind().is_some()).then(|| c.clone())
        })
        .collect()
}

#[test]
fn independent_drilling_can_move_last_without_regenerating_any_path() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let before = manager.cam_document();
    let original = manager.cam_plan(3).unwrap();
    let mut reordered = before.clone();
    reorder(&mut reordered, &[7, 9, 10, 8]);
    manager.set_cam_document(reordered).unwrap();
    assert!(statuses(&manager)
        .values()
        .all(|s| *s == CamToolpathStateDto::Current));
    assert_eq!(
        manager.cam_document().toolpath_generations,
        before.toolpath_generations
    );
    assert_eq!(
        motion(&original, 9),
        motion(&manager.cam_plan(3).unwrap(), 9)
    );
    assert_eq!(
        motion(&original, 10),
        motion(&manager.cam_plan(3).unwrap(), 10)
    );
    assert!(manager.cam_toolpath_safety_warning(3).unwrap().is_none());
    manager
        .cam_post(CamPostRequestDto {
            setup_id: 3,
            post: None,
            program_name: None,
        })
        .unwrap();

    let json = manager.export_project_model().unwrap();
    let mut loaded = SketchManager::new();
    let load = loaded.prepare_load_project(json).unwrap();
    loaded
        .commit_solid(CommitKernelRequest {
            transaction_id: load.transaction_id,
            scene: KernelSceneDto::default(),
        })
        .unwrap();
    assert!(statuses(&loaded)
        .values()
        .all(|s| *s == CamToolpathStateDto::Current));
}

#[test]
fn independent_drill_edits_and_suppression_do_not_stale_milling() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let original = manager.cam_document();
    let mut edited = original.clone();
    edited.tools[1].diameter = 7.0;
    manager.set_cam_document(edited).unwrap();
    let states = statuses(&manager);
    assert_eq!(states[&8], CamToolpathStateDto::Stale);
    for id in [7, 9, 10] {
        assert_eq!(states[&id], CamToolpathStateDto::Current);
    }
    let mut suppressed = original;
    if let CamOperationDto::Drill { enabled, .. } = &mut suppressed.setups[0].operations[1] {
        *enabled = false;
    }
    manager.set_cam_document(suppressed).unwrap();
    assert!(statuses(&manager)
        .values()
        .all(|s| *s == CamToolpathStateDto::Current));
}

#[test]
fn facing_stock_height_dependency_survives_independent_moves_but_not_a_missing_face() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let original = manager.cam_document();
    let mut reordered = original.clone();
    reorder(&mut reordered, &[8, 9, 10, 7]);
    manager.set_cam_document(reordered).unwrap();
    assert_eq!(
        statuses(&manager)[&7],
        CamToolpathStateDto::Current,
        "Face never depended on the drill or contours"
    );
    for status in manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .filter(|s| s.operation_id != 7)
    {
        assert_eq!(status.state, CamToolpathStateDto::Stale);
        assert!(status
            .reasons
            .iter()
            .any(|r| r.contains("incoming stock height")));
    }
    assert!(manager
        .cam_post(CamPostRequestDto {
            setup_id: 3,
            post: None,
            program_name: None
        })
        .unwrap_err()
        .to_string()
        .contains("NC posting blocked"));

    manager.set_cam_document(original).unwrap();
    assert!(statuses(&manager)
        .values()
        .all(|s| *s == CamToolpathStateDto::Current));
}

#[test]
fn selected_predrill_is_order_sensitive_even_with_the_contour_ramp_off() {
    let mut doc = job();
    if let CamOperationDto::Contour2d { path, .. } = &mut doc.setups[0].operations[2] {
        *path = vec![
            limo_cad_cam::Point2Dto::new(8.0, 7.0),
            limo_cad_cam::Point2Dto::new(22.0, 7.0),
            limo_cad_cam::Point2Dto::new(22.0, 13.0),
            limo_cad_cam::Point2Dto::new(8.0, 13.0),
        ];
    }
    let mut link = limo_cad_cam::CamLinkingDto {
        operation_id: 9,
        ..Default::default()
    };
    link.lead_in.linear_distance = 1.0;
    link.lead_out.linear_distance = 1.0;
    link.lead_in.horizontal_radius = 0.5;
    link.entry_positions = vec![limo_cad_cam::Point2Dto::new(15.0, 7.0)];
    doc.linking.push(link);

    let before = plan_setup(&doc, 3).unwrap();
    let first_feed = motion(&before, 9)
        .into_iter()
        .find_map(|c| match c {
            CamCommandDto::Linear { to, .. } => Some(to),
            _ => None,
        })
        .unwrap();
    let center = limo_cad_cam::Point2Dto::new(first_feed.x, first_feed.y);
    if let CamOperationDto::Drill { points, .. } = &mut doc.setups[0].operations[1] {
        *points = vec![center];
    }
    doc.linking[0].predrill_positions = vec![center];
    assert!(!doc.linking[0].ramp_enabled);
    let rules = limo_cad_cam::cam_operation_dependencies(
        &doc.setups[0],
        &doc.setups[0].operations[2],
        Some(&doc.linking[0]),
    );
    assert!(rules
        .iter()
        .any(|d| d.operation_id == 8 && d.kind == CamOperationDependencyKind::PredrilledEntry));
    let mut manager = SketchManager::new();
    manager.set_cam_document(doc).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let mut reordered = manager.cam_document();
    reorder(&mut reordered, &[7, 9, 10, 8]);
    manager.set_cam_document(reordered).unwrap();
    let states = statuses(&manager);
    assert_eq!(states[&9], CamToolpathStateDto::Stale);
    for id in [7, 8, 10] {
        assert_eq!(states[&id], CamToolpathStateDto::Current);
    }
    assert!(manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .find(|s| s.operation_id == 9)
        .unwrap()
        .reasons
        .iter()
        .any(|r| r.contains("Predrill")));
    assert!(manager
        .cam_regenerate_operation(9)
        .unwrap_err()
        .to_string()
        .contains("predrill"));
}

fn legacy_generations(manager: &SketchManager) -> Vec<CamToolpathGenerationDto> {
    let setup = &manager.cam.setups[0];
    let dependencies = manager.cam_setup_dependency_fingerprints(setup).unwrap();
    setup
        .operations
        .iter()
        .map(|operation| {
            manager
                .cam_generation_signature(setup, operation, &dependencies, true)
                .unwrap()
        })
        .collect()
}

#[test]
fn verified_legacy_stamps_upgrade_before_reorder_without_regeneration() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    manager.cam.toolpath_generations = legacy_generations(&manager);
    assert!(statuses(&manager)
        .values()
        .all(|s| *s == CamToolpathStateDto::Current));
    let mut moved = manager.cam_document();
    reorder(&mut moved, &[7, 9, 10, 8]);
    manager.set_cam_document(moved).unwrap();
    assert!(manager
        .cam
        .toolpath_generations
        .iter()
        .all(|s| s.order_dependencies.is_some()));
    assert!(statuses(&manager)
        .values()
        .all(|s| *s == CamToolpathStateDto::Current));
}

#[test]
fn legacy_upgrade_never_certifies_stale_or_changed_inputs() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    manager.cam.toolpath_generations = legacy_generations(&manager);

    manager.cam.tools[1].diameter = 7.0;
    let mut moved = manager.cam_document();
    reorder(&mut moved, &[7, 9, 10, 8]);
    manager.set_cam_document(moved).unwrap();
    assert_eq!(statuses(&manager)[&8], CamToolpathStateDto::Stale);
    assert!(manager
        .cam
        .toolpath_generations
        .iter()
        .find(|s| s.operation_id == 8)
        .unwrap()
        .order_dependencies
        .is_none());
    assert!(manager
        .cam_post(CamPostRequestDto {
            setup_id: 3,
            post: None,
            program_name: None
        })
        .is_err());
}

#[test]
fn legacy_translation_preserves_pre_edit_evidence_when_inputs_change_with_reorder() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    manager.cam.toolpath_generations = legacy_generations(&manager);
    let mut edited = manager.cam_document();
    reorder(&mut edited, &[7, 9, 10, 8]);
    edited.tools[1].diameter = 7.0;
    manager.set_cam_document(edited).unwrap();

    let saved = manager
        .cam
        .toolpath_generations
        .iter()
        .find(|s| s.operation_id == 8)
        .unwrap();
    assert!(saved.order_dependencies.is_some());
    let states = statuses(&manager);
    assert_eq!(states[&8], CamToolpathStateDto::Stale);
    for id in [7, 9, 10] {
        assert_eq!(states[&id], CamToolpathStateDto::Current);
    }
}

#[test]
fn unsupported_or_malformed_order_evidence_never_counts_as_current() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(job()).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let mut unknown = manager.cam_document();
    unknown.toolpath_generations[0]
        .order_dependencies
        .as_mut()
        .unwrap()
        .rules_revision += 1;
    manager.set_cam_document(unknown).unwrap();
    let face = manager
        .cam_toolpath_statuses()
        .unwrap()
        .into_iter()
        .find(|s| s.operation_id == 7)
        .unwrap();
    assert_eq!(face.state, CamToolpathStateDto::Stale);
    assert!(face
        .reasons
        .iter()
        .any(|r| r.contains("order-dependency rules")));
    let mut malformed = manager.cam_document();
    malformed.toolpath_generations[0]
        .order_dependencies
        .as_mut()
        .unwrap()
        .stock_height_fingerprint = "invalid".into();
    manager.set_cam_document(malformed).unwrap();
    assert_eq!(statuses(&manager)[&7], CamToolpathStateDto::NeverGenerated);
}

#[test]
fn cam_model_coordinate_json_transport_is_lossless() {
    for i in 1..20_000 {
        let coordinate = f64::from(i as f32 * 0.001_f32);
        let encoded = serde_json::to_string(&coordinate).unwrap();
        let decoded: f64 = serde_json::from_str(&encoded).unwrap();
        assert_eq!(
            coordinate.to_bits(),
            decoded.to_bits(),
            "mesh coordinate {encoded} drifted during JSON transport"
        );
    }
}

#[test]
fn later_operation_edit_through_json_never_changes_earlier_generation() {
    let mut manager = SketchManager::new();
    let mut doc = job();

    if let CamOperationDto::Contour2d { path, .. } = &mut doc.setups[0].operations[2] {
        for point in path {
            if point.x == 0.0 {
                point.x = f64::from(0.013_f32);
            }
        }
    }
    manager.set_cam_document(doc).unwrap();
    manager.cam_regenerate_setup(3).unwrap();
    let original = manager.cam_document();
    let mut edited = original.clone();
    if let CamOperationDto::Contour2d { cutting, .. } = &mut edited.setups[0].operations[3] {
        cutting.feed_xy += 50.0;
    }

    let received = serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
    manager.set_cam_document(received).unwrap();
    let states = statuses(&manager);
    for id in [7, 8, 9] {
        assert_eq!(
            states[&id],
            CamToolpathStateDto::Current,
            "earlier operation {id}"
        );
    }
    assert_eq!(states[&10], CamToolpathStateDto::Stale);
    manager.cam_regenerate_operation(10).unwrap();
    assert!(statuses(&manager)
        .values()
        .all(|s| *s == CamToolpathStateDto::Current));
    for id in [7, 8, 9] {
        assert_eq!(
            manager
                .cam
                .toolpath_generations
                .iter()
                .find(|s| s.operation_id == id),
            original
                .toolpath_generations
                .iter()
                .find(|s| s.operation_id == id)
        );
    }
}
