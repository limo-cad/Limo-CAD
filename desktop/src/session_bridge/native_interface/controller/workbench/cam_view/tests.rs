use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};

#[test]
fn physical_picker_visibility_restores_stock_hiding_and_retains_user_visibility() {
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let (mut app, services, _, document) = fixture_world(&fixture);
    let (_, _, mut before, _) = native_viewport::interface_view_snapshot(app.world());
    before.hidden_body_ids = vec![99];
    let mut after = before.clone();
    after.hidden_body_ids.push(1);
    let mut view = state(&services, &owner, document);
    view.applied = Some(Applied {
        owner: owner.clone(),
        preview_revision: 0,
        before_preview: default(),
        before_presentation: before,
        after_presentation: after.clone(),
        before_stock: None,
        stock_revision: 0,
        playback_stock_revision: None,
    });
    app.world_mut().insert_resource(view);
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        None,
        Some(after.clone()),
    )
    .unwrap();
    assert_eq!(geometry_hidden_bodies(app.world()).unwrap(), vec![99]);
    after.hidden_body_ids.push(88);
    native_viewport::apply_interface_view(app.world_mut(), &owner.document_id, None, Some(after))
        .unwrap();
    assert_eq!(
        geometry_hidden_bodies(app.world()).unwrap(),
        vec![99, 1, 88],
        "a newer user visibility change defeats the old restoration receipt"
    );
    Arc::make_mut(
        app.world_mut()
            .resource_mut::<State>()
            .document
            .as_mut()
            .unwrap(),
    )
    .setups[0]
        .resolved_stock = CamResolvedStockDto::ModelBody { body_id: 7 };
    assert_eq!(
        geometry_hidden_bodies(app.world()).unwrap(),
        vec![99, 1, 88, 7],
        "the Model picking view also hides a dedicated stock body"
    );
}

pub(super) fn job() -> CamDocumentDto {
    let mut document: CamDocumentDto = serde_json::from_value(json!({
        "setups":[{"id":1,"name":"Face test","body_ids":[1],
            "wcs":{"origin":{"x":0.,"y":0.,"z":4.},"x_axis":[1.,0.,0.],"y_axis":[0.,1.,0.],"z_axis":[0.,0.,1.]},
            "stock":{"min":{"x":0.,"y":0.,"z":-4.},"max":{"x":12.,"y":8.,"z":0.}},
            "operations":[{"kind":"face","id":1,"name":"Face top","enabled":true,"tool_id":1,
                "bounds":{"min":{"x":0.,"y":0.},"max":{"x":12.,"y":8.}},
                "top_z":0.,"target_z":-1.,"step_over":1.,"step_down":1.,
                "clearance_z":5.,"retract_z":2.,"feed_height_z":1.,
                "cutting":{"spindle_rpm":8000,"feed_xy":600.,"feed_z":150.,"coolant":"off"}}]}],
        "active_setup_id":1,
        "tools":[{"id":1,"number":1,"name":"2 mm end mill","kind":"flat_end_mill",
            "diameter":2.,"flute_length":8.,"overall_length":30.,"flute_count":2}],
        "next_setup_id":2,"next_tool_id":2,"next_operation_id":2
    })).unwrap();
    document.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
        Default::default(),
    ));
    document.validate_for_editing().unwrap();
    document
}

pub(super) fn fixture_world(fixture: &Fixture) -> (App, NativeServices, Entity, CamDocumentDto) {
    for (operation, arguments) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":12.,"y":8.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":3.}}),
        ),
        ("cam_set_document", serde_json::to_value(job()).unwrap()),
    ] {
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                operation,
                &arguments,
                || Ok(()),
            )
            .unwrap();
    }
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    app.world_mut().init_resource::<Assets<Image>>();
    app.world_mut().init_resource::<ViewportUiAssets>();
    let camera = app.world_mut().spawn(InterfaceCamera).id();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    (
        app,
        services,
        camera,
        fixture.engine.cam_document_snapshot(),
    )
}

pub(super) fn state(
    services: &NativeServices,
    owner: &DocumentContext,
    document: CamDocumentDto,
) -> State {
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, owner)
        .unwrap();
    State {
        key: Some(Key {
            owner: owner.clone(),
            revision: receipt.revision,
            selection: None,
        }),
        document: Some(Arc::new(document)),
        setup: Some(1),
        generation: 7,
        paths: true,
        ..default()
    }
}

