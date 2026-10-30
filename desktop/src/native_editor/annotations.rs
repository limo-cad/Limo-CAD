//! Dimension leaders and editable labels, projected by the same native camera
//! as sketch geometry. Cache model data; camera changes only redo projection.
use super::*;
fn annotation_keys() -> Vec<limo_cad_interface::KeyChord> {
    use limo_cad_interface::KeyChord;
    vec![
        KeyChord::plain("ContextMenu"),
        KeyChord {
            shift: true,
            ..KeyChord::plain("F10")
        },
    ]
}
use crate::native_viewport::interface_shell::ribbon::Icon;
use crate::native_viewport::{interface_shell, ViewportColorRole, ViewportLineLayer};
use crate::session_bridge::native_interface::controller::chrome::{rect, Widgets};
use limo_cad_sketch::{ConstraintKind, DimensionDto, EntityDto, Vec2 as Point};

mod layout;
pub(crate) mod teaching;

#[cfg(test)]
#[path = "annotations/context_tests.rs"]
mod context_tests;
#[cfg(test)]
#[path = "annotations/layout_tests.rs"]
mod layout_tests;
#[cfg(test)]
#[path = "annotations/leader_motion_tests.rs"]
mod leader_motion_tests;
#[cfg(test)]
#[path = "annotations/theme_tests.rs"]
mod theme_tests;

fn readable_angle(a: [f32; 2], b: [f32; 2]) -> f32 {
    let mut angle = (b[1] - a[1]).atan2(b[0] - a[0]);
    if angle > std::f32::consts::FRAC_PI_2 {
        angle -= std::f32::consts::PI;
    }
    if angle < -std::f32::consts::FRAC_PI_2 {
        angle += std::f32::consts::PI;
    }
    angle
}
#[derive(Resource, Default)]
struct AnnotationState {
    stamp: Option<Stamp>,
    sketch: Option<SketchDto>,
    view: Vec<u32>,
    widgets: Widgets,
}

pub(super) fn geometry_tool_active(editor: &Editor) -> bool {
    editor.draft.tool.is_some()
        || editor.interaction.dimension.is_some()
        || editor.interaction.reposition_dimension.is_some()
        || editor.interaction.relation.is_some()
        || editor.interaction.modify.is_some()
        || editor.interaction.form.is_some()
}

pub(super) fn editing_dimension(editor: &Editor, id: limo_cad_sketch::ConstraintId) -> bool {
    editor.interaction.dimension.is_some() && editor.interaction.dimension_id == Some(id)
}

fn annotation_input(world: &mut World, entity: Entity, geometry_tool: bool) {
    if world
        .get::<interface_shell::InterfaceCanvasAnnotation>(entity)
        .is_none()
    {
        world
            .entity_mut(entity)
            .insert(interface_shell::InterfaceCanvasAnnotation);
    }
    if geometry_tool {
        if world
            .get::<interface_shell::InterfacePointerPassthrough>(entity)
            .is_none()
        {
            world
                .entity_mut(entity)
                .insert(interface_shell::InterfacePointerPassthrough);
        }
    } else if world
        .get::<interface_shell::InterfacePointerPassthrough>(entity)
        .is_some()
    {
        world
            .entity_mut(entity)
            .remove::<interface_shell::InterfacePointerPassthrough>();
    }
}

