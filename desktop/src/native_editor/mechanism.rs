//! Pointer-plane inverse kinematics over the shared assembly solver. Motion is
//! coalesced while the worker runs; release commits once, cancellation restores.
use super::*;
use crate::native_viewport::{NativePickPurpose, ViewportCamera, ViewportPresentation};
use crate::session_bridge::native_interface::controller::assembly::{motion, studies};
use bevy::math::{DQuat, DVec3};
use limo_cad_sketch::{
    AssemblyDocumentDto, BodyPoseDto, JointMotionStateDto, MechanismDragRequestDto,
    MechanismPreviewDto,
};

#[derive(Resource, Default)]
struct State {
    drag: Option<Drag>,
    serial: u64,
}
struct Drag {
    owner: DocumentContext,
    revision: u64,
    serial: u64,
    camera: ViewportCamera,
    canvas: InterfaceRect,
    start: Vec2,
    point: Vec2,
    right: DVec3,
    up: DVec3,
    scale: f64,
    grabbed: DVec3,
    pose: BodyPoseDto,
    local: [f64; 3],
    occurrence: limo_cad_sketch::OccurrenceId,
    original: ViewportPresentation,
    motions: Vec<JointMotionStateDto>,
    requested: Option<Vec2>,
    solved: Option<(Vec2, MechanismPreviewDto)>,
    released: bool,
    cancelled: bool,
    engaged: bool,
}
impl Drag {
    fn same_view(&self, camera: ViewportCamera, canvas: InterfaceRect) -> bool {
        self.camera == camera && self.canvas == canvas
    }
    fn view_current(&self, world: &World, handle: &NativeInterfaceHandle) -> bool {
        let (document, camera) = native_viewport::interface_camera_snapshot(world);
        document == self.owner.document_id
            && handle.frame().is_some_and(|frame| {
                frame.context == self.owner
                    && frame.modal_stack.is_empty()
                    && frame.canvases.iter().any(|canvas| {
                        canvas.name == "viewport" && self.same_view(camera, canvas.bounds)
                    })
            })
    }
    fn moved(&self) -> bool {
        self.engaged
    }
    fn request(&self) -> MechanismDragRequestDto {
        let delta = self.point - self.start;
        let offset = (self.right * f64::from(delta.x) - self.up * f64::from(delta.y)) * self.scale;
        MechanismDragRequestDto {
            body_id: self.pose.body_id,
            occurrence_id: Some(self.occurrence),
            target_pose: BodyPoseDto {
                translation: (DVec3::from_array(self.pose.translation) + offset).to_array(),
                ..self.pose
            },
            grab_point_local: Some(self.local),
            target_point_world: Some((self.grabbed + offset).to_array()),
            initial_joint_motions: self.motions.clone(),
            solve_orientation: false,
            maximum_iterations: 12,
        }
    }
    fn observe(&mut self, event: &NativeHostInput) {
        if event.context.as_ref() != Some(&self.owner) {
            self.cancelled = true;
            return;
        }

        let lifecycle = matches!(&event.event, WindowEvent::WindowFocused(e) if !e.focused)
            || matches!(
                event.event,
                WindowEvent::CursorLeft(_)
                    | WindowEvent::WindowCloseRequested(_)
                    | WindowEvent::KeyboardFocusLost(_)
                    | WindowEvent::WindowDestroyed(_)
                    | WindowEvent::WindowResized(_)
                    | WindowEvent::WindowScaleFactorChanged(_)
                    | WindowEvent::WindowBackendScaleFactorChanged(_)
            )
            || matches!(
                event.event,
                WindowEvent::MouseWheel(_) | WindowEvent::PinchGesture(_)
            )
            || matches!(&event.event, WindowEvent::MouseButtonInput(e)
                if e.state == ButtonState::Pressed && matches!(e.button, MouseButton::Middle | MouseButton::Right))
            || matches!(&event.event, WindowEvent::KeyboardInput(e)
                if e.state == ButtonState::Pressed);
        if lifecycle {
            self.cancelled = true;
        }
        if self.released || self.cancelled {
            return;
        }
        match &event.event {
            WindowEvent::CursorMoved(e) => self.point = e.position,
            WindowEvent::MouseButtonInput(e)
                if e.button == MouseButton::Left && e.state == ButtonState::Released =>
            {
                if let Some(point) = event.cursor {
                    self.point = point;
                }
                self.released = true;
            }
            _ => {}
        }
        self.engaged |= self.start.distance(self.point) > 3.;
    }
}
fn allowed(world: &World) -> bool {
    !support::picking(world)
        && !motion::active(world)
        && !studies::active(world)
        && solid::allowed(world)
}
/// This touches only an already owned pointer draft, never the kernel or picks
/// from a busy scene. In particular mouse-up cannot be lost behind a preview.
pub(crate) fn observe_busy(world: &mut World, event: &NativeHostInput) {
    if let Some(mut state) = world.get_resource_mut::<State>() {
        if let Some(drag) = state.drag.as_mut() {
            drag.observe(event);
        }
    }
}
fn restore(world: &mut World, drag: &Drag) -> Result<(), String> {
    let (id, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    if id != drag.owner.document_id {
        return Ok(());
    }
    view.body_poses = drag.original.body_poses.clone();
    view.instance_body_poses = drag.original.instance_body_poses.clone();
    native_viewport::apply_interface_view(world, &id, None, Some(view))
}
pub(crate) fn cancel(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<State>() {
        if let Some(d) = state.drag.as_mut() {
            d.cancelled = true;
        }
    }
}
pub(super) fn pointer(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<bool, String> {
    world.init_resource::<State>();
    let mut state = world.remove_resource::<State>().unwrap();
    let result = (|| {
        if let Some(drag) = state.drag.as_mut() {
            drag.observe(event);
            if !allowed(world) || !drag.view_current(world, handle) {
                drag.cancelled = true;
            }
            if drag.cancelled {
                let owner = drag.owner.clone();
                let revision = drag.revision;
                services
                    .bridge
                    .with_native_document_receipt(&services.engine, &owner, |now| {
                        if now == revision {
                            restore(world, drag)?;
                        }
                        Ok(())
                    })?;
                state.drag = None;
                return Ok(true);
            }
            if drag.released && !drag.moved() {
                state.drag = None;
                return Ok(false);
            }
            return Ok(drag.moved());
        }
        if !matches!(&event.event,WindowEvent::MouseButtonInput(e) if e.button==MouseButton::Left&&e.state==ButtonState::Pressed)
            || event.modifiers.ctrl
            || event.modifiers.shift
            || event.modifiers.meta
            || event.modifiers.alt
            || !allowed(world)
        {
            return Ok(false);
        }
        let Some(frame) = handle.frame() else {
            return Ok(false);
        };
        if !frame.modal_stack.is_empty() || event.context.as_ref() != Some(&frame.context) {
            return Ok(false);
        }
        let Some(cursor) = event.cursor else {
            return Ok(false);
        };
        let Some(canvas) = frame.canvases.iter().find(|c| c.name == "viewport") else {
            return Ok(false);
        };
        let bounds = canvas.bounds;
        if handle.owns_pointer([cursor.x as f64, cursor.y as f64])
            || cursor.x < bounds.x as f32
            || cursor.y < bounds.y as f32
            || cursor.x >= (bounds.x + bounds.width) as f32
            || cursor.y >= (bounds.y + bounds.height) as f32
        {
            return Ok(false);
        }
        let revision = services
            .bridge
            .native_document_receipt(&services.engine, &frame.context)?
            .revision;
        services
            .bridge
            .with_native_document_receipt(&services.engine, &frame.context, |current| {
                if current != revision {
                    return Err("Assembly changed before the drag started".into());
                }
                let Some(hit) = native_viewport::interface_pick(
                    world,
                    &frame.context.document_id,
                    [cursor.x - bounds.x as f32, cursor.y - bounds.y as f32],
                    NativePickPurpose::Geometry,
                )?
                else {
                    return Ok(false);
                };
                let Some(occurrence) = hit.occurrence_id.map(limo_cad_sketch::OccurrenceId) else {
                    return Ok(false);
                };
                let a: AssemblyDocumentDto =
                    serde_json::from_value(crate::session_bridge::parse_engine_envelope(
                        services.engine.engine_call("assembly_document", ""),
                    )?)
                    .map_err(|e| e.to_string())?;
                if !a.can_drag_occurrence(
                    limo_cad_core::BodyId(hit.body_id),
                    occurrence,
                    native_viewport::interface_geometry(world).scene,
                ) {
                    return Ok(false);
                }
                let (_, camera, view, _) = native_viewport::interface_view(world);
                let Some(pose) = view
                    .instance_body_poses
                    .iter()
                    .find(|p| p.body_id.0 == hit.body_id && p.occurrence_id == occurrence)
                else {
                    return Ok(false);
                };
                let grabbed = DVec3::from_array(hit.point.map(f64::from));
                let position = DVec3::from_array(camera.position.map(f64::from));
                let forward =
                    (DVec3::from_array(camera.target.map(f64::from)) - position).normalize();
                let right = forward
                    .cross(DVec3::from_array(camera.up.map(f64::from)))
                    .normalize();
                let up = right.cross(forward).normalize();
                let depth = (grabbed - position).dot(forward).max(0.01);
                let scale =
                    2. * depth * (f64::from(camera.vertical_fov_degrees).to_radians() / 2.).tan()
                        / bounds.height.max(1.);
                let local = (DQuat::from_array(pose.rotation).inverse()
                    * (grabbed - DVec3::from_array(pose.translation)))
                .to_array();
                state.serial = state.serial.wrapping_add(1);
                state.drag = Some(Drag {
                    owner: frame.context.clone(),
                    revision,
                    serial: state.serial,
                    camera,
                    canvas: bounds,
                    start: cursor,
                    point: cursor,
                    right,
                    up,
                    scale,
                    grabbed,
                    pose: BodyPoseDto {
                        body_id: pose.body_id,
                        translation: pose.translation,
                        rotation: pose.rotation,
                    },
                    local,
                    occurrence,
                    original: view.clone(),
                    motions: vec![],
                    requested: None,
                    solved: None,
                    released: false,
                    cancelled: false,
                    engaged: false,
                });
                Ok(false)
            })
    })();
    world.insert_resource(state);
    result
}
pub(crate) fn active(world: &World) -> bool {
    world
        .get_resource::<State>()
        .is_some_and(|s| s.drag.is_some())
}
pub(crate) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
) -> Result<Option<Value>, String> {
    if worker::busy(world) {
        return Ok(None);
    }
    let Some(mut state) = world.remove_resource::<State>() else {
        return Ok(None);
    };
    let result = (|| {
        let Some(d) = state.drag.as_mut() else {
            return Ok(None);
        };
        let revision = services
            .bridge
            .native_document_receipt(&services.engine, owner)?
            .revision;
        if d.owner != *owner || d.revision != revision {
            state.drag = None;
            return Ok(None);
        }
        if !allowed(world) || !d.view_current(world, handle) {
            d.cancelled = true;
        }
        if d.cancelled {
            restore(world, d)?;
            state.drag = None;
            return Ok(None);
        }
        if !d.moved() {
            if d.released {
                state.drag = None;
            }
            return Ok(None);
        }
        if d.released && d.solved.as_ref().is_some_and(|(p, _)| *p == d.point) {
            let solution = d.solved.take().unwrap().1;
            if !solution.solution.solved {
                restore(world, d)?;
                state.drag = None;
                return Err("The mechanism could not solve this position".into());
            }
            let motions = solution.joint_motions;
            let captured_owner = d.owner.clone();
            let original = d.original.clone();
            let result = worker::enqueue_operation(
                world,
                captured_owner.clone(),
                revision,
                "assembly_apply_joint_motions".into(),
                json!({"motions":motions}),
                move |world, services, result| match result {
                    Ok(result) => Ok(finish_mutation(
                        &services.engine,
                        &services.bridge,
                        world,
                        "assembly_apply_joint_motions",
                        result,
                    )),
                    Err(error) => {
                        services.bridge.with_native_document_receipt(
                            &services.engine,
                            &captured_owner,
                            |now| {
                                if now == revision {
                                    let (_, _, mut view, _) =
                                        native_viewport::interface_view_snapshot(world);
                                    view.body_poses = original.body_poses;
                                    view.instance_body_poses = original.instance_body_poses;
                                    native_viewport::apply_interface_view(
                                        world,
                                        &captured_owner.document_id,
                                        None,
                                        Some(view),
                                    )?;
                                }
                                Ok(())
                            },
                        )?;
                        Err(error)
                    }
                },
            );
            state.drag = None;
            return result.map(Some);
        }
        if d.requested == Some(d.point) {
            return Ok(None);
        }
        let point = d.point;
        d.requested = Some(point);
        let request = d.request();
        let serial = d.serial;
        let captured_owner = d.owner.clone();
        let callback_handle = handle.clone();
        let pending = worker::enqueue_query(
            world,
            owner.clone(),
            revision,
            "assembly_preview_mechanism_drag".into(),
            json!(request),
            move |world, services, result| {
                let value = services.bridge.with_native_document_receipt(
                    &services.engine,
                    &captured_owner,
                    |now| {
                        if now != revision {
                            return Err("The mechanism changed during the drag".into());
                        }
                        let mut state = world
                            .remove_resource::<State>()
                            .ok_or("Drag was cancelled")?;
                        let result = (|| {
                            let Some(d) = state
                                .drag
                                .as_mut()
                                .filter(|d| d.owner == captured_owner && d.serial == serial)
                            else {
                                return Ok(json!({"cancelled":true}));
                            };
                            if d.cancelled
                                || !allowed(world)
                                || !d.view_current(world, &callback_handle)
                            {
                                restore(world, d)?;
                                state.drag = None;
                                return Ok(json!({"cancelled":true}));
                            }
                            let result: MechanismPreviewDto = match result.and_then(|r| {
                                serde_json::from_value(r.value).map_err(|e| e.to_string())
                            }) {
                                Ok(result) => result,
                                Err(error) => {
                                    restore(world, d)?;
                                    state.drag = None;
                                    return Err(error);
                                }
                            };
                            if !result.solution.solved {
                                restore(world, d)?;
                                state.drag = None;
                                return Err("The mechanism constraints could not be solved".into());
                            }
                            let (_, _, mut view, _) =
                                native_viewport::interface_view_snapshot(world);
                            view.body_poses = result.solution.body_poses.clone().into();
                            view.instance_body_poses =
                                result.solution.instance_body_poses.clone().into();
                            native_viewport::apply_interface_view(
                                world,
                                &captured_owner.document_id,
                                None,
                                Some(view),
                            )?;
                            d.motions = result.joint_motions.clone();
                            d.solved = Some((point, result));
                            Ok(json!({"mechanism_preview":true}))
                        })();
                        world.insert_resource(state);
                        result
                    },
                )?;
                Ok(tick(world, &callback_handle, services, &captured_owner)?.unwrap_or(value))
            },
        )?;
        handle.request_redraw();
        Ok(Some(pending))
    })();
    world.insert_resource(state);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::mouse::MouseButtonInput;
    use bevy::window::CursorMoved;
    fn drag() -> Drag {
        Drag {
            owner: DocumentContext {
                window_id: "main".into(),
                document_id: "a".into(),
                epoch: 1,
            },
            revision: 2,
            serial: 1,
            camera: ViewportCamera {
                position: [0., 0., 100.],
                target: [0., 0., 0.],
                up: [0., 1., 0.],
                vertical_fov_degrees: 15.2,
            },
            canvas: InterfaceRect {
                x: 20.,
                y: 50.,
                width: 800.,
                height: 600.,
            },
            start: Vec2::ZERO,
            point: Vec2::ZERO,
            right: DVec3::X,
            up: DVec3::Y,
            scale: 0.5,
            grabbed: DVec3::new(10., 20., 30.),
            pose: BodyPoseDto {
                body_id: limo_cad_core::BodyId(1),
                translation: [1., 2., 3.],
                rotation: [0., 0., 0., 1.],
            },
            local: [9., 18., 27.],
            occurrence: limo_cad_sketch::OccurrenceId(2),
            original: default(),
            motions: vec![],
            requested: None,
            solved: None,
            released: false,
            cancelled: false,
            engaged: false,
        }
    }
    #[test]
    fn busy_pointer_retains_latest_target_release_and_original_body_local_pick() {
        let mut d = drag();
        let window = Entity::from_bits(1);
        let event = |point| NativeHostInput {
            ui_scale: 1.,
            context: Some(drag().owner),
            cursor: Some(point),
            modifiers: default(),
            event: WindowEvent::CursorMoved(CursorMoved {
                window,
                position: point,
                delta: None,
            }),
            consumed: false,
            actions: vec![],
        };
        d.observe(&event(Vec2::new(4., 6.)));
        d.observe(&event(Vec2::new(20., -10.)));
        assert!(d.moved());
        let r = d.request();
        assert_eq!(r.target_pose.translation, [11., 7., 3.]);
        assert_eq!(r.target_point_world, Some([20., 25., 30.]));
        assert_eq!(r.grab_point_local, Some([9., 18., 27.]));
        d.observe(&event(Vec2::ZERO));
        assert!(
            d.moved(),
            "Returning to the start still requires a final solve"
        );
        let mut release = event(Vec2::new(24., -10.));
        release.event = WindowEvent::MouseButtonInput(MouseButtonInput {
            window,
            button: MouseButton::Left,
            state: ButtonState::Released,
        });
        d.observe(&release);
        assert!(d.released);
        assert_eq!(d.point, Vec2::new(24., -10.));
        d.observe(&event(Vec2::new(100., 100.)));
        assert_eq!(d.point, Vec2::new(24., -10.));
        let mut stale = event(Vec2::ZERO);
        stale.context.as_mut().unwrap().epoch += 1;
        d.observe(&stale);
        assert!(d.cancelled);
    }

    #[test]
    fn camera_and_canvas_changes_invalidate_the_original_drag_basis() {
        let d = drag();
        assert!(d.same_view(d.camera, d.canvas));
        let mut moved = d.camera;
        moved.position[0] += 1.;
        assert!(!d.same_view(moved, d.canvas));
        let mut resized = d.canvas;
        resized.height += 1.;
        assert!(!d.same_view(d.camera, resized));
        let mut shifted = d.canvas;
        shifted.x += 1.;
        assert!(!d.same_view(d.camera, shifted));
    }

    #[test]
    fn sketch_support_picker_retains_priority_over_component_drag() {
        let mut world = World::new();
        world.init_resource::<Editor>();
        world.resource_mut::<Editor>().support.active = true;

        assert!(!allowed(&world));
    }

    #[test]
    fn focus_loss_after_release_cancels_the_pending_final_preview() {
        let mut d = drag();
        d.released = true;
        d.engaged = true;
        d.observe(&NativeHostInput {
            ui_scale: 1.,
            context: Some(d.owner.clone()),
            cursor: None,
            modifiers: default(),
            event: WindowEvent::WindowFocused(bevy::window::WindowFocused {
                window: Entity::from_bits(1),
                focused: false,
            }),
            consumed: false,
            actions: vec![],
        });
        assert!(
            d.cancelled,
            "Mouse-up must not suppress lifecycle cancellation"
        );
    }

    #[test]
    fn queued_camera_input_cancels_before_the_displayed_camera_changes() {
        use bevy::input::{
            gestures::PinchGesture,
            mouse::{MouseScrollUnit, MouseWheel},
        };
        let window = Entity::from_bits(1);
        let events = [
            WindowEvent::MouseWheel(MouseWheel {
                window,
                unit: MouseScrollUnit::Line,
                phase: bevy::input::touch::TouchPhase::Moved,
                x: 0.,
                y: 1.,
            }),
            WindowEvent::PinchGesture(PinchGesture(0.25)),
            WindowEvent::MouseButtonInput(MouseButtonInput {
                window,
                button: MouseButton::Middle,
                state: ButtonState::Pressed,
            }),
            WindowEvent::MouseButtonInput(MouseButtonInput {
                window,
                button: MouseButton::Right,
                state: ButtonState::Pressed,
            }),
        ];
        for event in events {
            let mut d = drag();
            d.released = true;
            d.engaged = true;
            d.observe(&NativeHostInput {
                ui_scale: 1.,
                context: Some(d.owner.clone()),
                cursor: None,
                modifiers: default(),
                event,
                consumed: false,
                actions: vec![],
            });
            assert!(d.cancelled);
            assert!(
                d.same_view(d.camera, d.canvas),
                "Queued input precedes camera publication"
            );
        }
    }
}
