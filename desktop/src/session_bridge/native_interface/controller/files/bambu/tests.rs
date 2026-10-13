use super::*;

#[test]
fn native_bambu_preview_runs_detached_and_rejects_changed_or_replaced_receipts() {
    use super::super::tests::setup;
    use crate::session_bridge::native_interface::tests::Fixture;
    use std::cell::Cell;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mutate = |operation, arguments| {
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                operation,
                &arguments,
                || Ok(()),
            )
            .unwrap()
    };
    for (x, name) in [(0., "Sketch1"), (60., "Sketch2")] {
        mutate(
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        );
        mutate(
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+40.,"y":25.},"ctrl_held":true}),
        );
        mutate("sketch_finish", json!({}));
        mutate(
            "solid_extrude",
            json!({"sketch_name":name,"profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
        );
    }
    let model =
        || parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    // Obtain the real persistent source namespace through normal metadata edit.
    mutate(
        "print_intent_set_document",
        json!({"document":PrintIntentDocumentDto::default(),"expected_model_json":model()}),
    );
    let document: PrintIntentDocumentDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("print_intent_get", "")).unwrap(),
    )
    .unwrap();
    let before = model();
    let make_request = |body_ids| BambuExportRequest {
        export: MeshExportRequest {
            expected_model_json: Some(before.as_str().unwrap().into()),
            body_ids,
            ..default()
        },
        project: BambuProjectRequest {
            source_document_id: document.source_document_id.clone().unwrap(),
            ..default()
        },
        // Preserve an authentic generic archive's refusal, not a fabricated
        // native profile or a claim of successful Bambu compatibility.
        template_base64: STANDARD.encode(include_bytes!(
            "../../../../../../../crates/export/fixtures/smoke/cube_bambu_studio.3mf"
        )),
    };
    let (app, services, _) = setup(&fixture);
    let owner = fixture.owner();
    let receipt = current(app.world(), &services, &owner).unwrap();
    let expected = parse_engine_envelope(fixture.engine.engine_call(
        "bambu_project_preview",
        &serde_json::to_string(&make_request(vec![])).unwrap(),
    ))
    .unwrap_err();
    let validations = Cell::new(0);
    let error = preview_snapshot(
        &services,
        &receipt,
        make_request(vec![]),
        || {
            validations.set(validations.get() + 1);
            Ok(())
        },
        |prepared| {
            // Both real guards are held during native export meshing and ZIP work.
            // The owned preview must not reenter either publisher or live engine.
            services
                .bridge
                .with_native_document_receipt(&services.engine, &owner, |_| {
                    fixture.engine.with_drawing(|_| prepared.preview())
                })
        },
    )
    .unwrap_err();
    assert_eq!(error, expected);
    assert_eq!(validations.get(), 1);
    assert_eq!(model(), before);
    assert_eq!(current(app.world(), &services, &owner).unwrap(), receipt);
    let ids: Vec<_> = fixture
        .engine
        .solid_scene_snapshot()
        .bodies
        .iter()
        .map(|body| body.id)
        .collect();
    assert_eq!(ids.len(), 2);
    let duplicate = vec![ids[1], ids[0], ids[0]];
    assert_eq!(
        fixture
            .engine
            .prepare_bambu_preview(make_request(duplicate))
            .unwrap()
            .preview()
            .unwrap_err(),
        expected
    );
    let error = preview_snapshot(
        &services,
        &receipt,
        make_request(vec![ids[0]]),
        || Ok(()),
        |prepared| {
            let outcome = prepared.preview();
            fixture
                .rename(&owner, "Changed during detached preview")
                .unwrap();
            outcome
        },
    )
    .unwrap_err();
    assert!(
        error.contains("document changed during Bambu preview"),
        "{error}"
    );
    preview_snapshot(
        &services,
        &receipt,
        make_request(vec![]),
        || Ok(()),
        |_| panic!("Stale receipt must reject before meshing"),
    )
    .unwrap_err();
    let receipt = current(app.world(), &services, &owner).unwrap();
    let changed = model();
    let mut request = make_request(vec![]);
    request.export.expected_model_json = Some(changed.as_str().unwrap().into());
    let error = preview_snapshot(
        &services,
        &receipt,
        request,
        || Ok(()),
        |prepared| {
            services
                .bridge
                .apply_native_mutation(
                    &services.engine,
                    &owner,
                    "cad_load_project_model",
                    &json!({"model_json":changed}),
                    || Ok(()),
                )
                .unwrap();
            // The original independent native shapes survive actual replacement.
            assert_eq!(prepared.preview().unwrap_err(), expected);
            Err("Original preview refusal".into())
        },
    )
    .unwrap_err();
    assert!(error.contains("replaced"), "{error}");
    assert_ne!(fixture.owner().epoch, owner.epoch);
    assert_eq!(model(), changed);
}

