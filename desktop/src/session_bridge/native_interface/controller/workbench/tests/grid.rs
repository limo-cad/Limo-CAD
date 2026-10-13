use super::*;
use crate::native_editor::{self, EditorCommand, PaletteCommand};
use crate::native_viewport::{ViewportMode, ViewportPresentation};
use bevy::ui::ComputedStackIndex;
use limo_cad_interface::{ControlKey, Field};

#[test]
fn viewport_grid_button_activates_and_rejects_a_mismatched_rendered_document() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let (_, handle, _, _) = interface_shell::tests::fixture();
    let mut frame = handle.frame().unwrap();
    frame.context = owner.clone();
    handle.present(frame).unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    native_viewport::apply_interface_model(world, model_snapshot(&fixture.engine)).unwrap();
    world.init_resource::<Assets<Image>>();
    let camera = world.spawn(InterfaceCamera).id();
    let mut state = Workbench::default();
    state.widgets.begin();
    viewport::synchronize(
        world,
        camera,
        &HashMap::new(),
        1360.,
        860.,
        232.,
        &mut state,
    )
    .unwrap();
    state.widgets.finish(world);
    let grid = state.widgets.entity("nav-grid").unwrap();
    let display = state.widgets.entity("nav-display").unwrap();
    assert!(world.get::<InterfaceControl>(display).unwrap().disabled);
    assert!(!world.get::<InterfaceControl>(grid).unwrap().disabled);
    assert_eq!(
        world.get::<InterfaceControl>(grid).unwrap().selected,
        Some(true)
    );
    world.entity_mut(grid).insert((
        ComputedNode {
            size: Vec2::new(26., 24.),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_translation(Vec2::new(140., 140.)),
        ComputedStackIndex(1),
        InheritedVisibility::VISIBLE,
    ));
    interface_shell::tests::publish_layout_once(world, handle.clone());
    let original_camera = native_viewport::interface_view(world).1;
    let original_model = fixture.engine.engine_call("project_export_model", "");
    let revision = native_viewport::interface_model_revision(world);
    for visible in [false, true] {
        let action = handle.resolve_retained(ControlKey(grid.to_bits())).unwrap();
        let result = crate::session_bridge::native_interface::reduce_action(
            &fixture.engine,
            &fixture.bridge,
            world,
            &handle,
            &action,
        )
        .unwrap();
        assert_eq!(result["grid_visible"], visible);
        state.widgets.begin();
        viewport::synchronize(
            world,
            camera,
            &HashMap::new(),
            1360.,
            860.,
            232.,
            &mut state,
        )
        .unwrap();
        state.widgets.finish(world);
        assert_eq!(
            world.get::<InterfaceControl>(grid).unwrap().selected,
            Some(visible)
        );
        assert_eq!(
            native_viewport::interface_view(world).2.grid_visible(),
            visible
        );
        assert_eq!(native_viewport::interface_view(world).1, original_camera);
        assert_eq!(native_viewport::interface_model_revision(world), revision);
        assert_eq!(
            fixture.engine.engine_call("project_export_model", ""),
            original_model
        );
        interface_shell::tests::publish_layout_once(world, handle.clone());
    }
    let action = handle.resolve_retained(ControlKey(grid.to_bits())).unwrap();
    let mut replacement = model_snapshot(&fixture.engine);
    replacement.session_id = "different-rendered-document".into();
    native_viewport::apply_interface_model(world, replacement).unwrap();
    let before = native_viewport::interface_view(world).2.clone();
    let error = crate::session_bridge::native_interface::reduce_action(
        &fixture.engine,
        &fixture.bridge,
        world,
        &handle,
        &action,
    )
    .unwrap_err();
    assert!(
        error.contains("rendered document is not current"),
        "{error}"
    );
    assert_eq!(native_viewport::interface_view(world).2, &before);
}

#[test]
fn viewport_grid_and_sketch_palette_share_visibility_without_changing_snap_or_geometry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    for (operation, arguments) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":12.},"ctrl_held":false}),
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
    let world = app.world_mut();
    native_viewport::apply_interface_model(world, model_snapshot(&fixture.engine)).unwrap();
    native_viewport::apply_interface_view(
        world,
        &owner.document_id,
        None,
        Some(ViewportPresentation {
            mode: ViewportMode::Sketch,
            hide_ground_grid: true,
            ..default()
        }),
    )
    .unwrap();
    world.init_resource::<Assets<Image>>();
    world.spawn(InterfaceCamera);
    let handle = NativeInterfaceHandle::new(|| {});
    let area = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1360.,
        height: 860.,
    };
    let before = fixture.engine.engine_call("active_sketch", "");
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    for (toolbar, visible) in [(true, false), (false, true), (false, false), (true, true)] {
        if toolbar {
            execute(world, &Command::ToggleGrid).unwrap();
        } else {
            native_editor::execute(
                world,
                &fixture.engine,
                &fixture.bridge,
                &owner,
                EditorCommand::Palette(PaletteCommand::Grid),
                || Ok(()),
            )
            .unwrap();
        }
        native_editor::synchronize_controls(world, &handle, &services, &owner, area, area).unwrap();
        let grid = world
            .query::<&InterfaceControl>()
            .iter(world)
            .find(|c| c.label == "Sketch Grid")
            .unwrap();
        assert_eq!(grid.field, Field::Toggle(visible));
        let view = native_viewport::interface_view(world).2;
        assert_eq!(view.grid_visible(), visible);
        assert!(
            view.hide_ground_grid,
            "Sketch controls must retain the solid grid choice"
        );
        assert_eq!(fixture.engine.engine_call("active_sketch", ""), before);
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &owner)
                .unwrap(),
            receipt
        );
    }
}
