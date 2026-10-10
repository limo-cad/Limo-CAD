

fn manual_chamfer_fixture(hole: bool) -> CamDocumentDto {
    let path = if hole {
        (0..96)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / 96.0;
                Point2Dto::new(20.0 + 3.25 * a.cos(), 15.0 + 3.25 * a.sin())
            })
            .collect::<Vec<_>>()
    } else {
        vec![
            Point2Dto::new(10.0, 10.0),
            Point2Dto::new(30.0, 10.0),
            Point2Dto::new(30.0, 20.0),
            Point2Dto::new(10.0, 20.0),
        ]
    };
    let operation = serde_json::from_value(serde_json::json!({
        "kind":"chamfer2d", "id":1, "name":"Manual bevel", "enabled":true, "tool_id":3,
        "path":path, "closed":true,
        "modeled_chamfer":{"additional_width":0.0},
        "chain_ref":{"source":"model","keys":["edge:1:rim"]},
        "top_z":0.0, "chamfer_width":0.5, "tip_offset":1.0,
        "wall_side":if hole {"outside"} else {"inside"}, "direction":"climb",
        "clearance_z":10.0, "retract_z":3.0, "feed_height_z":1.0, "cutting":cutting()
    }))
    .unwrap();
    let mut doc = document(
        vec![operation],
        vec![tool(3, CamToolKind::ChamferMill, 6.0)],
    );
    doc.linking.push(CamLinkingDto {
        operation_id: 1,
        lead_in: crate::CamLeadDto {
            horizontal_radius: 0.4,
            sweep_degrees: 60.0,
            linear_distance: 0.5,
            vertical_radius: 0.2,
            ..Default::default()
        },
        lead_out: crate::CamLeadDto {
            horizontal_radius: 0.3,
            sweep_degrees: 120.0,
            linear_distance: 0.25,
            vertical_radius: 0.15,
            ..Default::default()
        },
        same_as_lead_in: false,
        lead_in_feed: 211.0,
        lead_out_feed: 311.0,
        ..Default::default()
    });
    doc
}

#[test]
fn chamfer_manual_radii_sweeps_feeds_and_vertical_rounding_are_real_motion() {
    for hole in [false, true] {
        for direction in [MillingDirection::Climb, MillingDirection::Conventional] {
            let mut doc = manual_chamfer_fixture(hole);
            let CamOperationDto::Chamfer2d {
                direction: d, path, ..
            } = &mut doc.setups[0].operations[0]
            else {
                unreachable!()
            };
            *d = direction;
            let boundary = path.clone();
            let before = doc.clone();
            let program = plan_setup(&doc, 1).unwrap();
            assert_eq!(doc, before, "planning must not adjust saved dimensions");
            assert!(!program.warnings.iter().any(|w| w.contains("reduced from")));
            let mut previous: Option<Point3Dto> = None;
            let mut arcs = Vec::new();
            let mut rounded_entry = 0;
            let mut rounded_exit = 0;
            let mut profile = 0;
            for c in &program.commands {
                let Some(to) = c.endpoint() else { continue };
                if let Some(from) = previous {
                    let a = Point2Dto::new(from.x, from.y);
                    let b = Point2Dto::new(to.x, to.y);
                    if let CamCommandDto::Circular {
                        center,
                        clockwise,
                        feed,
                        ..
                    } = c
                    {
                        let center = Point2Dto::new(center.x, center.y);
                        let angle = |p: Point2Dto| (p.y - center.y).atan2(p.x - center.x);
                        let sweep = ((if *clockwise { -1.0 } else { 1.0 }) * (angle(b) - angle(a)))
                            .rem_euclid(std::f64::consts::TAU)
                            .to_degrees();
                        arcs.push((distance_2d(a, center), sweep, *feed));
                        assert_eq!(to.z, -1.5);
                        let arc = LeadArc {
                            center,
                            clockwise: *clockwise,
                            arc_end: b,
                        };
                        assert!((0..boundary.len()).all(|i| arc_segment_distance(
                            a,
                            &arc,
                            boundary[i],
                            boundary[(i + 1) % boundary.len()]
                        ) >= 1.5 - 1e-6));
                    } else if let CamCommandDto::Linear { feed, .. } = c {
                        if to.z < 0.0 {
                            assert!((0..boundary.len()).all(|i| segment_segment_distance(
                                a,
                                b,
                                boundary[i],
                                boundary[(i + 1) % boundary.len()]
                            ) >= 1.5 - 1e-6));
                        }
                        if (from.z - to.z).abs() > EPSILON && distance_2d(a, b) > EPSILON {
                            if *feed == 211.0 {
                                rounded_entry += 1;
                            }
                            if *feed == 311.0 {
                                rounded_exit += 1;
                            }
                        }
                        if *feed == cutting().feed_xy {
                            profile += 1;
                            assert_eq!(to.z, -1.5);
                        }
                    }
                }
                previous = Some(to);
            }
            assert_eq!(arcs.len(), 2);
            for (actual, expected) in arcs.iter().zip([(0.4, 60.0, 211.0), (0.3, 120.0, 311.0)]) {
                assert!((actual.0 - expected.0).abs() < 1e-8);
                assert!((actual.1 - expected.1).abs() < 1e-8);
                assert_eq!(actual.2, expected.2);
            }
            assert!(rounded_entry > 10 && rounded_exit > 10 && profile >= 4);
        }
    }
}

