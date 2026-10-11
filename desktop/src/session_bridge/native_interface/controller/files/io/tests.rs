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

#[test]
fn portable_mesh_write_detaches_build_and_requires_current_export_job() {
    use std::cell::{Cell, RefCell};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for format in [Format::Stl, Format::ThreeMf] {
        let fixture = Fixture::new();
        solid(&fixture);
        let solution: limo_cad_sketch::AssemblySolutionDto = serde_json::from_value(
            parse_engine_envelope(fixture.engine.engine_call("assembly_solution", "")).unwrap(),
        )
        .unwrap();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                "assembly_duplicate_occurrence",
                &json!({"occurrence_id":solution.instance_body_poses[0].occurrence_id.0, "local_pose":{"translation":[60.,0.,0.],"rotation":[0.,0.,0.,1.]}}),
                || Ok(()),
            )
            .unwrap();
        let before = model(&fixture);
        let (mut app, services, _) = setup(&fixture);
        refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
        for case in [
            "valid",
            "overwrite",
            "layout",
            "layout-consent",
            "cancel",
            "dialog",
            "picker",
            "owner",
            "model",
        ] {
            if format == Format::Stl && case.starts_with("layout") {
                continue;
            }
            let owner = fixture.owner();
            let receipt = current(app.world(), &services, &owner).unwrap();
            let mut intent = capture(app.world(), &services, &receipt, format, false).unwrap();
            if case.starts_with("layout") {
                Arc::make_mut(&mut intent).print_bed = Some(limo_cad_core::PrintBedDto {
                    name: "10 mm fixture bed".into(),
                    size_mm: [10., 10., 10.],
                    margin_mm: 0.,
                    nozzle_mode: Default::default(),
                    origin_mm: [0., 0.],
                    printable_regions: Vec::new(),
                    excluded_regions: Vec::new(),
                    source: None,
                });
                Arc::make_mut(&mut intent).allow_layout_issues = case == "layout-consent";
            }
            assert!(!intent.bambu.enabled);
            let request = limo_cad_export::MeshExportRequest {
                expected_model_json: Some(
                    engine_text(before.clone(), "Project model not text").unwrap(),
                ),
                body_ids: intent.body_ids.clone(),
                scope: intent.scope,
                slicer_target: intent.slicer_target,
                named_view: intent.named_view.clone(),
                print_bed: intent.print_bed.clone(),
                include_appearance: format == Format::ThreeMf,
                ..default()
            };
            let payload = serde_json::to_string(&request).unwrap();
            let original = if format == Format::Stl {
                fixture.engine.export_stl(&payload)
            } else {
                fixture.engine.export_3mf(&payload)
            }
            .unwrap();
            assert!(!original.is_empty());
            // No options dialog survives portable picker completion. This is a
            // distinct job lease, issued by the same genuine export enqueue path.
            let operation = export_operation::begin_portable(app.world_mut(), &receipt).unwrap();
            let destination = path(&format!(
                "portable-detached-{}-{case}.{}",
                format.extension(),
                format.extension()
            ));
            assert!(!destination.exists());
            if case == "overwrite" {
                std::fs::write(&destination, b"Owned replaceable output").unwrap();
            }
            let admitted = Cell::new(0);
            let detached = RefCell::new(Vec::new());
            let result = portable_write_snapshot(
                &services,
                &receipt,
                &intent,
                &operation,
                (&destination, case == "overwrite"),
                || {
                    assert_eq!(
                        admitted.replace(admitted.get() + 1),
                        0,
                        "Disabled busy control cannot be admitted twice"
                    );
                    Ok(())
                },
                |prepared| {
                    // A real native tessellate + unchanged writer while both live
                    // locks are held proves the snapshot has no live reentry.
                    let finished = services.bridge.with_native_document_receipt(
                        &services.engine,
                        &owner,
                        |_| fixture.engine.with_drawing(|_| prepared.build()),
                    )?;
                    assert_eq!(finished.bytes(), original);
                    if case.starts_with("layout") {
                        assert!(
                            finished.layout_has_issues(),
                            "40 x 25 stock and translated repeat cannot fit a 10 x 10 bed"
                        );
                    }
                    detached.replace(finished.bytes().to_vec());
                    match case {
                        "cancel" => {
                            escape(app.world_mut());
                            assert_eq!(current(app.world(), &services, &owner)?, receipt);
                        }
                        "dialog" => {
                            show_dialog(
                                app.world_mut(),
                                receipt.clone(),
                                DialogKind::Rename("Replacement".into()),
                            )?;
                        }
                        "picker" => {
                            let (_send, receive) = mpsc::channel();
                            app.world_mut()
                                .resource_mut::<Files>()
                                .replace_picker(Some(Picker {
                                    receipt: receipt.clone(),
                                    kind: PickerKind::ImportStep,
                                    result: Mutex::new(receive),
                                }));
                        }
                        "owner" => {
                            services.bridge.apply_native_mutation(
                                &services.engine,
                                &owner,
                                "cad_load_project_model",
                                &json!({"model_json":before}),
                                || Ok(()),
                            )?;
                        }
                        "model" => {
                            fixture.rename(&owner, "Changed during detached build")?;
                        }
                        _ => {}
                    }
                    Ok(finished)
                },
            );
            assert_eq!(admitted.get(), 1);
            if matches!(case, "valid" | "overwrite" | "layout-consent") {
                let result = result.unwrap();
                assert_eq!(result.value["exported"], true);
                assert_eq!(result.value["export_mode"], "portable_model");
                assert_eq!(result.value["requires_reslicing"], false);
                assert_eq!(std::fs::read(&destination).unwrap(), *detached.borrow());
                assert!(
                    operation.commit(|| Ok(())).is_err(),
                    "Consumed job cannot write again"
                );
            } else {
                let error = result.unwrap_err();
                let expected = match case {
                    "cancel" | "dialog" | "picker" => "cancelled",
                    "owner" => "replaced",
                    "model" => "changed",
                    "layout" => "explicitly choose Export despite layout issues",
                    _ => unreachable!(),
                };
                assert!(error.contains(expected), "{format:?}/{case}: {error}");
                assert!(!destination.exists(), "Invalid job must not publish a file");
            }
            if case == "model" {
                assert_ne!(model(&fixture), before);
            } else {
                assert_eq!(model(&fixture), before);
            }
            app.world_mut().resource_mut::<Files>().replace_picker(None);
        }
    }
}

