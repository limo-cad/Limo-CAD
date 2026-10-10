use super::*;
use crate::session_bridge::native_interface::tests::Fixture;
use limo_cad_interface::ControlInput;

fn input() -> nc_input::Input {
    nc_input::Input {
        source: "N10 G21 G90\nN20 T1 M6\nN30 S8000 M3\nN40 G0 X1 Y4 Z1\nN50 G1 Z-1 F120\nN60 G1 X11 F120\nN70 G0 Z5\nN80 M30\n".into(),
        file_name: Some("physical.nc".into()),
        dialect: limo_cad_cam::CamGcodeDialectDto::Iso,
    }
}

#[test]
fn empty_setup_preview_is_neutral_and_explicit_generated_simulation_still_validates() {
    let mut document = super::super::tests::job();
    document.setups[0].operations.clear();
    let cancellation = CamSimulationCancellation::default();
    let preview = super::super::prepare(&document, 1, None, None, &cancellation, None).unwrap();
    assert!(preview.message.contains("simulate NC"));
    assert!(preview.simulation.is_none());
    assert!(preview.paths.is_empty());
    let request = serde_json::from_value(json!({"setup_id":1})).unwrap();
    assert!(
        super::super::prepare(&document, 1, None, Some(request), &cancellation, None)
            .err()
            .unwrap()
            .contains("no enabled operations")
    );
    document.setups[0].operations = super::super::tests::job().setups.remove(0).operations;
    document.setups[0]
        .operations
        .iter_mut()
        .for_each(|operation| match operation {
            limo_cad_cam::CamOperationDto::Face { enabled, .. } => *enabled = false,
            _ => unreachable!(),
        });
    assert!(super::super::prepare(&document, 1, None, None, &cancellation, None).is_ok());
}

#[test]
fn nc_without_operations_transfers_verified_stock_to_player_and_keeps_source_lines() {
    let mut document = super::super::tests::job();
    document.setups[0].operations.clear();
    let before = serde_json::to_value(&document).unwrap();
    let request: CamSimulationRequestDto = serde_json::from_value(json!({
        "setup_id":1,"voxel_size":0.5,"max_voxels":20000,"through_operation_id":999,
        "completed_steps":1,"playback_time_seconds":0.1
    }))
    .unwrap();
    let source = input();
    let mut prepared = prepare(
        &document,
        &source,
        request.clone(),
        &CamSimulationCancellation::default(),
        None,
    )
    .unwrap();
    let simulation = prepared.simulation.as_ref().unwrap();
    assert!(simulation.removed_volume_mm3 > 0.);
    assert!(prepared.message.starts_with("Simulation:"));
    assert!(prepared.details.contains("physical.nc"));
    assert!(prepared.details.contains("source line"));
    assert!(!prepared.paths.is_empty());
    let duration = simulation.estimated_seconds;
    let cutting = simulation
        .steps
        .iter()
        .find(|step| step.source_line == Some(60))
        .unwrap();
    let midpoint = cutting.cumulative_seconds - cutting.duration_seconds / 2.;
    assert_eq!(
        timeline::step_at(simulation, midpoint).unwrap().source_line,
        Some(60)
    );
    let mut expected =
        limo_cad_cam::CamPlayback::from_gcode(document.clone(), source.request(request), 0., None)
            .unwrap();
    let mut player = playback::Player::from_prepared(
        prepared.nc_kernel.take().unwrap().into_inner().unwrap(),
        None,
    )
    .unwrap();
    assert!(prepared.nc_kernel.is_none());
    let mut previous_stock = None;
    for time in [0., midpoint, duration, midpoint, 0.] {
        player.seek(time, duration);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !player.poll(duration).unwrap() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let sampled = expected.sample(time, None).unwrap();
        if sampled.stock_mesh.is_some() {
            previous_stock = crate::retained_cam_stock(&sampled);
        }
        let frame = player.frame.as_ref().unwrap();
        assert_eq!(frame.time, time);
        assert_eq!(
            frame.stock.as_ref().map(|s| &*s.positions),
            previous_stock.as_ref().map(|s| &*s.positions)
        );
    }
    assert_eq!(serde_json::to_value(&document).unwrap(), before);
    let mut unnumbered = source;
    unnumbered.source = unnumbered
        .source
        .lines()
        .map(|line| line.split_once(' ').unwrap().1)
        .collect::<Vec<_>>()
        .join("\n");
    let request: CamSimulationRequestDto =
        serde_json::from_value(json!({"setup_id":1,"voxel_size":0.5,"max_voxels":20000})).unwrap();
    let fallback = prepare(
        &document,
        &unnumbered,
        request,
        &CamSimulationCancellation::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        timeline::step_at(fallback.simulation.as_ref().unwrap(), midpoint)
            .unwrap()
            .source_line,
        Some(6)
    );
}