#[test]
fn scrolling_export_options_edits_the_owned_state_without_copying_the_model() {
    use super::super::tests::setup;
    use crate::session_bridge::native_interface::tests::Fixture;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    let receipt = current(app.world(), &services, &owner).unwrap();
    let bodies = refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    app.insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision: receipt.revision,
        bodies,
    });
    let mut intent = intent();
    intent.bambu.model = "model".repeat(200_000);
    let intent = Arc::new(intent);
    let pointer = Arc::as_ptr(&intent);
    let generation = intent.bambu.generation;
    app.world_mut().resource_mut::<Files>().dialog = Some(Dialog {
        token: 17,
        receipt,
        kind: DialogKind::Export(intent),
        error: None,
    });
    reduce(
        app.world_mut(),
        &handle,
        (&services, &owner),
        17,
        generation,
        Command::Scroll(1),
        &ControlInput::Click,
    )
    .unwrap();
    let dialog = app.world().resource::<Files>().dialog.as_ref().unwrap();
    let DialogKind::Export(intent) = &dialog.kind else {
        unreachable!()
    };
    assert_eq!(Arc::as_ptr(intent), pointer);
    assert_eq!(intent.bambu.scroll, 1);
    let retained = Arc::clone(intent);
    use bevy::ecs::change_detection::DetectChanges;
    let tick = app
        .world()
        .get_resource_ref::<Files>()
        .unwrap()
        .last_changed();
    for command in [Command::Info, Command::Field(Field::TemplatePath)] {
        reduce(
            app.world_mut(),
            &handle,
            (&services, &owner),
            17,
            generation + 1,
            command,
            &ControlInput::Click,
        )
        .unwrap();
        let dialog = app.world().resource::<Files>().dialog.as_ref().unwrap();
        let DialogKind::Export(intent) = &dialog.kind else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(intent, &retained));
        assert_eq!(
            app.world()
                .get_resource_ref::<Files>()
                .unwrap()
                .last_changed(),
            tick
        );
    }
}

#[test]
fn closing_options_does_not_suppress_committed_profile_publication_or_undo() {
    use super::super::tests::{drain, setup};
    use crate::session_bridge::native_interface::tests::Fixture;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _) = setup(&fixture);
    let owner = fixture.owner();
    let receipt = current(app.world(), &services, &owner).unwrap();
    let bodies = refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    app.insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision: receipt.revision,
        bodies,
    });
    let mut intent = intent();
    intent.bambu.model =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", ""))
            .unwrap()
            .as_str()
            .unwrap()
            .into();
    intent.bambu.document = Some(PrintIntentDocumentDto::default());
    intent
        .bambu
        .template
        .as_mut()
        .unwrap()
        .summary
        .process_defaults
        .wall_count = Some(2);
    app.world_mut().resource_mut::<Files>().dialog = Some(Dialog {
        token: 17,
        receipt: receipt.clone(),
        kind: DialogKind::Export(Arc::new(intent)),
        error: None,
    });
    metadata(
        app.world_mut(),
        &services,
        &owner,
        17,
        Command::ApplyProfile,
    )
    .unwrap();
    app.world_mut().resource_mut::<Files>().dialog = None;
    drain(app.world_mut(), &services).unwrap();
    let document =
        parse_engine_envelope(fixture.engine.engine_call("print_intent_get", "")).unwrap();
    assert_eq!(document["selected_process"]["defaults"]["wall_count"], 2);
    let revision = services
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    assert_eq!(
        app.world().resource::<NativeRenderedDocument>().revision,
        revision,
        "Closing optional export controls must not leave the owning presentation on an obsolete revision"
    );
    services
        .bridge
        .apply_native_history(&services.engine, &owner, false, || Ok(()))
        .unwrap();
    let undone = parse_engine_envelope(fixture.engine.engine_call("print_intent_get", "")).unwrap();
    assert!(undone["selected_process"].is_null());
    assert_eq!(
        undone["source_document_id"], document["source_document_id"],
        "Explicit defaults are undoable while the assigned source identity remains stable"
    );
}

pub(super) fn intent() -> io::ExportIntent {
    let summary: BambuTemplateSummary=serde_json::from_value(json!({
        "template_sha256":"a".repeat(64),"version":"1","printer_settings_id":"X2D",
        "printer_model":"Bambu Lab X2D","printer_variant":"0.4","process_settings_id":"fixture",
        "process_defaults":{},"nozzle_diameter_mm":[0.4],"filament_settings_ids":["PETG"],
        "filament_types":["PETG"],"filament_colors":["#000000"],"support_filament":0,
        "support_interface_filament":0,"filament_map":[],"filament_nozzle_map":[],"plate_count":4,
        "objects":[{"object_id":2,"object_ordinal":0,"name":"Same name","instance_count":2,
        "parts":[{"part_id":1,"name":"Same name","mesh_path":"3D/1.model","subtype":"normal_part","settings":{}}],"settings":{}}],"has_identity_manifest":false
    })).unwrap();
    let document = PrintIntentDocumentDto {
        source_document_id: Some("83117445-4c07-4f27-bcbb-81077efce39c".into()),
        ..Default::default()
    };
    io::ExportIntent {
        format: io::Format::ThreeMf,
        scope: limo_cad_export::MeshExportScope::Assembly,
        slicer_target: Default::default(),
        named_view: None,
        print_bed: None,
        layout_report: None,
        allow_layout_issues: false,
        body_ids: vec![limo_cad_core::BodyId(1)],
        occurrence_id: None,
        selected: false,
        bambu: Settings {
            enabled: true,
            model: "owned-model".into(),
            document: Some(document),
            template: Some(Template {
                path: PathBuf::from("D:/input.3mf"),
                encoded: Arc::new("c25hcHNob3Q=".into()),
                summary,
            }),
            ..default()
        },
    }
}

