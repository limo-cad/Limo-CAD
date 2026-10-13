use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use std::{fs, time::Instant};

fn job() -> CamDocumentDto {
    let mut document: CamDocumentDto = serde_json::from_value(json!({
        "setups":[{"id":1,"name":"Face test","stock":{"min":{"x":0.,"y":0.,"z":-4.},"max":{"x":12.,"y":8.,"z":0.}},
            "operations":[{"kind":"face","id":1,"name":"Face top","enabled":true,"tool_id":1,
                "bounds":{"min":{"x":0.,"y":0.},"max":{"x":12.,"y":8.}},
                "top_z":0.,"target_z":-1.,"step_over":1.,"step_down":1.,
                "clearance_z":5.,"retract_z":2.,"feed_height_z":1.,
                "cutting":{"spindle_rpm":8000,"feed_xy":600.,"feed_z":150.,"coolant":"off"}}]}],
        "active_setup_id":1,"tools":[{"id":1,"number":1,"name":"2 mm end mill","kind":"flat_end_mill",
            "diameter":2.,"flute_length":8.,"overall_length":30.,"flute_count":2}],
        "next_setup_id":2,"next_tool_id":2,"next_operation_id":2
    })).unwrap();
    document.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
        Default::default(),
    ));
    document
}
fn setup(fixture: &Fixture, generate: bool) -> (App, NativeServices) {
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cam_set_document",
            &serde_json::to_value(job()).unwrap(),
            || Ok(()),
        )
        .unwrap();
    if generate {
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                "cam_regenerate_setup",
                &json!({"setup_id":1}),
                || Ok(()),
            )
            .unwrap();
    }
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = App::new();
    worker::install(
        app.world_mut(),
        services.clone(),
        NativeInterfaceHandle::new(|| {}),
    )
    .unwrap();
    (app, services)
}
fn state(fixture: &Fixture, kind: Kind) -> State {
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    let mut draft = Draft::new(receipt, &fixture.engine.cam_document_snapshot(), 1, kind).unwrap();
    draft.reviewed_machine = true;
    State {
        draft: Some(draft),
        serial: 7,
        ..default()
    }
}
fn drain(world: &mut World, services: &NativeServices) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(outcome) = worker::poll(world, services) {
            return outcome.value;
        }
        assert!(
            Instant::now() < deadline,
            "Native CAM post worker timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn model(fixture: &Fixture) -> Value {
    parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()
}

#[test]
fn native_cam_post_preserves_exact_shared_output_and_can_return_to_settings() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services) = setup(&fixture, true);
    let before = model(&fixture);
    for kind in [Kind::Nc, Kind::Events] {
        let mut state = state(&fixture, kind);
        let (operation, arguments) = state.draft.as_ref().unwrap().request().unwrap();
        let expected = parse_engine_envelope(
            fixture
                .engine
                .engine_call(&operation, &arguments.to_string()),
        )
        .unwrap();
        prepare(app.world_mut(), &mut state).unwrap();
        app.world_mut().insert_resource(state);
        assert_eq!(drain(app.world_mut(), &services).unwrap()["prepared"], true);
        let mut state = app.world_mut().remove_resource::<State>().unwrap();
        let prepared = state.prepared.clone().unwrap();
        assert!(
            !prepared.warnings.is_empty(),
            "Shared machine/verification findings must be reviewable"
        );
        if kind == Kind::Nc {
            assert_eq!(
                String::from_utf8(prepared.bytes.clone()).unwrap(),
                expected["nc"].as_str().unwrap()
            );
        } else {
            assert_eq!(
                serde_json::from_slice::<Value>(&prepared.bytes).unwrap(),
                expected
            );
        }
        let path = std::env::temp_dir().join(format!(
            "limo-cad-native-post-{}.{}",
            uuid::Uuid::new_v4(),
            prepared.extension
        ));
        io::save(
            app.world_mut(),
            &mut state,
            prepared.clone(),
            path.clone(),
            false,
        )
        .unwrap();
        app.world_mut().insert_resource(state);
        assert_eq!(drain(app.world_mut(), &services).unwrap()["exported"], true);
        assert_eq!(fs::read(&path).unwrap(), prepared.bytes);
        fs::remove_file(path).unwrap();
        assert_eq!(model(&fixture), before);
        let mut state = app.world_mut().remove_resource::<State>().unwrap();
        let draft = state.draft.as_ref().unwrap();
        let settings = (
            draft.program_name.clone(),
            draft.program_number.clone(),
            draft.sequence_numbers,
        );
        let serial = state.serial;
        back_to_settings(&mut state).unwrap();
        assert!(state.prepared.is_none());
        assert_ne!(state.serial, serial);
        let draft = state.draft.as_ref().unwrap();
        assert_eq!(
            (
                draft.program_name.clone(),
                draft.program_number.clone(),
                draft.sequence_numbers
            ),
            settings
        );
        assert!(!draft.reviewed_machine);
        assert!(
            draft.request().is_err(),
            "Returning to settings requires the existing machine review again"
        );
        assert!(
            io::choose(app.world(), &NativeInterfaceHandle::new(|| {}), &mut state).is_err(),
            "Returning to settings must discard the old prepared output"
        );
        assert_eq!(model(&fixture), before);
    }
}

