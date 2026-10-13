//! Small paper controls over existing painted center strokes, with selected
//! extension grips matching the existing drawing workspace.
use super::super::*;
use super::{
    center,
    runtime::{Command, Editor, Tool},
};
use bevy::ui::UiTransform;
use limo_cad_sketch::DrawingAnnotationDto;

pub(super) fn geometry(
    world: &World,
    state: &Workbench,
    e: &Editor,
    id: u64,
) -> Option<center::Geometry> {
    let sheet = e
        .document
        .sheets
        .iter()
        .find(|s| Some(s.id) == e.stamp.as_ref().map(|s| s.sheet_id))?;
    let annotation = e
        .drag
        .as_ref()
        .filter(|d| d.draft.selection().annotation_id == id)
        .map(|d| d.draft.annotation())
        .or_else(|| sheet.annotations.iter().find(|a| a.id() == id))?;
    let view_id = match annotation {
        DrawingAnnotationDto::CenterMark { view_id, .. }
        | DrawingAnnotationDto::CenterLine { view_id, .. }
        | DrawingAnnotationDto::CenterLineBetweenEdges { view_id, .. }
        | DrawingAnnotationDto::AutomaticSymmetryAxis { view_id, .. }
        | DrawingAnnotationDto::BoltCircleCenterLine { view_id, .. } => *view_id,
        _ => return None,
    };
    drawing_paper::with_projections(world, state, |projections, _| {
        let (view, projection) = projections.get(&view_id)?;
        center::geometry(annotation, view, projection)
    })
    .flatten()
}
pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    e: &mut Editor,
    paper: Entity,
    transform: drawing_navigation::PaperTransform,
    state: &Workbench,
) -> Result<(), String> {
    let accent = crate::native_viewport::ui::theme(world).accent;
    if matches!(e.tool, Some(Tool::CenterMark | Tool::CenterLine)) {
        for index in 0..e.centers.len() {
            let target = &e.centers[index];
            if transform.pick(transform.to_screen(target.center)).is_none() {
                continue;
            }
            let selected = e.center.selected(target);
            let radius = (1.5 * transform.scale).max(4.);
            let bounds = Node {
                border_radius: BorderRadius::all(percent(50.)),
                ..rect(
                    (target.center[0] * transform.scale - radius) as f32,
                    (target.center[1] * transform.scale - radius) as f32,
                    (radius * 2.) as f32,
                    (radius * 2.) as f32,
                )
            };
            let mut control = InterfaceControl::button(
                "drawing/centers",
                format!("View {} circular center {}", target.view_id, index + 1),
            );
            control.selected = Some(selected);
            let key = format!("drawing-center-target-{index}");
            super::panel::target(
                (world, camera, e),
                &key,
                control,
                Command::Center(index),
                bounds,
                accent.with_alpha(if selected { 1. } else { 0.65 }),
                21,
            )?;
            e.widgets.parent(world, &key, paper);
        }
        return Ok(());
    }
    if e.tool.is_some() {
        return Ok(());
    }
    let Some(sheet) = e
        .document
        .sheets
        .iter()
        .find(|s| Some(s.id) == e.stamp.as_ref().map(|s| s.sheet_id))
    else {
        return Ok(());
    };
    let ids: Vec<_> = sheet
        .annotations
        .iter()
        .filter(|a| {
            matches!(
                a,
                DrawingAnnotationDto::CenterMark { .. }
                    | DrawingAnnotationDto::CenterLine { .. }
                    | DrawingAnnotationDto::CenterLineBetweenEdges { .. }
                    | DrawingAnnotationDto::AutomaticSymmetryAxis { .. }
                    | DrawingAnnotationDto::BoltCircleCenterLine { .. }
            )
        })
        .map(|a| a.id())
        .collect();
    let circles = drawing_paper::with_projections(world, state, |projections, _| {
        projections
            .values()
            .map(|(_, p)| p.circles.len().saturating_add(p.anchors.len()))
            .sum::<usize>()
    })
    .unwrap_or(0);
    let references = sheet
        .annotations
        .iter()
        .map(|a| match a {
            DrawingAnnotationDto::BoltCircleCenterLine { features, .. } => features.len(),
            DrawingAnnotationDto::CenterMark { .. }
            | DrawingAnnotationDto::CenterLine { .. }
            | DrawingAnnotationDto::CenterLineBetweenEdges { .. }
            | DrawingAnnotationDto::AutomaticSymmetryAxis { .. } => 4,
            _ => 0,
        })
        .sum::<usize>();
    if references > 4096 || references.saturating_mul(circles) > 2_000_000 {
        return Err("Too many center annotation references on this sheet".into());
    }
    for id in ids {
        let Some(g) = geometry(world, state, e, id) else {
            continue;
        };
        for (part, [a, b]) in g.segments.iter().copied().enumerate() {
            let delta = [b[0] - a[0], b[1] - a[1]];
            let center = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let length = delta[0].hypot(delta[1]) * transform.scale;
            let thickness = (2. * transform.scale).max(6.);
            let key = format!("drawing-center-{id}-stroke-{part}");
            let mut control = InterfaceControl::button(
                "drawing/annotation",
                if part == 0 {
                    format!("Edit annotation {id}")
                } else {
                    format!("Edit annotation {id} stroke {}", part + 1)
                },
            );
            control.selected = Some(e.selected == Some(id));
            let entity = {
                let fill = if e.selected == Some(id) {
                    accent.with_alpha(0.12)
                } else {
                    Color::NONE
                };
                super::panel::target(
                    (world, camera, e),
                    &key,
                    control,
                    Command::Select(id),
                    rect(
                        (center[0] * transform.scale - length * 0.5) as f32,
                        (center[1] * transform.scale - thickness * 0.5) as f32,
                        length as f32,
                        thickness as f32,
                    ),
                    fill,
                    19,
                )
            }?;
            e.widgets.parent(world, &key, paper);
            world
                .entity_mut(entity)
                .insert(UiTransform::from_rotation(Rot2::radians(
                    delta[1].atan2(delta[0]) as f32,
                )));
        }
        if e.selected == Some(id) {
            for (index, grip) in g.grips.iter().enumerate() {
                let radius = (1.2 * transform.scale).max(4.);
                let key = format!("drawing-center-{id}-grip-{index}");
                let entity = super::panel::target(
                    (world, camera, e),
                    &key,
                    InterfaceControl::button(
                        "drawing/annotation",
                        format!("Center extension {id} grip {}", index + 1),
                    ),
                    Command::CenterGrip(id, index),
                    Node {
                        border: UiRect::all(px(1.)),
                        ..rect(
                            (grip.point[0] * transform.scale - radius) as f32,
                            (grip.point[1] * transform.scale - radius) as f32,
                            (radius * 2.) as f32,
                            (radius * 2.) as f32,
                        )
                    },
                    Color::WHITE,
                    22,
                )?;
                world.entity_mut(entity).insert(BorderColor::all(accent));
                e.widgets.parent(world, &key, paper);
            }
        }
    }
    Ok(())
}