#[test]
fn exact_step_write_detaches_shapes_and_preserves_metadata_and_job_fences() {
    use std::cell::{Cell, RefCell};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid(&fixture);
    let scene = fixture.engine.solid_scene_snapshot();
    let body = &scene.bodies[0];
    let cap = body
        .faces
        .iter()
        .find(|face| face.plane.is_some_and(|plane| plane.normal[2] > 0.9))
        .unwrap();
    let plane = cap.plane.unwrap();
    let delta = [
        20. - plane.origin[0],
        12.5 - plane.origin[1],
        6. - plane.origin[2],
    ];
    let project = |axis: [f64; 3]| delta.iter().zip(axis).map(|(a, b)| a * b).sum::<f64>();
    fixture.bridge.apply_native_mutation(&fixture.engine, &fixture.owner(), "solid_hole", &json!({
        "body_id":body.id.0,"face_id":cap.id.0,"position":{"x":project(plane.u),"y":project(plane.v)},
        "diameter":5.,"extent":{"type":"through_all"},"style":"simple",
        "thread":{"standard":"iso_metric","series":"metric_coarse","designation":"M6 x 1 - 6H","class":"6H",
            "nominal_diameter":6.,"pitch":1.,"representation":"simplified"},"flip":false
    }), || Ok(())).unwrap();
    let solution: limo_cad_sketch::AssemblySolutionDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("assembly_solution", "")).unwrap(),
    )
    .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "assembly_duplicate_occurrence",
            &json!({"occurrence_id":solution.instance_body_poses[0].occurrence_id.0,
            "local_pose":{"translation":[60.,0.,0.],"rotation":[0.,0.,0.,1.]}}),
            || Ok(()),
        )
        .unwrap();
    let before = model(&fixture);
    let (mut app, services, _) = setup(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    for case in ["valid", "overwrite", "cancel", "dialog", "owner", "model"] {
        let owner = fixture.owner();
        let receipt = current(app.world(), &services, &owner).unwrap();
        let intent = capture(app.world(), &services, &receipt, Format::Step, false).unwrap();
        let request =
            step_request(&fixture.engine, &intent, before.as_str().unwrap().into()).unwrap();
        assert_eq!(request.occurrences.len(), 2);
        assert_eq!(request.thread_metadata.len(), 1);
        assert_eq!(request.thread_metadata[0].thread.designation, "M6 x 1 - 6H");
        assert_eq!(request.thread_metadata[0].predrill_diameter, 5.);
        let operation = export_operation::begin_portable(app.world_mut(), &receipt).unwrap();
        let destination = path(&format!("detached-exact-{case}.step"));
        assert!(!destination.exists());
        if case == "overwrite" {
            std::fs::write(&destination, b"Owned replaceable STEP output").unwrap();
        }
        let admitted = Cell::new(0);
        let expected = RefCell::new(Vec::new());
        let outcome = step_write_snapshot(
            &services,
            &receipt,
            &intent,
            &operation,
            (&destination, case == "overwrite"),
            || {
                assert_eq!(
                    admitted.replace(admitted.get() + 1),
                    0,
                    "Busy control must be admitted once"
                );
                Ok(())
            },
            |prepared| {
                let finished = services.bridge.with_native_document_receipt(
                    &services.engine,
                    &owner,
                    |_| fixture.engine.with_drawing(|_| prepared.build()),
                )?;
                let text = std::str::from_utf8(finished.bytes()).unwrap();
                assert!(text.starts_with("ISO-10303-21;"));
                assert!(text.to_ascii_uppercase().contains("AP242"));
                assert!(text.ends_with("END-ISO-10303-21;\n"));
                let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
                assert!(compact.contains("LIMO_CAD_THREAD_METADATA_V1_HEX="));
                assert!(
                    compact.contains("4d3620782031202d203648"),
                    "M6 callout remains in namespaced STEP metadata"
                );
                if case == "valid" {
                    // Independent real STEP importer checks physical millimetres and
                    // both exact placed copies, rather than comparing timestamped text.
                    let mut importer = limo_cad_occt::OcctKernel::new().unwrap();
                    let imported = importer
                        .recompute(&limo_cad_solid::RecomputePlanDto {
                            transaction_id: 1,
                            errors: Vec::new(),
                            jobs: vec![limo_cad_solid::KernelJobDto::ImportStep(
                                limo_cad_solid::KernelImportStepJobDto {
                                    feature_id: limo_cad_core::FeatureId(1),
                                    result_body_id: BodyId(1),
                                    data_base64: STANDARD.encode(finished.bytes()),
                                },
                            )],
                        })
                        .unwrap();
                    assert!(imported.errors.is_empty());
                    assert!(!imported.bodies.is_empty());
                    let mut min = [f32::INFINITY; 3];
                    let mut max = [f32::NEG_INFINITY; 3];
                    for body in &imported.bodies {
                        for point in body.positions.as_chunks::<3>().0 {
                            for axis in 0..3 {
                                min[axis] = min[axis].min(point[axis]);
                                max[axis] = max[axis].max(point[axis]);
                            }
                        }
                    }
                    for coordinate in min {
                        assert!(coordinate.abs() < 1e-3);
                    }
                    for (actual, expected) in max.into_iter().zip([100., 25., 6.]) {
                        assert!((actual - expected).abs() < 1e-3);
                    }
                }
                expected.replace(finished.bytes().to_vec());
                match case {
                    "cancel" => {
                        escape(app.world_mut());
                        assert_eq!(current(app.world(), &services, &owner)?, receipt);
                    }
                    "dialog" => {
                        show_dialog(
                            app.world_mut(),
                            receipt.clone(),
                            DialogKind::Rename("Replacement".into()),
                        )?;
                    }
                    "owner" => {
                        services.bridge.apply_native_mutation(
                            &services.engine,
                            &owner,
                            "cad_load_project_model",
                            &json!({"model_json":before}),
                            || Ok(()),
                        )?;
                    }
                    "model" => {
                        fixture.rename(&owner, "Changed during exact STEP build")?;
                    }
                    _ => {}
                }
                Ok(finished)
            },
        );
        assert_eq!(admitted.get(), 1);
        if matches!(case, "valid" | "overwrite") {
            assert_eq!(outcome.unwrap().value["exported"], true);
            assert_eq!(std::fs::read(&destination).unwrap(), *expected.borrow());
            assert!(operation.commit(|| Ok(())).is_err());
        } else {
            let error = outcome.unwrap_err();
            let expected = match case {
                "cancel" | "dialog" => "cancelled",
                "owner" => "replaced",
                "model" => "changed",
                _ => unreachable!(),
            };
            assert!(error.contains(expected), "{case}: {error}");
            assert!(!destination.exists());
        }
        if case == "model" {
            assert_ne!(model(&fixture), before);
        } else {
            assert_eq!(model(&fixture), before);
        }
    }
}
