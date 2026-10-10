use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

#[test]
fn long_browser_names_clip_before_actions_at_multiple_scales() {
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::text::TextLayoutInfo;
    use bevy::ui::{CalculatedClip, UiGlobalTransform};

    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let document_name =
        "Garden bench / named stock, face laps and referenced joinery / review assembly";
    let sketch_name =
        "Left rear leg and back post / 65 x 65 x 790 mm stock from A-B-C / mounting faces";
    for (operation, arguments) in [
        ("cad_set_document_name", json!({"name":document_name})),
        (
            "sketch_begin",
            json!({"name":sketch_name,"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap();
    }
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    app.add_schedule(Schedule::new(PostUpdate))
        .add_plugins((bevy::text::TextPlugin, bevy::ui::UiPlugin))
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<bevy::image::TextureAtlasLayout>>()
        .init_resource::<Time<bevy::time::Real>>()
        .init_resource::<bevy::input_focus::InputFocus>();
    app.world_mut().spawn((
        Camera2d,
        InterfaceCamera,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(1600, 1200),
                    scale_factor: 1.,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(1600, 1200),
                ..default()
            }),
            ..default()
        },
    ));
    app.world_mut()
        .run_system_once(crate::native_viewport::ui::load_system_font)
        .unwrap();
    let mut scroll = 0.;
    for (width, scale) in [(168., 1.), (240., 1.5), (360., 2.)] {
        app.world_mut().resource_mut::<bevy::ui::UiScale>().0 = scale;
        for _ in 0..2 {
            synchronize(
                app.world_mut(),
                &services,
                &owner,
                2,
                InterfaceRect {
                    x: 0.,
                    y: 120.,
                    width,
                    height: 440.,
                },
                &mut scroll,
            )
            .unwrap();
            app.world_mut().run_schedule(PostUpdate);
        }
        let world = app.world_mut();
        let row = world
            .query::<(Entity, &InterfaceControl)>()
            .iter(world)
            .find(|(_, control)| control.label == sketch_name)
            .unwrap()
            .0;
        let label = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .find(|(_, text)| text.0 == sketch_name)
            .unwrap()
            .0;
        let layout = world.get::<TextLayoutInfo>(label).unwrap();
        assert!(
            !layout.glyphs.is_empty(),
            "The regression must shape actual text"
        );
        assert!(
            layout.size.x > width as f32 * scale,
            "Use a name that actually overflows"
        );
        let clip = world.get::<CalculatedClip>(label).unwrap().clone();
        for action in [format!("Edit {sketch_name}"), format!("Hide {sketch_name}")] {
            let center = world
                .query::<(&InterfaceControl, &UiGlobalTransform)>()
                .iter(world)
                .find(|(control, _)| control.label == action)
                .unwrap()
                .1
                .translation;
            assert!(
                !clip.contains_point(center),
                "The caption paints over {action}"
            );
        }
        let row_center = world.get::<UiGlobalTransform>(row).unwrap().translation;
        assert!(
            clip.contains_point(Vec2::new(90. * scale, row_center.y)),
            "The caption must retain a visible region"
        );
        let title = world.resource::<Browser>().labels["document"];
        let units = world.resource::<Browser>().labels["units"];
        let units_center = world.get::<UiGlobalTransform>(units).unwrap().translation;
        assert!(
            !world
                .get::<CalculatedClip>(title)
                .unwrap()
                .contains_point(units_center),
            "The document title paints over its units"
        );
        let title_layout = world.get::<TextLayoutInfo>(title).unwrap();
        assert!(!title_layout.glyphs.is_empty());
    }
    let containers: Vec<_> = app
        .world()
        .resource::<Browser>()
        .text_boxes
        .values()
        .copied()
        .collect();
    hide(app.world_mut());
    assert!(containers
        .iter()
        .all(|entity| app.world().get_entity(*entity).is_err()));
    assert!(app.world().resource::<Browser>().text_boxes.is_empty());
    assert!(app
        .world_mut()
        .query::<&Text>()
        .iter(app.world())
        .all(|text| text.0 != sketch_name && text.0 != document_name));
}