pub(super) fn prepared(message: &str) -> Prepared {
    Prepared {
        paths: Vec::new(),
        tool: None,
        simulation: None,
        stock: None,
        message: message.into(),
        details: message.into(),
        path_id: 1,
        start_time: 0.,
        nc_kernel: None,
    }
}

#[test]
fn native_cam_playback_stock_pose_and_seek_share_one_physical_clock() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (app, _, _, document) = fixture_world(&fixture);
    let mut request = simulation_request(
        app.world(),
        &document,
        document.setup(1).unwrap(),
        Some(1),
        default(),
    )
    .unwrap();
    request.voxel_size = Some(0.5);
    request.max_voxels = Some(20_000);
    let complete = limo_cad_cam::simulate_setup(&document, &request).unwrap();
    let mut expected =
        limo_cad_cam::CamPlayback::new(document.clone(), request.clone(), 0., None).unwrap();
    let mut player = playback::Player::new(document.clone(), request, None).unwrap();
    let end = complete.estimated_seconds;
    let mut previous_stock = None;
    for time in [0., end * 0.75, end * 0.25, end] {
        player.seek(time, end);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !player.poll(end).unwrap() {
            assert!(
                std::time::Instant::now() < deadline,
                "Playback frame timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let frame = player.frame.as_ref().unwrap();
        assert_eq!(frame.time, time);
        let sampled = expected.sample(time, None).unwrap();
        if sampled.stock_mesh.is_some() {
            previous_stock = crate::retained_cam_stock(&sampled);
        }
        assert_eq!(
            frame.stock.as_ref().map(|s| &*s.positions),
            previous_stock.as_ref().map(|s| &*s.positions)
        );
        let (tool, progress) = timeline::pose(&document, &complete, 91, time).unwrap();
        let progress = progress.unwrap();
        assert_eq!(progress.time_seconds, frame.time);
        assert_eq!(progress.path_id, 91);
        assert_eq!(tool.unwrap().tip, progress.position);
    }
    player.seek(end * 0.8, end);
    assert!(!player.poll(end).unwrap());
    player.seek(end * 0.1, end);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !player.poll(end).unwrap() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(player.time(), end * 0.1);
    player.seek(end * 0.4, end);
    player.toggle(end * 0.4, end);
    assert_eq!(
        player.requested,
        Some(end * 0.4),
        "Play must keep the selected operation's start"
    );
}

#[test]
fn native_cam_timeline_uses_shared_helical_arcs_and_next_tool_at_boundary() {
    use limo_cad_cam::{CamArcPlane, CamSimulationStepDto, CamSimulationStepKind, Point3Dto};
    let point = |[x, y, z]: [f64; 3]| Point3Dto::new(x, y, z);
    for (plane, from, to, mid) in [
        (
            CamArcPlane::Xy,
            [5., 0., 2.],
            [0., 5., 6.],
            [5. / 2_f64.sqrt(), 5. / 2_f64.sqrt(), 4.],
        ),
        (
            CamArcPlane::Xz,
            [0., 2., 5.],
            [5., 6., 0.],
            [5. / 2_f64.sqrt(), 4., 5. / 2_f64.sqrt()],
        ),
        (
            CamArcPlane::Yz,
            [2., 5., 0.],
            [6., 0., 5.],
            [4., 5. / 2_f64.sqrt(), 5. / 2_f64.sqrt()],
        ),
    ] {
        let step = CamSimulationStepDto {
            command_index: 1,
            source_line: None,
            kind: CamSimulationStepKind::Circular,
            tool_id: Some(1),
            from: Some(point(from)),
            to: Some(point(to)),
            center: Some(point([0.; 3])),
            clockwise: Some(false),
            plane: Some(plane),
            duration_seconds: 2.,
            cumulative_seconds: 2.,
            removed_voxels: 0,
            gouged_voxels: 0,
        };
        assert_eq!(step.point_at_fraction(0.).unwrap(), Some(point(from)));
        assert_eq!(step.point_at_fraction(1.).unwrap(), Some(point(to)));
        let actual = step.point_at_fraction(0.5).unwrap().unwrap();
        assert!(
            (actual.x - mid[0]).abs() < 1e-10
                && (actual.y - mid[1]).abs() < 1e-10
                && (actual.z - mid[2]).abs() < 1e-10
        );
        assert!(step.point_at_fraction(f64::NAN).is_err());
        let mut result = limo_cad_cam::simulate_setup(
            &job(),
            &limo_cad_cam::CamSimulationRequestDto {
                setup_id: 1,
                voxel_size: Some(1.),
                max_voxels: Some(20_000),
                stock_mesh: None,
                target: None,
                through_operation_id: None,
                completed_steps: None,
                playback_time_seconds: None,
            },
        )
        .unwrap();
        let mut next = step.clone();
        next.tool_id = Some(2);
        next.cumulative_seconds = 4.;
        result.steps = vec![step, next];
        result.estimated_seconds = 4.;
        assert_eq!(timeline::step_at(&result, 2.).unwrap().tool_id, Some(2));
        assert_eq!(timeline::adjacent_move(&result, 0., true, 0.), 2.);
        assert_eq!(timeline::adjacent_move(&result, 2., true, 0.), 4.);
        assert_eq!(timeline::adjacent_move(&result, 4., true, 0.), 4.);
        assert_eq!(timeline::adjacent_move(&result, 4., false, 0.), 2.);
        assert_eq!(timeline::adjacent_move(&result, 2., false, 0.), 0.);
        assert_eq!(timeline::adjacent_move(&result, 3., false, 2.5), 2.5);
        let layers = timeline::paths(&result, 91, 0).unwrap();
        assert_eq!(layers[0].segments.len() / 6, 64);
        let times = &layers[0].playback.as_ref().unwrap().segment_times;
        assert_eq!(times.first(), Some(&0.));
        assert_eq!(times.last(), Some(&4.));
        assert!(!layers[0].playback.as_ref().unwrap().single_tool);
        assert!(timeline::paths(&result, 91, 2)
            .unwrap()
            .iter()
            .all(|layer| !layer.playback.as_ref().unwrap().single_tool));
        result.steps[1].tool_id = Some(1);
        assert!(timeline::paths(&result, 91, 0)
            .unwrap()
            .iter()
            .all(|layer| layer.playback.as_ref().unwrap().single_tool));
        result.steps[0].tool_id = None;
        assert!(timeline::paths(&result, 91, 2)
            .unwrap()
            .iter()
            .all(|layer| !layer.playback.as_ref().unwrap().single_tool));
    }
}

fn marker(value: f32) -> ViewportPreview {
    ViewportPreview {
        lines: vec![ViewportLineLayer {
            color: [0.2, 0.7, 0.3, 1.],
            width: 1.,
            segments: vec![value, 0., 0., value, 1., 0.].into(),
            ..default()
        }],
        ..default()
    }
}

#[test]
fn shared_planner_simulator_and_retained_mesh_match_without_mutating_intent() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (app, _, _, document) = fixture_world(&fixture);
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let setup = document.setup(1).unwrap();
    let mut request =
        simulation_request(app.world(), &document, setup, Some(1), default()).unwrap();
    request.voxel_size = Some(0.5);
    request.max_voxels = Some(20_000);
    assert_eq!(request.target.as_ref().unwrap().meshes.len(), 1);
    let expected = limo_cad_cam::simulate_setup(&document, &request).unwrap();
    let expected_stock = crate::retained_cam_stock(&expected).unwrap();
    let rendered = prepare(
        &document,
        1,
        Some(1),
        Some(request.clone()),
        &CamSimulationCancellation::default(),
        None,
    )
    .unwrap();
    assert!(rendered.paths.iter().any(|p| !p.segments.is_empty()));
    assert!(rendered.tool.is_some());
    let simulation = rendered.simulation.unwrap();
    assert!(simulation.stock_mesh.is_none() && simulation.native_stock_present);
    assert!(simulation.removed_volume_mm3 > 0.);
    assert_eq!(simulation.removed_volume_mm3, expected.removed_volume_mm3);
    assert_eq!(simulation.comparison, expected.comparison);
    let stock = rendered.stock.unwrap();
    assert_eq!(*stock.positions, *expected_stock.positions);
    assert_eq!(*stock.normals, *expected_stock.normals);
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    let cancelled = CamSimulationCancellation::default();
    cancelled.cancel();
    assert!(prepare(&document, 1, Some(1), Some(request), &cancelled, None).is_err());
}

