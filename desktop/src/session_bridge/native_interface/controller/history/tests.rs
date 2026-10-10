use super::*;
use crate::session_bridge::native_interface::tests::Fixture;
use crate::session_bridge::parse_engine_envelope;

fn solid(fixture: &Fixture) -> DocumentContext {
    let owner = fixture.owner();
    for (op, args) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":12.,"y":8.},"ctrl_held":false}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0]}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
            .unwrap();
    }
    owner
}
fn drain(world: &mut World, services: &NativeServices) -> Result<Value, String> {
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(outcome) = worker::poll(world, services) {
            return outcome.value;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "History worker timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn export_model(fixture: &Fixture) -> Value {
    serde_json::from_str(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", ""))
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn renaming_solid_history_preserves_geometry_undo_and_reload() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = solid(&fixture);
    let export = || export_model(&fixture);
    let before = export();
    let geometry = fixture.engine.viewport_frame();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), false).unwrap();
    worker::install(
        app.world_mut(),
        services.clone(),
        NativeInterfaceHandle::new(|| {}),
    )
    .unwrap();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    mutation(
        app.world_mut(),
        receipt,
        "solid_rename_feature",
        json!({"feature_id":2,"name":"Post / 630 mm stock"}),
    )
    .unwrap();
    let response = drain(app.world_mut(), &services).unwrap();
    assert!(response["render_error"].is_null(), "{response}");
    let after = export();
    let mut expected = before.clone();
    expected["document"]["history"]["features"][1]["name"] = json!("Post / 630 mm stock");
    expected["extrudes"][0]["name"] = json!("Post / 630 mm stock");
    assert_eq!(after, expected);
    let renamed = fixture.engine.viewport_frame();
    assert_eq!(renamed.geometry_revision, geometry.geometry_revision);
    assert!(
        Arc::ptr_eq(&renamed.document, &geometry.document),
        "Renaming must retain the complete viewport document"
    );
    for arguments in [
        json!({"feature_id":2,"name":"   "}),
        json!({"feature_id":2,"name":"a".repeat(257)}),
        json!({"feature_id":2,"name":"bad\nname"}),
        json!({"feature_id":999,"name":"Missing"}),
        json!({"feature_id":2,"name":"Valid","unexpected":true}),
    ] {
        assert!(fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                "solid_rename_feature",
                &arguments,
                || Ok(())
            )
            .is_err());
        assert_eq!(export(), after);
    }
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(), before);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(), after);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "solid_recompute",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(export(), after);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cad_load_project_model",
            &json!({"model_json":after.to_string()}),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(export(), after);
    assert_eq!(
        fixture.engine.viewport_snapshot().2,
        *geometry.document.scene
    );
}

#[test]
fn renaming_sketch_history_preserves_dependent_solid_undo_and_reload() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = solid(&fixture);
    let before = export_model(&fixture);
    let scene = fixture.engine.viewport_snapshot().2;
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "solid_rename_feature",
            &json!({"feature_id":1,"name":"Base outline"}),
            || Ok(()),
        )
        .unwrap();
    let after = export_model(&fixture);
    assert_eq!(
        after["document"]["history"]["features"][0]["name"],
        "Base outline"
    );
    assert_eq!(after["extrudes"][0]["sketch_name"], "Base outline");
    assert_eq!(fixture.engine.viewport_snapshot().2, scene);
    for (redo, expected) in [(false, &before), (true, &after)] {
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), redo, || Ok(()))
            .unwrap();
        assert_eq!(&export_model(&fixture), expected);
        assert_eq!(fixture.engine.viewport_snapshot().2, scene);
    }
    for (operation, args) in [
        ("solid_recompute", json!({})),
        (
            "cad_load_project_model",
            json!({"model_json":after.to_string()}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &fixture.owner(), operation, &args, || {
                Ok(())
            })
            .unwrap();
        assert_eq!(export_model(&fixture), after);
        assert_eq!(fixture.engine.viewport_snapshot().2, scene);
    }
}

