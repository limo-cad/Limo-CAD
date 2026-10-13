#[test]
fn deep_layers_step_up_to_terraces_within_each_axial_band() {
    let meshes = vec![
        cuboid([4., 4., -3.], [12., 10., -1.4]),
        cuboid([6., 5., -1.4], [10., 9., -0.3]),
    ];
    let doc = fixture(meshes.clone());
    let CamOperationDto::Adaptive3d { parameters, .. } = &doc.setups[0].operations[0] else {
        unreachable!()
    };
    let mut p = parameters.clone();
    p.maximum_stepdown = 0.8;
    let order = layers::depth_order(&doc.setups[0], &meshes, 0., -2., f64::INFINITY, &p, 0.).unwrap();
    for (a, b) in order.iter().zip([-0.8, -0.2, -1.6, -1.3, -2.]) {
        assert!((a - b).abs() < EPS);
    }
    assert_eq!(order.len(), 5);
    let mut deepest = 0.0_f64;
    for z in order {
        assert!(deepest - z <= p.maximum_stepdown + EPS);
        deepest = deepest.min(z);
    }
    p.maximum_stepdown = 3.;
    let order = layers::depth_order(&doc.setups[0], &meshes, 0., -2., f64::INFINITY, &p, 0.).unwrap();
    assert_eq!(order.len(), 3);
    for (a, b) in order.iter().zip([-2., -1.3, -0.2]) {
        assert!((a - b).abs() < EPS);
    }
}

#[test]
fn full_ap_cut_precedes_reachable_shoulder_and_top_cap() {
    for kind in [CamToolKind::FlatEndMill, CamToolKind::BullNoseEndMill] {
        let center = Point2Dto::new(8., 7.);
        let mut doc = with_linking(fixture(vec![
            cylinder(center, 5., -3., -1.4),
            cylinder(center, 2.5, -1.4, -0.3),
        ]));
        doc.tools[0].kind = kind;
        doc.tools[0].corner_radius = (kind == CamToolKind::BullNoseEndMill).then_some(0.4);
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0] else { unreachable!() };
        parameters.maximum_stepdown = 1.5;
        let program = plan_setup(&doc, 1).unwrap();
        let mut levels = Vec::new();
        for c in &program.commands {
            if let CamCommandDto::Circular { to, feed, clockwise: true, .. } = c {
                if (*feed - 600.).abs() < EPS {
                    if levels.last().is_none_or(|z: &f64| (*z - to.z).abs() > EPS) {
                        levels.push(to.z);
                    }
                    if to.z > -1. {
                        assert!(dist(xy(*to), center) < 6., "top restarted at billet diameter: {to:?}");
                    }
                }
            }
        }
        assert_eq!(levels.len(), 4, "{kind:?}: {levels:?}");
        for (actual, expected) in levels.iter().zip([-1.5, -1.3, -0.2, -2.]) {
            assert!((actual - expected).abs() < EPS, "{kind:?}: {levels:?}");
        }
        assert_adaptive_nc_roundtrip(doc);
    }
}

#[test]
fn upward_exterior_uses_full_width_stock_only_above_the_previous_corner() {
    let center = Point2Dto::new(8., 7.);
    for (shoulder, expected_stock_radius) in [(-1.3, 5.1), (-1.7, 5.5)] {
        let mut doc = with_linking(fixture(vec![
            cylinder(center, 5., -3., shoulder - 0.1),
            cylinder(center, 2.5, shoulder - 0.1, -0.3),
        ]));
        doc.tools[0].kind = CamToolKind::BullNoseEndMill;
        doc.tools[0].corner_radius = Some(0.4);
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0] else { unreachable!() };
        parameters.maximum_stepdown = 3.;
        let program = plan_setup(&doc, 1).unwrap();
        let mut position = None;
        let mut first_radius = None;
        for command in &program.commands {
            match command {
                CamCommandDto::Circular { to, clockwise: true, feed, .. }
                    if (to.z - shoulder).abs() < EPS && (*feed - 600.).abs() < EPS => {
                    first_radius = Some(dist(position.unwrap(), center));
                    break;
                }
                CamCommandDto::Circular { to, .. }
                | CamCommandDto::Linear { to, .. }
                | CamCommandDto::Rapid { to } => position = Some(xy(*to)),
                _ => {}
            }
        }
        let radius = first_radius.expect("shoulder must be machined");
        assert!((radius - (expected_stock_radius + 1.)).abs() < 0.015,
            "shoulder {shoulder}: start radius {radius}");
        assert_adaptive_nc_roundtrip(doc);
    }
}