#[test]
fn accepted_preview_from_reordered_manual_bindings_keeps_exact_write_review() {
    for reset in [false, true] {
        let mut intent = intent();
        intent.bambu.start_reviewed_baseline = reset;
        intent.bambu.bindings = [1, 0]
            .into_iter()
            .map(|instance_id| BambuPartBinding {
                body_id: limo_cad_core::BodyId(1),
                occurrence_id: if instance_id == 0 { 11 } else { 21 },
                object_id: 2,
                instance_id,
                part_id: 1,
            })
            .collect();
        let manually_ordered = intent.bambu.bindings.clone();
        let preview_request = request(&intent).unwrap();
        assert_ne!(preview_request.project.bindings, manually_ordered);
        let key = review_key(&preview_request, &intent.bambu.template().unwrap().summary);
        let parts: Vec<_> = preview_request.project.bindings.iter().map(|binding| json!({
            "binding":binding,"target_uuid":"reviewed-volume","geometry_sha256":"a".repeat(64),
            "triangle_count":12,"filament_index":1,"filament_type":"PETG","filament_color":"#000000",
            "world_transform":[1.,0.,0.,0.,1.,0.,0.,0.,1.,0.,0.,0.],"plate_index":1,
            "effective_sources":{},"inherited_settings":{},"written_overrides":{},"effective_settings":{}
        })).collect();
        let report: BambuProjectReport = serde_json::from_value(json!({
            "template":intent.bambu.template().unwrap().summary,
            "source_document_id":preview_request.project.source_document_id,
            "output_sha256":"b".repeat(64),"placement":"template","parts":parts,"modifiers":[],
            "invalidated_entries":[],"warnings":[],"metadata_readback_verified":true,
            "installed_slicer_imported":false,"toolpaths_generated":false,
            "refresh_reference":{
                "version":1,"source_document_id":preview_request.project.source_document_id,
                "original_template_sha256":"a".repeat(64),"profile_sha256":"b".repeat(64),
                "profile_identity_sha256":"c".repeat(64),"baseline_project_settings":{},
                "written_project_settings":{},"parts":[],"modifiers":[],"height_objects":[]
            }
        }))
        .unwrap();
        let generation = intent.bambu.generation;
        // Exercise the exact production post-worker adoption and write gate.
        accept_preview(&mut intent, key, report);
        assert_eq!(intent.bambu.generation, generation + 1);
        assert_eq!(intent.bambu.bindings, preview_request.project.bindings);
        check_review(&intent).unwrap();
        assert_eq!(
            request_with_template(&intent, false).unwrap().project,
            preview_request.project
        );
        intent.bambu.bindings.reverse();
        check_review(&intent).unwrap();
        intent.bambu.bindings[0].occurrence_id = 999;
        assert!(
            check_review(&intent).is_err(),
            "A changed source-to-target mapping is a real choice"
        );
        intent.bambu.bindings = manually_ordered;
        intent.bambu.start_reviewed_baseline = !reset;
        assert!(
            check_review(&intent).is_err(),
            "Baseline consent remains part of the exact write review"
        );
    }
}

#[test]
fn reviewed_baseline_requires_complete_ui_bindings_and_explicit_preview_intent() {
    let mut intent = intent();
    assert!(
        !request(&intent)
            .unwrap()
            .project
            .start_reviewed_native_baseline
    );
    assert!(!reviewed_baseline_bindings_ready(&intent.bambu));
    for (instance_id, occurrence_id) in [(0, 11), (1, 21)] {
        intent.bambu.sources.push(Source {
            body_id: limo_cad_core::BodyId(1),
            occurrence_id,
            label: "Repeated part".into(),
        });
        intent.bambu.bindings.push(BambuPartBinding {
            body_id: limo_cad_core::BodyId(1),
            occurrence_id,
            object_id: 2,
            instance_id,
            part_id: 1,
        });
    }
    assert!(reviewed_baseline_bindings_ready(&intent.bambu));
    let ordinary = request(&intent).unwrap();
    assert!(
        !ordinary.project.start_reviewed_native_baseline,
        "Complete binds alone never infer reset consent"
    );
    let old_review = review_key(&ordinary, &intent.bambu.template().unwrap().summary);
    intent.bambu.start_reviewed_baseline = true;
    intent.bambu.invalidate();
    let reviewed = request(&intent).unwrap();
    assert!(reviewed.project.start_reviewed_native_baseline);
    assert!(reviewed.project.refresh_reference.is_none());
    assert_eq!(reviewed.project.bindings.len(), 2);
    assert_ne!(
        old_review,
        review_key(&reviewed, &intent.bambu.template().unwrap().summary),
        "Changing baseline consent invalidates the old write review"
    );
    intent.bambu.bindings[1].instance_id = 0;
    assert!(
        !reviewed_baseline_bindings_ready(&intent.bambu),
        "Two source repeats cannot map to one target"
    );
    intent.bambu.bindings[1].instance_id = 1;
    intent.bambu.bindings.pop();
    assert!(
        !reviewed_baseline_bindings_ready(&intent.bambu),
        "Incomplete repeated source/target coverage cannot enable the UI action"
    );
}

