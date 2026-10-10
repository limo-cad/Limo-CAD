
#[test]
fn roughing_leads_fit_near_the_cut_without_changing_requested_sweeps() {
    let stock = [
        Point2Dto::new(-5., -5.),
        Point2Dto::new(5., -5.),
        Point2Dto::new(5., 5.),
        Point2Dto::new(-5., 5.),
    ];
    let tangent = Point2Dto::new(0., -1.);
    for sweep in [30., 45., 90., 135., 180.] {
        let mut builder = ProgramBuilder::new();
        let mut link = CamLinkingDto::default();
        link.lead_in.horizontal_radius = 1.;
        link.lead_in.sweep_degrees = sweep;
        link.lead_in.linear_distance = 0.5;
        link.lead_in.vertical_radius = 0.5;
        let original = link.clone();
        builder.linking = Some(link);
        for entry in [true, false] {
            let cut = Point2Dto::new(7.01, 0.);
            let d = linking_planner::fit_air_lead_distance(
                &builder, cut, tangent, 2., &stock, entry, 20.,
            )
            .unwrap();
            assert!(d < 0.01, "unnecessary detached lead at sweep {sweep}: {d}");
            assert_eq!(builder.linking.as_ref().unwrap(), &original);
            let offset = if entry { -d } else { d };
            let anchor = Point2Dto::new(cut.x, cut.y - offset);
            let (a, b) = if entry {
                (anchor, Point2Dto::new(anchor.x, anchor.y - 1.))
            } else {
                (Point2Dto::new(anchor.x, anchor.y + 1.), anchor)
            };
            let (leads, _, _) =
                linking_planner::air_leads_against_stock(&builder, a, b, 2., &stock).unwrap();
            let arc = if entry {
                leads.start_arc.unwrap()
            } else {
                leads.end_arc.unwrap()
            };
            let from = if entry { leads.line_end } else { b };
            let v = Point2Dto::new(from.x - arc.center.x, from.y - arc.center.y);
            let w = Point2Dto::new(arc.arc_end.x - arc.center.x, arc.arc_end.y - arc.center.y);
            assert!((v.x.hypot(v.y) - 1.).abs() < 1e-8);
            assert!((v.x * w.x + v.y * w.y - sweep.to_radians().cos()).abs() < 1e-8);



            let cut = Point2Dto::new(6., 0.);
            let d = linking_planner::fit_air_lead_distance(
                &builder, cut, tangent, 2., &stock, entry, 20.,
            )
            .unwrap();
            assert!(d > 5. && d < 20., "unsafe or unfitted anchor: {d}");
            let anchor = Point2Dto::new(cut.x, if entry { d } else { -d });
            let (a, b) = if entry {
                (anchor, Point2Dto::new(anchor.x, anchor.y - 1.))
            } else {
                (Point2Dto::new(anchor.x, anchor.y + 1.), anchor)
            };
            assert!(linking_planner::air_leads_against_stock(&builder, a, b, 2., &stock).is_ok());
        }
    }
}

fn linked_contour(mode: CompensationMode) -> CamDocumentDto {
    let op = closed_boss_operation(mode, ContourCompensation::Outside);
    let mut doc = document(vec![op], vec![tool(1, CamToolKind::FlatEndMill, 6.0)]);
    doc.linking.push(CamLinkingDto {
        operation_id: 1,
        lead_in: crate::CamLeadDto {
            horizontal_radius: 1.2,
            linear_distance: 1.2,
            vertical_radius: 0.5,
            ..Default::default()
        },
        ..Default::default()
    });
    doc
}

#[test]
fn linking_invalid_saved_intent_loses_generation_stamp_and_stays_repairable() {
    let mut doc = linked_contour(CompensationMode::InControl);
    doc.toolpath_generations
        .push(crate::CamToolpathGenerationDto {
            operation_id: 1,
            planner_revision: 7,
            model_fingerprint: "0".repeat(16),
            setup_fingerprint: "1".repeat(16),
            operation_fingerprint: "2".repeat(16),
            tool_fingerprint: "3".repeat(16),
            upstream_fingerprint: "4".repeat(16),
            order_dependencies: None,
        });
    doc.linking[0].lead_in.vertical_radius = -1.0;
    doc.soften_for_load();
    assert!(doc.toolpath_generations.is_empty());
    assert_eq!(doc.linking[0].lead_in.vertical_radius, -1.0);
    assert!(doc
        .load_warnings
        .iter()
        .any(|w| w.operation_id == Some(1) && w.message.contains("radii")));
    assert!(plan_setup(&doc, 1).is_err());
}