fn projected(p: Point, a: Point, b: Point) -> Point {
    let d = b - a;
    let n = d.dot(d);
    if n < 1e-20 {
        a
    } else {
        a + d * ((p - a).dot(d) / n)
    }
}
fn radial(e: &EntityDto) -> Option<(Point, f64)> {
    match e {
        EntityDto::Arc { center, radius, .. } | EntityDto::Circle { center, radius, .. } => {
            Some((*center, *radius))
        }
        _ => None,
    }
}
fn line(e: &EntityDto) -> Option<(Point, Point)> {
    match e {
        EntityDto::Line { start, end, .. } => Some((*start, *end)),
        _ => None,
    }
}
fn anchor(e: &EntityDto) -> Point {
    match e {
        EntityDto::Point { position, .. } => *position,
        EntityDto::Line { start, end, .. } => (*start + *end) * 0.5,
        EntityDto::Arc { center, .. } | EntityDto::Circle { center, .. } => *center,
        EntityDto::Spline { points, .. } => points.first().copied().unwrap_or(Point::ZERO),
    }
}
fn relation_icon(kind: &str) -> &'static str {
    match kind {
        "horizontal" | "vertical" | "horizontal_points" | "vertical_points" => "hv",
        "parallel" => "parallel",
        "perpendicular" => "perpendicular",
        "tangent" => "tangent",
        "equal" | "equal_distance" => "equal",
        "fix" => "fix",
        "midpoint" | "reference_midpoint" | "span_midpoint" => "midpointC",
        "concentric" => "concentric",
        "collinear" => "collinear",
        "symmetry" => "symmetry",
        _ => "coincident",
    }
}

/// Pure annotation geometry. Extension endpoints come from authoritative
/// entities, not the formatted value (which may be rounded or a reference).
#[cfg(test)]
fn leaders(dim: &DimensionDto, entities: &[EntityDto], arrow: f64) -> Vec<[Point; 2]> {
    let refs: Vec<_> = dim
        .entities
        .iter()
        .filter_map(|id| entities.iter().find(|e| e.id() == *id))
        .collect();
    if refs.len() != dim.entities.len() {
        return vec![];
    }
    leaders_for(dim, &refs, arrow)
}

fn angle_geometry(refs: &[&EntityDto]) -> Option<(Point, f64, f64)> {
    if let [EntityDto::Arc {
        center,
        radius,
        start_angle,
        end_angle,
        ..
    }] = refs
    {
        if ![center.x, center.y, *radius, *start_angle, *end_angle]
            .into_iter()
            .all(f64::is_finite)
            || *radius <= 0.
        {
            return None;
        }
        let delta = end_angle - start_angle;
        if !delta.is_finite() {
            return None;
        }
        // Match the sketch's authoritative arc_span convention, including
        // wrapped, reflex and full-turn arcs. Never infer geometry from text.
        let sweep = if delta.abs() >= std::f64::consts::TAU - 1e-9 {
            std::f64::consts::TAU
        } else {
            delta.rem_euclid(std::f64::consts::TAU)
        };
        return (sweep > 1e-12).then_some((*center, *start_angle, sweep));
    }
    let [first, second] = refs else { return None };
    let ((a, b), (c, d)) = (line(first)?, line(second)?);
    let u = b - a;
    let v = d - c;
    let cross = u.x * v.y - u.y * v.x;
    if cross.abs() < 1e-12 {
        return None;
    }
    let t = ((c.x - a.x) * v.y - (c.y - a.y) * v.x) / cross;
    let start = u.y.atan2(u.x);
    let mut sweep = (v.y.atan2(v.x) - start).rem_euclid(std::f64::consts::TAU);
    if sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    Some((a + u * t, start, sweep))
}

