//! Native placement previews and six-axis handles, with no kernel work during a drag.
use super::*;
use crate::native_forms::MoveMode;
use bevy::math::{DQuat, DVec3, Vec2};
use native_viewport::{ViewportArrow, ViewportLineLayer, ViewportPointLayer};
use nbcad_sketch::{BodyPoseDto, InstanceBodyPoseDto};

#[derive(Default)]
pub(super) struct View {
    camera: Option<native_viewport::ViewportCamera>,
    poses: Vec<BodyPoseDto>,
    instances: Vec<InstanceBodyPoseDto>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Handle {
    Translate(usize),
    Rotate(usize),
}
#[derive(Clone, Copy)]
pub(super) struct Drag {
    kind: Handle,
    start: Vec2,
    direction: Vec2,
    pixels: f32,
    translation: [f64; 3],
    rotation: [f64; 4],
}
const COLORS: [[f32; 4]; 3] = [
    [0.906, 0.373, 0.384, 1.],
    [0.329, 0.741, 0.471, 1.],
    [0.310, 0.616, 0.871, 1.],
];
fn xyz(p: nbcad_solid::Point3Dto) -> DVec3 {
    DVec3::new(p.x, p.y, p.z)
}

pub(super) fn restore(editor: &mut Editor, world: &mut World) -> Result<(), String> {
    let Some(previous) = editor.move_view.take() else {
        return Ok(());
    };
    let (id, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    if id != editor.snapshot.receipt.owner.document_id {
        return Ok(());
    }
    view.body_poses = previous.poses;
    view.instance_body_poses = previous.instances;
    native_viewport::apply_interface_view(world, &id, None, Some(view))
}

pub(super) fn preview(editor: &mut Editor, world: &mut World) -> Result<ViewportPreview, String> {
    let model = editor.snapshot.model(None);
    let (id, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    let original = editor.move_view.get_or_insert_with(|| View {
        camera: None,
        poses: view.body_poses.clone(),
        instances: view.instance_body_poses.clone(),
    });
    original.camera = Some(native_viewport::interface_camera_snapshot(world).1);
    let request = editor.form.move_request(&model).ok();
    let mut next = ViewportPreview::default();
    let mut poses = original.poses.clone();
    let mut instances = original.instances.clone();
    if let Some(request) = request {
        let q = DQuat::from_array(request.rotation);
        let pivot = xyz(request.pivot);
        let translation = xyz(request.translation);
        let offset = pivot + translation - q * pivot;
        if !offset.is_finite() || offset.abs().max_element() > f32::MAX as f64 / 4. {
            return Err("The move is outside the renderer's range".into());
        }
        let component = editor.form.move_is_component();
        let occurrence_ids = if component {
            editor
                .form
                .move_occurrence_targets(&model)
                .unwrap_or_default()
        } else {
            Default::default()
        };
        let mut targets = Vec::new();
        for p in &original.instances {
            if p.visible
                && if component {
                    occurrence_ids.contains(&p.occurrence_id.0)
                } else {
                    request.body_ids.contains(&p.body_id)
                }
            {
                targets.push((p.body_id, Some(p.occurrence_id), p.translation, p.rotation));
            }
        }
        if !component {
            for &body in &request.body_ids {
                if !targets.iter().any(|(id, _, _, _)| *id == body) {
                    let p = original
                        .poses
                        .iter()
                        .find(|p| p.body_id == body)
                        .copied()
                        .unwrap_or_else(|| BodyPoseDto::identity(body));
                    targets.push((body, None, p.translation, p.rotation));
                }
            }
        }
        // Retained body meshes are instanced with new poses. Only the transient
        // selection highlight is transformed; no kernel or tessellation runs.
        let mut segments = Vec::new();
        for (body_id, occurrence, base_translation, base_rotation) in targets {
            let base_q = DQuat::from_array(base_rotation);
            let base_t = DVec3::from_array(base_translation);
            let (display_q, display_t) = if component {
                (q * base_q, q * base_t + offset)
            } else {
                (base_q * q, base_t + base_q * offset)
            };
            if !request.copy {
                if let Some(occurrence) = occurrence {
                    if let Some(p) = instances
                        .iter_mut()
                        .find(|p| p.body_id == body_id && p.occurrence_id == occurrence)
                    {
                        p.translation = display_t.to_array();
                        p.rotation = display_q.to_array();
                    }
                } else {
                    if !poses.iter().any(|p| p.body_id == body_id) {
                        poses.push(BodyPoseDto::identity(body_id));
                    }
                    let p = poses.iter_mut().find(|p| p.body_id == body_id).unwrap();
                    p.translation = display_t.to_array();
                    p.rotation = display_q.to_array();
                }
            }
            let mut fill = super::preview::body_fill(
                model.scene,
                body_id,
                [1., 0.65, 0.25, if request.copy { 0.45 } else { 0.25 }],
            )?;
            for p in fill.positions.chunks_exact_mut(3) {
                let v = display_q * DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64) + display_t;
                if !v.as_vec3().is_finite() {
                    return Err("The move exceeds the renderer's range".into());
                }
                p.copy_from_slice(&v.as_vec3().to_array());
            }
            next.triangles.push(fill);
            for edge in &model
                .scene
                .bodies
                .iter()
                .find(|b| b.id == body_id)
                .ok_or("Body no longer exists")?
                .edges
            {
                for pair in edge.points.windows(2) {
                    if segments.len() / 6 >= 100_000 {
                        return Err("The move preview has too many edge segments".into());
                    }
                    for p in pair {
                        segments.extend((display_q * xyz(*p) + display_t).as_vec3().to_array());
                    }
                }
            }
        }
        next.lines.push(ViewportLineLayer {
            color: [1., 0.8, 0.5, 1.],
            width: 2.,
            segments,
            ..Default::default()
        });
        if editor.form.move_mode() == Some(MoveMode::Free) {
            if let Some(gizmo) = Gizmo::new(world, editor) {
                gizmo.draw(&mut next, editor.move_hover);
            }
        }
    }
    if view.body_poses != poses || view.instance_body_poses != instances {
        view.body_poses = poses;
        view.instance_body_poses = instances;
        native_viewport::apply_interface_view(world, &id, None, Some(view))?;
    }
    for (body, edge, occurrence) in editor.form.move_edges() {
        if let Some(edge) = model
            .scene
            .bodies
            .iter()
            .find(|b| b.id == body)
            .and_then(|b| b.edges.iter().find(|e| e.id == edge))
        {
            let pose = occurrence.and_then(|id| {
                model
                    .assembly_solution?
                    .instance_body_poses
                    .iter()
                    .find(|p| p.occurrence_id.0 == id && p.body_id == body)
            });
            let q = pose
                .map(|p| DQuat::from_array(p.rotation))
                .unwrap_or(DQuat::IDENTITY);
            let t = pose
                .map(|p| DVec3::from_array(p.translation))
                .unwrap_or(DVec3::ZERO);
            let segments = edge
                .points
                .windows(2)
                .flat_map(|pair| {
                    pair.iter()
                        .flat_map(|p| (q * xyz(*p) + t).as_vec3().to_array())
                })
                .collect();
            next.lines.push(ViewportLineLayer {
                color: [0.45, 0.72, 1., 1.],
                width: 3.,
                segments,
                ..Default::default()
            });
        }
    }
    if let Some(body) = editor.hovered_body {
        let mut fill = super::preview::body_fill(model.scene, body, [0.45, 0.72, 1., 0.24])?;
        let pose = editor.hovered_occurrence.and_then(|id| {
            model
                .assembly_solution?
                .instance_body_poses
                .iter()
                .find(|p| p.occurrence_id.0 == id && p.body_id == body)
        });
        let q = pose
            .map(|p| DQuat::from_array(p.rotation))
            .unwrap_or(DQuat::IDENTITY);
        let t = pose
            .map(|p| DVec3::from_array(p.translation))
            .unwrap_or(DVec3::ZERO);
        for p in fill.positions.chunks_exact_mut(3) {
            let v = q * DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64) + t;
            p.copy_from_slice(&v.as_vec3().to_array());
        }
        next.triangles.push(fill);
    }
    if let Some(p) = editor
        .hovered_point
        .filter(|_| editor.pick_target.is_some_and(SolidField::is_move_point))
    {
        let (_, camera) = native_viewport::interface_camera_snapshot(world);
        let eye = DVec3::from_array(camera.position.map(f64::from));
        next.points.push(ViewportPointLayer {
            color: [1., 0.7, 0.2, 1.],
            color_role: Default::default(),
            radius: (eye.distance(DVec3::from_array(p)) * 0.0015) as f32,
            hollow: false,
            positions: p.map(|v| v as f32).to_vec(),
        });
    }
    Ok(next)
}
pub(super) fn needs_refresh(editor: &Editor, world: &World) -> bool {
    editor.form.kind() == SolidFormKind::MoveCopy
        && editor
            .move_view
            .as_ref()
            .is_none_or(|v| v.camera != Some(native_viewport::interface_camera_snapshot(world).1))
}

struct Gizmo {
    pivot: DVec3,
    length: f64,
}
impl Gizmo {
    fn new(world: &World, editor: &Editor) -> Option<Self> {
        if editor.form.move_mode() != Some(MoveMode::Free) {
            return None;
        }
        let r = editor
            .form
            .move_request(&editor.snapshot.model(None))
            .ok()?;
        let pivot = xyz(r.pivot) + xyz(r.translation);
        let (_, camera, _, size) = native_viewport::interface_view_snapshot(world);
        let eye = DVec3::from_array(camera.position.map(f64::from));
        let forward = (DVec3::from_array(camera.target.map(f64::from)) - eye).try_normalize()?;
        let depth = (pivot - eye).dot(forward);
        if depth <= 0. || !depth.is_finite() {
            return None;
        }
        let world_per_pixel =
            2. * depth * (f64::from(camera.vertical_fov_degrees).to_radians() * 0.5).tan()
                / f64::from(size[1].max(1.));
        Some(Self {
            pivot,
            length: (world_per_pixel * 96.).max(6.),
        })
    }
    fn axis(i: usize) -> DVec3 {
        [DVec3::X, DVec3::Y, DVec3::Z][i]
    }
    fn radial(i: usize) -> DVec3 {
        [
            DVec3::new(0., 1., 1.),
            DVec3::new(1., 0., 1.),
            DVec3::new(1., 1., 0.),
        ][i]
            .normalize()
    }
    fn bead(&self, i: usize) -> DVec3 {
        self.pivot + Self::radial(i) * self.length * 0.62
    }
    fn draw(&self, preview: &mut ViewportPreview, hover: Option<Handle>) {
        for i in 0..3 {
            let axis = Self::axis(i);
            let arrow_color = if hover == Some(Handle::Translate(i)) {
                [1., 0.8, 0.25, 1.]
            } else {
                COLORS[i]
            };
            let ring_color = if hover == Some(Handle::Rotate(i)) {
                [1., 0.8, 0.25, 1.]
            } else {
                COLORS[i]
            };
            preview.arrows.push(ViewportArrow {
                start: self.pivot.as_vec3().to_array(),
                end: (self.pivot + axis * self.length).as_vec3().to_array(),
                color: arrow_color,
                width: 3.,
                xray: true,
            });
            let a = Self::radial(i);
            let b = axis.cross(a);
            let at = |n: usize| {
                let angle = n as f64 * std::f64::consts::TAU / 64.;
                (self.pivot + (a * angle.cos() + b * angle.sin()) * self.length * 0.62)
                    .as_vec3()
                    .to_array()
            };
            let segments = (0..64)
                .flat_map(|n| [at(n), at(n + 1)].into_iter().flatten())
                .collect();
            preview.lines.push(ViewportLineLayer {
                color: ring_color,
                width: if hover == Some(Handle::Rotate(i)) {
                    4.
                } else {
                    2.4
                },
                segments,
                ..Default::default()
            });
            preview.points.push(ViewportPointLayer {
                color: ring_color,
                color_role: Default::default(),
                radius: (self.length * 6.5 / 96.) as f32,
                hollow: false,
                positions: self.bead(i).as_vec3().to_array().to_vec(),
            });
        }
    }
    fn project(&self, world: &World, id: &str, p: DVec3) -> Option<Vec2> {
        native_viewport::interface_world_point(world, id, p.to_array())
            .ok()
            .flatten()
            .map(Vec2::from_array)
    }
    fn hit(&self, world: &World, id: &str, p: Vec2) -> Option<(Handle, Vec2, f32)> {
        // Beads make rotation explicit; the rest of each ring is not a drag target.
        for i in 0..3 {
            let bead = self.bead(i);
            let screen = self.project(world, id, bead)?;
            if p.distance(screen) <= 10. {
                let tangent = Self::axis(i).cross(Self::radial(i));
                let d = self.project(world, id, bead + tangent)? - screen;
                let pixels = d.length() * self.length as f32 * 0.62 * std::f32::consts::PI / 180.;
                if pixels > 0.08 {
                    return Some((Handle::Rotate(i), d.normalize(), pixels));
                }
            }
        }
        let center = self.project(world, id, self.pivot)?;
        let mut best: Option<(f32, Handle, Vec2, f32)> = None;
        for i in 0..3 {
            let d = self.project(world, id, self.pivot + Self::axis(i) * self.length)? - center;
            let length = d.length();
            if length < 4. {
                continue;
            }
            let direction = d / length;
            let along = (p - center).dot(direction);
            if along < 16_f32.min(length * 0.25) || along > length + 10. {
                continue;
            }
            let distance = ((p - center) - direction * along).length();
            if distance < 10. && best.as_ref().is_none_or(|(old, ..)| distance < *old) {
                let pixels =
                    (self.project(world, id, self.pivot + Self::axis(i))? - center).length();
                if pixels > 0.1 {
                    best = Some((distance, Handle::Translate(i), direction, pixels));
                }
            }
        }
        best.map(|(_, kind, d, pixels)| (kind, d, pixels))
    }
}

pub(super) fn pointer(
    world: &mut World,
    services: &super::super::controller::NativeServices,
    owner: &DocumentContext,
    event: super::manipulator::Pointer,
    point: Option<[f32; 2]>,
) -> Result<bool, String> {
    use super::manipulator::Pointer;
    let mut state = world.remove_resource::<NativeFeature>().unwrap_or_default();
    let result = (|| {
        let Some(editor) = state
            .editor
            .as_mut()
            .filter(|e| e.form.kind() == SolidFormKind::MoveCopy)
        else {
            return Ok(false);
        };
        if matches!(event, Pointer::Cancel) || editor.form.is_busy() {
            let hover = editor.move_hover.take().is_some();
            let drag = editor.move_drag.take().is_some();
            if hover && !editor.form.is_busy() {
                with_receipt(&services.bridge, &services.engine, owner, |receipt| {
                    check_revision(editor, &receipt)?;
                    update_preview(editor, world)
                })?;
            }
            return Ok(drag);
        }
        let Some(point) = point.map(Vec2::from_array).filter(|p| p.is_finite()) else {
            return Ok(false);
        };
        with_receipt(&services.bridge, &services.engine, owner, |receipt| {
            check_revision(editor, &receipt)?;
            if let Some(drag) = editor.move_drag {
                if !matches!(event, Pointer::Move | Pointer::Release) {
                    return Ok(true);
                };
                let delta = f64::from((point - drag.start).dot(drag.direction) / drag.pixels);
                let mut t = drag.translation;
                let mut q = DQuat::from_array(drag.rotation);
                match drag.kind {
                    Handle::Translate(i) => t[i] = ((t[i] + delta) * 100.).round() / 100.,
                    Handle::Rotate(i) => {
                        q = DQuat::from_axis_angle(Gizmo::axis(i), delta.to_radians()) * q
                    }
                }
                editor
                    .form
                    .drag_move(t, q.to_array(), &editor.snapshot.model(None))?;
                if matches!(event, Pointer::Release) {
                    editor.move_drag = None;
                }
                update_preview(editor, world)?;
                return Ok(true);
            }
            let hit =
                Gizmo::new(world, editor).and_then(|g| g.hit(world, &owner.document_id, point));
            let hover = hit.map(|(h, _, _)| h);
            if hover != editor.move_hover {
                editor.move_hover = hover;
                update_preview(editor, world)?;
            }
            if matches!(event, Pointer::Press) {
                if let Some((kind, direction, pixels)) = hit {
                    let request = editor
                        .form
                        .move_request(&editor.snapshot.model(None))
                        .map_err(|errors| {
                            errors
                                .into_iter()
                                .next()
                                .map(|(_, e)| e)
                                .unwrap_or_else(|| "Invalid move".into())
                        })?;
                    editor.move_drag = Some(Drag {
                        kind,
                        start: point,
                        direction,
                        pixels,
                        translation: xyz(request.translation).to_array(),
                        rotation: request.rotation,
                    });
                    return Ok(true);
                }
            }
            Ok(false)
        })
    })();
    world.insert_resource(state);
    result
}
