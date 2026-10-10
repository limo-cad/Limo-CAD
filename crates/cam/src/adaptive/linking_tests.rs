fn with_linking(mut doc: CamDocumentDto) -> CamDocumentDto {
    let mut link = crate::CamLinkingDto {
        operation_id: 1,
        ramp_enabled: true,
        minimum_helix_diameter: 1.6,
        helix_diameter: 3.8,
        ramp_stepdown: 0.5,
        lead_in_feed: 250.0,
        lead_out_feed: 300.0,
        ..Default::default()
    };
    link.lead_in = crate::CamLeadDto {
        horizontal_radius: 0.4,
        vertical_radius: 0.3,
        linear_distance: 0.0,
        ..Default::default()
    };
    doc.linking.push(link);
    doc
}

#[test]
fn adaptive_linking_exterior_roundtrip_and_cache_invalidation() {
    let mut doc = with_linking(fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, 0.0])]));
    assert_adaptive_nc_roundtrip(doc.clone());
    let before = plan_setup(&doc, 1).unwrap();
    doc.linking[0].lead_in_feed = 50.0;
    let after = plan_setup(&doc, 1).unwrap();
    assert_ne!(before.commands, after.commands);
    assert!(after.stats.estimated_seconds > before.stats.estimated_seconds);
    let full = after.stats.rapid_distance;
    for policy in [
        crate::CamRetractionPolicy::Minimum,
        crate::CamRetractionPolicy::Shortest,
    ] {
        doc.linking[0].retraction_policy = policy;
        let program = plan_setup(&doc, 1).unwrap();
        assert!(program.stats.rapid_distance <= full + EPS);
        assert_adaptive_nc_roundtrip(doc.clone());
    }
}

#[test]
fn adaptive_exterior_stays_down_with_equal_verified_removal() {
    for kind in [CamToolKind::FlatEndMill, CamToolKind::BullNoseEndMill, CamToolKind::FaceMill] {
    let mut doc = with_linking(fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, 0.0])]));
    doc.tools[0].kind = kind;
    if kind != CamToolKind::FlatEndMill {
        doc.tools[0].corner_radius = Some(0.4);
    }
    if kind == CamToolKind::FaceMill {
        doc.tools[0].maximum_axial_depth = Some(1.0);
    }
    let retracted = plan_setup(&doc, 1).unwrap();
    let request: crate::CamSimulationRequestDto = serde_json::from_value(serde_json::json!({
        "setup_id": 1, "voxel_size": 0.25,
    })).unwrap();
    let before_stock = crate::simulate_setup(&doc, &request).unwrap();
    doc.linking[0].keep_tool_down = true;
    doc.linking[0].maximum_stay_down = 60.0;
    doc.linking[0].stay_down_level = 100;
    let linked = plan_setup(&doc, 1).unwrap();
    assert!(linked.stats.rapid_distance < retracted.stats.rapid_distance * 0.75);
    assert!(linked.stats.estimated_seconds < retracted.stats.estimated_seconds);
    let after_stock = crate::simulate_setup(&doc, &request).unwrap();
    assert_eq!(before_stock.remaining_voxels, after_stock.remaining_voxels);
    assert!(after_stock.collisions.is_empty());
    assert_adaptive_nc_roundtrip(doc.clone());

    doc.linking[0].entry_positions.push(Point2Dto::new(8.0, -5.0));
    assert_adaptive_nc_roundtrip(doc);
    }
}

#[test]
fn adaptive_linking_cavity_rolls_and_taper_are_checked() {
    let mut doc = with_linking(cavity_fixture());
    doc.linking[0].keep_tool_down = true;
    doc.linking[0].maximum_stay_down = 60.0;
    doc.linking[0].stay_down_level = 100;
    doc.linking[0].minimum_clearance = 0.05;
    doc.linking[0].lift_height = 0.1;
    doc.linking[0].ramp_taper_angle = 1.0;
    assert_adaptive_nc_roundtrip(doc.clone());
    let program = plan_setup(&doc, 1).unwrap();
    assert!(program.commands.iter().any(
        |c| matches!(c,CamCommandDto::Linear{feed,..} if (*feed-doc.linking[0].ramp_feed).abs()<EPS)
    ));
    doc.linking[0].helix_diameter = 1.6;
    assert!(plan_setup(&doc, 1)
        .unwrap_err()
        .0
        .contains("diameter range"));
}