#[test]
fn entering_sketch_reveals_its_browser_row_without_overriding_later_collapse() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    world.spawn(InterfaceCamera);
    let bounds = InterfaceRect {
        x: 0.,
        y: 120.,
        width: 232.,
        height: 700.,
    };
    let mut scroll = 0.;
    synchronize(world, &services, &owner, 0, bounds, &mut scroll).unwrap();
    crate::session_bridge::parse_engine_envelope(
        fixture
            .engine
            .engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#),
    )
    .unwrap();
    synchronize(world, &services, &owner, 1, bounds, &mut scroll).unwrap();
    assert!(world
        .query::<&InterfaceControl>()
        .iter(world)
        .any(|c| c.label == "Sketch1"));
    let folder = world
        .resource::<Browser>()
        .document
        .as_ref()
        .unwrap()
        .browser
        .iter()
        .find(|n| n.kind == Kind::SketchesFolder)
        .unwrap()
        .id
        .0;
    assert!(!world.resource::<Browser>().collapsed.contains(&folder));
    world.resource_mut::<Browser>().collapsed.insert(folder);
    crate::session_bridge::parse_engine_envelope(
        fixture
            .engine
            .engine_call("add_point", r#"{"position":{"x":20.0,"y":30.0}}"#),
    )
    .unwrap();
    synchronize(world, &services, &owner, 2, bounds, &mut scroll).unwrap();
    assert!(world.resource::<Browser>().collapsed.contains(&folder));
    assert!(!world
        .query::<&InterfaceControl>()
        .iter(world)
        .any(|c| c.label == "Sketch1"));
}

#[test]
fn face_sketch_origin_cancel_and_revisions_preserve_the_support_contract() {
    use crate::native_editor::{execute, support, EditorCommand};
    use limo_cad_sketch::FaceSketchOrigin;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    for (op, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":10.,"y":20.},"p2":{"x":40.,"y":60.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":12.}}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
            .unwrap();
    }
    let face = fixture.engine.viewport_snapshot().2.bodies[0]
        .faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| p.normal[2] > 0.99))
        .unwrap()
        .id;
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
    let set_face = |world: &mut World| {
        let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
        view.selected_face_ids = vec![face.0];
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view)).unwrap();
    };
    let command = |world: &mut World, c| {
        execute(
            world,
            &fixture.engine,
            &fixture.bridge,
            &owner,
            c,
            || Ok(()),
        )
    };
    let drain = |world: &mut World| {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(outcome) = worker::poll(world, &services) {
                outcome.value.unwrap();
                if !worker::busy(world) {
                    break;
                }
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    set_face(app.world_mut());
    let original = fixture.engine.document_snapshot().features;
    command(
        app.world_mut(),
        EditorCommand::Support(support::Command::Start),
    )
    .unwrap();
    assert_eq!(support::modal(app.world()), Some("sketch-origin"));
    command(app.world_mut(), EditorCommand::Cancel).unwrap();
    assert!(support::modal(app.world()).is_none());
    assert_eq!(fixture.engine.document_snapshot().features, original);
    assert_eq!(
        native_viewport::interface_view_snapshot(app.world()).2.mode,
        native_viewport::ViewportMode::Solid
    );
    for (origin, expected) in [
        (FaceSketchOrigin::FaceCenter, [25., 40., 12.]),
        (FaceSketchOrigin::GlobalOriginProjection, [0., 0., 12.]),
    ] {
        set_face(app.world_mut());
        command(
            app.world_mut(),
            EditorCommand::Support(support::Command::Start),
        )
        .unwrap();
        command(
            app.world_mut(),
            EditorCommand::Support(support::Command::Origin(origin)),
        )
        .unwrap();
        command(
            app.world_mut(),
            EditorCommand::Support(support::Command::Confirm),
        )
        .unwrap();
        drain(app.world_mut());
        let sketch = crate::native_editor::active(&fixture.engine)
            .unwrap()
            .unwrap();
        assert_eq!(
            sketch.plane,
            limo_cad_core::PlaneRef::PlanarFace { face_id: face }
        );
        for (actual, expected) in sketch.basis.origin.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6, "{sketch:?}");
        }
        command(app.world_mut(), EditorCommand::Finish).unwrap();
        drain(app.world_mut());
    }
    set_face(app.world_mut());
    command(
        app.world_mut(),
        EditorCommand::Support(support::Command::Start),
    )
    .unwrap();
    fixture.rename(&owner, "Revised support").unwrap();
    assert!(command(
        app.world_mut(),
        EditorCommand::Support(support::Command::Confirm)
    )
    .is_err());
    assert!(crate::native_editor::active(&fixture.engine)
        .unwrap()
        .is_none());
    assert!(!support::picking(app.world()));
}

