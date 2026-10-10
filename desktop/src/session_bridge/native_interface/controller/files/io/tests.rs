use super::super::tests::{drain, path, setup};
use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

fn solid(fixture: &Fixture) {
    for (operation, arguments) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
        ),
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
}
fn model(fixture: &Fixture) -> Value {
    parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()
}

#[test]
fn layout_warning_requires_deliberate_export_and_does_not_leak_into_definition_scope() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid(&fixture);
    let (mut app, services, _) = setup(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let assembly: limo_cad_sketch::AssemblyDocumentDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("assembly_document", "")).unwrap(),
    )
    .unwrap();
    let bodies: Vec<_> = fixture
        .engine
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .map(|b| b.id.0)
        .collect();
    fixture.bridge.apply_native_mutation(&fixture.engine, &fixture.owner(), "upsert_named_view", &json!({
        "name":"Below bed", "camera":{"position":[100.,-100.,100.],"target":[0.,0.,0.],"up":[0.,0.,1.]},
        "visible_body_ids":bodies,"occurrence_offsets":[{"occurrence_id":assembly.component_structure.occurrences[0].id,
        "translation":[0.,0.,-10.],"rotation":[0.,0.,0.,1.]}],"print_layout":false
    }), || Ok(())).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "recall_named_view",
            &json!({"name":"Below bed"}),
            || Ok(()),
        )
        .unwrap();
    refresh_native_model(&fixture.engine, app.world_mut(), false).unwrap();
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let mut intent = capture(app.world(), &services, &receipt, Format::ThreeMf, false).unwrap();
    let report = fresh_layout_report(&fixture.engine, &intent).unwrap();
    Arc::make_mut(&mut intent).layout_report = report;
    assert!(view_choices(&fixture.engine)
        .unwrap()
        .iter()
        .any(|choice| choice.value == "saved:Below bed"));
    assert!(layout_has_issues(&intent));
    let destination = path("deliberate-warning.3mf");
    assert!(export(
        app.world_mut(),
        receipt.clone(),
        intent.clone(),
        destination.clone(),
        false
    )
    .unwrap_err()
    .contains("Export despite layout issues"));
    assert!(!destination.exists());
    assert!(!worker::busy(app.world()));
    Arc::make_mut(&mut intent).scope = MeshExportScope::Definition;
    assert!(check_layout_confirmation(&intent).is_ok());
    Arc::make_mut(&mut intent).scope = MeshExportScope::Assembly;
    Arc::make_mut(&mut intent).allow_layout_issues = true;
    export(app.world_mut(), receipt, intent, destination.clone(), false).unwrap();
    assert_eq!(drain(app.world_mut(), &services).unwrap()["exported"], true);
    assert!(std::fs::read(destination).unwrap().starts_with(b"PK"));
}

#[test]
fn exchange_exports_and_embedded_step_import_preserve_project_destination_and_undo() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid(&fixture);
    let (mut app, services, handle) = setup(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let project = path("exchange-project.limo");
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
    for format in [Format::Step, Format::ThreeMf, Format::Stl] {
        let mut intent = capture(app.world(), &services, &receipt, format, false).unwrap();
        Arc::make_mut(&mut intent).scope = MeshExportScope::Definition;
        let destination = path(&format!("exchange.{}", format.extension()));
        export(
            app.world_mut(),
            receipt.clone(),
            intent,
            destination.clone(),
            false,
        )
        .unwrap();
        assert_eq!(drain(app.world_mut(), &services).unwrap()["exported"], true);
        let bytes = std::fs::read(destination).unwrap();
        match format {
            Format::Step => assert!(bytes.starts_with(b"ISO-10303-21")),
            Format::ThreeMf => assert!(bytes.starts_with(b"PK")),
            Format::Stl => assert!(bytes.len() > 84),
        }
        assert_eq!(model(&fixture), before);
        assert_eq!(
            current(app.world(), &services, &fixture.owner()).unwrap(),
            receipt
        );
        let tab = tabs(app.world(), &services, &fixture.owner())
            .unwrap()
            .remove(0);
        assert_eq!(tab.path, Some(project.clone()));
        assert!(!tab.dirty);
    }
    import(app.world_mut(), receipt, path("exchange.step")).unwrap();
    drain(app.world_mut(), &services).unwrap();
    let imported = model(&fixture);
    assert_ne!(imported, before);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 2);
    let definitions: Value =
        parse_engine_envelope(fixture.engine.engine_call("body_feature_definitions", "")).unwrap();
    let source = definitions
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["type"] == "import_step")
        .unwrap();
    assert_eq!(source["file_name"], "exchange.step");
    assert_eq!(
        STANDARD
            .decode(source["data_base64"].as_str().unwrap())
            .unwrap(),
        std::fs::read(path("exchange.step")).unwrap()
    );
    assert_eq!(
        native_viewport::interface_view_snapshot(app.world())
            .2
            .selected_body_ids,
        vec![2]
    );
    let tab = tabs(app.world(), &services, &fixture.owner())
        .unwrap()
        .remove(0);
    assert_eq!(tab.path, Some(project));
    assert!(tab.dirty);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
        .unwrap();
    let restored: Value = serde_json::from_str(model(&fixture).as_str().unwrap()).unwrap();
    let imported_model: Value = serde_json::from_str(imported.as_str().unwrap()).unwrap();
    let mut expected: Value = serde_json::from_str(before.as_str().unwrap()).unwrap();
    let floor_path = "/assembly/component_structure/next_occurrence_id";
    *expected.pointer_mut(floor_path).unwrap() =
        imported_model.pointer(floor_path).unwrap().clone();
    assert_eq!(
        restored, expected,
        "Undo restores the source exactly while imported occurrence IDs stay reserved"
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(model(&fixture), imported);
}

