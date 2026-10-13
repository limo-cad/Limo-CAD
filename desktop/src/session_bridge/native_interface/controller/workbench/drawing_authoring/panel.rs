use super::super::*;
use super::fields::Kind;
use super::runtime::{native, Command, Editor, Tool};
use bevy::ui::UiTransform;
use limo_cad_interface::{Field as UiField, KeyChord};

pub(super) fn target(
    (world, camera, e): (&mut World, Entity, &mut Editor),
    key: &str,
    mut control: InterfaceControl,
    command: Command,
    bounds: Node,
    color: Color,
    z: i32,
) -> Result<Entity, String> {
    e.widgets.panel(world, camera, key, bounds, color, z);
    let entity = e.widgets.entity(key).unwrap();
    world
        .entity_mut(entity)
        .remove::<interface_shell::InterfaceOccluder>();
    let command = native(e.serial, command);
    if let Some(existing) = world.get::<InterfaceControl>(entity) {
        control.binding = existing.binding;
    }
    if world.get::<InterfaceControl>(entity) != Some(&control) {
        world.entity_mut(entity).insert(control);
    }
    if world
        .get::<NativeCommandBinding>(entity)
        .is_none_or(|binding| binding.command != command)
    {
        bind_command(world, entity, command)?;
    }
    Ok(entity)
}

