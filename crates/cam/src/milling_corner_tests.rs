#[test]
fn contour_and_pocket_support_corner_end_mills_but_not_drills_or_chamfer_mills() {
    for kind in ["contour2d", "pocket2d"] {
        let boundary = serde_json::json!([
            {"x":10.,"y":5.},{"x":30.,"y":5.},{"x":30.,"y":25.},{"x":10.,"y":25.}
        ]);
        let operation: CamOperationDto = serde_json::from_value(serde_json::json!({
            "kind":kind,"id":1,"name":kind,"enabled":true,"tool_id":1,
            "path":boundary,"outline":boundary,"closed":true,
            "compensation":"inside","compensation_mode":"in_software",
            "lead_in":1.,"lead_out":1.,"lead_arc_radius":0.5,
            "top_z":0.,"bottom_z":-0.5,"step_down":0.5,"step_over":2.,"direction":"climb",
            "clearance_z":10.,"retract_z":3.,"feed_height_z":1.,"cutting":cutting()
        }))
        .unwrap();
        let mut doc = document(vec![operation], vec![tool(1, CamToolKind::FlatEndMill, 6.)]);
        let mut volumes = Vec::new();
        let mut first_motion = None;
        for shape in 0..4 {
            let tool = &mut doc.tools[0];
            tool.kind = if shape == 1 {
                CamToolKind::BullNoseEndMill
            } else {
                CamToolKind::FlatEndMill
            };
            tool.corner_radius = matches!(shape, 1 | 2).then_some(1.);
            tool.corner_chamfer = (shape == 3).then_some(crate::CamCornerChamferDto {
                width: 1.,
                angle_degrees: 45.,
            });
            let program = plan_setup(&doc, 1).unwrap();
            if let Some(first) = &first_motion {
                assert_eq!(&program.commands, first);
            } else {
                first_motion = Some(program.commands);
            }
            let sim = crate::simulate_setup(
                &doc,
                &crate::CamSimulationRequestDto {
                    voxel_size: Some(0.25),
                    max_voxels: Some(1_000_000),
                    ..preview_request(None)
                },
            )
            .unwrap();
            assert!(sim.collisions.is_empty(), "{kind}: {:?}", sim.collisions);
            volumes.push(sim.remaining_voxels);
        }
        assert!(
            volumes[0] < volumes[1],
            "{kind}: round floor stock must remain at Ap < corner radius"
        );
        assert_eq!(volumes[1], volumes[2]);
        assert!(
            volumes[2] < volumes[3],
            "{kind}: beveled corner stock must remain"
        );
        for kind in [
            CamToolKind::Drill,
            CamToolKind::ChamferMill,
            CamToolKind::Tap,
            CamToolKind::ThreadMill,
        ] {
            let tool = &mut doc.tools[0];
            tool.kind = kind;
            tool.corner_radius = None;
            tool.corner_chamfer = None;
            tool.point_angle_degrees = match kind {
                CamToolKind::Drill => Some(118.),
                CamToolKind::ChamferMill => Some(90.),
                _ => None,
            };
            assert!(
                doc.validate().is_err(),
                "{kind:?} must not side-mill with center_cutting checked"
            );
            assert!(plan_setup(&doc, 1).is_err());

            doc.validate_for_editing().unwrap();
        }
    }
}