#[test]
fn native_cam_post_cannot_bypass_stale_generation_or_install_cancelled_review() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services) = setup(&fixture, false);
    let before = model(&fixture);
    let mut state = state(&fixture, Kind::Nc);
    prepare(app.world_mut(), &mut state).unwrap();
    app.world_mut().insert_resource(state);
    assert!(drain(app.world_mut(), &services).is_err());
    assert!(app.world().resource::<State>().prepared.is_none());
    assert_eq!(model(&fixture), before);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cam_regenerate_setup",
            &json!({"setup_id":1}),
            || Ok(()),
        )
        .unwrap();
    let mut state = self::state(&fixture, Kind::Nc);
    prepare(app.world_mut(), &mut state).unwrap();
    app.world_mut().insert_resource(state);
    escape(app.world_mut());
    assert_eq!(
        drain(app.world_mut(), &services).unwrap()["discarded"],
        true
    );
    assert!(app.world().resource::<State>().prepared.is_none());
}

#[test]
fn native_cam_prepared_bytes_cannot_overwrite_after_edit_or_owner_replacement() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (_, services) = setup(&fixture, true);
    let draft = state(&fixture, Kind::Nc).draft.unwrap();
    let (operation, arguments) = draft.request().unwrap();
    let result = parse_engine_envelope(
        fixture
            .engine
            .engine_call(&operation, &arguments.to_string()),
    )
    .unwrap();
    let prepared = Prepared::from_result(&draft, result).unwrap();
    let path = std::env::temp_dir().join(format!(
        "limo-cad-native-post-{}.{}",
        uuid::Uuid::new_v4(),
        prepared.extension
    ));
    fs::write(&path, b"existing program").unwrap();
    assert!(io::write_for_test(&services, &prepared, &path, false).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"existing program");
    fixture
        .rename(&fixture.owner(), "Changed during picker")
        .unwrap();
    assert!(io::write_for_test(&services, &prepared, &path, true).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"existing program");
    let before = model(&fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    assert!(io::write_for_test(&services, &prepared, &path, true).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"existing program");
    assert_ne!(model(&fixture), before);
    fs::remove_file(path).unwrap();
}

#[test]
fn native_cam_program_overrides_preserve_machine_specific_settings() {
    let document = job();
    let fixture_receipt = DocumentReceipt {
        owner: DocumentContext {
            window_id: "main".into(),
            document_id: "test".into(),
            epoch: 1,
        },
        revision: 1,
    };
    let mut draft = Draft::new(fixture_receipt, &document, 1, Kind::Nc).unwrap();
    assert!(draft.request().is_err());
    draft.reviewed_machine = true;
    draft.program_number = "1234".into();
    draft.sequence_numbers = true;
    let (_, args) = draft.request().unwrap();
    let request: CamPostRequestDto = serde_json::from_value(args).unwrap();
    let mut expected = document.setups[0]
        .machine
        .as_ref()
        .unwrap()
        .profile
        .post
        .clone();
    expected.program_number = Some(1234);
    expected.sequence_numbers = true;
    assert_eq!(request.post, Some(expected));
    draft.program_number = "1.5".into();
    assert!(draft.request().is_err());
}