fn distance_endpoints(refs: &[&EntityDto], position: Point) -> Option<(Point, Point)> {
    match refs {
        [EntityDto::Line { start, end, .. }] => Some((*start, *end)),
        [EntityDto::Point { position: a, .. }, EntityDto::Point { position: b, .. }] => {
            Some((*a, *b))
        }
        [EntityDto::Point { position, .. }, other] | [other, EntityDto::Point { position, .. }] => {
            line(other).map(|(a, b)| (*position, projected(*position, a, b)))
        }
        [a, b] => {
            if let (Some((a, _)), Some((c, d))) = (line(a), line(b)) {
                Some((a, projected(a, c, d)))
            } else if let (Some((a, r)), Some((b, s))) = (radial(a), radial(b)) {
                let delta = position - a;
                let u = delta * (1. / delta.length().max(1e-12));
                Some((a + u * r, b + u * s))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn leaders_for(dim: &DimensionDto, refs: &[&EntityDto], arrow: f64) -> Vec<[Point; 2]> {
    let mut out = vec![];
    let mut arrow_at = |tip: Point, toward: Point| {
        let delta = toward - tip;
        let length = delta.length();
        if length < 1e-12 {
            return;
        }
        let d = delta * (arrow / length);
        let n = Point::new(-d.y, d.x) * 0.4;
        out.extend([[tip, tip + d + n], [tip, tip + d - n]]);
    };
    if matches!(dim.kind.as_str(), "radius" | "diameter") {
        if let Some((center, radius)) = refs.first().and_then(|e| radial(e)) {
            let d = dim.text_pos - center;
            let length = d.length();
            let mut unit = if length > 1e-12 {
                d * (1. / length)
            } else {
                Point::new(1., 0.)
            };
            if dim.kind == "radius" {
                if let Some(EntityDto::Arc {
                    start_angle,
                    end_angle,
                    ..
                }) = refs.first().copied()
                {
                    let angle = start_angle
                        + (end_angle - start_angle).rem_euclid(std::f64::consts::TAU) * 0.5;
                    unit = Point::new(angle.cos(), angle.sin());
                }
            }
            let edge = center + unit * radius;
            arrow_at(edge, dim.text_pos);
            let start = if dim.kind == "diameter" {
                center - unit * radius
            } else {
                center
            };
            if dim.kind == "diameter" {
                arrow_at(start, edge);
            }
            out.extend([[start, edge], [edge, dim.text_pos]]);
        }
        return out;
    }
    if dim.kind == "angle" {
        let Some((center, start, sweep)) = angle_geometry(refs) else {
            return out;
        };
        let radius = dim.text_pos.distance(center);
        if radius < 1e-8 {
            return out;
        }
        let at = |t: f64| {
            center + Point::new((start + sweep * t).cos(), (start + sweep * t).sin()) * radius
        };
        arrow_at(at(0.), at(0.1));
        arrow_at(at(1.), at(0.9));
        for i in 0..48 {
            out.push([at(i as f64 / 48.), at((i + 1) as f64 / 48.)]);
        }
        return out;
    }
    if dim.kind != "distance" {
        return out;
    }
    if let Some((a, b)) = distance_endpoints(refs, dim.text_pos) {
        let d = b - a;
        let len = d.length();
        if len < 1e-12 {
            return out;
        }
        let u = d * (1. / len);
        let n = Point::new(-u.y, u.x);
        let offset = n * (dim.text_pos - a).dot(n);
        let p = a + offset;
        let q = b + offset;
        arrow_at(p, q);
        arrow_at(q, p);
        out.extend([[a, p], [b, q], [p, q]]);
    }
    out
}

fn with_operands<T>(
    dim: &DimensionDto,
    sketch: &SketchDto,
    use_operands: impl FnOnce(&[&EntityDto]) -> T,
) -> Option<T> {
    let mut refs = dim
        .entities
        .iter()
        .map(|id| sketch.entities.iter().find(|entity| entity.id() == *id))
        .collect::<Option<Vec<_>>>()?;
    let reference = if let Some(edge) = dim.reference_edge {
        let reference = sketch.projected_edges.iter().find(|e| e.edge_id == edge)?;
        let (start, end) = reference.straight_line()?;
        // Borrow a presentation operand without copying the sketch or adding geometry.
        let id = limo_cad_sketch::EntityId(reference.id);
        Some(EntityDto::Line {
            id,
            start_id: id,
            end_id: id,
            start,
            end,
            consumed: false,
            fully_defined: true,
        })
    } else {
        None
    };
    if let Some(reference) = &reference {
        refs.push(reference);
    }
    Some(use_operands(&refs))
}

fn dimension_leaders(dim: &DimensionDto, sketch: &SketchDto, arrow: f64) -> Vec<[Point; 2]> {
    with_operands(dim, sketch, |refs| leaders_for(dim, refs, arrow)).unwrap_or_default()
}

/// Share the leader's actual operands so placement cannot change its measurement.
pub(super) fn preview_position(
    dim: &DimensionDto,
    sketch: &SketchDto,
    cursor: Point,
) -> Option<Point> {
    if !cursor.x.is_finite() || !cursor.y.is_finite() {
        return None;
    }
    let position = with_operands(dim, sketch, |refs| match dim.kind.as_str() {
        "distance" => {
            let (a, b) = distance_endpoints(refs, dim.text_pos)?;
            let span = b - a;
            let length = span.length();
            if length < 1e-12 {
                return None;
            }
            let normal = span.perp() * (1. / length);
            let middle = (a + b) * 0.5;
            Some(middle + normal * (cursor - middle).dot(normal))
        }
        "angle" => {
            let (center, start, sweep) = angle_geometry(refs)?;
            let angle = start + sweep * 0.5;
            let radius = cursor.distance(center);
            (radius > 1e-8).then_some(center + Point::new(angle.cos(), angle.sin()) * radius)
        }
        "radius" | "diameter" => Some(cursor),
        _ => None,
    })??;
    (position.x.is_finite() && position.y.is_finite()).then_some(position)
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    editor: &Editor,
    canvas: InterfaceRect,
) -> Result<(), String> {
    let mut state = world
        .remove_resource::<AnnotationState>()
        .unwrap_or_default();
    let result = (|| {
        let (_, view, _, size) = native_viewport::interface_view(world);
        let visibility = palette::visibility(world);
        let geometry_tool = geometry_tool_active(editor);
        let mut key: Vec<_> = view
            .position
            .into_iter()
            .chain(view.target)
            .chain(view.up)
            .chain([view.vertical_fov_degrees])
            .chain(size)
            .chain([canvas.x as f32, canvas.y as f32])
            .map(f32::to_bits)
            .collect();
        let selected = editor
            .interaction
            .constraint
            .as_ref()
            .map(|c| c.id.0)
            .unwrap_or(u64::MAX);
        key.extend([selected as u32, (selected >> 32) as u32]);
        let editing = editor
            .interaction
            .dimension_id
            .map(|id| id.0)
            .unwrap_or(u64::MAX);
        key.extend([editing as u32, (editing >> 32) as u32]);
        if let Some(preview) = &editor.interaction.dimension_preview {
            key.extend([editor.form_serial as u32, (editor.form_serial >> 32) as u32]);
            for position in [preview.text_pos.x, preview.text_pos.y] {
                let bits = position.to_bits();
                key.extend([bits as u32, (bits >> 32) as u32]);
            }
        }
        key.extend([
            visibility.hide_dimensions as u32,
            visibility.hide_constraints as u32,
            geometry_tool as u32,
            world
                .get_resource::<bevy::ui::UiScale>()
                .map_or(1., |scale| scale.0)
                .to_bits(),
        ]);
        let appearance = native_viewport::ui::appearance_revision(world);
        key.extend([appearance as u32, (appearance >> 32) as u32]);
        if state.stamp == editor.stamp && state.view == key {
            return Ok(());
        }
        if state.stamp != editor.stamp {
            state.sketch = active(&services.engine)?;
            state.stamp = editor.stamp.clone();
        }
        state.view = key;
        state.widgets.begin();
        let dimension = native_viewport::ui::palette(world).dimension;
        let mut segments = vec![];
        if let Some(sketch) = &state.sketch {
            let (_, _, _, size) = native_viewport::interface_view(world);
            let offset = [canvas.x as f32, canvas.y as f32];
            // Dimensions keep their preferred position first. Constraint
            // badges then share the same rotation-aware collision layout.
            let mut layout = layout::Layout::new(size);
            for dim in
                sketch
                    .dimensions
                    .iter()
                    .filter(|_| !visibility.hide_dimensions)
                    .map(|stored| {
                        editor
                            .interaction
                            .dimension_preview
                            .as_ref()
                            .filter(|preview| {
                                editor.interaction.dimension_tool
                                    && preview.constraint_id == stored.constraint_id
                            })
                            .unwrap_or(stored)
                    })
                    .chain(editor.interaction.dimension_preview.iter().filter(|d| {
                        d.constraint_id.0 == 0 && editor.interaction.dimension.is_some()
                    }))
            {
                let world_point = |p: Point| sketch.basis.to_3d([p.x, p.y]);
                let Some(screen) = native_viewport::interface_world_point(
                    world,
                    &owner.document_id,
                    world_point(dim.text_pos),
                )?
                else {
                    continue;
                };
                if screen[0] < 0. || screen[1] < 0. || screen[0] > size[0] || screen[1] > size[1] {
                    continue;
                }
                let mut unit = 0.01_f32;
                for axis in [Point::new(1., 0.), Point::new(0., 1.)] {
                    if let Some(p) = native_viewport::interface_world_point(
                        world,
                        &owner.document_id,
                        world_point(dim.text_pos + axis),
                    )? {
                        unit = unit.max((p[0] - screen[0]).hypot(p[1] - screen[1]));
                    }
                }
                let mut lines = dimension_leaders(dim, sketch, 6. / f64::from(unit));
                if dim.constraint_id.0 == 0 {
                    for pair in lines {
                        for point in pair {
                            segments.extend(world_point(point).map(|v| v as f32));
                        }
                    }
                    continue;
                }
                let angle = if sketch.dimension_style == limo_cad_core::DimensionStyle::Aligned
                    && dim.kind != "angle"
                {
                    if let Some([a, b]) = lines.last() {
                        match (
                            native_viewport::interface_world_point(
                                world,
                                &owner.document_id,
                                world_point(*a),
                            )?,
                            native_viewport::interface_world_point(
                                world,
                                &owner.document_id,
                                world_point(*b),
                            )?,
                        ) {
                            (Some(a), Some(b)) => readable_angle(a, b),
                            _ => 0.,
                        }
                    } else {
                        0.
                    }
                } else {
                    0.
                };
                let w = (dim.text.chars().count() as f32 * 7. + 12.).max(36.);
                let preferred = [screen[0] + angle.sin() * 12., screen[1] - angle.cos() * 12.];
                let Some(center) = layout.place(preferred, [w, 20.], angle) else {
                    continue;
                };
                if center != preferred {
                    // This connector is presentation-only: keep the stored
                    // anchor and exact measured extension endpoints intact.
                    if let Some(point) = native_viewport::interface_sketch_point(
                        world,
                        &owner.document_id,
                        [
                            screen[0] + center[0] - preferred[0],
                            screen[1] + center[1] - preferred[1],
                        ],
                        sketch.basis,
                    )? {
                        lines.push([dim.text_pos, point]);
                    }
                }
                for pair in lines {
                    for point in pair {
                        segments.extend(world_point(point).map(|v| v as f32));
                    }
                }
                let mut bounds = rect(
                    offset[0] + center[0] - w / 2.,
                    offset[1] + center[1] - 10.,
                    w,
                    20.,
                );
                bounds.justify_content = JustifyContent::Center;
                let mut c = InterfaceControl::button(
                    "sketch/dimension",
                    format!("Edit dimension {}: {}", dim.constraint_id.0, dim.text),
                );
                // The first click opens the value editor. Keep that label
                // interactive so the second click cannot fall through to the
                // canvas and blur the editor it just opened.
                let blocked = geometry_tool && !editing_dimension(editor, dim.constraint_id);
                c.disabled = blocked;
                c.owned_keys = annotation_keys();
                let entity = state.widgets.button(
                    world,
                    camera,
                    &format!("dim-{}", dim.constraint_id.0),
                    c,
                    Some(&dim.text),
                    NativeCommand::Sketch(EditorCommand::Interaction(
                        InteractionCommand::EditDimension(dim.constraint_id),
                    )),
                    bounds,
                    None,
                    22,
                )?;
                annotation_input(world, entity, blocked);
                interface_shell::dimension_label(
                    world,
                    entity,
                    Color::srgb(dimension[0], dimension[1], dimension[2]),
                );
                world
                    .entity_mut(entity)
                    .insert(UiTransform::from_rotation(Rot2::radians(angle)));
            }
            for constraint in sketch
                .constraints
                .iter()
                .filter(|_| !visibility.hide_constraints)
            {
                if constraint.constraint.kind() != ConstraintKind::Geometric {
                    continue;
                }
                let ids = constraint.constraint.referenced_entities();
                let positions: Vec<_> = ids
                    .iter()
                    .filter_map(|id| sketch.entities.iter().find(|e| e.id() == *id))
                    .map(anchor)
                    .collect();
                if positions.is_empty() {
                    continue;
                }
                let center = positions.iter().fold(Point::ZERO, |a, b| a + *b)
                    * (1. / positions.len() as f64);
                let Some(screen) = native_viewport::interface_world_point(
                    world,
                    &owner.document_id,
                    sketch.basis.to_3d([center.x, center.y]),
                )?
                else {
                    continue;
                };
                if screen[0] < 0. || screen[1] < 0. || screen[0] > size[0] || screen[1] > size[1] {
                    continue;
                }
                let Some([x, y]) = layout.place([screen[0], screen[1] + 14.], [22., 22.], 0.)
                else {
                    continue;
                };
                let kind = constraint.constraint.kind_str();
                let mut control = InterfaceControl::button(
                    "sketch/constrain",
                    format!("Constraint {}: {}", constraint.id.0, kind.replace('_', " ")),
                );
                control.selected = Some(selected == constraint.id.0);
                control.disabled = geometry_tool;
                control.owned_keys = annotation_keys();
                let entity = state.widgets.button(
                    world,
                    camera,
                    &format!("constraint-{}", constraint.id.0),
                    control,
                    Some(""),
                    NativeCommand::Sketch(EditorCommand::Interaction(
                        InteractionCommand::ConstraintInfo(constraint.id),
                    )),
                    rect(offset[0] + x - 11., offset[1] + y - 11., 22., 22.),
                    Some(Icon::Relation(relation_icon(kind))),
                    22,
                )?;
                annotation_input(world, entity, geometry_tool);
            }
        }
        state.widgets.finish(world);
        native_viewport::apply_interface_sketch_lines(
            world,
            &owner.document_id,
            if segments.is_empty() {
                vec![]
            } else {
                vec![ViewportLineLayer {
                    color: [1.; 4],
                    color_role: ViewportColorRole::SketchDimension,
                    width: 1.5,
                    segments: segments.into(),
                    ..default()
                }]
            },
        )
    })();
    world.insert_resource(state);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aligned_labels_follow_the_projected_line_and_remain_upright() {
        let q = std::f32::consts::FRAC_PI_4;
        assert!((readable_angle([100., 30.], [110., 40.]) - q).abs() < 1e-6);
        assert!((readable_angle([110., 40.], [100., 30.]) - q).abs() < 1e-6);
        assert!((readable_angle([2., 4.], [2., 8.]) - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }
    #[test]
    fn radius_leader_touches_the_actual_arc_even_when_the_label_is_on_its_missing_half() {
        let arc: EntityDto = serde_json::from_value(json!({"kind":"arc","id":3,"center":{"x":0.,"y":0.},"radius":10.,"start_angle":0.,"end_angle":std::f64::consts::PI,"fully_defined":false})).unwrap();
        let dim: DimensionDto = serde_json::from_value(json!({"constraint_id":1,"mode":"driving","kind":"radius","entities":[3],"value":10.,"text":"R10.00","text_pos":{"x":0.,"y":-15.}})).unwrap();
        let lines = leaders(&dim, &[arc], 0.5);
        let [_, tip] = lines[lines.len() - 2];
        assert!(tip.x.abs() < 1e-10 && (tip.y - 10.).abs() < 1e-10);
    }
    #[test]
    fn dimension_leaders_use_model_endpoints_instead_of_rounded_text() {
        let e = EntityDto::Line {
            id: limo_cad_sketch::EntityId(3),
            start_id: limo_cad_sketch::EntityId(1),
            end_id: limo_cad_sketch::EntityId(2),
            start: Point::new(0., 0.),
            end: Point::new(12.345, 0.),
            fully_defined: false,
            consumed: false,
        };
        let d:DimensionDto=serde_json::from_value(json!({"constraint_id":1,"mode":"driving","kind":"distance","entities":[3],"value":12.345,"text":"12.35","text_pos":{"x":5.,"y":-4.}})).unwrap();
        let lines = leaders(&d, &[e], 0.5);
        assert!(lines.contains(&[Point::new(0., -4.), Point::new(12.345, -4.)]));
    }
}