#[test]
fn exchange_receipts_reject_changes_and_picker_cancellation_never_writes() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid(&fixture);
    let (mut app, services, _) = setup(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let intent = capture(app.world(), &services, &receipt, Format::Step, false).unwrap();
    for kind in [PickerKind::ImportStep, PickerKind::Export(intent.clone())] {
        let (send, receive) = mpsc::channel();
        app.world_mut().resource_mut::<Files>().picker = Some(Picker {
            receipt: receipt.clone(),
            kind,
            result: Mutex::new(receive),
        });
        send.send(None).unwrap();
        poll(app.world_mut(), &services).unwrap();
        assert!(!awaiting(app.world()));
        assert!(!worker::busy(app.world()));
    }
    fixture.rename(&fixture.owner(), "Newer model").unwrap();
    let current_model = model(&fixture);
    let destination = path("obsolete.step");
    export(
        app.world_mut(),
        receipt.clone(),
        intent,
        destination.clone(),
        false,
    )
    .unwrap();
    assert!(drain(app.world_mut(), &services)
        .unwrap_err()
        .contains("document changed"));
    assert!(!destination.exists());
    import(app.world_mut(), receipt, destination).unwrap();
    assert!(drain(app.world_mut(), &services)
        .unwrap_err()
        .contains("document changed"));
    assert_eq!(model(&fixture), current_model);
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    assert!(import(
        app.world_mut(),
        receipt.clone(),
        PathBuf::from("relative.step")
    )
    .is_err());
    let intent = capture(app.world(), &services, &receipt, Format::Step, false).unwrap();
    let existing = path("existing.step");
    std::fs::write(&existing, b"Keep these bytes").unwrap();
    export(
        app.world_mut(),
        receipt.clone(),
        intent,
        existing.clone(),
        false,
    )
    .unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert_eq!(std::fs::read(existing).unwrap(), b"Keep these bytes");
    let invalid = path("invalid.step");
    std::fs::write(&invalid, b"not a STEP file").unwrap();
    import(app.world_mut(), receipt, invalid).unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert_eq!(model(&fixture), current_model);
}

#[test]
fn selected_step_retains_occurrence_and_mesh_scope_retains_repeats() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid(&fixture);
    let solution: limo_cad_sketch::AssemblySolutionDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("assembly_solution", "")).unwrap(),
    )
    .unwrap();
    let occurrence = solution.instance_body_poses[0].occurrence_id.0;
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "assembly_duplicate_occurrence",
            &json!({"occurrence_id":occurrence}),
            || Ok(()),
        )
        .unwrap();
    let (mut app, services, _) = setup(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let (_, _, mut view, _) = native_viewport::interface_view_snapshot(app.world());
    view.selected_body_ids = vec![1];
    view.selected_occurrence_id = Some(occurrence);
    native_viewport::apply_interface_view(
        app.world_mut(),
        &fixture.owner().document_id,
        None,
        Some(view),
    )
    .unwrap();
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let selected = capture(app.world(), &services, &receipt, Format::Step, true).unwrap();
    let all = capture(app.world(), &services, &receipt, Format::Step, false).unwrap();
    assert_eq!(
        step_request(
            &fixture.engine,
            &selected,
            model(&fixture).as_str().unwrap().into()
        )
        .unwrap()
        .occurrences
        .iter()
        .map(|p| p.occurrence_id)
        .collect::<Vec<_>>(),
        vec![occurrence]
    );
    assert_eq!(
        step_request(
            &fixture.engine,
            &all,
            model(&fixture).as_str().unwrap().into()
        )
        .unwrap()
        .occurrences
        .len(),
        2
    );
    let mut lengths = Vec::new();
    for scope in [MeshExportScope::Definition, MeshExportScope::Assembly] {
        let mut intent = capture(app.world(), &services, &receipt, Format::Stl, true).unwrap();
        Arc::make_mut(&mut intent).scope = scope;
        let file = path(if scope == MeshExportScope::Assembly {
            "placed.stl"
        } else {
            "definition.stl"
        });
        export(
            app.world_mut(),
            receipt.clone(),
            intent,
            file.clone(),
            false,
        )
        .unwrap();
        drain(app.world_mut(), &services).unwrap();
        lengths.push(std::fs::metadata(file).unwrap().len() - 84);
    }
    assert_eq!(lengths[1], 2 * lengths[0]);
}