#[test]
fn explicit_native_targets_preserve_repeated_instances_without_name_matching() {
    let intent = intent();
    let targets = targets(&intent.bambu);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].0, "object:2:instance:0:part:1");
    assert_eq!(targets[1].0, "object:2:instance:1:part:1");
    assert!(
        intent.bambu.bindings.is_empty(),
        "Inspecting a template must not infer any binding from duplicate names"
    );
    let request = request(&intent).unwrap();
    assert_eq!(
        request.export.expected_model_json.as_deref(),
        Some("owned-model")
    );
    assert_eq!(request.template_base64, "c25hcHNob3Q=");
    assert!(request.project.bindings.is_empty());
    assert!(!request.project.allow_template_appearance);
    assert!(!request.project.accept_native_setting_changes);
}

#[test]
fn portable_mode_and_template_placement_have_distinct_preflight_policies() {
    let mut intent = intent();
    assert!(check_review(&intent).is_err());
    assert!(io::needs_layout_check(&intent));
    intent.bambu.placement = BambuPlacementMode::Template;
    intent.layout_report = Some(json!({"issues":[{"code":"below_bed"}]}));
    assert!(!io::needs_layout_check(&intent));
    assert!(
        !io::layout_has_issues(&intent),
        "CAD placement warnings do not describe preserved template plates"
    );
    assert!(io::check_layout_confirmation(&intent).is_ok());
    intent.bambu.enabled = false;
    assert!(check_review(&intent).is_ok());
    assert!(io::needs_layout_check(&intent));
    assert!(io::check_layout_confirmation(&intent).is_err());
}

#[test]
fn review_identity_includes_model_bindings_and_deliberate_confirmations() {
    let mut intent = intent();
    let original = request(&intent).unwrap();
    let key = review_key(&original, &intent.bambu.template().unwrap().summary);
    intent.bambu.output = "D:/different-output.3mf".into();
    assert_eq!(
        key,
        review_key(
            &request(&intent).unwrap(),
            &intent.bambu.template().unwrap().summary
        )
    );
    intent.bambu.allow_appearance = true;
    assert_ne!(
        key,
        review_key(
            &request(&intent).unwrap(),
            &intent.bambu.template().unwrap().summary
        )
    );
    intent.bambu.allow_appearance = false;
    intent.bambu.model = "changed-model".into();
    assert_ne!(
        key,
        review_key(
            &request(&intent).unwrap(),
            &intent.bambu.template().unwrap().summary
        )
    );
    intent.bambu.document.as_mut().unwrap().source_document_id = None;
    assert!(request(&intent).unwrap_err().contains("source identity"));
}

#[test]
fn read_only_review_preserves_exact_model_and_key_shape_without_owned_model_input() {
    let mut intent = intent();
    intent.bambu.model = "large immutable saved model ".repeat(32_768);
    let original = request(&intent).unwrap();
    let key = review_key(&original, &intent.bambu.template().unwrap().summary);
    let report: BambuProjectReport = serde_json::from_value(json!({
        "template":intent.bambu.template().unwrap().summary,
        "source_document_id":original.project.source_document_id,
        "output_sha256":"b".repeat(64),"placement":"template","parts":[],"modifiers":[],
        "invalidated_entries":[],"warnings":[],"metadata_readback_verified":true,
        "installed_slicer_imported":false,"toolpaths_generated":false,
        "refresh_reference":{
            "version":1,"source_document_id":original.project.source_document_id,
            "original_template_sha256":"a".repeat(64),"profile_sha256":"b".repeat(64),
            "profile_identity_sha256":"c".repeat(64),"baseline_project_settings":{},
            "written_project_settings":{},"parts":[],"modifiers":[],"height_objects":[]
        }
    }))
    .unwrap();
    // Exercise the same preview adoption and gate used by panel layout and Write.
    accept_preview(&mut intent, key.clone(), report);
    let choices = request_fields(&intent, false, false).unwrap();
    assert!(choices.export.expected_model_json.is_none());
    assert!(choices.template_base64.is_empty());
    for _ in 0..3 {
        check_review(&intent).unwrap();
    }
    for pointer in ["", "/export", "/project"] {
        let mut malformed = key.clone();
        malformed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected_field".into(), Value::Null);
        intent.bambu.reviewed.as_mut().unwrap().0 = malformed;
        assert!(check_review(&intent).is_err());
    }
    for pointer in ["/export", "/project"] {
        let mut malformed = key.clone();
        *malformed.pointer_mut(pointer).unwrap() = Value::Null;
        intent.bambu.reviewed.as_mut().unwrap().0 = malformed;
        assert!(check_review(&intent).is_err());
    }
    for field in key["export"].as_object().unwrap().keys() {
        let mut malformed = key.clone();
        malformed["export"].as_object_mut().unwrap().remove(field);
        intent.bambu.reviewed.as_mut().unwrap().0 = malformed;
        assert!(check_review(&intent).is_err());
    }
    for field in key.as_object().unwrap().keys() {
        let mut malformed = key.clone();
        malformed.as_object_mut().unwrap().remove(field);
        intent.bambu.reviewed.as_mut().unwrap().0 = malformed;
        assert!(check_review(&intent).is_err());
    }
    for value in [Value::Null, json!(true), json!("different saved model")] {
        let mut malformed = key.clone();
        malformed["export"]["expected_model_json"] = value;
        intent.bambu.reviewed.as_mut().unwrap().0 = malformed;
        assert!(check_review(&intent).is_err());
    }
    intent.bambu.reviewed.as_mut().unwrap().0 = key;
    intent.bambu.model.push('!');
    assert!(check_review(&intent).is_err());
    intent.bambu.model.pop();
    check_review(&intent).unwrap();
    intent.bambu.allow_appearance = true;
    assert!(check_review(&intent).is_err());
    intent.bambu.allow_appearance = false;
    intent
        .bambu
        .template
        .as_mut()
        .unwrap()
        .summary
        .template_sha256 = "c".repeat(64);
    assert!(check_review(&intent).is_err());
}