#[test]
fn adaptive_linking_predrill_is_proof_not_an_xy_permission() {
    let doc = with_linking(cavity_fixture());
    let setup = &doc.setups[0];
    let CamOperationDto::Adaptive3d { parameters, .. } = &setup.operations[0] else {
        unreachable!()
    };
    let c = Point2Dto::new(8.0, 7.0);
    let mut builder = ProgramBuilder::new();
    builder.tool_radius = 2.0;
    builder.incoming_top = 0.0;
    builder.feed_height_z = 1.0;
    builder.retract_z = 3.0;
    builder.clearance_z = 5.0;
    let mut link = doc.linking[0].clone();
    link.ramp_type = crate::CamRampType::Predrill;
    link.predrill_positions.push(c);
    builder.linking = Some(link);
    assert!(configured_ramp(&mut builder, c, 0.8, -1.0, parameters)
        .unwrap_err()
        .0
        .contains("earlier enabled hole"));
    builder
        .predrilled
        .push(super::super::linking_planner::PredrilledHole {
            center: c,
            radius: 2.5,
            bottom: -0.5,
        });
    assert!(
        configured_ramp(&mut builder, c, 0.8, -1.0, parameters).is_err(),
        "drill tip is not cylindrical depth"
    );
    builder.predrilled[0].bottom = -2.0;
    configured_ramp(&mut builder, c, 0.8, -1.0, parameters).unwrap();
    builder.linking.as_mut().unwrap().ramp_type = crate::CamRampType::Plunge;
    builder.predrilled.clear();
    configured_ramp(&mut builder, c, 0.8, -1.0, parameters).unwrap();
    assert!(builder
        .warnings
        .iter()
        .any(|w| w.contains("full-width axial cutting")));
}

#[test]
fn automatic_link_feeds_follow_cutting_feed_and_preserve_manual_overrides() {
    let mut doc = with_linking(fixture(vec![cuboid([6.0, 5.0, -3.0], [10.0, 9.0, 0.0])]));
    doc.linking[0].lead_in_feed_auto = true;
    doc.linking[0].lead_out_feed_auto = true;
    doc.linking[0].no_engagement_feed_auto = true;
    for feed in [600., 777.] {
        let CamOperationDto::Adaptive3d { cutting, .. } = &mut doc.setups[0].operations[0] else { unreachable!() };
        cutting.feed_xy = feed;
        let mut explicit = doc.clone();
        explicit.linking[0].resolve_feeds(feed);
        explicit.linking[0].lead_in_feed_auto = false;
        explicit.linking[0].lead_out_feed_auto = false;
        explicit.linking[0].no_engagement_feed_auto = false;
        assert_eq!(plan_setup(&doc, 1).unwrap().commands, plan_setup(&explicit, 1).unwrap().commands);
    }
    doc.linking[0].lead_in_feed_auto = false;
    doc.linking[0].lead_in_feed = 123.;
    let loaded: CamDocumentDto = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(doc.linking, loaded.linking);
    let path = plan_setup(&loaded, 1).unwrap();
    assert!(path.commands.iter().any(|c| matches!(c, CamCommandDto::Linear { feed, .. } | CamCommandDto::Circular { feed, .. } if (*feed - 123.).abs() < EPS)));
    let mut legacy = serde_json::to_value(&doc.linking[0]).unwrap();
    for key in ["lead_in_feed_auto", "lead_out_feed_auto", "no_engagement_feed_auto"] {
        legacy.as_object_mut().unwrap().remove(key);
    }
    let mut legacy: crate::CamLinkingDto = serde_json::from_value(legacy).unwrap();
    let before = legacy.clone();
    legacy.resolve_feeds(999.);
    assert_eq!(legacy, before, "legacy numeric feeds are explicit overrides");
}
