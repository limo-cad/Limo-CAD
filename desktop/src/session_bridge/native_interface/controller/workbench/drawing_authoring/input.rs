use super::super::*;
use super::runtime::{preview, submit, Command, Drag, Editor, Tool};
use super::{
    draft::{Draft, Selection},
    *,
};
use crate::native_viewport::winit_host::NativeHostInput;
use bevy::{
    input::{keyboard::Key, ButtonState},
    window::WindowEvent,
};

fn annotation_at(
    world: &World,
    handle: &NativeInterfaceHandle,
    cursor: [f64; 2],
) -> Option<(u64, Option<usize>)> {
    let key = handle.hit_key(cursor)?;
    let binding = world.get::<NativeCommandBinding>(Entity::from_bits(key.0))?;
    match &binding.command {
        NativeCommand::Drawing(drawing_editor::Command::Annotation(_, Command::Select(id))) => {
            Some((*id, None))
        }
        NativeCommand::Drawing(drawing_editor::Command::Annotation(
            _,
            Command::CloudEdge(id, edge),
        )) => Some((*id, Some(*edge))),
        _ => None,
    }
}
pub(super) fn claim_radial_target(
    world: &World,
    handle: &NativeInterfaceHandle,
    cursor: [f64; 2],
) -> bool {
    let owned = handle.hit_key(cursor).is_some_and(|key| {
        matches!(
            world
                .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                .map(|b| &b.command),
            Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                _,
                Command::Circle(_)
            )))
        )
    });
    if owned {
        handle.cancel_pointer();
    }
    owned
}
pub(super) fn claim_cloud_target(
    world: &World,
    handle: &NativeInterfaceHandle,
    cursor: [f64; 2],
) -> bool {
    let owned = handle.hit_key(cursor).is_some_and(|key| {
        matches!(
            world
                .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                .map(|b| &b.command),
            Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                _,
                Command::CloudEdge(_, _)
            )))
        )
    });
    if owned {
        handle.cancel_pointer();
    }
    owned
}
pub(in super::super) fn process(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    input: &NativeHostInput,
) -> Result<bool, String> {
    let Some(mut editor) = world.remove_resource::<Editor>() else {
        return Ok(false);
    };
    let result = inner(world, handle, services, input, &mut editor);
    if let Err(error) = &result {
        editor.message = error.clone();
        editor.drag = None;
        editor.pair.cancel();
        editor.angular.cancel();
        editor.series.cancel();
        editor.straight.cancel();
        editor.chamfer.cancel();
        editor.cloud.cancel();
        editor.center.cancel();
        editor.technical.cancel();
    }
    world.insert_resource(editor);
    if result.as_ref().is_ok_and(|handled| *handled) {
        refresh_preview(world)?;
        handle.invalidate_presentation();
    }
    result
}
fn inner(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    input: &NativeHostInput,
    e: &mut Editor,
) -> Result<bool, String> {
    if workspace(world) != Workspace::Drawing {
        e.drag = None;
        e.pair.cancel();
        e.angular.cancel();
        e.series.cancel();
        e.straight.cancel();
        e.chamfer.cancel();
        e.cloud.cancel();
        e.center.cancel();
        e.technical.cancel();
        return Ok(false);
    }
    let cancel = matches!(&input.event,WindowEvent::WindowFocused(f) if !f.focused)
        || matches!(
            &input.event,
            WindowEvent::KeyboardFocusLost(_)
                | WindowEvent::CursorLeft(_)
                | WindowEvent::WindowCloseRequested(_)
                | WindowEvent::WindowDestroyed(_)
                | WindowEvent::WindowResized(_)
                | WindowEvent::WindowScaleFactorChanged(_)
                | WindowEvent::WindowBackendScaleFactorChanged(_)
        );
    let escape = matches!(&input.event,WindowEvent::KeyboardInput(k) if k.state==ButtonState::Pressed && k.logical_key==Key::Escape && !input.consumed);
    if cancel || escape {
        let active = e.drag.take().is_some()
            || e.pair.first.is_some()
            || !e.angular.picks.is_empty()
            || !e.series.picks.is_empty()
            || e.center.active()
            || e.straight.active()
            || e.chamfer.active()
            || !e.cloud.points.is_empty()
            || e.tool.is_some()
            || e.selected.is_some();
        e.pair.cancel();
        e.angular.cancel();
        e.series.cancel();
        e.straight.cancel();
        e.chamfer.cancel();
        e.cloud.cancel();
        e.center.cancel();
        e.technical.cancel();
        if escape && !e.dirty() {
            e.clear();
        }
        return Ok(active);
    }
    let Some(stamp) = e.stamp.clone() else {
        return Ok(false);
    };
    let valid = handle.read_surface(|owner, frame| {
        input.context.as_ref() == Some(owner)
            && owner == &stamp.owner
            && frame.modal_stack.is_empty()
    })?;
    if !valid {
        e.drag = None;
        e.pair.cancel();
        e.angular.cancel();
        e.series.cancel();
        e.straight.cancel();
        e.chamfer.cancel();
        e.cloud.cancel();
        e.center.cancel();
        e.technical.cancel();
        return Ok(false);
    }
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, &stamp.owner)?;
    if receipt.revision != stamp.revision {
        e.drag = None;
        e.pair.cancel();
        e.angular.cancel();
        e.series.cancel();
        e.straight.cancel();
        e.chamfer.cancel();
        e.cloud.cancel();
        e.center.cancel();
        e.technical.cancel();
        return Ok(false);
    }
    let Some(transform) = world
        .get_resource::<Workbench>()
        .and_then(drawing_paper::transform)
    else {
        return Ok(false);
    };
    let Some(cursor) = input
        .cursor
        .filter(|p| p.is_finite())
        .map(|p| p.as_dvec2().to_array())
    else {
        return Ok(false);
    };
    if let Some(drag) = &mut e.drag {
        if drag.stamp != stamp {
            e.drag = None;
            return Ok(false);
        }
        if drag.projection.as_ref().is_some_and(|source| {
            !drawing_paper::same_projection(world.resource::<Workbench>(), source)
        }) {
            e.drag = None;
            return Ok(true);
        }
        if matches!(&input.event, WindowEvent::CursorMoved(_)) {
            let point = transform.to_paper(cursor);
            let delta = [point[0] - drag.start[0], point[1] - drag.start[1]];
            if delta[0].hypot(delta[1]) * transform.scale >= 3. {
                drag.moved = true;
            }
            if drag.moved {
                if let Some(grip) = drag.center {
                    drag.draft
                        .center_extension(center::extension_at(grip, point)?)?;
                } else if let Some([a, b]) = drag.linear_points {
                    drag.draft.move_linear(a, b, delta)?;
                } else if drag.ordinate_points.is_some() {
                    drag.draft.move_ordinate(delta)?;
                } else if let Some(g) = &drag.radial {
                    drag.draft
                        .move_radial(g.center, g.paper_radius, g.shoulder, delta)?;
                } else if let Some(g) = &drag.angular {
                    if matches!(
                        drag.draft.annotation(),
                        limo_cad_sketch::DrawingAnnotationDto::ArcLengthDimension { .. }
                    ) {
                        drag.draft.move_arc_length(g.vertex, g.text, delta)?;
                    } else {
                        drag.draft.move_angular(g.vertex, g.text, delta)?;
                    }
                } else if matches!(
                    drag.draft.annotation(),
                    limo_cad_sketch::DrawingAnnotationDto::RevisionCloud { .. }
                ) {
                    drag.draft.move_revision_cloud(delta, transform.sheet_mm)?;
                } else if matches!(
                    drag.draft.annotation(),
                    limo_cad_sketch::DrawingAnnotationDto::ChamferNote { .. }
                ) {
                    drag.draft.move_chamfer(delta, transform.sheet_mm)?;
                } else if matches!(
                    drag.draft.annotation(),
                    limo_cad_sketch::DrawingAnnotationDto::HoleNote { .. }
                ) {
                    drag.draft.move_hole(delta, transform.sheet_mm)?;
                } else if matches!(
                    drag.draft.annotation(),
                    limo_cad_sketch::DrawingAnnotationDto::LineDimension { .. }
                        | limo_cad_sketch::DrawingAnnotationDto::PointLineDimension { .. }
                ) {
                    drag.draft.move_straight(delta, transform.sheet_mm)?;
                } else if matches!(
                    drag.draft.annotation(),
                    limo_cad_sketch::DrawingAnnotationDto::JoggedRadiusDimension { .. }
                        | limo_cad_sketch::DrawingAnnotationDto::DatumFeature { .. }
                        | limo_cad_sketch::DrawingAnnotationDto::GdtFrame { .. }
                        | limo_cad_sketch::DrawingAnnotationDto::SurfaceTexture { .. }
                        | limo_cad_sketch::DrawingAnnotationDto::EdgeRequirement { .. }
                        | limo_cad_sketch::DrawingAnnotationDto::WeldSymbol { .. }
                        | limo_cad_sketch::DrawingAnnotationDto::ItemBalloon { .. }
                ) {
                    drag.draft.move_technical(delta, transform.sheet_mm)?;
                } else if let limo_cad_sketch::DrawingAnnotationDto::Note { position, .. } =
                    Draft::new(&e.document, drag.draft.selection())?.annotation()
                {
                    drag.draft.move_note(
                        [position[0] + delta[0], position[1] + delta[1]],
                        transform.sheet_mm,
                    )?;
                }
            }
            return Ok(true);
        }
        if matches!(&input.event,WindowEvent::MouseButtonInput(b) if b.button==MouseButton::Left && b.state==ButtonState::Released)
        {
            let drag = e.drag.take().unwrap();
            if drag.moved && drag.draft.dirty() {
                drag.draft.verify(&e.document)?;
                e.pending_selected = Some(drag.draft.selection().annotation_id);
                submit(
                    world,
                    handle,
                    &services.engine,
                    &services.bridge,
                    &stamp,
                    "drawing_update_annotation",
                    json!({"sheet_id":drag.draft.selection().sheet_id,"annotation":drag.draft.annotation()}),
                )?;
            }
            return Ok(true);
        }
    }
    if e.tool == Some(Tool::Chamfer)
        && e.chamfer.active()
        && matches!(&input.event, WindowEvent::CursorMoved(_))
        && !input.consumed
        && handle.hit_key(cursor).is_none()
    {
        if let Some(point) = transform.pick(cursor) {
            e.chamfer.move_to(point, transform.sheet_mm)?;
            return Ok(true);
        }
    }
    if e.tool == Some(Tool::Linear)
        && e.straight.active()
        && matches!(&input.event, WindowEvent::CursorMoved(_))
        && !input.consumed
        && handle.hit_key(cursor).is_none()
    {
        if let Some(point) = transform.pick(cursor) {
            e.straight.move_to(point, transform.sheet_mm)?;
            return Ok(true);
        }
    }
    if !matches!(&input.event,WindowEvent::MouseButtonInput(b) if b.button==MouseButton::Left && b.state==ButtonState::Pressed)
    {
        return Ok(false);
    }
    let Some(point) = transform.pick(cursor) else {
        return Ok(false);
    };
    if matches!(e.tool, Some(Tool::Technical(_))) {
        let command = handle.hit_key(cursor).and_then(|key| {
            match world
                .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                .map(|b| &b.command)
            {
                Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                    _,
                    command @ (Command::Anchor(_) | Command::Circle(_) | Command::Line(_)),
                ))) => Some(command.clone()),
                _ => None,
            }
        });
        if let Some(command) = command {
            handle.cancel_pointer();
            let Some(Tool::Technical(tool)) = e.tool else {
                unreachable!()
            };
            let tolerance = 1.5_f64.max(4. / transform.scale);
            let anchors_visible = tool.anchors()
                && repair::allows(e, repair::Kind::Anchor)
                && (tool != technical::Tool::ArcLength || !e.technical.circles.is_empty());
            let anchor = anchors_visible
                .then(|| {
                    e.targets
                        .iter()
                        .enumerate()
                        .filter_map(|(i, t)| {
                            let distance = (point[0] - t.paper[0]).hypot(point[1] - t.paper[1]);
                            (distance <= tolerance).then_some((i, distance))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| Command::Anchor(i))
                })
                .flatten();
            let circle = (tool.circles()
                && repair::allows(e, repair::Kind::Circle)
                && (tool != technical::Tool::ArcLength || e.technical.circles.is_empty()))
            .then(|| {
                radial::hit(&e.circles, point, 2_f64.max(3. / transform.scale)).map(Command::Circle)
            })
            .flatten();
            let line = (tool.lines() && repair::allows(e, repair::Kind::Line))
                .then(|| straight::hit(&e.lines, point, tolerance).map(Command::Line))
                .flatten();
            let hit = anchor.or(circle).or(line);
            let _ = command;
            if let Some(hit) = hit {
                drawing_editor::guard_sheet_edit(world)?;
                if let Some(next) = technical_runtime::pick(world, e, &stamp, &hit)? {
                    submit(
                        world,
                        handle,
                        &services.engine,
                        &services.bridge,
                        &stamp,
                        "drawing_add_annotation",
                        runtime::created_annotation(next, &stamp)?,
                    )?;
                }
            }
            return Ok(true);
        }
    }
    if e.tool == Some(Tool::Chamfer)
        && handle.hit_key(cursor).is_some_and(|key| {
            matches!(
                world
                    .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                    .map(|b| &b.command),
                Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                    _,
                    Command::Chamfer(_)
                )))
            )
        })
    {
        handle.cancel_pointer();
        if e.chamfer_source.as_ref().is_none_or(|source| {
            !drawing_paper::same_projection(world.resource::<Workbench>(), source)
        }) {
            return Err("Projection changed; choose refreshed geometry".into());
        }
        if let Some(index) = chamfer::hit(&e.chamfers, point, 1.5_f64.max(4. / transform.scale)) {
            drawing_editor::guard_sheet_edit(world)?;
            e.pick_chamfer(&stamp, index, transform.sheet_mm)?;
        }
        return Ok(true);
    }
    if e.tool == Some(Tool::Linear)
        && handle.hit_key(cursor).is_some_and(|key| {
            matches!(
                world
                    .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                    .map(|b| &b.command),
                Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                    _,
                    Command::Line(_)
                )))
            )
        })
    {
        handle.cancel_pointer();
        if e.line_source.as_ref().is_none_or(|source| {
            !drawing_paper::same_projection(world.resource::<Workbench>(), source)
        }) {
            return Err("Projection changed; choose refreshed geometry".into());
        }
        if let Some(index) = straight::hit(&e.lines, point, 1.5_f64.max(4. / transform.scale)) {
            drawing_editor::guard_sheet_edit(world)?;
            e.pick_line(&stamp, index)?;
        }
        return Ok(true);
    }
    if matches!(e.tool, Some(Tool::CenterMark | Tool::CenterLine)) {
        let owned = handle.hit_key(cursor).is_some_and(|key| {
            matches!(
                world
                    .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                    .map(|b| &b.command),
                Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                    _,
                    Command::Center(_)
                )))
            )
        });
        if !owned {
            return Ok(false);
        }
        handle.cancel_pointer();
        if e.center_source
            .as_ref()
            .is_none_or(|s| !drawing_paper::same_projection(world.resource::<Workbench>(), s))
        {
            return Err("Projection changed; choose refreshed circles".into());
        }
        if let Some(index) = e
            .centers
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let distance = (point[0] - c.center[0]).hypot(point[1] - c.center[1]);
                (distance <= 1.5_f64.max(4. / transform.scale)).then_some((i, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
        {
            drawing_editor::guard_sheet_edit(world)?;
            if let Some(next) = e.center.click(
                &stamp,
                &e.centers[index],
                e.tool == Some(Tool::CenterLine),
                &e.document,
            )? {
                e.pending_selected = Some(e.document.next_annotation_id);
                submit(
                    world,
                    handle,
                    &services.engine,
                    &services.bridge,
                    &stamp,
                    "drawing_add_annotation",
                    runtime::created_annotation(next, &stamp)?,
                )?;
            }
        }
        return Ok(true);
    }
    if matches!(e.tool, Some(Tool::Radial(_) | Tool::HoleNote)) {
        if e.circles.len() > 4096 {
            return Err("Too many circular pick targets on this sheet".into());
        }
        if !claim_radial_target(world, handle, cursor) {
            return Ok(false);
        }
        if let Some(index) = radial::hit(&e.circles, point, 2_f64.max(3. / transform.scale)) {
            drawing_editor::guard_sheet_edit(world)?;
            if e.tool == Some(Tool::HoleNote) {
                if e.hole_source.as_ref().is_none_or(|source| {
                    !drawing_paper::same_projection(world.resource::<Workbench>(), source)
                }) {
                    return Err("Projection changed; choose the refreshed hole circle".into());
                }
                e.pending_selected = Some(e.document.next_annotation_id);
                hole::submit(
                    world,
                    handle,
                    &services.engine,
                    &services.bridge,
                    &stamp,
                    &e.circles[index],
                )?;
                return Ok(true);
            }
            let Some(Tool::Radial(mode)) = e.tool else {
                unreachable!()
            };
            let args = radial::request(&stamp, &e.circles[index], mode)?;
            e.pending_selected = Some(e.document.next_annotation_id);
            submit(
                world,
                handle,
                &services.engine,
                &services.bridge,
                &stamp,
                "drawing_add_radial_dimension",
                serde_json::to_value(args).map_err(|x| x.to_string())?,
            )?;
        }
        return Ok(true);
    }
    let grip = handle.hit_key(cursor).and_then(|key| {
        match world
            .get::<NativeCommandBinding>(Entity::from_bits(key.0))
            .map(|b| &b.command)
        {
            Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                _,
                Command::CenterGrip(id, index),
            ))) => Some((*id, *index)),
            _ => None,
        }
    });
    if let Some((id, index)) = grip {
        if e.dirty() {
            return Err("Apply or reset the annotation edit first".into());
        }
        drawing_editor::guard_sheet_edit(world)?;
        let grip = super::center_panel::geometry(world, world.resource::<Workbench>(), e, id)
            .and_then(|g| g.grips.get(index).copied())
            .ok_or("Repair the center annotation's projected references before dragging it")?;
        handle.cancel_pointer();
        e.select(id)?;
        e.drag = Some(Drag {
            stamp,
            start: point,
            draft: Draft::new(
                &e.document,
                Selection {
                    sheet_id: e.stamp.as_ref().unwrap().sheet_id,
                    annotation_id: id,
                },
            )?,
            linear_points: None,
            radial: None,
            angular: None,
            ordinate_points: None,
            center: Some(grip),
            projection: drawing_paper::projection_stamp(world.resource::<Workbench>()),
            moved: false,
        });
        return Ok(true);
    }
    if let Some((mut id, cloud_edge)) = annotation_at(world, handle, cursor) {
        if cloud_edge.is_some() {
            if !claim_cloud_target(world, handle, cursor) {
                return Ok(false);
            }
            let sheet = e
                .document
                .sheets
                .iter()
                .find(|s| s.id == stamp.sheet_id)
                .ok_or("Drawing sheet changed")?;
            let Some(hit) = cloud::hit(sheet, point) else {
                return Ok(true);
            };
            id = hit;
        }
        if e.dirty() {
            return Err("Apply or reset the annotation edit first".into());
        }
        drawing_editor::guard_sheet_edit(world)?;
        if e.document
            .sheets
            .iter()
            .flat_map(|s| &s.annotations)
            .any(|a| {
                a.id() == id
                    && matches!(
                        a,
                        limo_cad_sketch::DrawingAnnotationDto::CenterMark { .. }
                            | limo_cad_sketch::DrawingAnnotationDto::CenterLine { .. }
                            | limo_cad_sketch::DrawingAnnotationDto::CenterLineBetweenEdges { .. }
                            | limo_cad_sketch::DrawingAnnotationDto::AutomaticSymmetryAxis { .. }
                            | limo_cad_sketch::DrawingAnnotationDto::BoltCircleCenterLine { .. }
                    )
            })
        {
            handle.cancel_pointer();
            e.select(id)?;
            return Ok(true);
        }
        let mark = drawing_paper::annotation_marks(world.resource::<Workbench>())
            .iter()
            .find(|m| m.id == id)
            .ok_or("Annotation layout changed")?;
        let draft = Draft::new(
            &e.document,
            Selection {
                sheet_id: stamp.sheet_id,
                annotation_id: id,
            },
        )?;
        let unresolved = match draft.annotation() {
            limo_cad_sketch::DrawingAnnotationDto::LinearDimension { .. }
            | limo_cad_sketch::DrawingAnnotationDto::ChainDimension { .. } => {
                mark.linear_points.is_none()
            }
            limo_cad_sketch::DrawingAnnotationDto::OrdinateDimension { .. } => {
                mark.ordinate_points.is_none()
            }
            limo_cad_sketch::DrawingAnnotationDto::RadialDimension { .. } => mark.radial.is_none(),
            limo_cad_sketch::DrawingAnnotationDto::AngularDimension { .. }
            | limo_cad_sketch::DrawingAnnotationDto::ArcLengthDimension { .. } => {
                mark.angular.is_none()
            }
            limo_cad_sketch::DrawingAnnotationDto::LineDimension { .. }
            | limo_cad_sketch::DrawingAnnotationDto::PointLineDimension { .. }
            | limo_cad_sketch::DrawingAnnotationDto::ChamferNote { .. }
            | limo_cad_sketch::DrawingAnnotationDto::HoleNote { .. }
            | limo_cad_sketch::DrawingAnnotationDto::JoggedRadiusDimension { .. }
            | limo_cad_sketch::DrawingAnnotationDto::DatumFeature { .. }
            | limo_cad_sketch::DrawingAnnotationDto::GdtFrame { .. }
            | limo_cad_sketch::DrawingAnnotationDto::SurfaceTexture { .. }
            | limo_cad_sketch::DrawingAnnotationDto::EdgeRequirement { .. }
            | limo_cad_sketch::DrawingAnnotationDto::WeldSymbol { .. }
            | limo_cad_sketch::DrawingAnnotationDto::ItemBalloon { .. } => !mark.position_resolved,
            _ => false,
        };
        if unresolved {
            return Err("Repair the dimension's projected references before dragging it".into());
        }
        if cloud_edge.is_some() && e.selected != Some(id) {
            e.select(id)?;
        }
        let projection = drawing_paper::projection_stamp(world.resource::<Workbench>());
        e.drag = Some(Drag {
            stamp,
            start: point,
            draft,
            linear_points: mark.linear_points,
            radial: mark.radial,
            angular: mark.angular,
            ordinate_points: mark.ordinate_points,
            moved: false,
            center: None,
            projection,
        });
        return Ok(true);
    }
    if handle.hit_key(cursor).is_some() || handle.has_capture() {
        return Ok(false);
    }
    if e.tool == Some(Tool::RevisionCloud) {
        drawing_editor::guard_sheet_edit(world)?;
        if let Some(next) = e.cloud.click(&stamp, point, &e.document)? {
            e.pending_selected = Some(e.document.next_annotation_id);
            submit(
                world,
                handle,
                &services.engine,
                &services.bridge,
                &stamp,
                "drawing_add_annotation",
                runtime::created_annotation(next, &stamp)?,
            )?;
            e.cloud.cancel();
        }
        return Ok(true);
    }
    if e.tool == Some(Tool::Note) {
        drawing_editor::guard_sheet_edit(world)?;
        let mut note = fields::note_request(stamp.sheet_id, &e.fields)?;
        note.position = point;
        e.pending_selected = Some(e.document.next_annotation_id);
        submit(
            world,
            handle,
            &services.engine,
            &services.bridge,
            &stamp,
            "drawing_add_note",
            serde_json::to_value(note).map_err(|x| x.to_string())?,
        )?;
        return Ok(true);
    }
    if e.tool == Some(Tool::Chamfer) && e.chamfer.active() {
        drawing_editor::guard_sheet_edit(world)?;
        if e.chamfer_source.as_ref().is_none_or(|source| {
            !drawing_paper::same_projection(world.resource::<Workbench>(), source)
        }) {
            return Err("Projection changed; choose refreshed geometry".into());
        }
        e.chamfer.move_to(point, transform.sheet_mm)?;
        let next = e.chamfer.create(&e.document, &stamp)?;
        e.pending_selected = Some(e.document.next_annotation_id);
        submit(
            world,
            handle,
            &services.engine,
            &services.bridge,
            &stamp,
            "drawing_add_annotation",
            runtime::created_annotation(next, &stamp)?,
        )?;
        e.chamfer.cancel();
        return Ok(true);
    }
    if e.tool == Some(Tool::Linear) && e.straight.active() {
        drawing_editor::guard_sheet_edit(world)?;
        if e.line_source.as_ref().is_none_or(|source| {
            !drawing_paper::same_projection(world.resource::<Workbench>(), source)
        }) {
            return Err("Projection changed; choose refreshed geometry".into());
        }
        e.straight.move_to(point, transform.sheet_mm)?;
        let next = e.straight.create(&e.document, &stamp)?;
        e.pending_selected = Some(e.document.next_annotation_id);
        submit(
            world,
            handle,
            &services.engine,
            &services.bridge,
            &stamp,
            "drawing_add_annotation",
            runtime::created_annotation(next, &stamp)?,
        )?;
        e.straight.cancel();
        return Ok(true);
    }
    Ok(false)
}