#[test]
fn rounded_major_bands_start_deep_and_overlap_the_corner_height() {
    let center = Point2Dto::new(8., 7.);
    let mut doc = with_linking(fixture(vec![
        cylinder(center, 5., -3., -1.4),
        cylinder(center, 2.5, -1.4, -0.3),
    ]));
    doc.tools[0].kind = CamToolKind::BullNoseEndMill;
    doc.tools[0].corner_radius = Some(0.4);
    let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0] else {
        unreachable!()
    };
    parameters.maximum_stepdown = 1.;
    let program = plan_setup(&doc, 1).unwrap();
    let mut levels = Vec::new();
    for c in &program.commands {
        if let CamCommandDto::Circular { to, feed, clockwise: true, .. } = c {
            if (*feed - 600.).abs() < EPS
                && levels.last().is_none_or(|z: &f64| (*z - to.z).abs() > EPS)
            {
                levels.push(to.z);
            }
        }
    }
    let expected = [-1., -0.2, -1.6, -1.3, -2.];
    assert_eq!(levels.len(), expected.len(), "{levels:?}");
    for (actual, expected) in levels.iter().zip(expected) {
        assert!((actual - expected).abs() < EPS, "{levels:?}");
    }


    let mut deepest = 0.;
    for z in levels {
        if z < deepest - EPS {
            if deepest < -EPS {
                assert!(deepest + 0.4 <= z + 1. + EPS);
            }
            deepest = z;
        }
    }
    assert_adaptive_nc_roundtrip(doc);
}

#[test]
fn full_radius_removal_cannot_be_used_below_its_corner_height() {
    let origin = Point2Dto::new(0., 0.);

    let history = vec![layers::Removal {
        depth: -0.6,
        exterior: None,
        centers: vec![origin],
    }];
    for (ceiling, expected) in [(-1., false), (-0.61, false), (-0.6, true), (0., true)] {
        let mut upper = Cleared::new(3., origin);
        layers::restore(&history, ceiling, &mut upper, &mut Work::default()).unwrap();
        assert_eq!(upper.contains_capsule(origin, origin, 3.), expected);
        assert!(!upper.contains_capsule(Point2Dto::new(4., 0.), Point2Dto::new(4., 0.), 3.));
    }
}

#[test]
fn removal_history_proves_upward_clearance_but_not_deeper_or_disconnected_stock() {
    let origin = Point2Dto::new(0., 0.);
    let history = vec![layers::Removal {
        depth: -2.,
        exterior: None,
        centers: vec![origin, Point2Dto::new(10., 0.)],
    }];
    for (z, expected) in [(-3., false), (-2., true), (-1., true)] {
        let mut c = Cleared::new(2., origin);
        layers::restore(&history, z, &mut c, &mut Work::default()).unwrap();
        assert_eq!(c.contains_capsule(origin, origin, 2.), expected);
        assert!(
            !c.contains(Point2Dto::new(5., 0.)),
            "separate cleared pockets must not erase the intervening wall"
        );
    }
}

fn stepped_cavity_job(mixed: bool) -> CamDocumentDto {
    let mut doc = with_linking(cavity_fixture());
    let CamOperationDto::Adaptive3d {
        bottom_z,
        parameters,
        geometry: Some(g),
        ..
    } = &mut doc.setups[0].operations[0]
    else {
        unreachable!()
    };
    *bottom_z = -2.;
    parameters.maximum_stepdown = 3.;
    g.targets.push(cuboid([2., 2., -2.5], [5., 12., -1.]));
    if mixed {
        doc.setups[0].stock.min.x = -3.;
        doc.setups[0].stock.min.y = -3.;
        doc.setups[0].stock.max.x = 19.;
        doc.setups[0].stock.max.y = 17.;
    }
    doc
}