#[test]
fn linking_compensated_contour_post_roundtrip_keeps_the_same_stock() {
    use crate::{
        post::post_setup_unchecked as post_setup, simulate_gcode, simulate_setup, CamGcodeDialectDto,
        CamGcodeSimulationRequestDto, CamPostRequestDto, CamSimulationRequestDto,
    };
    use crate::{CamPostConfigDto, PostDialect, Siemens828dPostConfigDto};
    let mut doc = linked_contour(CompensationMode::InControl);
    crate::post::tests::bind_test_names(&mut doc, &[(1,"ContourTool")]);
    doc.tools[0].number = Some(1);
    doc.linking[0].lead_in.sweep_degrees = 60.0;
    doc.linking[0].same_as_lead_in = false;
    doc.linking[0].lead_out = crate::CamLeadDto {
        horizontal_radius: 0.9,
        sweep_degrees: 120.0,
        linear_distance: 1.4,
        vertical_radius: 0.3,
        ..Default::default()
    };
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
        let output = post_setup(
            &doc,
            &CamPostRequestDto {
                setup_id: 1,
                program_name: Some("LEAD_TEST".into()),
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
        let nc = simulate_gcode(
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
            predicted.removed_voxels.abs_diff(nc.removed_voxels) <= 4,
            "CAM removed {}, NC removed {}",
            predicted.removed_voxels,
            nc.removed_voxels
        );
        assert!(nc.collisions.is_empty(), "{:?}", nc.collisions);
    }
}

#[test]
fn linking_vertical_quarters_have_expected_radius_tangents_and_endpoints() {
    for entry in [true, false] {
        for angle in [0.0_f64, 0.37, 1.57, 3.1] {
            let t = Point2Dto::new(angle.cos(), angle.sin());
            let anchor = Point2Dto::new(3.0, 4.0);
            let p = linking_planner::vertical_points(anchor, t, -2.0, 1.2, entry).unwrap();
            assert!(p.len() > 10 && p.len() < 100);
            let center = Point3Dto::new(3.0, 4.0, -0.8);
            for point in &p {
                assert!((distance(*point, center) - 1.2).abs() < 1e-8);
            }
            let (a, b) = if entry {
                (p[p.len() - 2], p[p.len() - 1])
            } else {
                (p[0], p[1])
            };
            let horizontal = Point2Dto::new(b.x - a.x, b.y - a.y);
            assert!((horizontal.x * t.y - horizontal.y * t.x).abs() < 1e-10);
            assert!((b.z - a.z).abs() / distance(a, b) < 0.05);
            assert!((if entry { p.last().unwrap().z } else { p[0].z } + 2.0).abs() < 1e-8);
        }
    }
}

#[test]
fn linking_independent_sweeps_and_compensation_preserve_physical_radii() {
    for angle in [0.0, 30.0, 45.0, 90.0, 135.0, 180.0] {
        for control in [false, true] {
            let path = [Point2Dto::new(0.0, 0.0), Point2Dto::new(20.0, 0.0)];
            let mut link = CamLinkingDto::default();
            link.lead_in.horizontal_radius = 2.0;
            link.lead_in.sweep_degrees = angle;
            link.same_as_lead_in = false;
            link.lead_out.horizontal_radius = 3.0;
            link.lead_out.sweep_degrees = 45.0;
            let leads = linking_planner::contour_leads(
                &path,
                ContourLeadOptions {
                    closed: false,
                    inside_closed: false,
                    lead_in: 5.0,
                    lead_out: 5.0,
                    arc_radius: None,
                    bend_left: true,
                    control_compensation: control.then_some((true, 3.0)),
                },
                &link,
            )
            .unwrap();
            let physical =
                physical_contour_leads(&leads, &path, control.then_some((true, 3.0))).unwrap();
            assert_eq!(physical.start_arc.is_some(), angle > 0.0);
            if let Some(a) = &physical.start_arc {
                assert!((distance_2d(physical.line_end, a.center) - 2.0).abs() < 1e-8);
            }
            let a = physical.end_arc.unwrap();
            assert!((distance_2d(a.arc_end, a.center) - 3.0).abs() < 1e-8);
        }
    }
}

#[test]
fn linking_rounds_outside_compensation_and_keeps_g40_on_a_linear_block() {
    let doc = linked_contour(CompensationMode::InControl);
    let plan = plan_setup(&doc, 1).unwrap();
    for (i, c) in plan.commands.iter().enumerate() {
        if matches!(
            c,
            CamCommandDto::CutterCompensationOn { .. } | CamCommandDto::CutterCompensationOff
        ) {
            assert!(matches!(plan.commands[i + 1], CamCommandDto::Linear { .. }));
        }
    }
    assert!(plan.commands.len() > 40);
    let mut invalid = doc.clone();
    invalid.linking[0].lead_in.enabled = false;
    assert!(plan_setup(&invalid, 1)
        .unwrap_err()
        .0
        .contains("requires enabled"));
    invalid = doc.clone();
    invalid.linking[0].lead_in.perpendicular = true;
    assert!(plan_setup(&invalid, 1)
        .unwrap_err()
        .0
        .contains("software compensation"));
    invalid = doc.clone();
    invalid.linking[0].lead_in.vertical_radius = 100.0;
    assert!(plan_setup(&invalid, 1).is_err());
}

#[test]
fn linking_face_transitions_and_feed_rapids_are_real_commands() {
    let op = CamOperationDto::Face {
        id: 1,
        name: "Face".into(),
        enabled: true,
        tool_id: 1,
        bounds: Rect2Dto {
            min: Point2Dto::new(0.0, 0.0),
            max: Point2Dto::new(40.0, 30.0),
        },
        top_z: 0.0,
        target_z: -1.0,
        step_over: 5.0,
        step_down: 1.0,
        safe_distance: 1.0,
        direction: FaceDirection::BothWays,
        clearance_z: 10.0,
        retract_z: 5.0,
        feed_height_z: 3.0,
        cutting: cutting(),
    };
    let mut doc = document(vec![op], vec![tool(1, CamToolKind::FaceMill, 10.0)]);
    doc.linking.push(CamLinkingDto {
        operation_id: 1,
        keep_tool_down: true,
        maximum_stay_down: 100.0,
        lead_in: crate::CamLeadDto {
            linear_distance: 0.0,
            vertical_radius: 1.0,
            ..Default::default()
        },
        ..Default::default()
    });
    let smooth = plan_setup(&doc, 1).unwrap();
    assert!(smooth
        .commands
        .iter()
        .any(|c| matches!(c, CamCommandDto::Circular { .. })));
    doc.linking[0].transition = crate::CamFaceTransition::NoContact;
    let retract = plan_setup(&doc, 1).unwrap();
    assert!(!retract
        .commands
        .iter()
        .any(|c| matches!(c, CamCommandDto::Circular { .. })));
    assert!(retract.stats.rapid_distance > smooth.stats.rapid_distance);
    doc.linking[0].high_feed_mode = CamHighFeedMode::Always;
    let fed = plan_setup(&doc, 1).unwrap();
    assert!(!fed
        .commands
        .iter()
        .any(|c| matches!(c, CamCommandDto::Rapid { .. })));
    assert!(fed
        .commands
        .iter()
        .any(|c| matches!(c,CamCommandDto::Linear{feed,..} if *feed==5000.0)));
    doc.linking[0].high_feed_mode = CamHighFeedMode::Preserve;
    doc.linking[0].allow_rapid_retract = false;
    let fed = plan_setup(&doc, 1).unwrap();
    let mut previous = None;
    for c in &fed.commands {
        if let Some(to) = c.endpoint() {
            if let Some(from) = previous {
                let from: Point3Dto = from;
                if to.z > from.z && to.x == from.x && to.y == from.y {
                    assert!(!matches!(c, CamCommandDto::Rapid { .. }));
                }
            }
            previous = Some(to);
        }
    }
}

#[test]
fn linking_position_hints_and_profile_ramp_survive_serialization() {
    let mut doc = linked_contour(CompensationMode::InSoftware);
    doc.linking[0].entry_positions = vec![Point2Dto::new(10.0, 5.0)];
    let first = plan_setup(&doc, 1).unwrap();
    doc.linking[0].entry_positions = vec![Point2Dto::new(10.0, 15.0)];
    let second = plan_setup(&doc, 1).unwrap();
    assert_ne!(first.commands, second.commands);
    doc.linking[0].exit_positions = vec![Point2Dto::new(15.0, 10.0)];
    assert!(plan_setup(&doc, 1).unwrap().commands.len() > second.commands.len());
    doc.linking[0].exit_positions.clear();
    doc.linking[0].ramp_enabled = true;
    doc.linking[0].ramp_clearance = 0.1;
    doc.linking[0].lead_in.vertical_radius = 0.1;
    doc.linking[0].ramp_stepdown = 0.5;
    doc.linking[0].ramp_angle = 1.0;
    let plan = plan_setup(&doc, 1).unwrap();
    assert!(
        plan.commands
            .iter()
            .filter(
                |c| matches!(c,CamCommandDto::Linear{feed,..}if *feed==doc.linking[0].ramp_feed)
            )
            .count()
            > 10
    );
    let reopened: CamDocumentDto =
        serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(plan, plan_setup(&reopened, 1).unwrap());
}
