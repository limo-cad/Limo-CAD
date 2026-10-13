use super::super::tests::{drain, path, setup};
use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

fn seed(fixture: &Fixture) {
    for (operation, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":-20.,"y":10.},"p2":{"x":40.,"y":50.},"ctrl_held":true}),
        ),
        (
            "sketch_add_circle",
            json!({"mode":"center_diameter","p1":{"x":10.,"y":30.},"p2":{"x":14.,"y":30.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &fixture.owner(), operation, &args, || {
                Ok(())
            })
            .unwrap();
    }
}
fn model(fixture: &Fixture) -> Value {
    parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()
}

#[test]
fn profile_file_uses_actual_catalog_and_preserves_document_history_and_destination() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    seed(&fixture);
    let (mut app, services, handle) = setup(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let project = path("profile-output-project.limo");
    request(
        app.world_mut(),
        &handle,
        &services,
        &fixture.owner(),
        &json!({"command":"save","path":project}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let before = model(&fixture);
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let selection = capture(&services, &receipt).unwrap();
    assert_eq!(
        selection.choices.len(),
        1,
        "Hole wires must not appear as selectable material regions"
    );
    let intent = selection.choices[0].clone();
    let catalog = catalog(&services).unwrap();
    let expected =
        limo_cad_export::profile_dxf::write_profile_dxf(&catalog[0], intent.profile_index).unwrap();
    assert!(expected.contains("CIRCLE"));
    assert!(expected.contains("PROFILE_HOLES"));
    let destination = path("manufacturing-profile.dxf");
    let command = json!({"command":"export_profile_dxf","feature_id":intent.feature_id,"profile_index":intent.profile_index,"path":destination});
    request(
        app.world_mut(),
        &handle,
        &services,
        &fixture.owner(),
        &command,
    )
    .unwrap();
    let result = drain(app.world_mut(), &services).unwrap();
    assert_eq!(result["units"], "mm");
    assert_eq!(result["scale"], 1);
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), expected);
    assert_eq!(
        current(app.world(), &services, &fixture.owner()).unwrap(),
        receipt
    );
    assert_eq!(model(&fixture), before);
    let tab = tabs(app.world(), &services, &fixture.owner())
        .unwrap()
        .remove(0);
    assert_eq!(tab.path, Some(project));
    assert!(!tab.dirty);
    request(
        app.world_mut(),
        &handle,
        &services,
        &fixture.owner(),
        &command,
    )
    .unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert_eq!(std::fs::read_to_string(&destination).unwrap(), expected);
    for command in [
        json!({"command":"export_profile_dxf","feature_id":999,"profile_index":0,"path":destination}),
        json!({"command":"export_profile_dxf","feature_id":intent.feature_id,"profile_index":999,"path":destination}),
        json!({"command":"export_profile_dxf","feature_id":intent.feature_id,"profile_index":0,"path":"relative.dxf"}),
    ] {
        assert!(request(
            app.world_mut(),
            &handle,
            &services,
            &fixture.owner(),
            &command
        )
        .is_err());
    }
    assert_eq!(std::fs::read_to_string(destination).unwrap(), expected);
}

#[test]
fn profile_cancel_stale_revision_and_replaced_owner_preserve_output() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _) = setup(&fixture);
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    assert!(capture(&services, &receipt).is_err());
    seed(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let intent = capture(&services, &receipt).unwrap().choices.remove(0);
    let before = model(&fixture);
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<Files>().picker = Some(Picker {
        receipt: receipt.clone(),
        kind: PickerKind::Profile(intent.clone()),
        result: Mutex::new(receive),
    });
    send.send(None).unwrap();
    poll(app.world_mut(), &services).unwrap();
    assert!(!awaiting(app.world()));
    assert!(!worker::busy(app.world()));
    assert_eq!(model(&fixture), before);
    let destination = path("profile-previous.dxf");
    std::fs::write(&destination, b"Reviewed previous output").unwrap();
    fixture
        .rename(&fixture.owner(), "Changed while choosing")
        .unwrap();
    export(
        app.world_mut(),
        receipt.clone(),
        intent.clone(),
        destination.clone(),
        true,
    )
    .unwrap();
    assert!(drain(app.world_mut(), &services)
        .unwrap_err()
        .contains("document changed"));
    let mut replaced = receipt;
    replaced.owner.epoch += 1;
    export(app.world_mut(), replaced, intent, destination.clone(), true).unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert_eq!(
        std::fs::read(destination).unwrap(),
        b"Reviewed previous output"
    );
}

#[test]
fn profile_choice_keyboard_and_invalid_values_never_change_identity_or_model() {
    let mut selection = Selection {
        selected: 0,
        choices: vec![
            ExportIntent {
                feature_id: 4,
                profile_index: 0,
                sketch_name: "First".into(),
            },
            ExportIntent {
                feature_id: 5,
                profile_index: 2,
                sketch_name: "Second".into(),
            },
        ],
    };
    selection
        .select(&ControlInput::SetValue("5:2".into()))
        .unwrap();
    assert_eq!(selection.selected, 1);
    assert!(selection
        .select(&ControlInput::SetValue("4:1".into()))
        .is_err());
    assert_eq!(selection.selected, 1);
    selection
        .select(&ControlInput::SetValue("4:0".into()))
        .unwrap();
    assert_eq!(selection.selected, 0);
    for (key, expected) in [
        ("End", 1),
        ("ArrowDown", 1),
        ("ArrowUp", 0),
        ("ArrowLeft", 0),
        ("ArrowRight", 1),
        ("Home", 0),
    ] {
        selection
            .select(&ControlInput::Key(limo_cad_interface::KeyChord::plain(key)))
            .unwrap();
        assert_eq!(selection.selected, expected);
    }
}
