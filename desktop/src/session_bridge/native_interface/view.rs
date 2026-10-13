//! State-relative view commands over the renderer's actual camera and poses.

use bevy::prelude::{Vec3, World};
use limo_cad_interface::DocumentContext;
use serde_json::{json, Value};

use super::{model_snapshot, NativeCommand};
use crate::{
    native_viewport::{self, ViewportCamera, ViewportPresentation},
    state::AppState,
};
mod motion;
mod navigation;
pub(super) mod six_dof;
pub(super) use motion::{advance, cancel, pending, poll, request};
pub(super) use navigation::{cancel_pointer, navigate, pointer_active};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewDirection {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Isometric,
}

impl ViewDirection {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "front" => Ok(Self::Front),
            "back" => Ok(Self::Back),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            "isometric" | "iso" => Ok(Self::Isometric),
            _ => Err("Unknown view direction".into()),
        }
    }
    fn axes(self) -> (Vec3, Vec3) {
        match self {
            Self::Front => (Vec3::NEG_Y, Vec3::Z),
            Self::Back => (Vec3::Y, Vec3::Z),
            Self::Left => (Vec3::NEG_X, Vec3::Z),
            Self::Right => (Vec3::X, Vec3::Z),
            Self::Top => (Vec3::Z, Vec3::Y),
            Self::Bottom => (Vec3::NEG_Z, Vec3::NEG_Y),
            Self::Isometric => (Vec3::new(1., -1., 1.).normalize(), Vec3::Z),
        }
    }
}

pub(crate) fn clear_selection(value: &mut ViewportPresentation) {
    value.selected_origin_plane = None;
    value.hovered_origin_plane = None;
    value.selected_datum_plane_id = None;
    value.hovered_datum_plane_id = None;
    value.selected_body_ids.clear();
    value.hovered_body_id = None;
    value.selected_occurrence_id = None;
    value.hovered_occurrence_id = None;
    value.selected_face_ids.clear();
    value.hovered_face_id = None;
    value.selected_edge_ids.clear();
    value.hovered_edge_id = None;
    value.selected_sketch_entity_ids.clear();
    value.hovered_sketch_entity_id = None;
    value.constraint_related_sketch_entity_ids.clear();
    value.selected_finished_sketch_entities.clear();
    value.hovered_finished_sketch_entity = None;
    value.selected_sketch_points.clear();
    value.hovered_sketch_point = None;
    value.selected_surface_point = None;
    value.hovered_surface_point = None;
    value.selected_profiles.clear();
    value.hovered_profile = None;
}