#[test]
fn stepped_cavity_and_mixed_job_reuse_the_deep_entry_without_recutting_exterior() {
    for mixed in [false, true] {
        let doc = stepped_cavity_job(mixed);
        let program = plan_setup(&doc, 1).unwrap();
        assert!(
            program
                .warnings
                .iter()
                .any(|w| w.contains("1 helical entries")),
            "mixed={mixed}: {:?}",
            program.warnings
        );
        let mut levels = Vec::new();
        for c in &program.commands {
            if let CamCommandDto::Circular { to, feed, .. } = c {
                if (*feed - 600.).abs() < EPS
                    && levels.last().is_none_or(|z: &f64| (*z - to.z).abs() > EPS)
                {
                    levels.push(to.z);
                }
            }
        }
        assert!(
            levels.len() >= 2,
            "both the deep pocket and upper shelf must be cut: {levels:?}"
        );
        assert!(
            (levels[0] + 2.).abs() < EPS && (levels.last().unwrap() + 0.9).abs() < EPS,
            "{levels:?}"
        );
        if mixed {
            assert!(program
                .warnings
                .iter()
                .any(|w| w.contains("continuous exterior passes")));


            for c in &program.commands {
                if let CamCommandDto::Circular { to, feed, .. } = c {
                    if (*feed - 600.).abs() < EPS && to.z > -2. + EPS {
                        assert!(
                            to.x >= 0. && to.x <= 16. && to.y >= 0. && to.y <= 14.,
                            "{to:?}"
                        );
                    }
                }
            }
        }
        assert_adaptive_nc_roundtrip(doc);
    }
}

#[test]
fn stepped_exterior_cuts_deep_then_limits_top_to_remaining_small_diameter() {
    let center = Point2Dto::new(8., 7.);
    let mut doc = with_linking(fixture(vec![
        cylinder(center, 5., -3., -1.4),
        cylinder(center, 2.5, -1.4, -0.3),
    ]));
    doc.setups[0].stock_spec = CamStockSpecDto::FromModel {
        shape: CamStockShape::Cylinder,
        offsets: CamStockOffsetsDto::default(),
    };
    doc.setups[0].resolved_stock = CamResolvedStockDto::Cylinder { center, radius: 7. };
    let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0] else {
        unreachable!()
    };
    parameters.maximum_stepdown = 3.;
    let program = plan_setup(&doc, 1).unwrap();
    let mut levels = Vec::new();
    for c in &program.commands {
        if let CamCommandDto::Circular {
            to,
            feed,
            clockwise: true,
            ..
        } = c
        {
            if (*feed - 600.).abs() < EPS {
                if levels.last().is_none_or(|z: &f64| (*z - to.z).abs() > EPS) {
                    levels.push(to.z);
                }
                if to.z > -1. {
                    assert!(
                        dist(xy(*to), center) < 4.7,
                        "top cut restarted at billet diameter: {to:?}"
                    );
                }
            }
        }
    }
    assert_eq!(levels.len(), 3);
    for (a, b) in levels.iter().zip([-2., -1.3, -0.2]) {
        assert!((a - b).abs() < EPS);
    }
    assert_adaptive_nc_roundtrip(doc);
}

#[test]
fn exterior_removal_does_not_erase_a_disabled_pocket_or_bypass_a_narrow_neck() {
    for narrow_neck in [false, true] {
        let mut doc = stepped_cavity_job(true);
        let CamOperationDto::Adaptive3d {
            parameters,
            geometry: Some(g),
            ..
        } = &mut doc.setups[0].operations[0]
        else {
            unreachable!()
        };
        if narrow_neck {


            g.targets.extend([
                cuboid([2., 2., -1.], [6., 12., 0.]),
                cuboid([10., 2., -1.], [14., 12., 0.]),
            ]);
        } else {
            parameters.machine_cavities = false;
        }
        let program = plan_setup(&doc, 1).unwrap();
        assert!(program
            .warnings
            .iter()
            .any(|w| w.contains("0 helical entries")));
        assert!(!point_is_cut_at_depth(
            &program,
            Point2Dto::new(8., 7.),
            -2.,
            2.
        ));
        assert_adaptive_nc_roundtrip(doc);
    }
}

