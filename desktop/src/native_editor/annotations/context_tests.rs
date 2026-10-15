use super::*;
use crate::native_viewport::interface_shell::{InterfaceFrame, PointerButton, PointerPhase};
use bevy::ui::ComputedStackIndex;
use limo_cad_interface::{Canvas, ControlInput, ControlKey, KeyChord, Surface};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

struct SessionDirectory(std::path::PathBuf);
impl Drop for SessionDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn pixels(value: Val) -> f32 {
    if let Val::Px(value) = value {
        value
    } else {
        0.
    }
}

// Supply layout output to the real pointer adapter without launching a GUI.
fn layout(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    owner: &DocumentContext,
    canvas: InterfaceRect,
) {
    let mut query = world.query::<(Entity, &Node, Option<&ZIndex>, Option<&UiTransform>)>();
    let items: Vec<_> = query
        .iter(world)
        .map(|(entity, node, z, transform)| {
            let size = Vec2::new(pixels(node.width), pixels(node.height));
            let center = Vec2::new(pixels(node.left), pixels(node.top)) + size * 0.5;
            (
                entity,
                size,
                center,
                z.map_or(0, |z| z.0),
                transform.map_or(Rot2::IDENTITY, |t| t.rotation),
            )
        })
        .collect();
    for (entity, size, center, z, rotation) in items {
        world.entity_mut(entity).insert((
            ComputedNode {
                size,
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from(bevy::math::Affine2::from_scale_angle_translation(
                Vec2::ONE,
                rotation.as_radians(),
                center,
            )),
            ComputedStackIndex(z.max(0) as u32),
            InheritedVisibility::VISIBLE,
        ));
    }
    let client = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1000.,
        height: 800.,
    };
    handle
        .present(InterfaceFrame {
            context: owner.clone(),
            client,
            surface: client,
            canvases: vec![Canvas {
                name: "viewport".into(),
                bounds: canvas,
            }],
            surfaces: panel::modal(world)
                .map(|name| Surface {
                    name: name.into(),
                    text: None,
                })
                .into_iter()
                .collect(),
            modal_stack: panel::modal(world).map(str::to_owned).into_iter().collect(),
            document_visible: true,
        })
        .unwrap();
    interface_shell::tests::publish_layout_once(world, handle.clone());
}

fn reduce(
    world: &mut World,
    services: &NativeServices,
    handle: &NativeInterfaceHandle,
    key: ControlKey,
    input: ControlInput,
    owner: &DocumentContext,
) -> Result<Value, String> {
    let action = handle.resolve_input(key, input, owner)?;
    crate::session_bridge::native_interface::reduce_action(
        &services.engine,
        &services.bridge,
        world,
        handle,
        &action,
    )
}