#[test]
fn saved_native_identity_does_not_resubmit_obsolete_object_numbers() {
    let mut intent = intent();
    intent.bambu.bindings.push(BambuPartBinding {
        body_id: limo_cad_core::BodyId(1),
        occurrence_id: 1,
        object_id: 999,
        instance_id: 0,
        part_id: 888,
    });
    intent.bambu.reference = Some(BambuRefreshReference {
        version: 1,
        source_document_id: intent
            .bambu
            .document
            .as_ref()
            .unwrap()
            .source_document_id
            .clone()
            .unwrap(),
        original_template_sha256: "a".repeat(64),
        profile_sha256: "b".repeat(64),
        profile_identity_sha256: "c".repeat(64),
        baseline_project_settings: Default::default(),
        written_project_settings: Default::default(),
        parts: vec![],
        modifiers: vec![],
        height_objects: vec![],
    });
    let saved = request(&intent).unwrap();
    assert!(
        saved.project.bindings.is_empty(),
        "Saved UUID/instance references must be resolved by the shared adapter against current native object IDs"
    );
    assert!(saved.project.refresh_reference.is_some());
    intent.bambu.reference = None;
    assert_eq!(
        request(&intent).unwrap().project.bindings[0].object_id,
        999,
        "Fresh foreign templates still require the explicit current numeric mapping"
    );
}

#[test]
fn actual_template_z_issues_require_deliberate_confirmation_and_invalidate_with_preview() {
    let mut intent = intent();
    intent.bambu.placement = BambuPlacementMode::Template;
    let report: BambuProjectReport = serde_json::from_value(json!({
        "template": intent.bambu.template.as_ref().unwrap().summary,
        "source_document_id": intent.bambu.document.as_ref().unwrap().source_document_id,
        "output_sha256": "e".repeat(64), "placement": "template", "parts": [], "modifiers": [],
        "invalidated_entries": [], "warnings": [], "metadata_readback_verified": true,
        "installed_slicer_imported": false, "toolpaths_generated": false,
        "refresh_reference": {
            "version": 1, "source_document_id": intent.bambu.document.as_ref().unwrap().source_document_id,
            "original_template_sha256": "a".repeat(64), "profile_sha256": "b".repeat(64),
            "profile_identity_sha256": "c".repeat(64), "baseline_project_settings": {},
            "written_project_settings": {}, "parts": [], "modifiers": [], "height_objects": []
        },
        "z_preflight": [{"object_id":2,"instance_id":0,"plate_index":1,"source_bindings":[],
            "world_bounds":{"min_mm":[0.,0.,8.6],"max_mm":[10.,10.,18.6]},
            "issues":[{"code":"above_bed","message":"Above bed","occurrence_ids":[1]}],
            "proposed_translation_mm":[0.,0.,-8.6],"correction_target":"saved_template"}]
    })).unwrap();
    intent.bambu.reviewed = Some((json!({}), report.clone()));
    assert!(io::layout_has_issues(&intent));
    assert!(io::check_layout_confirmation(&intent).is_err());
    intent.allow_layout_issues = true;
    assert!(io::check_layout_confirmation(&intent).is_ok());
    assert_eq!(
        intent.bambu.reviewed.as_ref().unwrap().1,
        report,
        "Deliberate export never applies the proposal"
    );
    intent.bambu.invalidate();
    assert!(
        !io::layout_has_issues(&intent),
        "Obsolete preview diagnostics are not current placement evidence"
    );
    assert!(
        check_review(&intent).is_err(),
        "Export still requires a fresh complete preview"
    );
}