#[test]
fn finishing_an_existing_sketch_rebuilds_its_solid_without_duplicating_history() {
    use crate::native_editor::{execute, EditorCommand};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    for (op, args) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_circle",
            json!({"mode":"center_diameter","p1":{"x":5.,"y":5.},"p2":{"x":15.,"y":5.},"ctrl_held":true}),
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
    let drain = |world: &mut World| {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(outcome) = worker::poll(world, &services) {
                let result = outcome.value.unwrap();
                assert!(result["model_error"].is_null(), "{result}");
                if !worker::busy(world) {
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Native sketch replay timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    execute(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &owner,
        EditorCommand::Edit("Sketch1".into()),
        || Ok(()),
    )
    .unwrap();
    drain(app.world_mut());
    let active = crate::session_bridge::parse_engine_envelope(
        fixture.engine.engine_call("active_sketch", ""),
    )
    .unwrap();
    let entities = active["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_u64())
        .collect::<Vec<_>>();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "sketch_move_copy",
            &json!({"entity_ids":entities,"dx":25.,"dy":0.,"copy":false}),
            || Ok(()),
        )
        .unwrap();
    execute(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &owner,
        EditorCommand::Finish,
        || Ok(()),
    )
    .unwrap();
    drain(app.world_mut());
    assert_eq!(fixture.engine.document_snapshot().features, original);
    let min_x = fixture
        .engine
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .flat_map(|b| b.mesh.positions.as_chunks::<3>().0.iter().map(|p| p[0]))
        .fold(f32::INFINITY, f32::min);
    assert!(
        (min_x - 20.).abs() < 0.1,
        "Dependent solid was not moved with its sketch: {min_x}"
    );
}

#[test]
fn browser_visibility_preserves_other_targets_and_roundtrips_the_project() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    for (op, args) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":12.,"y":8.},"ctrl_held":false}),
        ),
        ("sketch_finish", json!({})),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
            .unwrap();
    }
    let document = fixture.engine.document_snapshot();
    let mut flattened = vec![];
    rows(&document.browser, &HashSet::new(), 0, &mut flattened);
    let sketch = flattened
        .iter()
        .find(|(_, node)| node.kind == Kind::Sketch)
        .unwrap()
        .1;
    let sketch = action_node(&fixture.engine, sketch.id.0).unwrap();
    let name = sketch.name.as_ref().unwrap();
    let first = visibility_arguments(&fixture.engine, &sketch).unwrap();
    assert_eq!(first["hidden_sketch_names"], json!([name]));
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "project_set_visibility",
            &first,
            || Ok(()),
        )
        .unwrap();
    let second = visibility_arguments(&fixture.engine, &sketch).unwrap();
    assert_eq!(second["hidden_sketch_names"], json!([]));
    assert_eq!(first["hidden_body_ids"], second["hidden_body_ids"]);
    assert_eq!(
        first["hidden_datum_plane_ids"],
        second["hidden_datum_plane_ids"]
    );
    let archive = crate::session_bridge::parse_engine_envelope(
        fixture.engine.engine_call("project_export_model", ""),
    )
    .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "cad_load_project_model",
            &json!({"model_json":archive.as_str().unwrap()}),
            || Ok(()),
        )
        .unwrap();
    let restored = crate::session_bridge::parse_engine_envelope(
        fixture.engine.engine_call("project_visibility", ""),
    )
    .unwrap();
    assert_eq!(restored, first);
}

#[test]
fn collapsed_folders_never_expose_hidden_children_as_rows() {
    let parent =
        BrowserNode::new(limo_cad_core::NodeId(2), Kind::SketchesFolder).with_children(vec![
            BrowserNode::new(limo_cad_core::NodeId(3), Kind::Sketch).named("Profile"),
        ]);
    let nodes = [parent];
    let mut visible = vec![];
    rows(&nodes, &HashSet::from([2]), 0, &mut visible);
    assert_eq!(visible.len(), 1);
    visible.clear();
    rows(&nodes, &HashSet::new(), 0, &mut visible);
    assert_eq!(
        visible
            .iter()
            .map(|(depth, n)| (*depth, n.id.0))
            .collect::<Vec<_>>(),
        vec![(0, 2), (1, 3)]
    );
}