#[test]
fn chamfer_manual_does_not_shrink_even_when_automatic_can_fit() {
    let mut doc = manual_chamfer_fixture(true);
    let link = &mut doc.linking[0];
    link.lead_in.horizontal_radius = 1.5;
    link.lead_in.linear_distance = 1.5;
    link.lead_in.sweep_degrees = 90.0;
    link.lead_in.vertical_radius = 0.0;
    link.same_as_lead_in = true;
    let requested = doc.linking.clone();
    let error = plan_setup(&doc, 1).unwrap_err().0;
    assert!(
        error.contains("manual lead-in/out") && error.contains("not changed"),
        "{error}"
    );
    assert_eq!(doc.linking, requested);
    doc.linking.clear();
    let automatic = plan_setup(&doc, 1).unwrap();
    assert!(automatic
        .warnings
        .iter()
        .any(|w| w.contains("1.500 to 1.125")));
}

#[test]
fn chamfer_vertical_extent_is_checked_even_when_horizontal_leads_fit() {
    let mut doc = manual_chamfer_fixture(true);
    assert!(plan_setup(&doc, 1).is_ok());
    doc.linking[0].lead_in.vertical_radius = 2.4;
    let error = plan_setup(&doc, 1).unwrap_err().0;
    assert!(error.contains("protected profile"), "{error}");
    doc.linking[0].lead_in.vertical_radius = 100.0;
    let error = plan_setup(&doc, 1).unwrap_err().0;
    assert!(error.contains("Feed Height"), "{error}");
}