fn publish_history(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    owner: &DocumentContext,
    document: &DocumentDto,
) {
    use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
    let previous = world
        .query::<(Entity, &NativeCommandBinding)>()
        .iter(world)
        .filter(|(_, binding)| matches!(binding.command, NativeCommand::History(_)))
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    for entity in previous {
        world.despawn(entity);
    }
    for (index, command) in document
        .features
        .iter()
        .map(|feature| HistoryCommand::Select(feature.id.0))
        .chain(std::iter::once(HistoryCommand::RollbackMarker))
        .enumerate()
    {
        let entity = world
            .spawn((
                InterfaceControl::button("document/history", format!("History {index}")),
                ComputedNode {
                    size: Vec2::new(80., 24.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(370. + index as f32 * 90., 830.)),
                ComputedStackIndex(index as u32 + 1),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        bind_command(world, entity, NativeCommand::History(command)).unwrap();
    }
    let bounds = limo_cad_interface::Rect {
        x: 0.,
        y: 0.,
        width: 1200.,
        height: 860.,
    };
    handle
        .present(InterfaceFrame {
            context: owner.clone(),
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![Surface {
                name: "document/history".into(),
                text: None,
            }],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    interface_shell::tests::publish_layout_once(world, handle.clone());
}

fn gesture(
    owner: &DocumentContext,
    x: f32,
    y: f32,
    state: Option<bevy::input::ButtonState>,
) -> NativeHostInput {
    let cursor = Vec2::new(x, y);
    let event = if let Some(state) = state {
        WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Left,
            state,
            window: Entity::PLACEHOLDER,
        })
    } else {
        WindowEvent::CursorMoved(bevy::window::CursorMoved {
            window: Entity::PLACEHOLDER,
            position: cursor,
            delta: None,
        })
    };
    NativeHostInput {
        ui_scale: 1.,
        context: Some(owner.clone()),
        cursor: Some(cursor),
        modifiers: default(),
        event,
        consumed: false,
        actions: vec![],
    }
}

#[test]
fn stationary_history_edge_hold_scrolls_and_restores_idle_cadence() {
    use bevy::{
        input::ButtonState::Pressed,
        winit::{UpdateMode, WinitSettings},
    };
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = solid(&fixture);
    let document = fixture.engine.document_snapshot();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let handle = NativeInterfaceHandle::new(|| {});
    let idle_mode = UpdateMode::reactive_low_power(Duration::MAX);
    app.insert_resource(WinitSettings {
        focused_mode: idle_mode,
        unfocused_mode: idle_mode,
    });
    publish_history(app.world_mut(), &handle, &owner, &document);
    let second = app
        .world_mut()
        .query::<(Entity, &NativeCommandBinding)>()
        .iter(app.world())
        .find(|(_, binding)| {
            binding.command
                == NativeCommand::History(HistoryCommand::Select(document.features[1].id.0))
        })
        .unwrap()
        .0;
    app.world_mut()
        .get_mut::<InterfaceControl>(second)
        .unwrap()
        .visible = false;
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 360., 830., Some(Pressed)),
    )
    .unwrap();
    pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 405., 830., None),
    )
    .unwrap();
    drag::tick_at(
        app.world_mut(),
        &handle,
        &services,
        std::time::Instant::now(),
    )
    .unwrap();
    assert!(
        matches!(app.world().resource::<WinitSettings>().focused_mode, UpdateMode::Reactive { wait, .. } if wait <= Duration::from_millis(120))
    );
    drag::tick_at(
        app.world_mut(),
        &handle,
        &services,
        std::time::Instant::now() + Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        app.world().resource::<History>().scroll,
        1,
        "A timed update scrolls without CursorMoved"
    );
    assert_eq!(
        fixture.engine.document_snapshot().features,
        document.features
    );
    cancel_drag(app.world_mut());
    assert_eq!(
        app.world().resource::<WinitSettings>().focused_mode,
        idle_mode
    );
}

