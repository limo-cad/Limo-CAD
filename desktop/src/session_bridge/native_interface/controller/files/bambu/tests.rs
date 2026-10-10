use super::*;

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