#[test]
fn selected_operation_preview_uses_shared_prefix_and_arc_axes() {
    let mut document = job();
    let mut invalid_later = document.setups[0].operations[0].clone();
    if let limo_cad_cam::CamOperationDto::Face { id, tool_id, .. } = &mut invalid_later {
        *id = 2;
        *tool_id = 999;
    }
    document.setups[0].operations.push(invalid_later);
    document.next_operation_id = 3;
    assert!(prepare(
        &document,
        1,
        None,
        None,
        &CamSimulationCancellation::default(),
        None
    )
    .is_err());
    assert!(prepare(
        &document,
        1,
        Some(1),
        None,
        &CamSimulationCancellation::default(),
        None
    )
    .is_ok());
    for (plane, from, to) in [
        (limo_cad_cam::CamArcPlane::Xy, [1., 0., 0.], [0., 1., 2.]),
        (limo_cad_cam::CamArcPlane::Xz, [0., 0., 1.], [1., 2., 0.]),
        (limo_cad_cam::CamArcPlane::Yz, [0., 1., 0.], [2., 0., 1.]),
    ] {
        let point = |[x, y, z]: [f64; 3]| limo_cad_cam::Point3Dto::new(x, y, z);
        let points = geometry::arc_points(point(from), point(to), point([0.; 3]), plane, false);
        assert_eq!(points.last(), Some(&point(to)));
        assert!(points.len() >= 8);
        let first = points[0];
        let radial = match plane {
            limo_cad_cam::CamArcPlane::Xy => first.x.hypot(first.y),
            limo_cad_cam::CamArcPlane::Xz => first.z.hypot(first.x),
            limo_cad_cam::CamArcPlane::Yz => first.y.hypot(first.z),
        };
        assert!((radial - 1.).abs() < 1e-10);
    }
}