#[test]
fn history_drag_commits_once_on_drop_rejects_stale_receipts_and_moves_the_rollback_marker() {
    use bevy::input::ButtonState::{Pressed, Released};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = solid(&fixture);
    for (op, args) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":20.,"y":0.},"p2":{"x":24.,"y":4.},"ctrl_held":false}),
        ),
        ("sketch_finish", json!({})),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
            .unwrap();
    }
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let handle = NativeInterfaceHandle::new(|| {});
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    let original = fixture.engine.document_snapshot();
    assert_eq!(original.features.len(), 3);
    publish_history(app.world_mut(), &handle, &owner, &original);
    assert!(!pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 550., 830., Some(Pressed))
    )
    .unwrap());
    assert!(pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 340., 830., None)
    )
    .unwrap());
    assert!(
        !worker::busy(app.world()),
        "Moving previews without rebuilding"
    );
    assert_eq!(
        fixture.engine.document_snapshot().features,
        original.features
    );
    assert!(pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 340., 830., Some(Released))
    )
    .unwrap());
    drain(app.world_mut(), &services).unwrap();
    let reordered = fixture.engine.document_snapshot();
    assert_eq!(
        reordered.features.iter().map(|f| f.id).collect::<Vec<_>>(),
        vec![
            original.features[2].id,
            original.features[0].id,
            original.features[1].id
        ]
    );
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    publish_history(app.world_mut(), &handle, &owner, &reordered);
    for (x, y, state) in [
        (550., 830., Some(Pressed)),
        (340., 830., None),
        (340., 830., Some(Released)),
    ] {
        pointer(
            app.world_mut(),
            &handle,
            &services,
            &gesture(&owner, x, y, state),
        )
        .unwrap();
    }
    assert!(drain(app.world_mut(), &services)
        .unwrap_err()
        .contains("dependency"));
    assert_eq!(
        fixture.engine.document_snapshot().features,
        reordered.features
    );
    for (x, y, state) in [
        (370., 830., Some(Pressed)),
        (590., 830., None),
        (590., 700., Some(Released)),
    ] {
        pointer(
            app.world_mut(),
            &handle,
            &services,
            &gesture(&owner, x, y, state),
        )
        .unwrap();
    }
    assert!(
        !worker::busy(app.world()),
        "Dropping outside the strip cancels"
    );
    assert_eq!(
        fixture.engine.document_snapshot().features,
        reordered.features
    );
    for (start_x, outside_x) in [(550., 100.), (370., 1100.), (640., 100.), (640., 1100.)] {
        for (x, state) in [
            (start_x, Some(Pressed)),
            (outside_x, None),
            (outside_x, Some(Released)),
        ] {
            pointer(
                app.world_mut(),
                &handle,
                &services,
                &gesture(&owner, x, 830., state),
            )
            .unwrap();
        }
        assert!(
            !worker::busy(app.world()),
            "Horizontal escape must not enqueue a feature or rollback mutation"
        );
        assert_eq!(
            fixture.engine.document_snapshot().features,
            reordered.features
        );
        assert_eq!(fixture.engine.document_snapshot().rollback_index, 3);
    }
    pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 370., 830., Some(Pressed)),
    )
    .unwrap();
    pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 590., 830., None),
    )
    .unwrap();
    fixture.rename(&owner, "Changed during drag").unwrap();
    assert!(pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 590., 830., Some(Released))
    )
    .is_err());
    assert!(!worker::busy(app.world()));
    assert_eq!(
        fixture.engine.document_snapshot().features,
        reordered.features
    );
    publish_history(
        app.world_mut(),
        &handle,
        &owner,
        &fixture.engine.document_snapshot(),
    );
    pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 640., 830., Some(Pressed)),
    )
    .unwrap();
    pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 400., 830., None),
    )
    .unwrap();
    assert_eq!(fixture.engine.document_snapshot().rollback_index, 3);
    pointer(
        app.world_mut(),
        &handle,
        &services,
        &gesture(&owner, 400., 830., Some(Released)),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.engine.document_snapshot().rollback_index, 1);
    assert_eq!(
        fixture.engine.document_snapshot().features,
        reordered.features
    );
    assert!(fixture.engine.viewport_snapshot().2.bodies.is_empty());
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().rollback_index, 0);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, true, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().rollback_index, 1);
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    mutation(
        app.world_mut(),
        receipt,
        "solid_set_rollback",
        json!({"rollback_index":3}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.engine.document_snapshot().rollback_index, 3);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
}
#[test]
fn rollback_retains_features_and_delete_is_undoable_with_exact_owner_and_revision() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = solid(&fixture);
    let original = fixture.engine.document_snapshot().features;
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    worker::install(
        app.world_mut(),
        services.clone(),
        NativeInterfaceHandle::new(|| {}),
    )
    .unwrap();
    for index in [0, original.len()] {
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        mutation(
            app.world_mut(),
            receipt,
            "solid_set_rollback",
            json!({"rollback_index":index}),
        )
        .unwrap();
        drain(app.world_mut(), &services).unwrap();
        assert_eq!(fixture.engine.document_snapshot().features, original);
        assert_eq!(fixture.engine.document_snapshot().rollback_index, index);
        assert_eq!(
            fixture.engine.viewport_snapshot().2.bodies.len(),
            usize::from(index > 0)
        );
    }
    let stale = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    fixture.rename(&owner, "Changed while menu open").unwrap();
    mutation(
        app.world_mut(),
        stale,
        "solid_delete_feature",
        json!({"feature_id":original[1].id.0}),
    )
    .unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert_eq!(fixture.engine.document_snapshot().features, original);
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    mutation(
        app.world_mut(),
        receipt,
        "solid_delete_feature",
        json!({"feature_id":original[1].id.0}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.engine.document_snapshot().features.len(), 1);
    assert!(fixture.engine.viewport_snapshot().2.bodies.is_empty());
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().features, original);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    let restored = fixture.owner();
    assert_ne!(
        owner, restored,
        "Undo must retire stale controls and in-flight owners"
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &restored, true, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().features.len(), 1);
    let redone = fixture.owner();
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &redone, false, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().features, original);
}