#[test]
fn chamfer_disabled_and_same_as_entry_are_independent_of_exit_enable() {
    let mut doc = manual_chamfer_fixture(false);
    doc.linking[0].same_as_lead_in = true;
    doc.linking[0].lead_out.enabled = false;
    let plan = plan_setup(&doc, 1).unwrap();
    assert_eq!(
        plan.commands
            .iter()
            .filter(|c| matches!(c, CamCommandDto::Circular { .. }))
            .count(),
        1
    );
    doc.linking[0].lead_in.enabled = false;
    let plan = plan_setup(&doc, 1).unwrap();
    assert!(!plan
        .commands
        .iter()
        .any(|c| matches!(c, CamCommandDto::Circular { .. })));

    assert!(plan
        .commands
        .iter()
        .filter_map(|c| c.endpoint())
        .filter(|p| p.z < 0.0)
        .all(|p| p.z == -1.5));
    doc.linking[0].lead_in.enabled = true;
    doc.linking[0].lead_out.enabled = true;
    let plan = plan_setup(&doc, 1).unwrap();
    let circles = plan
        .commands
        .iter()
        .filter_map(|c| match c {
            CamCommandDto::Circular { to, center, .. } => {
                Some((to.x - center.x).hypot(to.y - center.y))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(circles.len(), 2);
    assert!(circles.iter().all(|r| (r - 0.4).abs() < 1e-8));
}

#[test]
fn chamfer_manual_open_perpendicular_straights_preserve_side_and_endpoints() {
    for wall in [ContourCompensation::Left, ContourCompensation::Right] {
        for direction in [MillingDirection::Climb, MillingDirection::Conventional] {
            let mut doc = manual_chamfer_fixture(false);
            if let CamOperationDto::Chamfer2d {
                path,
                closed,
                modeled_chamfer,
                chain_ref,
                wall_side,
                direction: d,
                ..
            } = &mut doc.setups[0].operations[0]
            {
                *path = vec![Point2Dto::new(10.0, 15.0), Point2Dto::new(30.0, 15.0)];
                *closed = false;
                *modeled_chamfer = None;
                *chain_ref = None;
                *wall_side = wall;
                *d = direction;
            }
            let link = &mut doc.linking[0];
            link.lead_in.horizontal_radius = 0.0;
            link.lead_in.linear_distance = 0.6;
            link.lead_in.vertical_radius = 0.0;
            link.lead_in.perpendicular = true;
            link.same_as_lead_in = true;
            let program = plan_setup(&doc, 1).unwrap();
            assert!(!program
                .commands
                .iter()
                .any(|c| matches!(c, CamCommandDto::Circular { .. })));
            let profile = program
                .commands
                .iter()
                .filter_map(|c| match c {
                    CamCommandDto::Linear { to, feed } if *feed == 800.0 => Some(*to),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(profile.len(), 1, "open edges must not get a closing cut");
            assert_eq!(
                profile[0].y,
                if wall == ContourCompensation::Left {
                    14.0
                } else {
                    16.0
                }
            );
            let forward =
                (wall == ContourCompensation::Right) == (direction == MillingDirection::Climb);
            assert_eq!(profile[0].x, if forward { 30.0 } else { 10.0 });
        }
    }
}

#[test]
fn chamfer_manual_multiple_chains_retract_and_name_the_failed_chain() {
    let mut doc = manual_chamfer_fixture(false);
    let hole = manual_chamfer_fixture(true).setups[0].operations[0]
        .chamfer_chains()
        .remove(0);
    if let CamOperationDto::Chamfer2d {
        additional_chains, ..
    } = &mut doc.setups[0].operations[0]
    {
        let mut hole = hole;
        hole.top_z = -2.0;
        hole.chain_ref.as_mut().unwrap().keys = vec!["edge:1:hole".into()];
        additional_chains.push(hole);
    }
    let plan = plan_setup(&doc, 1).unwrap();
    let depths = plan
        .commands
        .iter()
        .filter_map(|c| match c {
            CamCommandDto::Circular { to, .. } => Some(to.z),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(depths, vec![-1.5, -1.5, -3.5, -3.5]);
    let arcs = plan
        .commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| matches!(c, CamCommandDto::Circular { .. }).then_some(i))
        .collect::<Vec<_>>();
    assert!(plan.commands[arcs[1]..arcs[2]]
        .iter()
        .filter_map(|c| c.endpoint())
        .any(|p| p.z == 10.0));
    let link = &mut doc.linking[0];
    link.lead_in.horizontal_radius = 1.5;
    link.lead_in.linear_distance = 1.5;
    link.lead_in.sweep_degrees = 90.0;
    link.lead_in.vertical_radius = 0.0;
    link.same_as_lead_in = true;
    let error = plan_setup(&doc, 1).unwrap_err().0;
    assert!(
        error.contains("Chain 2") && error.contains("manual lead-in/out"),
        "{error}"
    );
}

#[test]
fn chamfer_manual_unsupported_link_policies_and_bad_numbers_fail_validation() {
    let doc = manual_chamfer_fixture(false);
    let op = &doc.setups[0].operations[0];
    for mutate in [
        |l: &mut CamLinkingDto| l.keep_tool_down = true,
        |l: &mut CamLinkingDto| l.retraction_policy = crate::CamRetractionPolicy::Minimum,
        |l: &mut CamLinkingDto| l.ramp_enabled = true,
        |l: &mut CamLinkingDto| l.entry_positions.push(Point2Dto::new(0.0, 0.0)),
        |l: &mut CamLinkingDto| l.exit_positions.push(Point2Dto::new(0.0, 0.0)),
        |l: &mut CamLinkingDto| l.predrill_positions.push(Point2Dto::new(0.0, 0.0)),
        |l: &mut CamLinkingDto| l.lead_in.horizontal_radius = -1.0,
        |l: &mut CamLinkingDto| l.lead_in.sweep_degrees = 181.0,
        |l: &mut CamLinkingDto| l.lead_out_feed = f64::NAN,
    ] {
        let mut link = doc.linking[0].clone();
        mutate(&mut link);
        assert!(link.validate(op).is_err());
    }
    let serialized = serde_json::to_string(&doc).unwrap();
    let reopened: CamDocumentDto = serde_json::from_str(&serialized).unwrap();
    assert_eq!(reopened.linking, doc.linking);
    assert_eq!(
        plan_setup(&reopened, 1).unwrap().commands,
        plan_setup(&doc, 1).unwrap().commands
    );
}

#[test]
fn chamfer_manual_feeds_and_rounding_roundtrip_to_nc_and_stock() {
    use crate::{
        post::post_setup_unchecked, simulate_gcode, simulate_setup, CamGcodeDialectDto,
        CamGcodeSimulationRequestDto, CamPostConfigDto, CamPostRequestDto, CamSimulationRequestDto,
        PostDialect, Siemens828dPostConfigDto,
    };
    let mut doc = manual_chamfer_fixture(true);
    crate::post::tests::bind_test_names(&mut doc, &[(3, "ChamferTool")]);
    doc.tools[0].number = Some(3);


    doc.linking[0].allow_rapid_retract = false;
    doc.linking[0].high_feed_mode = CamHighFeedMode::Always;
    let plan = plan_setup(&doc, 1).unwrap();
    assert!(!plan
        .commands
        .iter()
        .any(|c| matches!(c, CamCommandDto::Rapid { .. })));
    assert!(plan
        .commands
        .iter()
        .any(|c| matches!(c, CamCommandDto::Linear { feed, .. } if *feed == 5000.0)));
    let predicted = simulate_setup(
        &doc,
        &CamSimulationRequestDto {
            setup_id: 1,
            voxel_size: Some(0.5),
            max_voxels: None,
            stock_mesh: None,
            target: None,
            through_operation_id: None,
            completed_steps: None,
            playback_time_seconds: None,
        },
    )
    .unwrap();
    for (post, dialect) in [
        (PostDialect::Siemens828d, CamGcodeDialectDto::Siemens828d),
        (PostDialect::Fanuc, CamGcodeDialectDto::Fanuc),
    ] {
        let output = post_setup_unchecked(
            &doc,
            &CamPostRequestDto {
                setup_id: 1,
                program_name: Some("CHAMFER_LEADS".into()),
                post: Some(CamPostConfigDto {
                    machine_retract_z: Some(0.0),
                    tool_call_mode: crate::CamToolCallMode::Automatic,
                    dialect: post,
                    program_number: Some(123),
                    sequence_numbers: false,
                    siemens_828d: (post == PostDialect::Siemens828d)
                        .then(Siemens828dPostConfigDto::default),
                }),
            },
        )
        .unwrap();
        let actual = simulate_gcode(
            &doc,
            &CamGcodeSimulationRequestDto {
                setup_id: 1,
                source: output.nc,
                file_name: None,
                dialect,
                voxel_size: Some(0.5),
                max_voxels: None,
                stock_mesh: None,
                target: None,
                completed_steps: None,
            },
        )
        .unwrap();
        assert!(
            predicted.removed_voxels.abs_diff(actual.removed_voxels) <= 4,
            "CAM removed {}, NC removed {}",
            predicted.removed_voxels,
            actual.removed_voxels
        );
        assert!(actual.collisions.is_empty(), "{:?}", actual.collisions);
    }
}
