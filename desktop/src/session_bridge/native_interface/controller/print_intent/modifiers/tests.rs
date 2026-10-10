use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

fn zone(body: u64) -> PrintModifierDto {
    PrintModifierDto {
        id: "01234567-89ab-4cde-8123-456789abcdef".into(),
        name: "Drive reinforcement".into(),
        body_id: BodyId(body),
        enabled: true,
        local_pose: limo_cad_core::PrintLocalPoseDto {
            translation_mm: [5., 3., 1.5],
            rotation: [
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
        },
        primitive: PrintModifierPrimitiveDto::Box { size_mm: [2.; 3] },
        settings: PrintSettingsDto {
            infill_density_percent: Some(80.),
            ..Default::default()
        },
    }
}
fn seed(fixture: &Fixture) -> u64 {
    let owner = fixture.owner();
    for (op, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":6.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.}}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
            .unwrap();
    }
    fixture.engine.viewport_snapshot().2.bodies[0].id.0
}
fn read(fixture: &Fixture, operation: &str) -> Value {
    parse_engine_envelope(fixture.engine.engine_call(operation, "")).unwrap()
}

#[test]
fn saved_modifier_selection_rejects_unknown_and_cross_body_ids_without_changing_draft() {
    let own = zone(7);
    let mut other = zone(8);
    other.id = "01234567-89ab-4cde-8123-456789abcdee".into();
    let document = PrintIntentDocumentDto {
        modifiers: vec![own.clone(), other.clone()],
        ..Default::default()
    };
    let mut state = State {
        body: 7,
        modifier_scope: true,
        ..default()
    };
    state.accept(document.clone(), "before".into(), json!({}), 1);
    for id in [other.id.as_str(), "unknown"] {
        assert!(edit(&mut state, Field::Saved, id, UnitSystem::Mm).is_err());
        assert_eq!(state.modifier_selection, own.id);
        assert_eq!(state.draft, own.settings);
        assert!(!state.dirty());
    }
    state.modifier_selection = other.id.clone();
    assert!(canonical(&mut state, &document).is_none());
    assert_eq!(state.current(&document), PrintSettingsDto::default());
    edit(&mut state, Field::Saved, "", UnitSystem::Mm).unwrap();
    assert_eq!(state.modifier_selection, own.id);
}

#[test]
fn modifier_draft_uses_shared_inheritance_and_unit_inputs_without_changing_identity() {
    let mut document = PrintIntentDocumentDto::default();
    document.modifiers.push(zone(7));
    let mut state = State {
        body: 7,
        modifier_scope: true,
        ..default()
    };
    state.accept(document, "before".into(), json!({}), 1);
    assert!(!state.dirty());
    edit(&mut state, Field::Translation(0), "2 in", UnitSystem::Mm).unwrap();
    state.draft.wall_count = Some(0);
    let (op, args) = write(&state, &super::super::Command::Apply, UnitSystem::Mm).unwrap();
    assert_eq!(op, "print_modifier_update");
    assert_eq!(args["modifier"]["body_id"], 7);
    assert_eq!(args["modifier"]["id"], zone(7).id);
    assert!(
        (args["modifier"]["local_pose"]["translation_mm"][0]
            .as_f64()
            .unwrap()
            - 50.8)
            .abs()
            < 1e-9
    );
    assert_eq!(args["modifier"]["settings"]["wall_count"], 0);
    assert!(args["modifier"]["settings"]["top_shell_layers"].is_null());
    edit(&mut state, Field::Size(0), "-1 mm", UnitSystem::Mm).unwrap();
    assert!(write(&state, &super::super::Command::Apply, UnitSystem::Mm).is_err());
    edit(
        &mut state,
        Field::Name,
        "Requested drive zone",
        UnitSystem::Mm,
    )
    .unwrap();
    edit(&mut state, Field::Size(0), "4 mm", UnitSystem::Mm).unwrap();
    assert!(
        state.errors.is_empty(),
        "Correcting shape must clear the current zone validation error"
    );
    let (op, args) = write(&state, &super::super::Command::Inherit, UnitSystem::Mm).unwrap();
    assert_eq!(op, "print_modifier_reset");
    assert_eq!(args, json!({"id":zone(7).id}));
    let mut world = World::new();
    state.visible = true;
    world.insert_resource(state);
    assert!(ensure_clean(&world).is_err());
}

#[test]
fn overlays_follow_recalled_parent_and_local_rotation_for_every_repeat_and_restore_only_owned_layers(
) {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let body = seed(&fixture);
    let owner = fixture.owner();
    let component = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "assembly_create_component",
            &json!({"name":"Repeated print","body_ids":[body],"absorb_promoted_bodies":true}),
            || Ok(()),
        )
        .unwrap()
        .value;
    let root = read(&fixture, "assembly_document")["component_structure"]["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["component_id"] == component["id"])
        .unwrap()["id"]
        .clone();
    fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"assembly_create_occurrence",&json!({"component_id":component["id"],"name":"Intentional repeat","local_pose":{"translation":[40.,0.,0.],"rotation":[0.,0.,0.,1.]}}),||Ok(())).unwrap();
    fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"upsert_named_view",&json!({"name":"Zone layout","camera":{"position":[170.,-170.,130.],"target":[0.,0.,0.],"up":[0.,0.,1.]},"visible_body_ids":[body],"part_offsets":[],"occurrence_offsets":[{"occurrence_id":root,"translation":[100.,0.,0.],"rotation":[0.,0.,std::f64::consts::FRAC_1_SQRT_2,std::f64::consts::FRAC_1_SQRT_2]}]}),||Ok(())).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "recall_named_view",
            &json!({"name":"Zone layout"}),
            || Ok(()),
        )
        .unwrap();
    let document = PrintIntentDocumentDto {
        modifiers: vec![zone(body)],
        ..Default::default()
    };
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), false).unwrap();
    native_viewport::apply_interface_viewport(
        app.world_mut(),
        limo_cad_interface::Rect {
            x: 240.,
            y: 120.,
            width: 1120.,
            height: 692.,
        },
        1.,
    )
    .unwrap();
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        Some(native_viewport::ViewportCamera {
            position: [170., -170., 130.],
            target: [0., 0., 0.],
            up: [0., 0., 1.],
            vertical_fov_degrees: 15.2,
        }),
        None,
    )
    .unwrap();
    let baseline = ViewportPreview {
        lines: vec![ViewportLineLayer {
            segments: vec![777., 0., 0., 778., 0., 0.].into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    native_viewport::apply_interface_preview(app.world_mut(), &owner.document_id, baseline)
        .unwrap();
    let mut state = State {
        body,
        modifier_scope: true,
        owner: Some(owner.clone()),
        ..default()
    };
    state.accept(document, "fixture".into(), json!({}), 1);
    let revision = native_viewport::interface_model_revision(app.world());
    synchronize_overlay(app.world_mut(), &mut state, &owner).unwrap();
    let preview = native_viewport::interface_preview_snapshot(app.world());
    assert_eq!(state.modifier_overlays.labels.len(), 2);
    assert!(state.modifier_overlays.labels.iter().all(|(pixel, label)| {
        pixel.iter().all(|v| v.is_finite())
            && label.starts_with("Drive reinforcement · occurrence ")
    }));
    assert_ne!(
        state.modifier_overlays.labels[0].1,
        state.modifier_overlays.labels[1].1
    );
    assert_eq!(preview.triangles.len(), 2);
    assert_eq!(
        native_viewport::interface_model_revision(app.world()),
        revision
    );
    let mut points: Vec<_> = preview
        .triangles
        .iter()
        .map(|t| t.positions[..3].to_vec())
        .collect();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    for (actual, expected) in points.into_iter().zip([[46., 2., 0.5], [98., 6., 0.5]]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!((*a - e).abs() < 1e-4, "{actual:?}")
        }
    }
    restore_overlay(app.world_mut(), &mut state).unwrap();
    assert!(state.modifier_overlays.labels.is_empty());
    assert!(native_viewport::interface_preview_snapshot(app.world())
        .triangles
        .is_empty());
    assert_eq!(
        native_viewport::interface_preview_snapshot(app.world()).lines[0]
            .segments
            .as_slice(),
        [777., 0., 0., 778., 0., 0.]
    );
    synchronize_overlay(app.world_mut(), &mut state, &owner).unwrap();
    native_viewport::apply_interface_preview(
        app.world_mut(),
        &owner.document_id,
        ViewportPreview::default(),
    )
    .unwrap();
    restore_overlay(app.world_mut(), &mut state).unwrap();
    assert!(
        native_viewport::interface_preview_snapshot(app.world())
            .lines
            .is_empty(),
        "Closing zones must not overwrite a newer preview owner"
    );
}

#[test]
fn modifier_metadata_history_retains_recalled_layout_identity_and_portable_omission_report() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let body = seed(&fixture);
    let owner = fixture.owner();
    fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"upsert_named_view",&json!({"name":"Modifier metadata layout","camera":{"position":[170.,-170.,130.],"target":[0.,0.,0.],"up":[0.,0.,1.]},"visible_body_ids":[body],"part_offsets":[{"body_id":body,"translation":[25.,0.,0.]}],"occurrence_offsets":[]}),||Ok(())).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "recall_named_view",
            &json!({"name":"Modifier metadata layout"}),
            || Ok(()),
        )
        .unwrap();
    let before = read(&fixture, "project_export_model");
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "print_modifier_create",
            &json!({"modifier":zone(body),"expected_model_json":before}),
            || Ok(()),
        )
        .unwrap();
    let saved = read(&fixture, "print_intent_get");
    let preflight =
        parse_engine_envelope(fixture.engine.engine_call("solid_export_preflight", "{}")).unwrap();
    assert_eq!(
        preflight["print_intent"]["modifiers"][0]["modifier"]["id"],
        zone(body).id
    );
    assert!(preflight["print_intent"]["modifiers"][0]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap().contains("omitted")));
    for redo in [false, true] {
        let current = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap();
        assert_eq!(
            read(&fixture, "named_views")["active"],
            "Modifier metadata layout"
        );
        fixture
            .bridge
            .apply_native_history_at(
                &fixture.engine,
                &current.owner,
                current.revision,
                redo,
                || Ok(()),
            )
            .unwrap();
    }
    assert_eq!(read(&fixture, "print_intent_get"), saved);
    let actual: Value =
        serde_json::from_str(read(&fixture, "project_export_model").as_str().unwrap()).unwrap();
    let mut source: Value = serde_json::from_str(before.as_str().unwrap()).unwrap();
    source["print_intent"] = actual["print_intent"].clone();
    assert_eq!(
        actual, source,
        "Metadata history must preserve source geometry and hierarchy"
    );
}