#[test]
fn multipart_preflight_sources_remain_visible_and_distinct_across_repaint() {
    use super::super::super::chrome::Widgets;
    use limo_cad_interface::Field as ControlField;

    let mut world = World::new();
    world.init_resource::<ViewportUiAssets>();
    world.init_resource::<Files>();
    let camera = world.spawn_empty().id();
    let engine = AppState::new();
    let mut widgets = Widgets::default();
    let mut intent = intent();
    let template = &mut intent.bambu.template.as_mut().unwrap().summary;
    let mut second_volume = template.objects[0].parts[0].clone();
    second_volume.part_id = 2;
    template.objects[0].parts.push(second_volume);
    let mut report: BambuProjectReport = serde_json::from_value(json!({
        "template": template,
        "source_document_id": intent.bambu.document.as_ref().unwrap().source_document_id,
        "output_sha256": "e".repeat(64), "placement": "template", "parts": [], "modifiers": [],
        "invalidated_entries": [], "warnings": [], "metadata_readback_verified": true,
        "installed_slicer_imported": false, "toolpaths_generated": false,
        "refresh_reference": {
            "version": 1, "source_document_id": intent.bambu.document.as_ref().unwrap().source_document_id,
            "original_template_sha256": "a".repeat(64), "profile_sha256": "b".repeat(64),
            "profile_identity_sha256": "c".repeat(64), "baseline_project_settings": {},
            "written_project_settings": {}, "parts": [], "modifiers": [], "height_objects": []
        }
    })).unwrap();
    for instance_id in [0, 1] {
        report.z_preflight.push(BambuGroupZPreflight {
            object_id: 2,
            instance_id,
            plate_index: 1,
            source_bindings: [1, 2]
                .into_iter()
                .map(|part_id| BambuPartBinding {
                    body_id: limo_cad_core::BodyId(part_id as u64),
                    occurrence_id: ((instance_id + 1) * 10 + part_id) as u64,
                    object_id: 2,
                    instance_id,
                    part_id,
                })
                .collect(),
            world_bounds: limo_cad_core::PrintModifierBoundsDto {
                min_mm: [20., 20., 0.],
                max_mm: [30., 30., 10.],
            },
            issues: vec![],
            proposed_translation_mm: None,
            correction_target: BambuZCorrectionTarget::SavedTemplate,
        });
    }
    intent.bambu.placement = BambuPlacementMode::Template;
    intent.bambu.reviewed = Some((json!({}), report));
    // Ten visible tail rows include both multipart groups and the output controls.
    intent.bambu.scroll = usize::MAX;
    let expected = [
        (
            "Plate 1 object 2 instance 0 volume 1 source",
            "body 1 occurrence 11 volume 1",
        ),
        (
            "Plate 1 object 2 instance 0 volume 2 source",
            "body 2 occurrence 12 volume 2",
        ),
        (
            "Plate 1 object 2 instance 1 volume 1 source",
            "body 1 occurrence 21 volume 1",
        ),
        (
            "Plate 1 object 2 instance 1 volume 2 source",
            "body 2 occurrence 22 volume 2",
        ),
    ];
    let mut retained = None;
    for y in [0., 10.] {
        widgets.begin();
        panel::paint(
            (&mut world, camera, &mut widgets),
            &engine,
            &intent,
            17,
            (0., y, 760., 700.),
            None,
        )
        .unwrap();
        widgets.finish(&mut world);
        let visible_sources = world
            .query::<&InterfaceControl>()
            .iter(&world)
            .filter(|control| {
                control.label.starts_with("Plate ") && control.label.ends_with(" source")
            })
            .count();
        assert_eq!(
            visible_sources,
            expected.len(),
            "No supported source binding may be overwritten"
        );
        let mut entities = Vec::new();
        let mut positions = Vec::new();
        for (label, value) in expected {
            let entity = widgets.entity(&format!("bambu-row-{label}")).unwrap();
            let control = world.get::<InterfaceControl>(entity).unwrap();
            assert_eq!(control.label, label);
            assert!(control.visible && !control.disabled);
            assert_eq!(control.modal_scope.as_deref(), Some("file-dialog"));
            assert_eq!(
                control.field,
                ControlField::Text {
                    value: value.into(),
                    read_only: true,
                    selection: None,
                }
            );
            assert_eq!(
                world
                    .get::<bevy::text::EditableText>(entity)
                    .unwrap()
                    .value(),
                value
            );
            let label_entity = widgets.entity(&format!("bambu-label-{label}")).unwrap();
            assert_eq!(world.get::<Text>(label_entity).unwrap().0, label);
            let top = world.get::<Node>(entity).unwrap().top;
            assert!(
                !positions.contains(&top),
                "Every source needs its own visible row"
            );
            positions.push(top);
            entities.push((entity, label_entity));
        }
        if let Some(previous) = &retained {
            assert_eq!(
                previous, &entities,
                "Repaint must retain all source controls"
            );
        }
        retained = Some(entities);
    }
}

