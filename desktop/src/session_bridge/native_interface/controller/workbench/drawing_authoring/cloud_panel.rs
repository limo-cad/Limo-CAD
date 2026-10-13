use super::super::*;
use super::runtime::{Command, Editor, Tool};
use bevy::ui::UiTransform;
use limo_cad_sketch::DrawingAnnotationDto;

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    e: &mut Editor,
    paper: Entity,
    transform: drawing_navigation::PaperTransform,
    state: &Workbench,
) -> Result<(), String> {
    let accent = crate::native_viewport::ui::theme(world).accent;
    if e.tool == Some(Tool::RevisionCloud) {
        let points = e.cloud.points.clone();
        let mut remaining = 4096;
        for (edge, pair) in points.windows(2).enumerate() {
            let [a, b] = [pair[0], pair[1]];
            let d = [b[0] - a[0], b[1] - a[1]];
            let length = d[0].hypot(d[1]);
            if length < 1e-8 {
                continue;
            }
            let count = ((length / 3.).ceil() as usize).min(remaining);
            remaining -= count;
            let direction = d.map(|v| v / length);
            for index in 0..count {
                let start = index as f64 * 3.;
                let end = (start + 2.).min(length);
                let center = std::array::from_fn(|i| a[i] + direction[i] * (start + end) * 0.5);
                decoration(
                    (world, camera, e),
                    paper,
                    &format!("cloud-preview-{edge}-{index}"),
                    (center, [end - start, 0.55], d[1].atan2(d[0]) as f32),
                    transform,
                    accent,
                    false,
                );
            }
        }
        for (index, point) in points.into_iter().enumerate() {
            let radius = if index == 0 { 1.6 } else { 1.1 };
            decoration(
                (world, camera, e),
                paper,
                &format!("cloud-preview-point-{index}"),
                (point, [radius * 2.; 2], 0.),
                transform,
                Color::WHITE,
                true,
            );
        }
    }
    if e.tool.is_some() {
        return Ok(());
    }
    let Some((_, sheet, _)) = &state.paper_key else {
        return Ok(());
    };
    let edge_count: usize = sheet
        .annotations
        .iter()
        .filter_map(|a| match a {
            DrawingAnnotationDto::RevisionCloud { points, .. } => Some(points.len()),
            _ => None,
        })
        .sum();
    if edge_count > 4096 {
        return Ok(());
    }
    let clouds: Vec<_> = sheet
        .annotations
        .iter()
        .filter_map(|a| match a {
            DrawingAnnotationDto::RevisionCloud { id, points, .. } => Some((*id, points.clone())),
            _ => None,
        })
        .collect();
    for (id, points) in clouds {
        for edge in 0..points.len() {
            let [a, b] = [points[edge], points[(edge + 1) % points.len()]];
            let d = [b[0] - a[0], b[1] - a[1]];
            let length = d[0].hypot(d[1]);
            if !length.is_finite() || length < 1e-8 {
                continue;
            }
            let center = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let screen = transform.to_screen(center);
            let half = [d[0].abs() * 0.5 + 4.5, d[1].abs() * 0.5 + 4.5];
            if screen[0] + half[0] * transform.scale < transform.clip.x
                || screen[1] + half[1] * transform.scale < transform.clip.y
                || screen[0] - half[0] * transform.scale > transform.clip.x + transform.clip.width
                || screen[1] - half[1] * transform.scale > transform.clip.y + transform.clip.height
            {
                continue;
            }
            let key = format!("drawing-cloud-{id}-edge-{edge}");
            let mut control = InterfaceControl::button(
                "drawing/annotation",
                format!("Revision cloud {id} edge {}", edge + 1),
            );
            control.selected = Some(e.selected == Some(id));
            let entity = super::panel::target(
                (world, camera, e),
                &key,
                control,
                Command::CloudEdge(id, edge),
                rect(
                    ((center[0] - (length + 6.) * 0.5) * transform.scale) as f32,
                    ((center[1] - 4.5) * transform.scale) as f32,
                    ((length + 6.) * transform.scale) as f32,
                    (9. * transform.scale) as f32,
                ),
                Color::NONE,
                18,
            )?;
            e.widgets.parent(world, &key, paper);
            world
                .entity_mut(entity)
                .insert(UiTransform::from_rotation(Rot2::radians(
                    d[1].atan2(d[0]) as f32
                )));
        }
    }
    Ok(())
}

fn decoration(
    (world, camera, e): (&mut World, Entity, &mut Editor),
    paper: Entity,
    key: &str,
    (center, size, angle): ([f64; 2], [f64; 2], f32),
    transform: drawing_navigation::PaperTransform,
    color: Color,
    circle: bool,
) {
    let node = Node {
        border_radius: if circle {
            BorderRadius::all(percent(50.))
        } else {
            default()
        },
        border: if circle {
            UiRect::all(px((0.55 * transform.scale) as f32))
        } else {
            default()
        },
        ..rect(
            ((center[0] - size[0] * 0.5) * transform.scale) as f32,
            ((center[1] - size[1] * 0.5) * transform.scale) as f32,
            (size[0] * transform.scale) as f32,
            (size[1] * transform.scale) as f32,
        )
    };
    e.widgets.panel(world, camera, key, node, color, 20);
    let entity = e.widgets.entity(key).unwrap();
    let accent = crate::native_viewport::ui::theme(world).accent;
    world
        .entity_mut(entity)
        .remove::<interface_shell::InterfaceOccluder>()
        .insert((
            UiTransform::from_rotation(Rot2::radians(angle)),
            BorderColor::all(accent),
        ));
    e.widgets.parent(world, key, paper);
}