#[test]
fn cancelled_and_replaced_owner_completions_cannot_reinstall_cam_graphics() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, camera, document) = fixture_world(&fixture);
    let mut view = state(&services, &fixture.owner(), document);
    let (send, receiver) = mpsc::channel();
    let cancellation = CamSimulationCancellation::default();
    view.pending = Some(Pending {
        key: view.key.clone().unwrap(),
        generation: view.generation,
        cancellation: cancellation.clone(),
        receiver: Mutex::new(receiver),
    });
    app.world_mut().insert_resource(view);
    execute(app.world_mut(), &Command::Cancel).unwrap();
    assert!(cancellation.is_cancelled());
    assert!(send
        .send(Ok(prepared("Cancelled result must not display")))
        .is_ok());
    synchronize(
        app.world_mut(),
        camera,
        &services,
        &fixture.owner(),
        (1360., 860., 280.),
        true,
    )
    .unwrap();
    assert!(app.world().resource::<State>().prepared.is_none());
    assert!(app.world().resource::<State>().pending.is_none());
    let (send, receiver) = mpsc::channel();
    {
        let mut view = app.world_mut().resource_mut::<State>();
        view.pending = Some(Pending {
            key: view.key.clone().unwrap(),
            generation: view.generation,
            cancellation: CamSimulationCancellation::default(),
            receiver: Mutex::new(receiver),
        });
    }
    let old_owner = fixture.owner();
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &old_owner, false, || Ok(()))
        .unwrap();
    assert_ne!(fixture.owner(), old_owner);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    native_viewport::apply_interface_preview(
        app.world_mut(),
        &fixture.owner().document_id,
        marker(91.),
    )
    .unwrap();
    assert!(send
        .send(Ok(prepared("Retired document result must not display")))
        .is_ok());
    synchronize(
        app.world_mut(),
        camera,
        &services,
        &fixture.owner(),
        (1360., 860., 280.),
        true,
    )
    .unwrap();
    assert!(app.world().resource::<State>().prepared.is_none());
    assert_eq!(
        native_viewport::interface_preview_snapshot(app.world()).lines[0].segments[0],
        91.
    );
    synchronize(
        app.world_mut(),
        camera,
        &services,
        &fixture.owner(),
        (1360., 860., 280.),
        false,
    )
    .unwrap();
}