#[test]
fn annotation_context_menu_deletes_only_its_target_and_supports_undo_redo() {
    crate::session_bridge::with_isolated_session_test(|| {
        let directory = SessionDirectory(
            std::env::temp_dir().join(format!("sketch-annotation-menu-{}", uuid::Uuid::new_v4())),
        );
        std::env::set_var("LIMO_CAD_SESSION_DIR", &directory.0);
        for dimension in [true, false] {
            let engine = Arc::new(AppState::new());
            let bridge = Arc::new(SessionBridgeState::default());
            crate::session_bridge::parse_engine_envelope(bridge.with_project_session_transition(
                "main",
                &engine,
                || engine.bind_project_session("annotation-test"),
            ))
            .unwrap();
            let owner = bridge.native_document_context("main", &engine).unwrap();
            let services = NativeServices {
                engine: engine.clone(),
                bridge: bridge.clone(),
            };
            let mutate = |op, args: Value| {
                bridge
                    .apply_native_mutation(&engine, &owner, op, &args, || Ok(()))
                    .unwrap()
            };
            mutate("sketch_begin", json!({"type":"origin_plane","plane":"xy"}));
            mutate(
                "sketch_add_line",
                json!({"from":{"x":0.,"y":0.},"to_raw":{"x":20.,"y":0.},"ctrl_held":true}),
            );
            let sketch = active(&engine).unwrap().unwrap();
            let line = sketch
                .entities
                .iter()
                .find(|e| matches!(e, EntityDto::Line { .. }))
                .unwrap()
                .id();
            mutate(
                "sketch_add_constraints",
                json!({"constraints":[{"type":"horizontal","entity":line}]}),
            );
            mutate(
                "sketch_add_dimension",
                json!({"entities":[line],"text_pos":{"x":10.,"y":-4.}}),
            );
            let before = active(&engine).unwrap().unwrap();
            let target = if dimension {
                AnnotationTarget::Dimension(before.dimensions[0].constraint_id)
            } else {
                AnnotationTarget::Constraint(
                    before
                        .constraints
                        .iter()
                        .find(|c| c.constraint.kind() == ConstraintKind::Geometric)
                        .unwrap()
                        .id,
                )
            };
            let mut app = native_viewport::interface_scene_fixture();
            let world = app.world_mut();
            world.init_resource::<Assets<Image>>();
            world.spawn(InterfaceCamera);
            let handle = NativeInterfaceHandle::new(|| {});
            worker::install(world, services.clone(), handle.clone()).unwrap();
            native_viewport::apply_interface_edit_model(
                world,
                crate::session_bridge::native_interface::model_snapshot(&engine),
            )
            .unwrap();
            let canvas = InterfaceRect {
                x: 0.,
                y: 90.,
                width: 1000.,
                height: 650.,
            };
            let ribbon = InterfaceRect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 80.,
            };
            native_viewport::apply_interface_viewport(world, canvas, 1.).unwrap();
            native_viewport::apply_interface_view(
                world,
                &owner.document_id,
                Some(ViewportCamera {
                    position: [10., 0., 100.],
                    target: [10., 0., 0.],
                    up: [0., 1., 0.],
                    vertical_fov_degrees: 45.,
                }),
                None,
            )
            .unwrap();
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            let widget = match target {
                AnnotationTarget::Dimension(id) => format!("dim-{}", id.0),
                AnnotationTarget::Constraint(id) => format!("constraint-{}", id.0),
            };
            let entity = world
                .resource::<AnnotationState>()
                .widgets
                .entity(&widget)
                .unwrap();
            let key = ControlKey(entity.to_bits());
            let bounds = handle
                .read_surface(|_, frame| {
                    frame.controls.iter().find(|c| c.key == key).unwrap().bounds
                })
                .unwrap();
            let point = [
                bounds.x + bounds.width * 0.5,
                bounds.y + bounds.height * 0.5,
            ];
            assert!(handle
                .pointer(PointerPhase::Down, point, PointerButton::Secondary)
                .unwrap());
            assert!(handle
                .pointer(PointerPhase::Up, point, PointerButton::Secondary)
                .unwrap());
            let actions = handle.take_actions().unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].control.input, ControlInput::ContextMenu);
            crate::session_bridge::native_interface::reduce_action(
                &engine,
                &bridge,
                world,
                &handle,
                &actions[0],
            )
            .unwrap();
            assert_eq!(
                world
                    .resource::<Editor>()
                    .interaction
                    .annotation_menu
                    .unwrap()
                    .target,
                target
            );
            assert!(world.resource::<Editor>().interaction.dimension.is_none());
            assert_eq!(active(&engine).unwrap().unwrap(), before);
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            panel::escape(world);
            assert!(world
                .resource::<Editor>()
                .interaction
                .annotation_menu
                .is_none());
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            reduce(
                world,
                &services,
                &handle,
                key,
                ControlInput::Key(KeyChord {
                    shift: true,
                    ..KeyChord::plain("F10")
                }),
                &owner,
            )
            .unwrap();
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            let delete = world
                .query::<(Entity, &InterfaceControl)>()
                .iter(world)
                .find(|(_, c)| c.surface == "sketch/annotation-menu")
                .map(|(e, _)| ControlKey(e.to_bits()))
                .unwrap();
            reduce(
                world,
                &services,
                &handle,
                delete,
                ControlInput::Click,
                &owner,
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(outcome) = worker::poll(world, &services) {
                    outcome.value.unwrap();
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            let after = active(&engine).unwrap().unwrap();
            assert_eq!(after.entities, before.entities);
            assert_eq!(after.constraints.len(), before.constraints.len() - 1);
            assert_eq!(
                after.dimensions.len(),
                before.dimensions.len() - usize::from(dimension)
            );
            assert!(world
                .resource::<Editor>()
                .interaction
                .annotation_menu
                .is_none());
            bridge
                .apply_native_history(&engine, &owner, false, || Ok(()))
                .unwrap();
            let undo = active(&engine).unwrap().unwrap();
            assert_eq!(undo.entities, before.entities);
            assert_eq!(undo.constraints, before.constraints);
            assert_eq!(undo.dimensions, before.dimensions);
            bridge
                .apply_native_history(&engine, &owner, true, || Ok(()))
                .unwrap();
            assert_eq!(
                active(&engine).unwrap().unwrap().constraints,
                after.constraints
            );
            assert!(reduce(
                world,
                &services,
                &handle,
                delete,
                ControlInput::Click,
                &owner
            )
            .is_err());
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            assert!(handle
                .resolve_input(delete, ControlInput::Click, &owner)
                .is_err());
        }
    });
}