#[test]
fn nc_errors_and_cancellation_do_not_fall_back_to_generated_cam() {
    let document = super::super::tests::job();
    let request: CamSimulationRequestDto =
        serde_json::from_value(json!({"setup_id":1,"voxel_size":0.5,"max_voxels":20000})).unwrap();
    let cancel = CamSimulationCancellation::default();
    cancel.cancel();
    assert!(prepare(&document, &input(), request.clone(), &cancel, None).is_err());
    let mut invalid = input();
    invalid.source = "G71\n".into();
    assert!(prepare(
        &document,
        &invalid,
        request,
        &CamSimulationCancellation::default(),
        None
    )
    .err()
    .unwrap()
    .contains("controller-specific"));
}

#[test]
fn nc_settings_reprepare_same_source_and_cam_button_explicitly_retires_it() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _, document) = super::super::tests::fixture_world(&fixture);
    let owner = fixture.owner();
    let before = serde_json::to_value(&document).unwrap();
    app.world_mut()
        .insert_resource(super::super::tests::state(&services, &owner, document));
    request_nc(app.world_mut(), input()).unwrap();
    let revision = app
        .world()
        .resource::<State>()
        .key
        .as_ref()
        .unwrap()
        .revision;
    let generation = app.world().resource::<State>().generation;
    app.world_mut().resource_mut::<State>().settings_open = true;
    settings::reduce(
        app.world_mut(),
        &owner,
        revision,
        &Command::Detail,
        &ControlInput::SetValue("fast".into()),
    )
    .unwrap();
    let state = app.world().resource::<State>();
    assert_eq!(state.nc_input.as_ref(), Some(&input()));
    assert!(state.simulation_requested && state.request_pending);
    assert_eq!(state.generation, generation + 1);
    assert!(state.prepared.is_none());
    execute(app.world_mut(), &Command::Cancel).unwrap();
    assert!(app.world().resource::<State>().nc_input.is_none());
    settings::reduce(
        app.world_mut(),
        &owner,
        revision,
        &Command::Detail,
        &ControlInput::SetValue("fine".into()),
    )
    .unwrap();
    let state = app.world().resource::<State>();
    assert!(!state.simulation_requested);
    assert!(state.request_pending);
    assert!(
        state.nc_input.is_none(),
        "Settings after Cancel must request a plain preview, never NC without a request"
    );
    request_nc(app.world_mut(), input()).unwrap();
    execute(app.world_mut(), &Command::Simulate).unwrap();
    let state = app.world().resource::<State>();
    assert!(state.nc_input.is_none());
    assert!(state.simulation_requested && state.request_pending);
    assert_eq!(
        serde_json::to_value(fixture.engine.cam_document_snapshot()).unwrap(),
        before
    );
    assert_eq!(
        services
            .bridge
            .native_document_receipt(&services.engine, &owner)
            .unwrap()
            .revision,
        revision
    );
}

#[test]
fn inactive_workspace_drains_completed_stock_without_restarting_or_publishing() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, camera, document) = super::super::tests::fixture_world(&fixture);
    let owner = fixture.owner();
    let mut view = super::super::tests::state(&services, &owner, document);
    let (send, receiver) = mpsc::channel();
    let cancellation = CamSimulationCancellation::default();
    view.pending = Some(Pending {
        key: view.key.clone().unwrap(),
        generation: view.generation,
        cancellation: cancellation.clone(),
        receiver: Mutex::new(receiver),
    });
    view.request_pending = true;
    view.simulation_requested = true;
    view.nc_input = Some(input());
    app.world_mut().insert_resource(view);
    synchronize(
        app.world_mut(),
        camera,
        &services,
        &owner,
        (1360., 860., 280.),
        false,
    )
    .unwrap();
    assert!(cancellation.is_cancelled());
    assert!(
        app.world().resource::<State>().pending.is_some(),
        "Retain worker bound until completion"
    );
    send.send(Ok(super::super::tests::prepared("Retired NC result")))
        .unwrap();
    synchronize(
        app.world_mut(),
        camera,
        &services,
        &owner,
        (1360., 860., 280.),
        false,
    )
    .unwrap();
    let view = app.world().resource::<State>();
    assert!(view.pending.is_none());
    assert!(view.prepared.is_none() && view.nc_input.is_none());
    assert!(!view.request_pending && !view.simulation_requested);
}