#[test]
fn cam_overlay_restores_its_own_values_and_preserves_a_newer_overlay() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _, document) = fixture_world(&fixture);
    let owner = fixture.owner();
    let initial_stock = ViewportCamStock {
        time_seconds: None,
        positions: Arc::new(vec![0., 0., 0., 1., 0., 0., 0., 1., 0.]),
        normals: Arc::new(vec![0., 0., 1., 0., 0., 1., 0., 0., 1.]),
    };
    native_viewport::apply_interface_preview(app.world_mut(), &owner.document_id, marker(12.))
        .unwrap();
    native_viewport::apply_interface_cam_stock(
        app.world_mut(),
        &owner.document_id,
        Some(initial_stock.clone()),
    )
    .unwrap();
    let (_, _, mut initial, _) = native_viewport::interface_view_snapshot(app.world());
    initial.cam_stock_visible = true;
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        None,
        Some(initial.clone()),
    )
    .unwrap();
    let mut view = state(&services, &owner, document);
    display(app.world_mut(), &services, &mut view).unwrap();
    restore(app.world_mut(), &services, &mut view).unwrap();
    assert_eq!(
        native_viewport::interface_preview_snapshot(app.world()).lines[0].segments[0],
        12.
    );
    assert_eq!(
        *native_viewport::interface_cam_stock_snapshot(app.world())
            .1
            .unwrap()
            .positions,
        *initial_stock.positions
    );
    assert!(
        native_viewport::interface_view_snapshot(app.world())
            .2
            .cam_stock_visible
    );
    display(app.world_mut(), &services, &mut view).unwrap();
    native_viewport::apply_interface_preview(app.world_mut(), &owner.document_id, marker(52.))
        .unwrap();
    let newer_stock = ViewportCamStock {
        time_seconds: None,
        positions: Arc::new(vec![2., 0., 0., 3., 0., 0., 2., 1., 0.]),
        normals: initial_stock.normals.clone(),
    };
    native_viewport::apply_interface_cam_stock(
        app.world_mut(),
        &owner.document_id,
        Some(newer_stock.clone()),
    )
    .unwrap();
    let (_, _, mut newer, _) = native_viewport::interface_view_snapshot(app.world());
    newer.cam_stock_visible = false;
    newer.ghosted_body_ids = vec![1];
    newer.selected_body_ids = vec![1];
    native_viewport::apply_interface_view(app.world_mut(), &owner.document_id, None, Some(newer))
        .unwrap();
    restore(app.world_mut(), &services, &mut view).unwrap();
    assert_eq!(
        native_viewport::interface_preview_snapshot(app.world()).lines[0].segments[0],
        52.
    );
    assert_eq!(
        *native_viewport::interface_cam_stock_snapshot(app.world())
            .1
            .unwrap()
            .positions,
        *newer_stock.positions
    );
    let presentation = native_viewport::interface_view_snapshot(app.world()).2;
    assert!(!presentation.cam_stock_visible);
    assert_eq!(presentation.ghosted_body_ids, vec![1]);
    assert_eq!(presentation.selected_body_ids, vec![1]);
}

#[test]
fn leaving_cam_releases_the_completed_preview_document() {
    let document = Arc::new(job());
    let retired = Arc::downgrade(&document);
    let owner = DocumentContext {
        window_id: "main".into(),
        document_id: "preview-document".into(),
        epoch: 1,
    };
    let mut world = World::new();
    world.insert_resource(State {
        key: Some(Key {
            owner: owner.clone(),
            revision: 1,
            selection: None,
        }),
        document: Some(document),
        setup: Some(1),
        operation: Some(2),
        ..default()
    });
    let services = NativeServices {
        engine: Arc::new(AppState::new()),
        bridge: Arc::new(SessionBridgeState::default()),
    };
    synchronize(
        &mut world,
        Entity::PLACEHOLDER,
        &services,
        &owner,
        (1000., 700., 260.),
        false,
    )
    .unwrap();
    assert!(retired.upgrade().is_none());
    let state = world.resource::<State>();
    assert!(state.key.is_none());
    assert!(state.document.is_none());
    assert!(state.setup.is_none());
    assert!(state.operation.is_none());
}