#[test]
fn short_tools_add_safe_bands_for_lowered_top_and_keep_upward_cleanup() {
    use crate::{simulate_setup, CamSimulationRequestDto, CamSimulationTargetDto};
    for (kind, cavity) in [
        (CamToolKind::FlatEndMill, false),
        (CamToolKind::BullNoseEndMill, false),
        (CamToolKind::FlatEndMill, true),
    ] {
        let mut doc = if cavity { stepped_cavity_job(true) } else {
            with_linking(fixture(vec![
                cylinder(Point2Dto::new(8., 7.), 5., -3., -1.4),
                cylinder(Point2Dto::new(8., 7.), 2.5, -1.4, -0.3),
            ]))
        };
        doc.tools[0].kind = kind;
        doc.tools[0].corner_radius = (kind == CamToolKind::BullNoseEndMill).then_some(0.4);
        doc.tools[0].flute_length = 1.;
        doc.tools[0].maximum_axial_depth = Some(0.8);
        let CamOperationDto::Adaptive3d { top_z, parameters, geometry: Some(g), .. } = &mut doc.setups[0].operations[0] else { unreachable!() };
        *top_z = -0.5;
        parameters.maximum_stepdown = 3.;
        let meshes = g.targets.clone();
        let original = doc.clone();
        let program = plan_setup(&doc, 1).unwrap();
        assert_eq!(doc, original, "planning must not rewrite tool or user settings");
        let first_cut = program.commands.iter().find_map(|c| match c {
            CamCommandDto::Circular { to, feed, clockwise: true, .. } if (*feed - 600.).abs() < EPS => Some(to.z),
            _ => None,
        }).unwrap();
        assert!((first_cut + 0.8).abs() < EPS, "{kind:?} cavity={cavity}: {first_cut}");
        let request = CamSimulationRequestDto {
            setup_id: 1, voxel_size: Some(0.25), max_voxels: None, stock_mesh: None,
            target: Some(CamSimulationTargetDto { cache_key: None, meshes, tolerance_mm: 0.05 }),
            through_operation_id: None, completed_steps: None, playback_time_seconds: None,
        };
        let short = simulate_setup(&doc, &request).unwrap();
        assert!(short.removed_volume_mm3 > 0.);
        assert!(short.collisions.is_empty(), "{:?}", short.collisions);
        assert_eq!(short.comparison.as_ref().unwrap().gouged_voxels, 0);
        assert_adaptive_nc_roundtrip(doc.clone());


        doc.tools[0].flute_length = 10.;
        if let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0] {
            parameters.maximum_stepdown = 0.8;
        }

        doc.linking[0].ramp_stepdown = doc.linking[0].ramp_stepdown.min(
            if kind == CamToolKind::BullNoseEndMill { 0.6 } else { 0.8 });
        assert_eq!(program.commands, plan_setup(&doc, 1).unwrap().commands);
        let long = simulate_setup(&doc, &request).unwrap();
        assert_eq!(short.steps.iter().map(|s| s.removed_voxels).collect::<Vec<_>>(),
            long.steps.iter().map(|s| s.removed_voxels).collect::<Vec<_>>(),
            "non-cutting shaft touched remaining stock: {kind:?}, cavity={cavity}");
    }
}

#[test]
fn corner_radius_at_or_above_stepdown_still_cuts_every_band() {
    let center = Point2Dto::new(8., 7.);
    for (corner, stepdown) in [(0.5, 0.5), (0.8, 0.5)] {
        let mut doc = with_linking(fixture(vec![cylinder(center, 2.5, -3., -0.3)]));
        doc.tools[0].kind = CamToolKind::BullNoseEndMill;
        doc.tools[0].corner_radius = Some(corner);
        let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0] else {
            unreachable!()
        };
        parameters.maximum_stepdown = stepdown;
        let p = parameters.clone();
        let program = plan_setup(&doc, 1).unwrap();
        let mut levels = Vec::<f64>::new();
        for c in &program.commands {
            if let CamCommandDto::Circular { to, feed, .. } = c {
                if (*feed - 600.).abs() < EPS && !levels.iter().any(|z| (z - to.z).abs() < EPS) {
                    levels.push(to.z);
                }
            }
        }
        let order = layers::depth_order(&doc.setups[0], &[cylinder(center, 2.5, -3., -0.3)], 0., -2., f64::INFINITY, &p, corner).unwrap();
        assert!(order.len() >= 4, "{order:?}");
        for expected in order {
            assert!(
                levels.iter().any(|z| (z - expected).abs() < EPS),
                "R{corner} Ap{stepdown}: band {expected} not cut; cut levels {levels:?}"
            );
        }
        assert_adaptive_nc_roundtrip(doc);
    }
}

