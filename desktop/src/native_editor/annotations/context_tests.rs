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