#[test]
fn template_inspection_runs_outside_publisher_and_rejects_changed_source_receipts() {
    use super::super::tests::setup;
    use crate::session_bridge::native_interface::tests::Fixture;
    use std::cell::Cell;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (app, services, _) = setup(&fixture);
    let owner = fixture.owner();
    let receipt = current(app.world(), &services, &owner).unwrap();
    let intent = intent();
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    // This real generic 3MF lacks saved Bambu settings. Preserve the actual
    // parser failure; it is not a successful native-template qualification.
    let archive =
        include_bytes!("../../../../../../../crates/export/fixtures/smoke/cube_bambu_studio.3mf");
    let encoded = STANDARD.encode(archive);
    let parse = || {
        parse_engine_envelope(services.engine.engine_call(
            "bambu_template_inspect",
            &json!({"template_base64":encoded}).to_string(),
        ))
        .map(|summary| (encoded.clone(), summary))
    };
    let expected = parse().unwrap_err();
    let validations = Cell::new(0);
    let error = inspect_template_snapshot(
        &services,
        &receipt,
        &intent,
        || {
            if validations.get() != 0 {
                return Err("Initiating control is disabled by its admitted busy frame".into());
            }
            validations.set(validations.get() + 1);
            Ok(())
        },
        || {
            // Re-enter the actual live receipt while parsing. This would deadlock
            // if bulk inspection still retained the enclosing publisher guard.
            services
                .bridge
                .with_native_document_receipt(&services.engine, &owner, |revision| {
                    assert_eq!(revision, receipt.revision);
                    parse()
                })
        },
    )
    .unwrap_err();
    assert_eq!(error, expected);
    assert_eq!(validations.get(), 1);
    assert_eq!(current(app.world(), &services, &owner).unwrap(), receipt);
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );

    let error = inspect_template_snapshot(
        &services,
        &receipt,
        &intent,
        || Ok(()),
        || {
            let parsed = parse();
            fixture
                .rename(&owner, "Changed during template inspection")
                .unwrap();
            parsed
        },
    )
    .unwrap_err();
    assert!(
        error.contains("document changed during template inspection"),
        "{error}"
    );
    inspect_template_snapshot(
        &services,
        &receipt,
        &intent,
        || Ok(()),
        || panic!("A stale source must reject before reading the template"),
    )
    .unwrap_err();

    let receipt = current(app.world(), &services, &owner).unwrap();
    let changed =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let error = inspect_template_snapshot(
        &services,
        &receipt,
        &intent,
        || Ok(()),
        || {
            let parsed = parse();
            services
                .bridge
                .apply_native_mutation(
                    &services.engine,
                    &owner,
                    "cad_load_project_model",
                    &json!({"model_json":changed}),
                    || Ok(()),
                )
                .unwrap();
            parsed
        },
    )
    .unwrap_err();
    assert!(error.contains("replaced"), "{error}");
    let replacement = fixture.owner();
    assert_eq!(replacement.document_id, owner.document_id);
    assert_ne!(replacement.epoch, owner.epoch);
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        changed
    );
    let receipt = current(app.world(), &services, &replacement).unwrap();
    let error = inspect_template_snapshot(
        &services,
        &receipt,
        &intent,
        || Err("Original I/O ownership rejected".into()),
        || panic!("Failed I/O ownership must reject before template parsing"),
    )
    .unwrap_err();
    assert_eq!(error, "Original I/O ownership rejected");
}