#[test]
fn inline_dimension_pick_refines_measurement_and_enter_applies_one_undoable_change() {
    crate::session_bridge::with_isolated_session_test(|| {
        let directory = SessionDirectory(
            std::env::temp_dir().join(format!("sketch-inline-dimension-{}", uuid::Uuid::new_v4())),
        );
        std::env::set_var("LIMO_CAD_SESSION_DIR", &directory.0);
        let engine = Arc::new(AppState::new());
        let bridge = Arc::new(SessionBridgeState::default());
        crate::session_bridge::parse_engine_envelope(bridge.with_project_session_transition(
            "main",
            &engine,
            || engine.bind_project_session("inline-dimension-test"),
        ))
        .unwrap();
        let owner = bridge.native_document_context("main", &engine).unwrap();
        let services = NativeServices {
            engine: engine.clone(),
            bridge: bridge.clone(),
        };
        let mutate = |op, args: Value| {
            bridge
                .apply_native_mutation(&engine, &owner, op, &args, || Ok(()))
                .unwrap()
        };
        mutate("sketch_begin", json!({"type":"origin_plane","plane":"xy"}));
        for (a, b) in [
            ([0., 0.], [20., 0.]),
            ([0., 10.], [20., 10.]),
            ([30., 0.], [30., 20.]),
        ] {
            mutate(
                "sketch_add_line",
                json!({"from":{"x":a[0],"y":a[1]},"to_raw":{"x":b[0],"y":b[1]},"ctrl_held":true}),
            );
        }
        let before = active(&engine).unwrap().unwrap();
        let revision = engine.geometry_revision();
        let model = engine.engine_call("project_export_model", "");
        let mut app = native_viewport::interface_scene_fixture();
        app.add_plugins(bevy::text::TextPlugin);
        let world = app.world_mut();
        world.init_resource::<Assets<Image>>();
        world.spawn(InterfaceCamera);
        let handle = NativeInterfaceHandle::new(|| {});
        worker::install(world, services.clone(), handle.clone()).unwrap();
        world.insert_resource(handle.clone());
        world.init_resource::<bevy::input_focus::InputFocus>();
        native_viewport::apply_interface_edit_model(
            world,
            crate::session_bridge::native_interface::model_snapshot(&engine),
        )
        .unwrap();
        let canvas = InterfaceRect {
            x: 0.,
            y: 90.,
            width: 1000.,
            height: 650.,
        };
        let ribbon = InterfaceRect {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 80.,
        };
        native_viewport::apply_interface_viewport(world, canvas, 1.).unwrap();
        native_viewport::apply_interface_view(
            world,
            &owner.document_id,
            Some(ViewportCamera {
                position: [10., 10., 100.],
                target: [10., 10., 0.],
                up: [0., 1., 0.],
                vertical_fov_degrees: 45.,
            }),
            None,
        )
        .unwrap();
        synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
        crate::native_editor::execute(
            world,
            &engine,
            &bridge,
            &owner,
            EditorCommand::Interaction(InteractionCommand::Dimension),
            || Ok(()),
        )
        .unwrap();
        let pick = |world: &mut World, point: Point| {
            handle.blur();
            interface_shell::fields::after_window_input(world, &handle).unwrap();
            let pixel = native_viewport::interface_world_point(
                world,
                &owner.document_id,
                before.basis.to_3d([point.x, point.y]),
            )
            .unwrap()
            .unwrap();
            let pixel = Vec2::new(pixel[0] + canvas.x as f32, pixel[1] + canvas.y as f32);
            world.resource_scope(|world, mut editor: Mut<Editor>| {
                interaction::pointer(
                    world,
                    (&services, &owner),
                    &mut editor,
                    (pixel, pixel, canvas),
                    false,
                    false,
                )
                .unwrap();
            });
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            world
                .run_system_cached(interface_shell::fields::synchronize_fields)
                .unwrap();
            interface_shell::fields::apply_requested_focus(world);
            let field = world
                .query::<(Entity, &InterfaceControl)>()
                .iter(world)
                .find(|(_, c)| c.label == "Dimension value")
                .map(|(e, _)| e)
                .unwrap();
            assert_eq!(handle.focused_key(), Some(ControlKey(field.to_bits())));
            let value = world.get::<bevy::text::EditableText>(field).unwrap();
            assert_eq!(
                value.editor.raw_selection().text_range(),
                0..value.value().to_string().len()
            );
        };
        let hover = |world: &mut World, point: Point, expected: Point| {
            let pixel = native_viewport::interface_world_point(
                world,
                &owner.document_id,
                before.basis.to_3d([point.x, point.y]),
            )
            .unwrap()
            .unwrap();
            let pixel = Vec2::new(pixel[0] + canvas.x as f32, pixel[1] + canvas.y as f32);
            world.resource_scope(|world, mut editor: Mut<Editor>| {
                interaction::hover(world, &services, &owner, &mut editor, Some(pixel), canvas)
                    .unwrap();
            });
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            world
                .run_system_cached(interface_shell::fields::synchronize_fields)
                .unwrap();
            interface_shell::fields::apply_requested_focus(world);
            let position = world
                .resource::<Editor>()
                .interaction
                .dimension_position
                .unwrap();
            assert!(
                position.distance(expected) < 1e-5,
                "{position:?} != {expected:?}"
            );
            assert_eq!(
                world
                    .resource::<Editor>()
                    .interaction
                    .dimension_preview
                    .as_ref()
                    .unwrap()
                    .text_pos,
                position
            );
        };
        pick(world, Point::new(10., 0.));
        assert_eq!(
            world.resource::<Editor>().interaction.dimension.as_deref(),
            Some("20")
        );
        assert!(world
            .resource::<Editor>()
            .interaction
            .dimension_position
            .is_some());
        let first_generation = world.resource::<Editor>().form_serial;
        let first_field = handle.focused_key().unwrap();
        let first_node = world
            .get::<Node>(Entity::from_bits(first_field.0))
            .unwrap()
            .clone();
        reduce(
            world,
            &services,
            &handle,
            first_field,
            ControlInput::SetValue("23".into()),
            &owner,
        )
        .unwrap();
        hover(world, Point::new(15., 4.), Point::new(10., 4.));
        hover(world, Point::new(-30., 4.), Point::new(10., 4.));
        assert_eq!(world.resource::<Editor>().form_serial, first_generation);
        assert_eq!(handle.focused_key(), Some(first_field));
        assert_ne!(
            world.get::<Node>(Entity::from_bits(first_field.0)).unwrap(),
            &first_node
        );
        assert_eq!(
            world.resource::<Editor>().interaction.dimension.as_deref(),
            Some("23")
        );
        assert_eq!(
            world
                .get::<bevy::text::EditableText>(Entity::from_bits(first_field.0))
                .unwrap()
                .value()
                .to_string(),
            "23"
        );
        pick(world, Point::new(10., 10.));
        assert_eq!(
            world.resource::<Editor>().interaction.dimension.as_deref(),
            Some("10")
        );
        assert!(world.resource::<Editor>().form_serial > first_generation);
        assert_eq!(
            world
                .resource::<Editor>()
                .interaction
                .dimension_preview
                .as_ref()
                .unwrap()
                .kind,
            "distance"
        );
        assert_eq!(engine.geometry_revision(), revision);
        assert_eq!(engine.engine_call("project_export_model", ""), model);
        let field = world
            .query::<(Entity, &InterfaceControl)>()
            .iter(world)
            .find(|(_, control)| control.label == "Dimension value")
            .map(|(entity, _)| entity)
            .unwrap();
        let control = world.get::<InterfaceControl>(field).unwrap();
        assert!(
            matches!(&control.field, limo_cad_interface::Field::Text { value, read_only: false, .. } if value == "10")
        );
        assert!(control.owned_keys.contains(&KeyChord::plain("Enter")));
        assert!(!world
            .query::<&InterfaceControl>()
            .iter(world)
            .any(|c| c.label == "Apply Dimension"));
        let key = ControlKey(field.to_bits());
        reduce(
            world,
            &services,
            &handle,
            key,
            ControlInput::SetValue("6".into()),
            &owner,
        )
        .unwrap();
        let second_generation = world.resource::<Editor>().form_serial;
        hover(world, Point::new(14., 4.), Point::new(14., 5.));
        hover(world, Point::new(14., 40.), Point::new(14., 5.));
        assert_eq!(world.resource::<Editor>().form_serial, second_generation);
        assert_eq!(handle.focused_key(), Some(key));
        assert_eq!(
            world.resource::<Editor>().interaction.dimension.as_deref(),
            Some("6")
        );
        assert_eq!(engine.geometry_revision(), revision);
        assert_eq!(engine.engine_call("project_export_model", ""), model);
        reduce(
            world,
            &services,
            &handle,
            key,
            ControlInput::Key(KeyChord::plain("Enter")),
            &owner,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(outcome) = worker::poll(world, &services) {
                outcome.value.unwrap();
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let after = active(&engine).unwrap().unwrap();
        assert_eq!(after.dimensions.len(), 1);
        assert!((after.dimensions[0].value - 6.).abs() < 1e-8);
        assert!(after.dimensions[0].text_pos.distance(Point::new(14., 5.)) < 1e-5);
        assert!(world.resource::<Editor>().interaction.dimension.is_none());
        bridge
            .apply_native_history(&engine, &owner, false, || Ok(()))
            .unwrap();
        assert_eq!(active(&engine).unwrap().unwrap().entities, before.entities);
        assert!(active(&engine).unwrap().unwrap().dimensions.is_empty());

        bridge
            .apply_native_history(&engine, &owner, true, || Ok(()))
            .unwrap();
        crate::native_editor::execute(
            world,
            &engine,
            &bridge,
            &owner,
            EditorCommand::Interaction(InteractionCommand::Dimension),
            || Ok(()),
        )
        .unwrap();
        let midpoints: Vec<_> = after
            .entities
            .iter()
            .filter_map(|entity| match entity {
                EntityDto::Line { start, end, .. } => Some((*start + *end) * 0.5),
                _ => None,
            })
            .collect();
        pick(world, midpoints[0]);
        pick(world, midpoints[1]);
        assert_eq!(
            world.resource::<Editor>().interaction.dimension_id,
            Some(after.dimensions[0].constraint_id)
        );
        let moved = Point::new(18., (midpoints[0].y + midpoints[1].y) * 0.5);
        hover(world, Point::new(18., 40.), moved);
        assert_eq!(
            active(&engine).unwrap().unwrap().dimensions,
            after.dimensions
        );
        let key = handle.focused_key().unwrap();
        reduce(
            world,
            &services,
            &handle,
            key,
            ControlInput::SetValue("8".into()),
            &owner,
        )
        .unwrap();
        reduce(
            world,
            &services,
            &handle,
            key,
            ControlInput::Key(KeyChord::plain("Enter")),
            &owner,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(outcome) = worker::poll(world, &services) {
                outcome.value.unwrap();
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let edited = active(&engine).unwrap().unwrap();
        assert_eq!(edited.dimensions.len(), 1);
        assert!((edited.dimensions[0].value - 8.).abs() < 1e-8);
        assert!(edited.dimensions[0].text_pos.distance(moved) < 1e-5);
        bridge
            .apply_native_history(&engine, &owner, false, || Ok(()))
            .unwrap();
        let undone = active(&engine).unwrap().unwrap();
        assert_eq!(undone.entities, after.entities);
        assert_eq!(undone.dimensions, after.dimensions);
    });
}

#[test]
fn rectangle_dimension_enter_or_placement_click_accepts_unchanged_and_typed_values() {
    use crate::native_viewport::winit_host::{
        prepare_native_input, HostInputState, NativeHostInput,
    };
    use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
    crate::session_bridge::with_isolated_session_test(|| {
        let directory = SessionDirectory(std::env::temp_dir().join(format!(
            "rectangle-dimension-enter-{}",
            uuid::Uuid::new_v4()
        )));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &directory.0);
        for (height, typed, place) in [
            (20., None, false),
            (20., Some("20"), false),
            (20., Some("21"), false),
            (20., None, true),
            (20., Some("20"), true),
            (20., Some("21"), true),
            (6.53272650918237, None, false),
            (6.53272650918237, None, true),
            (6.53272650918237, Some("6.53272651"), false),
        ] {
            let engine = Arc::new(AppState::new());
            let bridge = Arc::new(SessionBridgeState::default());
            crate::session_bridge::parse_engine_envelope(bridge.with_project_session_transition(
                "main",
                &engine,
                || engine.bind_project_session("rectangle-dimension-test"),
            ))
            .unwrap();
            let owner = bridge.native_document_context("main", &engine).unwrap();
            let services = NativeServices {
                engine: engine.clone(),
                bridge: bridge.clone(),
            };
            for (operation, arguments) in [
                ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
                (
                    "sketch_add_rectangle",
                    json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":height},"ctrl_held":false}),
                ),
            ] {
                bridge
                    .apply_native_mutation(&engine, &owner, operation, &arguments, || Ok(()))
                    .unwrap();
            }
            let before = active(&engine).unwrap().unwrap();
            assert!(before.dimensions.is_empty());
            assert_eq!(before.dof.value, 2);
            let mut app = native_viewport::interface_scene_fixture();
            app.add_plugins(bevy::text::TextPlugin);
            interface_shell::fields::install(&mut app);
            let world = app.world_mut();
            world.init_resource::<Assets<Image>>();
            world.init_resource::<bevy::input_focus::InputFocus>();
            world.init_resource::<HostInputState>();
            world.spawn(InterfaceCamera);
            let handle = NativeInterfaceHandle::new(|| {});
            worker::install(world, services.clone(), handle.clone()).unwrap();
            world.insert_resource(handle.clone());
            native_viewport::apply_interface_edit_model(
                world,
                crate::session_bridge::native_interface::model_snapshot(&engine),
            )
            .unwrap();
            let canvas = InterfaceRect {
                x: 0.,
                y: 90.,
                width: 1000.,
                height: 650.,
            };
            let ribbon = InterfaceRect {
                x: 0.,
                y: 0.,
                width: 1000.,
                height: 80.,
            };
            native_viewport::apply_interface_viewport(world, canvas, 1.).unwrap();
            native_viewport::apply_interface_view(
                world,
                &owner.document_id,
                Some(ViewportCamera {
                    position: [10., 10., 100.],
                    target: [10., 10., 0.],
                    up: [0., 1., 0.],
                    vertical_fov_degrees: 45.,
                }),
                None,
            )
            .unwrap();
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            crate::native_editor::execute(
                world,
                &engine,
                &bridge,
                &owner,
                EditorCommand::Interaction(InteractionCommand::Dimension),
                || Ok(()),
            )
            .unwrap();
            let pixel = native_viewport::interface_world_point(
                world,
                &owner.document_id,
                before.basis.to_3d([0., height * 0.5]),
            )
            .unwrap()
            .unwrap();
            let pixel = Vec2::new(pixel[0] + canvas.x as f32, pixel[1] + canvas.y as f32);
            world.resource_scope(|world, mut editor: Mut<Editor>| {
                interaction::pointer(
                    world,
                    (&services, &owner),
                    &mut editor,
                    (pixel, pixel, canvas),
                    false,
                    false,
                )
                .unwrap();
            });
            synchronize_controls(world, &handle, &services, &owner, ribbon, canvas).unwrap();
            layout(world, &handle, &owner, canvas);
            world
                .run_system_cached(interface_shell::fields::synchronize_fields)
                .unwrap();
            interface_shell::fields::apply_requested_focus(world);
            assert_eq!(
                world.resource::<Editor>().interaction.dimension.as_deref(),
                Some(dimension::value_label(height).as_str())
            );
            assert!(!world.resource::<Editor>().interaction.dimension_reference);
            let key = handle.focused_key().unwrap();
            assert_eq!(
                world
                    .get::<bevy::text::EditableText>(Entity::from_bits(key.0))
                    .unwrap()
                    .editor
                    .raw_selection()
                    .text_range(),
                0..dimension::value_label(height).len()
            );
            assert_eq!(
                world.get::<Node>(Entity::from_bits(key.0)).unwrap().width,
                px(160.)
            );
            let keys = typed
                .into_iter()
                .flat_map(str::chars)
                .map(|c| (Key::Character(c.to_string().into()), Some(c.to_string())))
                .chain(std::iter::once((Key::Enter, None)));
            for (logical_key, text) in keys {
                if logical_key == Key::Enter && place {
                    let at = native_viewport::interface_world_point(
                        world,
                        &owner.document_id,
                        before.basis.to_3d([-6., 15.]),
                    )
                    .unwrap()
                    .unwrap();
                    let at = Vec2::new(at[0] + canvas.x as f32, at[1] + canvas.y as f32);
                    let mut click = NativeHostInput {
                        ui_scale: 1.,
                        context: handle.presented_context(),
                        cursor: Some(at),
                        modifiers: Default::default(),
                        event: WindowEvent::MouseButtonInput(
                            bevy::input::mouse::MouseButtonInput {
                                button: bevy::input::mouse::MouseButton::Left,
                                state: ButtonState::Pressed,
                                window: Entity::PLACEHOLDER,
                            },
                        ),
                        consumed: false,
                        actions: Vec::new(),
                    };
                    prepare_native_input(world, &handle, &mut click).unwrap();
                    for action in click.actions {
                        crate::session_bridge::native_interface::controller::reduce_control_input(
                            &engine, &bridge, world, &handle, &action,
                        )
                        .unwrap();
                    }
                    assert_eq!(
                        handle.focused_key(),
                        None,
                        "native canvas input blurs the value field"
                    );
                    world.resource_scope(|world, mut editor: Mut<Editor>| {
                        interaction::pointer(
                            world,
                            (&services, &owner),
                            &mut editor,
                            (at, at, canvas),
                            false,
                            false,
                        )
                        .unwrap();
                    });
                    assert!(worker::busy(world), "placement must commit without Enter");
                    break;
                }
                let mut input = NativeHostInput {
                    ui_scale: 1.,
                    context: handle.presented_context(),
                    cursor: Some(pixel),
                    modifiers: Default::default(),
                    event: WindowEvent::KeyboardInput(KeyboardInput {
                        key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                        logical_key,
                        state: ButtonState::Pressed,
                        text: text.map(Into::into),
                        repeat: false,
                        window: Entity::PLACEHOLDER,
                    }),
                    consumed: false,
                    actions: Vec::new(),
                };
                prepare_native_input(world, &handle, &mut input).unwrap();
                assert!(input.consumed, "keyboard input was lost for {typed:?}");
                for action in input.actions {
                    crate::session_bridge::native_interface::controller::reduce_control_input(
                        &engine, &bridge, world, &handle, &action,
                    )
                    .unwrap();
                }
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(outcome) = worker::poll(world, &services) {
                    outcome.value.unwrap();
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "Dimension confirmation did not finish for {typed:?}, click={place}"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            let after = active(&engine).unwrap().unwrap();
            assert_eq!(
                after.dimensions.len(),
                1,
                "Dimension confirmation did not create the dimension for {typed:?}, click={place}"
            );
            assert_eq!(
                after.dimensions[0].value,
                typed.map_or(height, |text| text.parse::<f64>().unwrap()),
                "untouched suggestions retain exact precision; replacements use typed precision"
            );
            assert_eq!(after.dof.value, 1);
            if place {
                assert!(
                    after.dimensions[0]
                        .text_pos
                        .distance(Point::new(-6., height * 0.5))
                        < 1e-5
                );
            }
            assert!(world.resource::<Editor>().interaction.dimension.is_none());
            bridge
                .apply_native_history(&engine, &owner, false, || Ok(()))
                .unwrap();
            let undone = active(&engine).unwrap().unwrap();
            assert_eq!(undone.entities, before.entities);
            assert!(undone.dimensions.is_empty());
            bridge
                .apply_native_history(&engine, &owner, true, || Ok(()))
                .unwrap();
            let redone = active(&engine).unwrap().unwrap();
            assert_eq!(redone.entities, after.entities);
            assert_eq!(redone.dimensions, after.dimensions);
        }
    });
}