pub(super) fn apply(
    engine: &AppState,
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    command: NativeCommand,
) -> Result<Value, String> {
    if native_viewport::interface_view(world).0 != owner.document_id {
        return Err("The rendered document is not current".into());
    }
    if matches!(command, NativeCommand::Fit | NativeCommand::Orient(_)) {
        let view = match command {
            NativeCommand::Orient(direction) => format!("{direction:?}").to_lowercase(),
            _ => "current".into(),
        };
        return request(
            world,
            owner,
            revision,
            &json!({"view":view,"fit":true,"duration_ms":300,"expires_ms":crate::session_bridge::now_ms()+5000}),
        );
    }
    let (session, camera, mut presentation, _) = native_viewport::interface_view_snapshot(world);
    match command {
        NativeCommand::ClearSelection => {
            clear_selection(&mut presentation);
            super::controller::workbench::execute(
                world,
                &super::controller::workbench::Command::Navigation(
                    super::controller::workbench::NavigationTool::Select,
                ),
            )?;
        }
        NativeCommand::SelectBody {
            body_id,
            occurrence_id,
        } => {
            let model = model_snapshot(engine);
            if !model
                .document
                .scene
                .bodies
                .iter()
                .any(|body| body.id.0 == body_id)
            {
                return Err("The selected body no longer exists".into());
            }
            if let Some(id) = occurrence_id {
                if !model
                    .instance_body_poses
                    .iter()
                    .any(|row| row.occurrence_id.0 == id && row.body_id.0 == body_id && row.visible)
                {
                    return Err("The selected occurrence is unavailable".into());
                }
            }
            clear_selection(&mut presentation);
            presentation.selected_body_ids.push(body_id);
            presentation.selected_occurrence_id = occurrence_id;
        }
        _ => return Err("The requested command is not a view operation".into()),
    }
    let selection = json!({"selected_body_ids":presentation.selected_body_ids,"selected_occurrence_id":presentation.selected_occurrence_id});
    native_viewport::apply_interface_view(world, &session, Some(camera), Some(presentation))?;
    Ok(json!({"camera":camera,"selection":selection}))
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}
impl Bounds {
    fn add(bounds: &mut Option<Self>, point: Vec3) {
        if !point.is_finite() {
            return;
        }
        if let Some(bounds) = bounds {
            bounds.min = bounds.min.min(point);
            bounds.max = bounds.max.max(point);
        } else {
            *bounds = Some(Self {
                min: point,
                max: point,
            });
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    All,
    Solids,
    Body(u64),
    Component(u64),
    ActiveSketch,
}
fn target_bounds(
    world: &World,
    model: native_viewport::ViewportGeometry<'_>,
    presentation: &ViewportPresentation,
    target: Target,
) -> Option<Bounds> {
    if let Some((min, max)) = native_viewport::section_view::bounds(world) {
        return Some(Bounds {
            min: Vec3::from_array(min),
            max: Vec3::from_array(max),
        });
    }
    let mut bounds = None;
    for body in &model.scene.bodies {
        if target == Target::ActiveSketch || matches!(target, Target::Body(id) if body.id.0 != id) {
            continue;
        }
        if presentation.hidden_body_ids.contains(&body.id.0) {
            continue;
        }
        let occurrences = native_viewport::interface_visible_occurrences(world, body.id.0);
        for occurrence in occurrences {
            if let Target::Component(id) = target {
                if !model.instance_body_poses.iter().any(|p| {
                    p.component_id.0 == id
                        && p.body_id == body.id
                        && Some(p.occurrence_id.0) == occurrence
                        && p.visible
                }) {
                    continue;
                }
            }
            let transform = native_viewport::interface_body_transform(world, body.id.0, occurrence);
            for point in body.mesh.positions.as_chunks::<3>().0 {
                Bounds::add(
                    &mut bounds,
                    transform.transform_point(Vec3::new(point[0], point[1], point[2])),
                );
            }
        }
    }
    let sketches = model
        .active_sketch
        .into_iter()
        .chain(model.finished_sketches.iter().filter(|s| {
            target == Target::All && !presentation.hidden_sketch_names.contains(&s.name)
        }))
        .filter(|_| matches!(target, Target::All | Target::ActiveSketch));
    for sketch in sketches {
        add_sketch_bounds(&mut bounds, sketch);
    }
    bounds
}

fn add_sketch_bounds(bounds: &mut Option<Bounds>, sketch: &limo_cad_sketch::SketchDto) {
    use limo_cad_sketch::EntityDto;
    let mut add = |x: f64, y: f64| {
        Bounds::add(
            bounds,
            Vec3::from_array(sketch.basis.to_3d([x, y]).map(|v| v as f32)),
        )
    };
    for entity in &sketch.entities {
        match entity {
            EntityDto::Point { position, .. } => add(position.x, position.y),
            EntityDto::Line {
                start,
                end,
                consumed,
                ..
            } => {
                if !consumed {
                    add(start.x, start.y);
                    add(end.x, end.y);
                }
            }
            EntityDto::Circle { center, radius, .. } | EntityDto::Arc { center, radius, .. } => {
                for dx in [-*radius, *radius] {
                    for dy in [-*radius, *radius] {
                        add(center.x + dx, center.y + dy);
                    }
                }
            }
            EntityDto::Spline { tessellation, .. } => {
                for p in tessellation {
                    add(p.x, p.y);
                }
            }
        }
    }
}

/// Begin frames authored sketch geometry in the canvas area left of its palette.
/// Empty sketches keep their existing plane-origin entry view.
pub(crate) fn fit_sketch_begin_camera(
    sketch: &limo_cad_sketch::SketchDto,
    camera: ViewportCamera,
    size: [f32; 2],
    reserved_right: f32,
) -> Result<Option<ViewportCamera>, String> {
    let mut bounds = None;
    add_sketch_bounds(&mut bounds, sketch);
    if bounds.is_none()
        || !reserved_right.is_finite()
        || reserved_right < 0.
        || size[0] <= reserved_right
    {
        return Ok(None);
    }
    let direction = (Vec3::from_array(camera.position) - Vec3::from_array(camera.target))
        .try_normalize()
        .ok_or("The sketch camera direction is invalid")?;
    let right = (-direction)
        .cross(Vec3::from_array(camera.up))
        .try_normalize()
        .ok_or("The sketch camera up direction is invalid")?;
    let mut fitted = fit_bounds(bounds, camera, [size[0] - reserved_right, size[1]], None)?;
    let previous_distance =
        Vec3::from_array(camera.position).distance(Vec3::from_array(camera.target));
    let distance = Vec3::from_array(fitted.position)
        .distance(Vec3::from_array(fitted.target))
        .max(previous_distance)
        .max(100.);
    // Projection still spans the full canvas: pan by half the palette width
    // so the geometry centre projects into the unobscured area, not underneath it.
    let shift = distance * (fitted.vertical_fov_degrees.to_radians() * 0.5).tan() * reserved_right
        / size[1];
    let target = Vec3::from_array(fitted.target) + right * shift;
    let position = target + direction * distance;
    if !target.is_finite() || !position.is_finite() {
        return Err("The sketch bounds exceed the camera range".into());
    }
    fitted.target = target.to_array();
    fitted.position = position.to_array();
    Ok(Some(fitted))
}

fn fit_bounds(
    bounds: Option<Bounds>,
    camera: ViewportCamera,
    size: [f32; 2],
    direction: Option<ViewDirection>,
) -> Result<ViewportCamera, String> {
    if size.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("Cannot frame a viewport without a positive size".into());
    }
    let (axis, up) = direction.map(ViewDirection::axes).unwrap_or_else(|| {
        (
            (Vec3::from_array(camera.position) - Vec3::from_array(camera.target))
                .try_normalize()
                .unwrap_or(Vec3::new(1., -1., 1.).normalize()),
            Vec3::from_array(camera.up),
        )
    });
    let Some(bounds) = bounds else {
        let home = ViewportCamera::default();
        return Ok(ViewportCamera {
            position: (axis * Vec3::from_array(home.position).length()).to_array(),
            target: [0.; 3],
            up: up.to_array(),
            ..home
        });
    };
    let half_vertical = camera.vertical_fov_degrees.to_radians() / 2.;
    if !half_vertical.is_finite()
        || half_vertical <= 0.
        || half_vertical >= std::f32::consts::FRAC_PI_2
    {
        return Err("Invalid camera field of view".into());
    }
    let half_horizontal = (half_vertical.tan() * size[0] / size[1]).atan();
    // Coincident-only bounds need a useful framing distance, not a microscopic
    // camera pose that magnifies the sketch grips' depth floor and plane lift.
    let radius = if bounds.min == bounds.max {
        1.0
    } else {
        ((bounds.max - bounds.min) * 0.5).length().max(0.01)
    };
    let distance = radius / half_horizontal.min(half_vertical).sin() * 1.15;
    let target = bounds.min * 0.5 + bounds.max * 0.5;
    let position = target + axis * distance;
    if !position.is_finite() {
        return Err("The model bounds exceed the camera range".into());
    }
    Ok(ViewportCamera {
        position: position.to_array(),
        target: target.to_array(),
        up: up.to_array(),
        ..camera
    })
}

pub(super) fn fit_camera(
    world: &World,
    model: native_viewport::ViewportGeometry<'_>,
    presentation: &ViewportPresentation,
    camera: ViewportCamera,
    size: [f32; 2],
    direction: Option<ViewDirection>,
) -> Result<ViewportCamera, String> {
    fit_bounds(
        target_bounds(world, model, presentation, Target::All),
        camera,
        size,
        direction,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_keeps_every_box_corner_inside_wide_and_portrait_views() {
        for size in [[1600., 400.], [220., 1400.], [1000., 1000.]] {
            for scale in [0.01, 1., 100_000.] {
                let bounds = Bounds {
                    min: Vec3::new(-20., -4., -8.) * scale,
                    max: Vec3::new(40., 12., 30.) * scale,
                };
                for direction in [
                    ViewDirection::Isometric,
                    ViewDirection::Top,
                    ViewDirection::Front,
                ] {
                    let camera = fit_bounds(
                        Some(bounds),
                        ViewportCamera::default(),
                        size,
                        Some(direction),
                    )
                    .unwrap();
                    let eye = Vec3::from_array(camera.position);
                    let forward = (Vec3::from_array(camera.target) - eye).normalize();
                    let right = forward.cross(Vec3::from_array(camera.up)).normalize();
                    let up = right.cross(forward);
                    let tan = (camera.vertical_fov_degrees.to_radians() / 2.).tan();
                    for x in [bounds.min.x, bounds.max.x] {
                        for y in [bounds.min.y, bounds.max.y] {
                            for z in [bounds.min.z, bounds.max.z] {
                                let p = Vec3::new(x, y, z) - eye;
                                let depth = p.dot(forward);
                                assert!(depth > 0.);
                                assert!(p.dot(right).abs() < depth * tan * size[0] / size[1]);
                                assert!(p.dot(up).abs() < depth * tan);
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn clearing_selection_preserves_model_visibility_and_simulation_state() {
        let mut p = ViewportPresentation {
            hidden_body_ids: vec![9],
            ghosted_body_ids: vec![4],
            cam_stock_visible: true,
            selected_body_ids: vec![8],
            selected_occurrence_id: Some(5),
            hovered_body_id: Some(3),
            ..Default::default()
        };
        clear_selection(&mut p);
        assert!(p.selected_body_ids.is_empty());
        assert_eq!(p.selected_occurrence_id, None);
        assert_eq!(p.hovered_body_id, None);
        assert_eq!(p.hidden_body_ids, vec![9]);
        assert_eq!(p.ghosted_body_ids, vec![4]);
        assert!(p.cam_stock_visible);
    }
}
