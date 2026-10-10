

fn preview_face() -> CamOperationDto {
    CamOperationDto::Face {
        id: 1, name: "Face".into(), enabled: true, tool_id: 1,
        bounds: Rect2Dto { min: Point2Dto::new(0.0, 0.0), max: Point2Dto::new(40.0, 30.0) },
        top_z: 0.0, target_z: -2.0, step_over: 10.0, step_down: 1.0,
        safe_distance: 5.0, direction: FaceDirection::BothWays,
        clearance_z: 10.0, retract_z: 3.0, feed_height_z: 1.0, cutting: cutting(),
    }
}

fn preview_request(through: Option<u64>) -> crate::CamSimulationRequestDto {
    crate::CamSimulationRequestDto {
        setup_id: 1, voxel_size: Some(1.0), max_voxels: Some(50_000),
        stock_mesh: None, target: None, through_operation_id: through,
        completed_steps: None, playback_time_seconds: None,
    }
}

#[test]
fn facing_coverage_uses_flat_land_not_the_outer_diameter() {
    let stock = StockBoxDto { min: Point3Dto::new(-17.0, -9.5, 0.0), max: Point3Dto::new(17.0, 9.5, 14.0) };
    let mut face = tool(1, CamToolKind::FaceMill, 63.0);
    face.corner_radius = Some(0.5);
    assert!(facing_clears_stock_floor(&face, &stock, &[0.0], -48.5, 48.5));
    assert!(!facing_clears_stock_floor(&face, &stock, &[30.0], -48.5, 48.5));
    assert!(!facing_clears_stock_floor(&face, &stock, &[0.0], -16.0, 48.5));
    assert!(!facing_clears_stock_floor(&face, &stock, &[], -48.5, 48.5));

    face.diameter = 10.0;
    face.corner_radius = Some(1.0);

    assert!(!facing_clears_stock_floor(&face, &stock, &[-5.0, 5.0], -48.5, 48.5));
    assert!(!facing_clears_stock_floor(&face, &stock, &[-7.0, 2.0, 7.0], -48.5, 48.5));
    assert!(facing_clears_stock_floor(&face, &stock, &[-7.0, 0.0, 7.0], -48.5, 48.5));
    face.corner_radius = None;
    face.corner_chamfer = Some(crate::CamCornerChamferDto { width: 1.0, angle_degrees: 45.0 });
    assert!(facing_clears_stock_floor(&face, &stock, &[-7.0, 0.0, 7.0], -48.5, 48.5));
    assert!(!facing_clears_stock_floor(&face, &stock, &[-5.0, 5.0], -48.5, 48.5));
}

#[test]
fn corner_treated_facing_certifies_a_fully_covered_floor_in_both_linking_modes() {
    for linked in [false, true] {
        for chamfer in [false, true] {
            let mut face_tool = tool(1, CamToolKind::FaceMill, 63.0);
            if chamfer {
                face_tool.corner_chamfer = Some(crate::CamCornerChamferDto { width: 0.5, angle_degrees: 45.0 });
            } else {
                face_tool.corner_radius = Some(0.5);
            }
            let mut drill = drill_operation(DrillCycle::Drill);
            if let CamOperationDto::Drill { id, top_z, feed_height_z, .. } = &mut drill {
                *id = 2; *top_z = -2.0; *feed_height_z = -1.0;
            }
            let mut doc = document(vec![preview_face(), drill], vec![face_tool, tool(2, CamToolKind::Drill, 5.5)]);
            if linked { doc.linking.push(CamLinkingDto { operation_id: 1, ..Default::default() }); }
            let plan = plan_setup(&doc, 1).unwrap();
            assert_eq!(plan.stats.operation_count, 2);
            let stock = crate::simulate_setup(&doc, &preview_request(None)).unwrap();
            assert!(stock.removed_voxels > 0);
            assert!(stock.collisions.is_empty(), "{:?}", stock.collisions);



            doc.tools[0].diameter = 10.0;
            assert!(plan_setup(&doc, 1).unwrap_err().0.contains("incoming stock top"));
        }
    }
}

#[test]
fn later_invalid_operation_cannot_erase_earlier_paths_stock_or_playback() {
    let mut earlier = drill_operation(DrillCycle::Drill);
    if let CamOperationDto::Drill { id, .. } = &mut earlier { *id = 8; }
    let mut later = earlier.clone();
    if let CamOperationDto::Drill { id, name, top_z, feed_height_z, .. } = &mut later {
        *id = 2; *name = "Unsafe later drill".into(); *top_z = -2.0; *feed_height_z = -1.0;
    }
    let mut doc = document(vec![earlier, later], vec![tool(2, CamToolKind::Drill, 5.5)]);
    let original = serde_json::to_value(&doc).unwrap();
    assert!(plan_setup(&doc, 1).unwrap_err().0.contains("Unsafe later drill"));
    let prefix = plan_setup_through(&doc, 1, 8).unwrap();
    assert_eq!(prefix.stats.operation_count, 1);
    assert!(prefix.commands.iter().any(|c| matches!(c, CamCommandDto::SectionStart { operation_id: 8, .. })));
    assert!(!prefix.commands.iter().any(|c| matches!(c, CamCommandDto::SectionStart { operation_id: 2, .. })));
    let request = preview_request(Some(8));
    let stock = crate::simulate_setup(&doc, &request).unwrap();
    assert!(stock.removed_voxels > 0);
    let end = prefix.commands.iter().position(|c| matches!(c, CamCommandDto::SectionEnd)).unwrap();
    assert!(stock.steps.iter().all(|step| step.command_index < end));
    let mut playback = crate::CamPlayback::new(doc.clone(), request.clone(), 0.0, None).unwrap();
    assert_eq!(playback.sample(stock.estimated_seconds, None).unwrap().remaining_voxels, stock.remaining_voxels);
    let mut frame_request = request.clone();
    frame_request.playback_time_seconds = Some(stock.estimated_seconds / 2.0);
    assert!(crate::simulate_setup(&doc, &frame_request).is_ok());
    assert!(crate::simulate_setup(&doc, &preview_request(None)).is_err());
    assert!(crate::simulate_setup(&doc, &preview_request(Some(2))).is_err());
    assert!(plan_setup_through(&doc, 1, 99).is_err());
    assert_eq!(serde_json::to_value(&doc).unwrap(), original, "preview must not mutate saved intent");



    if let CamOperationDto::Drill { bottom_z, .. } = &mut doc.setups[0].operations[1] { *bottom_z = 20.0; }
    assert!(doc.validate().is_err());
    assert!(plan_setup_through(&doc, 1, 8).is_ok());
    assert!(crate::simulate_setup(&doc, &request).is_ok());
    doc.setups[0].operations[0].set_enabled(false);
    let incoming = crate::simulate_setup(&doc, &request).unwrap();
    assert_eq!(incoming.removed_voxels, 0);
    assert!(incoming.steps.is_empty());
}