/// Reuse retained projections for drag feedback and cancellation; no OCCT
/// query or shared-document mutation occurs while the pointer moves.
pub(super) fn refresh_preview(world: &mut World) -> Result<(), String> {
    let Some(mut state) = world.remove_resource::<Workbench>() else {
        return Ok(());
    };
    let result = (|| {
        let Some(e) = world.get_resource::<Editor>() else {
            return Ok(());
        };
        let Some(stamp) = &e.stamp else { return Ok(()) };
        if state.owner.as_ref() != Some(&stamp.owner) {
            return Ok(());
        }
        let Some(sheet) = e.document.sheets.iter().find(|s| s.id == stamp.sheet_id) else {
            return Ok(());
        };
        let sheet = preview(world, sheet, &stamp.owner, stamp.revision).into_owned();
        drawing_paper::annotation_preview(world, &mut state, &sheet)
    })();
    world.insert_resource(state);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::keyboard::{KeyCode, KeyboardInput};
    #[test]
    fn escape_belongs_only_to_active_drawing_authoring() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = crate::session_bridge::native_interface::tests::Fixture::new();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let (mut app, handle, _, _) = interface_shell::tests::fixture();
        let event = NativeHostInput {
            ui_scale: 1.,
            context: Some(fixture.owner()),
            cursor: None,
            modifiers: default(),
            actions: vec![],
            consumed: false,
            event: WindowEvent::KeyboardInput(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                text: None,
                state: ButtonState::Pressed,
                repeat: false,
                window: Entity::PLACEHOLDER,
            }),
        };
        let world = app.world_mut();
        for workspace in [Workspace::Solid, Workspace::Cam, Workspace::Drawing] {
            world.insert_resource(Workbench {
                workspace,
                ..default()
            });
            let mut editor = Editor::default();
            assert!(
                !inner(world, &handle, &services, &event, &mut editor).unwrap(),
                "Inactive authoring swallowed {workspace:?} Escape"
            );
        }
        let mut editor = Editor {
            tool: Some(Tool::Note),
            ..default()
        };
        assert!(inner(world, &handle, &services, &event, &mut editor).unwrap());
        assert!(editor.tool.is_none());
    }
}
