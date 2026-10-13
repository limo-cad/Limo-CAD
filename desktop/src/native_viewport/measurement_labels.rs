//! Screen-sized readouts owned by measurement guides, independently of previews.
use super::*;

#[derive(Component)]
pub(super) struct MeasurementLabel {
    line: usize,
    estimated_size: Vec2,
}

#[derive(Default)]
pub(super) struct Labels {
    camera: Option<Entity>,
    lines: Arc<Vec<ViewportMeasurementLine>>,
}

fn anchor(
    basis: &CameraProjectionBasis,
    viewport: (f32, f32),
    line: &ViewportMeasurementLine,
) -> Option<Vec2> {
    let mut a = Vec3::from_slice(&line.points[..3]);
    let mut b = Vec3::from_slice(&line.points[3..]);
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    let da = (a - basis.origin).dot(basis.forward);
    let db = (b - basis.origin).dot(basis.forward);
    let near = 0.001;
    if da < near && db < near {
        return None;
    }
    if da < near {
        a = a.lerp(b, (near - da) / (db - da));
    } else if db < near {
        b = b.lerp(a, (near - db) / (da - db));
    }
    let (a, _) = edge_picking::project(basis, viewport, a)?;
    let (b, _) = edge_picking::project(basis, viewport, b)?;
    let delta = b - a;
    // Center the readout on the visible part of a guide, including one whose
    // endpoints are outside the canvas. A zero-length distance still has a label.
    let mut first = 0f32;
    let mut last = 1f32;
    for axis in 0..2 {
        let limit = [viewport.0, viewport.1][axis];
        if delta[axis].abs() < 1e-6 {
            if !(0.0..=limit).contains(&a[axis]) {
                return None;
            }
        } else {
            let start = -a[axis] / delta[axis];
            let end = (limit - a[axis]) / delta[axis];
            first = first.max(start.min(end));
            last = last.min(start.max(end));
        }
    }
    (first <= last).then_some(a + delta * ((first + last) * 0.5))
}

fn position(anchor: Vec2, size: Vec2, viewport: Vec2, occupied: &[Rect]) -> Vec2 {
    let min = Vec2::splat(4.);
    let max = (viewport - size - min).max(min);
    let mut position = (anchor - Vec2::new(size.x * 0.5, size.y + 6.)).clamp(min, max);
    let preferred = position.y;
    let mut candidates = vec![preferred];
    for bounds in occupied {
        candidates.extend([bounds.min.y - size.y - 4., bounds.max.y + 4.]);
    }
    candidates.retain(|y| *y >= min.y && *y <= max.y);
    candidates.sort_by(|a, b| {
        (a - preferred)
            .abs()
            .total_cmp(&(b - preferred).abs())
            .then(a.total_cmp(b))
    });
    if let Some(y) = candidates.into_iter().find(|y| {
        occupied.iter().all(|bounds| {
            position.x >= bounds.max.x + 3.
                || position.x + size.x <= bounds.min.x - 3.
                || *y >= bounds.max.y + 3.
                || *y + size.y <= bounds.min.y - 3.
        })
    }) {
        position.y = y;
    }
    position
}

pub(super) fn update(
    mut commands: Commands,
    (presentation, camera, size): (
        Res<PresentationResource>,
        Res<CameraResource>,
        Res<ViewportSizeResource>,
    ),
    (palette, assets): (Res<PaletteResource>, Res<ViewportUiAssets>),
    mut retained: Local<Labels>,
    mut existing: Query<(Entity, &MeasurementLabel, &mut Node, &ComputedNode)>,
    cameras: Query<Entity, With<NativeOverlayCamera>>,
) {
    let overlay_camera = cameras.single().ok();
    let viewport = (size.logical_width, size.logical_height);
    let basis = camera_projection(camera.camera, viewport);
    let lines = &presentation.0.measurement_lines;
    if retained.camera != overlay_camera
        || !Arc::ptr_eq(&retained.lines, lines)
        || palette.is_changed()
        || assets.is_changed()
    {
        for (entity, ..) in &existing {
            commands.entity(entity).despawn();
        }
        retained.camera = overlay_camera;
        retained.lines = lines.clone();
        let Some(overlay_camera) = overlay_camera else {
            return;
        };
        let mut occupied = vec![];
        for (index, line) in lines.iter().enumerate() {
            let Some(text) = line.label.as_ref().filter(|text| !text.trim().is_empty()) else {
                continue;
            };
            let estimated_size = Vec2::new(text.chars().count() as f32 * 6.5 + 12., 24.);
            let anchor = basis
                .as_ref()
                .and_then(|basis| anchor(basis, viewport, line));
            let position = position(
                anchor.unwrap_or_default(),
                estimated_size,
                Vec2::new(viewport.0, viewport.1),
                &occupied,
            );
            if anchor.is_some() {
                occupied.push(Rect::from_corners(position, position + estimated_size));
            }
            commands.spawn((
                Name::new("Measurement guide value"),
                MeasurementLabel {
                    line: index,
                    estimated_size,
                },
                UiTargetCamera(overlay_camera),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(position.x),
                    top: px(position.y),
                    display: if anchor.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    },
                    padding: UiRect::axes(px(6), px(3)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BackgroundColor(rgba(palette.0.header, 0.96)),
                BorderColor::all(Color::srgba(
                    line.color[0],
                    line.color[1],
                    line.color[2],
                    0.9,
                )),
                Text::new(text.clone()),
                ViewportUiTheme::from_palette(&palette.0).text(&assets, 11., FontWeight::SEMIBOLD),
                TextLayout::new(Justify::Center, bevy::text::LineBreak::NoWrap),
                TextColor(rgb(palette.0.ink)),
                ZIndex(19),
                bevy::picking::Pickable::IGNORE,
            ));
        }
        return;
    }
    let mut positions = vec![];
    for (entity, label, _, computed) in &existing {
        let anchor = basis
            .as_ref()
            .and_then(|basis| anchor(basis, viewport, &lines[label.line]));
        let measured_size = computed.size() * computed.inverse_scale_factor();
        let size = if measured_size.min_element() > 0. {
            measured_size
        } else {
            label.estimated_size
        };
        positions.push((label.line, entity, anchor, size));
    }
    positions.sort_by_key(|(line, ..)| *line);
    let mut occupied = vec![];
    for (_, entity, anchor, size) in positions {
        let (_, _, mut node, _) = existing.get_mut(entity).unwrap();
        let mut updated = node.clone();
        updated.display = if anchor.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(anchor) = anchor {
            let position = position(anchor, size, Vec2::new(viewport.0, viewport.1), &occupied);
            updated.left = px(position.x);
            updated.top = px(position.y);
            occupied.push(Rect::from_corners(position, position + size));
        }
        node.set_if_neq(updated);
    }
}

#[cfg(test)]
#[path = "measurement_labels/tests.rs"]
mod tests;
