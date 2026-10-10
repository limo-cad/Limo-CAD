use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

#[test]
fn display_units_do_not_change_physical_tolerance_or_simulation_density() {
    let original = Settings::default();
    assert_eq!(
        original
            .edited(
                &Command::Detail,
                &ControlInput::Key(KeyChord::plain("End")),
                CamUnits::Millimeters
            )
            .unwrap()
            .detail,
        Detail::Fast
    );
    assert_eq!(
        original
            .edited(
                &Command::Detail,
                &ControlInput::Click,
                CamUnits::Millimeters
            )
            .unwrap()
            .detail,
        Detail::Fine
    );
    let metric = original
        .edited(
            &Command::Tolerance,
            &ControlInput::SetValue("0.254".into()),
            CamUnits::Millimeters,
        )
        .unwrap();
    let inch = original
        .edited(
            &Command::Tolerance,
            &ControlInput::SetValue("0.01".into()),
            CamUnits::Inches,
        )
        .unwrap();
    assert!((metric.tolerance_mm - inch.tolerance_mm).abs() < 1e-12);
    for invalid in ["-1", "NaN", "inf", "1e999", "text"] {
        assert!(original
            .edited(
                &Command::Tolerance,
                &ControlInput::SetValue(invalid.into()),
                CamUnits::Millimeters
            )
            .is_err());
    }
    let document = super::super::tests::job();
    let setup = document.setup(1).unwrap();
    for (detail, samples, budget) in [
        (Detail::Auto, 352., 8_000_000),
        (Detail::Fine, 512., 8_000_000),
        (Detail::Balanced, 256., 4_000_000),
        (Detail::Fast, 128., 1_000_000),
    ] {
        let settings = Settings { detail, ..metric };
        let mut request: CamSimulationRequestDto =
            serde_json::from_value(json!({"setup_id":1,"target":{"meshes":[],"tolerance_mm":0.1}}))
                .unwrap();
        settings.apply(setup, &mut request);
        assert_eq!(request.voxel_size, Some(12. / samples));
        assert_eq!(request.max_voxels, Some(budget));
        assert_eq!(request.target.unwrap().tolerance_mm, metric.tolerance_mm);
    }
}

#[test]
fn preference_change_cancels_stale_work_without_mutating_the_cam_document() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, camera, document) = super::super::tests::fixture_world(&fixture);
    let before = serde_json::to_value(&document).unwrap();
    let owner = fixture.owner();
    let mut view = super::super::tests::state(&services, &owner, document);
    view.settings_open = true;
    view.simulation_requested = true;
    view.prepared = Some(super::super::tests::prepared("Old tolerance result"));
    let revision = view.key.as_ref().unwrap().revision;
    let generation = view.generation;
    let (send, receiver) = mpsc::channel();
    let cancellation = CamSimulationCancellation::default();
    view.pending = Some(Pending {
        key: view.key.clone().unwrap(),
        generation,
        cancellation: cancellation.clone(),
        receiver: Mutex::new(receiver),
    });
    app.world_mut().insert_resource(view);
    assert!(reduce(
        app.world_mut(),
        &owner,
        revision + 1,
        &Command::Detail,
        &ControlInput::SetValue("fast".into())
    )
    .is_err());
    assert!(!cancellation.is_cancelled());
    reduce(
        app.world_mut(),
        &owner,
        revision,
        &Command::Detail,
        &ControlInput::SetValue("fast".into()),
    )
    .unwrap();
    assert!(cancellation.is_cancelled());
    let state = app.world().resource::<State>();
    assert_eq!(state.generation, generation + 1);
    assert!(state.prepared.is_none());
    assert!(state.request_pending);
    send.send(Ok(super::super::tests::prepared("Stale result")))
        .unwrap();
    app.world_mut().resource_mut::<State>().request_pending = false;
    synchronize(
        app.world_mut(),
        camera,
        &services,
        &owner,
        (1360., 860., 280.),
        true,
    )
    .unwrap();
    assert!(app.world().resource::<State>().prepared.is_none());
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
    escape(app.world_mut());
    assert!(modal(app.world()).is_none());
    assert!(reduce(
        app.world_mut(),
        &owner,
        revision,
        &Command::Detail,
        &ControlInput::SetValue("auto".into())
    )
    .is_err());
}