#[test]
fn native_bambu_write_detaches_build_and_requires_current_dialog_operation() {
    use super::super::tests::{path, setup};
    use crate::session_bridge::native_interface::tests::Fixture;
    use std::cell::{Cell, RefCell};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let archive = include_bytes!("../../../../../../fixtures/bambu-owned-template.3mf");
    let summary = inspect_bambu_template(archive).unwrap();
    assert_eq!(
        summary.template_sha256,
        "bc256381139337ea5f6dde0dc78c353dad0cfeafce6567a3b53c4e2594b199b6"
    );
    let targets: Vec<_> = summary
        .objects
        .iter()
        .flat_map(|object| {
            (0..object.instance_count).flat_map(move |instance| {
                object
                    .parts
                    .iter()
                    .map(move |part| (object.object_id, instance, part.part_id))
            })
        })
        .collect();
    assert_eq!(targets.len(), 5);
    for index in 0..targets.len() {
        let x = index as f64 * 12.;
        for (operation, arguments) in [
            (
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            ),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+8.,"y":8.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":format!("Sketch{}",index+1),"profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
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
    fixture.bridge.apply_native_mutation(&fixture.engine, &fixture.owner(), "print_intent_set_document", &json!({"document":PrintIntentDocumentDto::default(),"expected_model_json":parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()}), || Ok(())).unwrap();
    let model =
        || parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let before = model();
    let document: PrintIntentDocumentDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("print_intent_get", "")).unwrap(),
    )
    .unwrap();
    let solution: limo_cad_sketch::AssemblySolutionDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("assembly_solution", "")).unwrap(),
    )
    .unwrap();
    assert_eq!(solution.instance_body_poses.len(), targets.len());
    let mut source = intent();
    source.body_ids = solution
        .instance_body_poses
        .iter()
        .map(|p| p.body_id)
        .collect();
    source.bambu.document = Some(document);
    source.bambu.model = before.as_str().unwrap().into();
    let input_path = path("read-only-input.3mf");
    assert!(!input_path.exists());
    std::fs::write(&input_path, archive).unwrap();
    source.bambu.template = Some(Template {
        path: input_path.clone(),
        encoded: Arc::new(STANDARD.encode(archive)),
        summary,
    });
    source.bambu.allow_appearance = true;
    source.bambu.start_reviewed_baseline = true;
    source.bambu.placement = BambuPlacementMode::Template;
    source.bambu.bindings = solution
        .instance_body_poses
        .iter()
        .zip(&targets)
        .map(
            |(pose, &(object_id, instance_id, part_id))| BambuPartBinding {
                body_id: pose.body_id,
                occurrence_id: pose.occurrence_id.0,
                object_id,
                instance_id,
                part_id,
            },
        )
        .collect();
    let req = request(&source).unwrap();
    let key = review_key(&req, &source.bambu.template().unwrap().summary);
    let report = fixture
        .engine
        .prepare_bambu_export(req)
        .unwrap()
        .build()
        .unwrap()
        .report()
        .clone();
    accept_preview(&mut source, key, report);
    check_review(&source).unwrap();
    assert!(target_layout_has_issues(&source));
    let (mut app, services, handle) = setup(&fixture);
    for case in [
        "layout",
        "valid",
        "overwrite",
        "cancel",
        "choices",
        "dialog",
        "owner",
        "model",
    ] {
        let owner = fixture.owner();
        let receipt = current(app.world(), &services, &owner).unwrap();
        let mut case_source = source.clone();
        // This authentic template reports layout warnings for the replacement
        // stock. Review establishes its choices; exporting those warnings needs
        // separate explicit consent, which the refusal case deliberately omits.
        case_source.allow_layout_issues = case != "layout";
        let source = Arc::new(case_source);
        show_dialog(
            app.world_mut(),
            receipt.clone(),
            DialogKind::Export(source.clone()),
        )
        .unwrap();
        let token = app
            .world()
            .resource::<Files>()
            .dialog
            .as_ref()
            .unwrap()
            .token;
        let operation = export_operation::begin(app.world(), &receipt, &source).unwrap();
        let destination = path(&format!("native-bambu-write-{case}.3mf"));
        assert!(!destination.exists());
        if case == "overwrite" {
            std::fs::write(&destination, b"Existing explicitly replaceable output").unwrap();
        }
        let admitted = Cell::new(0);
        let built = Cell::new(0);
        let expected = RefCell::new(Vec::new());
        let outcome = io::bambu_write_snapshot(
            &services,
            &receipt,
            &source,
            &operation,
            (&destination, case == "overwrite"),
            || {
                assert_eq!(
                    admitted.replace(admitted.get() + 1),
                    0,
                    "Busy control must never be admitted twice"
                );
                Ok(())
            },
            |prepared| {
                assert_eq!(built.replace(built.get() + 1), 0);
                // Real native meshing/archive construction while both original live
                // locks are held proves the detached builder cannot reenter them.
                let finished = services.bridge.with_native_document_receipt(
                    &services.engine,
                    &owner,
                    |_| fixture.engine.with_drawing(|_| prepared.build()),
                )?;
                assert!(!finished.source_layout_has_issues());
                expected.replace(finished.bytes().to_vec());
                match case {
                    "cancel" => {
                        execute(
                            app.world_mut(),
                            &handle,
                            &services,
                            &owner,
                            FileCommand::Cancel(token),
                        )?;
                        assert_eq!(current(app.world(), &services, &owner)?, receipt);
                    }
                    "choices" => {
                        reduce(
                            app.world_mut(),
                            &handle,
                            (&services, &owner),
                            token,
                            source.bambu.generation,
                            Command::AcceptNativeChanges,
                            &ControlInput::Click,
                        )?;
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
                        fixture.rename(&owner, "Changed during detached Bambu write")?;
                    }
                    "valid" | "overwrite" => {}
                    _ => unreachable!(),
                }
                Ok(finished)
            },
        );
        assert_eq!(admitted.get(), 1);
        assert_eq!(built.get(), usize::from(case != "layout"));
        if matches!(case, "valid" | "overwrite") {
            let (mut result, bytes) = outcome.unwrap();
            assert_eq!(bytes, *expected.borrow());
            assert_eq!(std::fs::read(&destination).unwrap(), bytes);
            assert_eq!(result.value["exported"], true);
            io::prepare_written_template(&mut result, bytes).unwrap();
            assert!(result.value["written_template"]["summary"].is_object());
            assert!(operation
                .commit::<()>(|| panic!("Completed operation cannot write twice"))
                .is_err());
        } else {
            let error = outcome.unwrap_err();
            let expected_refusal = match case {
                "layout" => "explicitly choose Export despite layout issues",
                "cancel" | "choices" | "dialog" => "cancelled or its choices were replaced",
                "owner" => "replaced",
                "model" => "changed",
                _ => unreachable!(),
            };
            assert!(error.contains(expected_refusal), "{case}: {error}");
            assert!(
                !destination.exists(),
                "Revoked operation must materialize no output"
            );
            if case == "layout" {
                assert!(expected.borrow().is_empty());
            }
        }
        if case == "model" {
            assert_ne!(model(), before);
        } else {
            assert_eq!(model(), before);
        }
    }
    assert_eq!(
        inspect_bambu_template(archive).unwrap().template_sha256,
        "bc256381139337ea5f6dde0dc78c353dad0cfeafce6567a3b53c4e2594b199b6"
    );
    let retained_input = std::fs::read(input_path).unwrap();
    assert_eq!(retained_input, archive);
    assert_eq!(
        inspect_bambu_template(&retained_input)
            .unwrap()
            .template_sha256,
        "bc256381139337ea5f6dde0dc78c353dad0cfeafce6567a3b53c4e2594b199b6"
    );
}