#[test]
fn end_mill_roughing_depth_must_fit_the_declared_tool_length() {
    let center = Point2Dto::new(8., 7.);
    let mut doc = with_linking(fixture(vec![cylinder(center, 2.5, -3., -0.3)]));
    doc.tools[0].flute_length = 1.2;
    doc.tools[0].overall_length = 1.5;
    let error = plan_setup(&doc, 1).unwrap_err();
    assert!(error.0.contains("overall length"), "{error:?}");
    doc.tools[0].overall_length = 2.;
    plan_setup(&doc, 1).unwrap();
}

#[test]
fn model_shelves_above_the_selected_top_are_not_cut() {
    let center = Point2Dto::new(8., 7.);
    let mut doc = with_linking(fixture(vec![
        cylinder(center, 5., -3., -1.4),
        cylinder(center, 2.5, -1.4, -0.3),
    ]));
    let CamOperationDto::Adaptive3d { top_z, parameters, .. } = &mut doc.setups[0].operations[0] else {
        unreachable!()
    };
    *top_z = -1.4;
    parameters.maximum_stepdown = 3.;
    let program = plan_setup(&doc, 1).unwrap();
    let mut levels = Vec::<f64>::new();
    for c in &program.commands {
        if let CamCommandDto::Circular { to, feed, .. } = c {
            if (*feed - 600.).abs() < EPS && !levels.iter().any(|z| (z - to.z).abs() < EPS) {
                levels.push(to.z);
            }
        }
    }
    assert!(!levels.iter().any(|z| (z + 0.2).abs() < EPS), "boss top cut: {levels:?}");
    assert!(!levels.iter().any(|z| (z + 1.3).abs() < EPS), "Top shelf cut: {levels:?}");
    assert!(levels.iter().all(|z| *z <= -1.4 + EPS), "{levels:?}");
    assert_adaptive_nc_roundtrip(doc);
}

#[test]
fn later_roughing_starts_from_what_earlier_operations_left() {
    let center = Point2Dto::new(8., 7.);
    let mut doc = with_linking(fixture(vec![
        cylinder(center, 5., -3., -1.4),
        cylinder(center, 2.5, -1.4, -0.3),
    ]));
    let CamOperationDto::Adaptive3d { parameters, .. } = &mut doc.setups[0].operations[0] else {
        unreachable!()
    };
    parameters.maximum_stepdown = 3.;
    let single = plan_setup(&doc, 1).unwrap();
    let mut repeat = doc.setups[0].operations[0].clone();
    if let CamOperationDto::Adaptive3d { id, name, .. } = &mut repeat {
        *id = 2;
        *name = "Repeat".into();
    }
    doc.setups[0].operations.push(repeat);
    doc.next_operation_id = doc.next_operation_id.max(3);
    let mut link = doc.linking[0].clone();
    link.operation_id = 2;
    doc.linking.push(link);
    let program = plan_setup(&doc, 1).unwrap();
    let first = &program.per_operation[0];
    let second = program.per_operation.iter().find(|o| o.operation_id == 2);
    assert!((first.cutting_distance - single.per_operation[0].cutting_distance).abs() < 1e-6);
    let repeated = second.map_or(0., |o| o.cutting_distance);
    assert!(
        repeated < first.cutting_distance * 0.25,
        "repeat re-cut {repeated:.1} of {:.1} mm",
        first.cutting_distance
    );
    assert!(program.warnings.iter().any(|w| w.contains("simulated remaining stock")));
    let deps = crate::cam_operation_dependencies(&doc.setups[0], &doc.setups[0].operations[1], doc.linking.get(1));
    assert!(deps.iter().any(|d| d.operation_id == 1 && d.kind == crate::CamOperationDependencyKind::IncomingStockHeight));
}