pub(super) fn button(
    (world, camera, editor): (&mut World, Entity, &mut Editor),
    key: &str,
    label: &str,
    command: Command,
    bounds: Node,
    disabled: bool,
) -> Result<Entity, String> {
    let mut control = InterfaceControl::button("drawing/annotation", label);
    control.disabled = disabled;
    editor.widgets.button(
        world,
        camera,
        key,
        control,
        None,
        native(editor.serial, command),
        bounds,
        None,
        46,
    )
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    e: &mut Editor,
    height: f32,
    side: f32,
    state: &Workbench,
) -> Result<(), String> {
    let theme = crate::native_viewport::ui::theme(world);
    let Some(transform) = drawing_paper::transform(state) else {
        return Ok(());
    };
    let Some(paper) = state.widgets.entity("drawing-paper") else {
        return Ok(());
    };
    super::cloud_panel::paint(world, camera, e, paper, transform, state)?;
    super::center_panel::paint(world, camera, e, paper, transform, state)?;
    if e.tool.is_none() {
        for mark in drawing_paper::annotation_marks(state) {
            let center = transform.to_screen(mark.center);
            if center[0] < transform.clip.x - 100.
                || center[1] < transform.clip.y - 100.
                || center[0] > transform.clip.x + transform.clip.width + 100.
                || center[1] > transform.clip.y + transform.clip.height + 100.
            {
                continue;
            }
            let key = if mark.part == 0 {
                format!("drawing-annotation-{}", mark.id)
            } else {
                format!("drawing-annotation-{}-{}", mark.id, mark.part)
            };
            let mut control = InterfaceControl::button(
                "drawing/annotation",
                if mark.part == 0 {
                    format!("Edit annotation {}", mark.id)
                } else {
                    format!("Edit annotation {} part {}", mark.id, mark.part + 1)
                },
            );
            control.selected = Some(e.selected == Some(mark.id));
            let bounds = rect(
                ((mark.center[0] - mark.size[0] * 0.5) * transform.scale) as f32,
                ((mark.center[1] - mark.size[1] * 0.5) * transform.scale) as f32,
                (mark.size[0] * transform.scale).max(8.) as f32,
                (mark.size[1] * transform.scale).max(8.) as f32,
            );
            let selected = e.selected == Some(mark.id);
            let entity = target(
                (world, camera, e),
                &key,
                control,
                Command::Select(mark.id),
                bounds,
                if selected {
                    theme.accent.with_alpha(0.12)
                } else {
                    Color::NONE
                },
                19,
            )?;
            e.widgets.parent(world, &key, paper);
            world
                .entity_mut(entity)
                .insert(UiTransform::from_rotation(Rot2::radians(mark.angle)));
        }
    }
    if matches!(
        e.tool,
        Some(Tool::Linear | Tool::Angular | Tool::Series(_) | Tool::Ordinate)
    ) || matches!(e.tool, Some(Tool::Technical(t)) if t.anchors() && super::repair::allows(e, super::repair::Kind::Anchor) && (t != super::technical::Tool::ArcLength || !e.technical.circles.is_empty()))
    {
        let visible: Vec<_> = e
            .targets
            .iter()
            .enumerate()
            .filter(|(_, target)| transform.pick(transform.to_screen(target.paper)).is_some())
            .map(|(index, _)| index)
            .collect();
        if visible.len() > 4096 {
            e.message = "Too many visible anchors; zoom into the required view".into();
        } else {
            for index in visible {
                let target_data = &e.targets[index];
                let key = format!("drawing-anchor-{index}");
                let radius = (1.15 * transform.scale).max(3.);
                let selected = super::repair::selected(e, &Command::Anchor(index))
                    || e.angular
                        .selected(target_data.view_id, &target_data.reference)
                    || e.technical.anchor.as_ref().is_some_and(|a| {
                        a.view_id == target_data.view_id
                            && super::anchors::same_anchor(&a.reference, &target_data.reference)
                    })
                    || e.series
                        .selected(target_data.view_id, &target_data.reference)
                    || e.straight
                        .selected_anchor(target_data.view_id, &target_data.reference)
                    || e.pair.first.as_ref().is_some_and(|(_, view, a)| {
                        *view == target_data.view_id
                            && super::anchors::same_anchor(a, &target_data.reference)
                    });
                let mut control = InterfaceControl::button(
                    "drawing/anchors",
                    format!("View {} anchor {}", target_data.view_id, index + 1),
                );
                control.selected = Some(selected);
                let bounds = Node {
                    border_radius: BorderRadius::all(percent(50.)),
                    ..rect(
                        (target_data.paper[0] * transform.scale - radius) as f32,
                        (target_data.paper[1] * transform.scale - radius) as f32,
                        (radius * 2.) as f32,
                        (radius * 2.) as f32,
                    )
                };
                target(
                    (world, camera, e),
                    &key,
                    control,
                    Command::Anchor(index),
                    bounds,
                    theme.accent.with_alpha(if selected { 1. } else { 0.65 }),
                    21,
                )?;
                e.widgets.parent(world, &key, paper);
            }
        }
    }
    if matches!(e.tool, Some(Tool::Radial(_) | Tool::HoleNote))
        || matches!(e.tool,Some(Tool::Technical(t)) if t.circles() && super::repair::allows(e, super::repair::Kind::Circle) && (t != super::technical::Tool::ArcLength || e.technical.circles.is_empty()))
    {
        if e.circles.len() > 4096 {
            return Err("Too many circular pick targets on this sheet".into());
        }
        for index in 0..e.circles.len() {
            let circle = &e.circles[index];
            let radius = circle.radius * transform.scale;
            let center = transform.to_screen(circle.center);
            if center[0] + radius < transform.clip.x
                || center[1] + radius < transform.clip.y
                || center[0] - radius > transform.clip.x + transform.clip.width
                || center[1] - radius > transform.clip.y + transform.clip.height
            {
                continue;
            }
            let key = format!("drawing-circle-{index}");
            let mut control = InterfaceControl::button(
                "drawing/circles",
                format!("View {} circular edge {}", circle.view_id, index + 1),
            );
            control.selected = Some(super::repair::selected(e, &Command::Circle(index)));
            let bounds = Node {
                border_radius: BorderRadius::all(percent(50.)),
                border: UiRect::all(px(1.25)),
                ..rect(
                    (circle.center[0] * transform.scale - radius) as f32,
                    (circle.center[1] * transform.scale - radius) as f32,
                    (radius * 2.) as f32,
                    (radius * 2.) as f32,
                )
            };
            let entity = target(
                (world, camera, e),
                &key,
                control,
                Command::Circle(index),
                bounds,
                Color::NONE,
                21,
            )?;
            world
                .entity_mut(entity)
                .insert(BorderColor::all(theme.accent.with_alpha(0.75)));
            e.widgets.parent(world, &key, paper);
        }
    }
    if matches!(e.tool, Some(Tool::Linear | Tool::Chamfer))
        || matches!(e.tool,Some(Tool::Technical(t)) if t.lines() && super::repair::allows(e, super::repair::Kind::Line))
    {
        let chamfer = e.tool == Some(Tool::Chamfer);
        let count = if chamfer {
            e.chamfers.len()
        } else {
            e.lines.len()
        };
        for index in 0..count {
            let (line, selected) = if chamfer {
                let t = &e.chamfers[index];
                (t.line.clone(), e.chamfer.selected(t))
            } else {
                (
                    e.lines[index].clone(),
                    super::repair::selected(e, &Command::Line(index))
                        || e.straight.selected(&e.lines[index])
                        || e.technical.line.as_ref().is_some_and(|l| {
                            l.view_id == e.lines[index].view_id
                                && super::straight::same_line(
                                    &l.reference,
                                    &e.lines[index].reference,
                                )
                        }),
                )
            };
            let segments = line.pick_segments.clone();
            for (part, [a, b]) in segments.into_iter().enumerate() {
                let center = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                let half = [(a[0] - b[0]).abs() * 0.5, (a[1] - b[1]).abs() * 0.5];
                let screen = transform.to_screen(center);
                if screen[0] + half[0] * transform.scale < transform.clip.x
                    || screen[1] + half[1] * transform.scale < transform.clip.y
                    || screen[0] - half[0] * transform.scale
                        > transform.clip.x + transform.clip.width
                    || screen[1] - half[1] * transform.scale
                        > transform.clip.y + transform.clip.height
                {
                    continue;
                }
                let family = if chamfer { "chamfer" } else { "edge" };
                let key = if part == 0 {
                    format!("drawing-{family}-{index}")
                } else {
                    format!("drawing-{family}-{index}-part-{part}")
                };
                let mut control = InterfaceControl::button(
                    "drawing/edges",
                    format!(
                        "View {} {} edge {} body {} occurrence {}",
                        line.view_id,
                        if chamfer { "chamfer" } else { "straight" },
                        line.reference.edge_id.0,
                        line.reference.body_id.0,
                        line.reference
                            .occurrence_id
                            .map_or_else(|| "definition".into(), |id| id.0.to_string())
                    ),
                );
                control.selected = Some(selected);
                let length = (b[0] - a[0]).hypot(b[1] - a[1]) * transform.scale;
                let thickness = (transform.scale * 1.5).max(6.);
                let angle = (b[1] - a[1]).atan2(b[0] - a[0]) as f32;
                let entity = target(
                    (world, camera, e),
                    &key,
                    control,
                    if chamfer {
                        Command::Chamfer(index)
                    } else {
                        Command::Line(index)
                    },
                    rect(
                        (center[0] * transform.scale - length * 0.5) as f32,
                        (center[1] * transform.scale - thickness * 0.5) as f32,
                        length as f32,
                        thickness as f32,
                    ),
                    theme.accent.with_alpha(if selected { 0.35 } else { 0.10 }),
                    20,
                )?;
                e.widgets.parent(world, &key, paper);
                world
                    .entity_mut(entity)
                    .insert(UiTransform::from_rotation(Rot2::radians(angle)));
            }
        }
    }
    if e.tool.is_none() && e.selected.is_none() {
        return Ok(());
    }
    let width = side.max(248.);
    let bottom = (height - 66.).max(280.);
    e.widgets.panel(
        world,
        camera,
        "annotation-panel",
        rect(0., 112., width, bottom - 112.),
        theme.panel,
        44,
    );
    let title = match e.tool {
        Some(Tool::Technical(t)) => t.label(),
        Some(Tool::Note) => "Place note",
        Some(Tool::HoleNote) => "Hole note",
        Some(Tool::RevisionCloud) => "Revision cloud",
        Some(Tool::CenterMark) => "Center mark",
        Some(Tool::CenterLine) => "Centerline between circles",
        Some(Tool::Chamfer) => "Chamfer note",
        Some(Tool::Linear) => "Dimension",
        Some(Tool::Angular) => "Angular dimension",
        Some(Tool::Series(limo_cad_sketch::DrawingChainDimensionLayout::Chain)) => {
            "Chain dimension"
        }
        Some(Tool::Series(limo_cad_sketch::DrawingChainDimensionLayout::Baseline)) => {
            "Baseline dimensions"
        }
        Some(Tool::Series(limo_cad_sketch::DrawingChainDimensionLayout::Continued)) => {
            "Continued dimensions"
        }
        Some(Tool::Ordinate) => "Ordinate dimension",
        Some(Tool::Radial(limo_cad_sketch::DrawingRadialDimensionMode::Radius)) => {
            "Radius dimension"
        }
        Some(Tool::Radial(limo_cad_sketch::DrawingRadialDimensionMode::Diameter)) => {
            "Diameter dimension"
        }
        None => match e.draft.as_ref().map(|draft| draft.annotation()) {
            Some(limo_cad_sketch::DrawingAnnotationDto::HoleNote { .. }) => "Hole note",
            Some(limo_cad_sketch::DrawingAnnotationDto::CenterMark { .. }) => "Center mark",
            Some(limo_cad_sketch::DrawingAnnotationDto::CenterLine { .. }) => {
                "Centerline between circles"
            }
            Some(limo_cad_sketch::DrawingAnnotationDto::RevisionCloud { .. }) => "Revision cloud",
            Some(limo_cad_sketch::DrawingAnnotationDto::LineDimension { mode, .. }) => match mode {
                limo_cad_sketch::DrawingLineDimensionMode::Length => "Edge length dimension",
                limo_cad_sketch::DrawingLineDimensionMode::Distance => "Parallel edge distance",
                limo_cad_sketch::DrawingLineDimensionMode::Angle => "Edge angle dimension",
            },
            Some(limo_cad_sketch::DrawingAnnotationDto::PointLineDimension { .. }) => {
                "Point-line dimension"
            }
            Some(limo_cad_sketch::DrawingAnnotationDto::ChamferNote { .. }) => "Chamfer note",
            _ => "Edit annotation",
        },
    };
    e.widgets.text(
        world,
        camera,
        "annotation-title",
        rect(12., 121., width - 24., 20.),
        title,
        12.,
        45,
    );
    button(
        (world, camera, e),
        "annotation-back",
        "Sheet setup",
        Command::Cancel,
        rect(10., 148., width - 20., 28.),
        false,
    )?;
    if e.tool.is_some_and(|tool| tool != Tool::Note) {
        let message = match e.tool {
            Some(Tool::Technical(t)) => t.instruction(&e.technical),
            Some(Tool::HoleNote) => "Choose a complete circular hole edge, then edit its callout. Drag the saved label to move its leader.",
            Some(Tool::CenterMark) => "Choose the highlighted center of a complete circle.",
            Some(Tool::CenterLine) if e.center.active() => "Choose a second distinct circular center in the same view.",
            Some(Tool::CenterLine) => "Choose two circular centers in one view. Select a saved centerline to edit its extension.",
            Some(Tool::RevisionCloud) if e.cloud.points.len() >= 3 => "Click near the first point to close a triangle, or click a fourth corner to finish.",
            Some(Tool::RevisionCloud) => "Click three cloud corners on the paper, then close near the first point or add a fourth corner.",
            Some(Tool::Chamfer) if e.chamfer.active() => "Click paper to place the chamfer note, or use Place note.",
            Some(Tool::Chamfer) => "Choose a straight chamfer edge in a true-shape view. Both ends need adjacent carrier edges.",
            Some(Tool::Linear) if e.straight.active() => {
                "Pick another edge or anchor to change the relation. Click paper to place, or use Place dimension."
            }
            Some(Tool::Linear) if e.pair.first.is_some() => {
                "Choose another anchor or a straight edge in the same view."
            }
            Some(Tool::Linear) => "Choose projected anchors, circle centers, or straight edges in one view.",
            Some(Tool::Series(_)) => match e.series.picks.len() {
                0 => "Choose the first projected endpoint (datum for Baseline).",
                1 => "Choose the second projected endpoint in the same view.",
                _ => "Choose the third projected endpoint in the same view.",
            },
            Some(Tool::Ordinate) if e.series.picks.is_empty() => {
                "Choose the datum origin on a projected endpoint."
            }
            Some(Tool::Ordinate) => "Choose the measured endpoint in the same view.",
            Some(Tool::Angular) => match e.angular.picks.len() {
                0 => "Choose the angular vertex on a projected endpoint.",
                1 => "Choose an endpoint on the first angular ray.",
                _ => "Choose an endpoint on the second angular ray in the same view.",
            },
            Some(Tool::Radial(limo_cad_sketch::DrawingRadialDimensionMode::Diameter)) => {
                "Choose a closed circular edge on the paper."
            }
            _ => "Choose a circular edge or open arc on the paper.",
        };
        e.widgets.text(
            world,
            camera,
            "annotation-instruction",
            rect(12., 192., width - 24., 86.),
            message,
            11.,
            45,
        );
    }
    let mut y = if e.tool.is_some_and(|tool| tool != Tool::Note) {
        285.
    } else {
        188.
    };
    if matches!(e.tool, Some(Tool::Technical(_))) {
        {
            let reset_caption = if super::repair::active(e) {
                "Reset replacement"
            } else {
                "Reset picks"
            };
            button(
                (world, camera, e),
                "annotation-reset-picks",
                reset_caption,
                Command::Reset,
                rect(10., y, width - 20., 28.),
                false,
            )
        }?;
        y += 34.;
    }
    if super::repair::active(e) {
        y = super::repair::paint(world, camera, e, width, y)?;
    }
    if let Some(
        annotation @ limo_cad_sketch::DrawingAnnotationDto::HoleNote {
            source_feature_id,
            feature_name,
            ..
        },
    ) = e.draft.as_ref().map(|d| d.annotation())
    {
        if let Some(id) = source_feature_id {
            let caption = format!(
                "Modeled hole: {}",
                if feature_name.is_empty() {
                    format!("Feature {id}")
                } else {
                    feature_name.clone()
                }
            );
            e.widgets.text(
                world,
                camera,
                "annotation-hole-feature",
                Node {
                    overflow: Overflow::clip(),
                    ..rect(12., y, width - 24., 32.)
                },
                &caption,
                11.,
                45,
            );
            y += 36.;
        }
        if let Some((_, sheet, units)) = state.paper_key.as_ref() {
            let caption =
                super::fields::hole_preview(annotation, &e.fields, *units, sheet.standard)
                    .unwrap_or_else(|| "Correct the invalid callout value to preview.".into());
            e.widgets.text(
                world,
                camera,
                "annotation-hole-callout",
                Node {
                    overflow: Overflow::clip(),
                    ..rect(12., y, width - 24., 60.)
                },
                &caption,
                11.,
                45,
            );
            y += 66.;
        }
    }
    if let Some(limo_cad_sketch::DrawingAnnotationDto::RevisionCloud { points, .. }) =
        e.draft.as_ref().map(|d| d.annotation())
    {
        e.widgets.text(
            world,
            camera,
            "annotation-cloud-summary",
            rect(12., y, width - 24., 36.),
            &format!(
                "{} paper-space cloud vertices. Drag the cloud to reposition it.",
                points.len()
            ),
            11.,
            45,
        );
        y += 42.;
    }
    let staged = e.chamfer.annotation(0);
    if let (Some(annotation), Some((_, sheet, units))) = (
        e.draft.as_ref().map(|d| d.annotation()).or(staged.as_ref()),
        state.paper_key.as_ref(),
    ) {
        if let Some(caption) = drawing_paper::chamfer_caption(annotation, *units, sheet.standard) {
            e.widgets.text(
                world,
                camera,
                "annotation-chamfer-callout",
                rect(12., y, width - 24., 32.),
                &caption,
                11.,
                45,
            );
            y += 36.;
        }
    }
    let available = (bottom - 130. - y).max(48.);
    let page_size = ((available / 50.).floor() as usize).clamp(1, 5);
    let visible = super::fields::visible(&e.fields);
    e.page = e.page.min(visible.len().saturating_sub(1) / page_size);
    for index in visible
        .iter()
        .skip(e.page * page_size)
        .take(page_size)
        .copied()
    {
        let field = &e.fields[index];
        let field_key = format!("{:?}", field.id);
        let toggle = matches!(field.kind, Kind::Toggle);
        if !toggle {
            e.widgets.text(
                world,
                camera,
                &format!("annotation-label-{field_key}"),
                rect(12., y, width - 24., 16.),
                field.label,
                10.,
                45,
            );
        }
        let mut control = InterfaceControl::button("drawing/annotation", field.label);
        let h = if matches!(field.kind, Kind::Multiline) {
            64.
        } else {
            28.
        };
        let options = if field.id == super::fields::Id::Technical("/bom_item_id") {
            Some(super::fields::bom_options(
                &e.document,
                e.stamp.as_ref().ok_or("Create a sheet first")?.sheet_id,
            ))
        } else {
            field.options()
        };
        let caption = options
            .as_ref()
            .and_then(|opts| opts.iter().find(|o| o.value == field.text))
            .map(|o| o.label.clone())
            .unwrap_or_else(|| field.caption());
        if let Some(options) = options {
            control.role = "combobox".into();
            control.owned_keys = [
                "ArrowUp",
                "ArrowDown",
                "ArrowLeft",
                "ArrowRight",
                "Home",
                "End",
            ]
            .map(KeyChord::plain)
            .into();
            control.field = UiField::Choice {
                value: field.text.clone(),
                options,
            };
        } else if toggle {
            control.role = "checkbox".into();
            control.selected = Some(field.text == "true");
            control.field = UiField::Toggle(field.text == "true");
        } else {
            control.field = UiField::Text {
                value: field.text.clone(),
                read_only: false,
                selection: None,
            };
        }
        let entity = e.widgets.button(
            world,
            camera,
            &format!("annotation-field-{field_key}"),
            control,
            Some(&caption),
            native(e.serial, Command::Field(field.id)),
            rect(10., y + 16., width - 20., h),
            None,
            46,
        )?;
        if toggle {
            interface_shell::checkbox_button(world, entity, camera, field.text == "true");
        }
        if matches!(field.kind, Kind::Multiline) {
            interface_shell::fields::multiline::enable(world, entity)?;
        }
        y += h + 22.;
    }
    if visible.len() > page_size {
        {
            let previous_disabled = e.page == 0;
            button(
                (world, camera, e),
                "annotation-fields-prev",
                "Previous fields",
                Command::Fields(-1),
                rect(10., y, (width - 26.) / 2., 26.),
                previous_disabled,
            )
        }?;
        {
            let next_disabled = (e.page + 1) * page_size >= visible.len();
            button(
                (world, camera, e),
                "annotation-fields-next",
                "More fields",
                Command::Fields(1),
                rect(width / 2. + 3., y, (width - 26.) / 2., 26.),
                next_disabled,
            )
        }?;
        y += 32.;
    }
    if !e.fields.is_empty() || e.straight.active() || e.chamfer.active() {
        let label = if e.tool == Some(Tool::Chamfer) && e.chamfer.active() {
            "Place note"
        } else if e.tool == Some(Tool::Linear) && e.straight.active() {
            "Place dimension"
        } else if e.tool == Some(Tool::Note) {
            "Place note"
        } else {
            "Apply annotation"
        };
        {
            let apply_disabled = e.straight.active() && !e.straight.valid();
            button(
                (world, camera, e),
                "annotation-apply",
                label,
                Command::Apply,
                rect(10., y, (width - 26.) / 2., 28.),
                apply_disabled,
            )
        }?;
        button(
            (world, camera, e),
            "annotation-reset",
            "Reset annotation",
            Command::Reset,
            rect(width / 2. + 3., y, (width - 26.) / 2., 28.),
            false,
        )?;
        y += 34.;
    }
    if e.selected.is_some() {
        button(
            (world, camera, e),
            "annotation-delete",
            "Delete annotation",
            Command::Delete,
            rect(10., y, width - 20., 28.),
            false,
        )?;
        y += 34.;
    }
    let message = if !e.message.is_empty() {
        &e.message
    } else if e.dirty() {
        "Apply or reset before editing another item"
    } else if e.tool == Some(Tool::Note) {
        "Click the paper or enter a paper position, then Place note."
    } else if e.straight.active() && !e.straight.valid() {
        "Choose geometry with a nonzero projected dimension."
    } else if e.selected.is_some() {
        "Drag the annotation on paper to move it."
    } else {
        ""
    };
    e.widgets.text(
        world,
        camera,
        "annotation-message",
        rect(12., y, width - 24., (bottom - y - 8.).max(32.)),
        message,
        10.,
        45,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
    use interface_shell::{PointerButton, PointerPhase};
    use limo_cad_interface::{ControlInput, ControlKey};

    #[test]
    fn paper_targets_receive_pointer_actions_without_masking_themselves() {
        let (mut app, handle, _, _) = interface_shell::tests::fixture();
        let camera = app.world_mut().spawn_empty().id();
        let mut editor = Editor::default();
        for command in [
            Command::Select(25),
            Command::Anchor(0),
            Command::Circle(0),
            Command::Line(0),
            Command::Chamfer(0),
            Command::CloudEdge(25, 0),
        ] {
            let entity = target(
                (app.world_mut(), camera, &mut editor),
                "paper-target",
                InterfaceControl::button("Viewport", "Paper annotation"),
                command.clone(),
                rect(180., 190., 40., 20.),
                Color::NONE,
                21,
            )
            .unwrap();
            app.world_mut().entity_mut(entity).insert((
                ComputedNode {
                    size: Vec2::new(40., 20.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(200., 200.)),
                ComputedStackIndex(21),
                InheritedVisibility::VISIBLE,
            ));
            app.update();
            assert!(app
                .world()
                .get::<interface_shell::InterfaceOccluder>(entity)
                .is_none());
            assert_eq!(
                handle.hit_key([300., 300.]),
                Some(ControlKey(entity.to_bits()))
            );
            assert_eq!(
                super::super::input::claim_radial_target(app.world(), &handle, [300., 300.]),
                matches!(command, Command::Circle(_))
            );
            assert!(handle
                .pointer(PointerPhase::Down, [300., 300.], PointerButton::Primary)
                .unwrap());
            assert!(handle
                .pointer(PointerPhase::Up, [300., 300.], PointerButton::Primary)
                .unwrap());
            let actions = handle.take_actions().unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].control.key, ControlKey(entity.to_bits()));
            assert_eq!(actions[0].control.input, ControlInput::Click);
            handle.validate_action(&actions[0]).unwrap();
            assert_eq!(
                app.world()
                    .get::<NativeCommandBinding>(entity)
                    .unwrap()
                    .command,
                native(editor.serial, command)
            );
        }
    }
    #[test]
    fn circular_ring_picker_does_not_claim_a_keyless_topmost_occluder() {
        let (mut app, handle, _, _) = interface_shell::tests::fixture();
        let camera = app.world_mut().spawn_empty().id();
        let mut editor = Editor::default();
        let entity = target(
            (app.world_mut(), camera, &mut editor),
            "ring",
            InterfaceControl::button("drawing/circles", "Circular edge"),
            Command::Circle(0),
            rect(180., 190., 40., 20.),
            Color::NONE,
            21,
        )
        .unwrap();
        let geometry = || {
            (
                ComputedNode {
                    size: Vec2::new(40., 20.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(200., 200.)),
                InheritedVisibility::VISIBLE,
            )
        };
        app.world_mut()
            .entity_mut(entity)
            .insert((geometry(), ComputedStackIndex(21)));
        app.update();
        assert!(super::super::input::claim_radial_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
        handle
            .pointer(PointerPhase::Down, [300., 300.], PointerButton::Primary)
            .unwrap();
        assert!(handle.has_capture());
        assert!(super::super::input::claim_radial_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
        assert!(!handle.has_capture());
        handle
            .pointer(PointerPhase::Up, [300., 300.], PointerButton::Primary)
            .unwrap();
        assert!(
            handle.take_actions().unwrap().is_empty(),
            "Rectangular release must not bypass the geometric ring hit"
        );
        let blocker = app
            .world_mut()
            .spawn((
                Node::default(),
                geometry(),
                ComputedStackIndex(22),
                interface_shell::InterfaceOccluder,
            ))
            .id();
        app.update();
        assert!(handle.owns_pointer([300., 300.]));
        assert_eq!(handle.hit_key([300., 300.]), None);
        assert!(!super::super::input::claim_radial_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
        app.world_mut().despawn(blocker);
        app.update();
        assert!(super::super::input::claim_radial_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
    }
    #[test]
    fn revision_cloud_picker_does_not_claim_a_keyless_topmost_occluder() {
        let (mut app, handle, _, _) = interface_shell::tests::fixture();
        let camera = app.world_mut().spawn_empty().id();
        let mut editor = Editor::default();
        let entity = target(
            (app.world_mut(), camera, &mut editor),
            "ring",
            InterfaceControl::button("drawing/circles", "Revision cloud edge"),
            Command::CloudEdge(25, 0),
            rect(180., 190., 40., 20.),
            Color::NONE,
            21,
        )
        .unwrap();
        let geometry = || {
            (
                ComputedNode {
                    size: Vec2::new(40., 20.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(200., 200.)),
                InheritedVisibility::VISIBLE,
            )
        };
        app.world_mut()
            .entity_mut(entity)
            .insert((geometry(), ComputedStackIndex(21)));
        app.update();
        assert!(super::super::input::claim_cloud_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
        handle
            .pointer(PointerPhase::Down, [300., 300.], PointerButton::Primary)
            .unwrap();
        assert!(handle.has_capture());
        assert!(super::super::input::claim_cloud_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
        assert!(!handle.has_capture());
        handle
            .pointer(PointerPhase::Up, [300., 300.], PointerButton::Primary)
            .unwrap();
        assert!(
            handle.take_actions().unwrap().is_empty(),
            "Rectangular release must not bypass the geometric scallop hit"
        );
        let blocker = app
            .world_mut()
            .spawn((
                Node::default(),
                geometry(),
                ComputedStackIndex(22),
                interface_shell::InterfaceOccluder,
            ))
            .id();
        app.update();
        assert!(handle.owns_pointer([300., 300.]));
        assert_eq!(handle.hit_key([300., 300.]), None);
        assert!(!super::super::input::claim_cloud_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
        app.world_mut().despawn(blocker);
        app.update();
        assert!(super::super::input::claim_cloud_target(
            app.world(),
            &handle,
            [300., 300.]
        ));
    }
}
