//! feature reference acquisition from the same camera and geometry as rendering.
use super::*;
use crate::native_viewport::NativePickPurpose;
use crate::session_bridge::native_interface::controller::NativeServices;
use limo_cad_core::FaceId;
use limo_cad_solid::Point2Dto;

pub(crate) fn hover_references(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    point: Option<[f32; 2]>,
) -> Result<bool, String> {
    let mut state = world.remove_resource::<NativeFeature>().unwrap_or_default();
    let result = (|| {
        let Some(editor) = state.editor.as_mut().filter(|e| {
            matches!(
                e.pick_target,
                Some(
                    SolidField::Source
                        | SolidField::StopFace
                        | SolidField::Targets
                        | SolidField::Path
                        | SolidField::Guide
                        | SolidField::AxisLine
                        | SolidField::Edges
                        | SolidField::FromPoint
                        | SolidField::ToPoint
                        | SolidField::PivotPoint
                        | SolidField::HoleSupport
                        | SolidField::HolePositions
                        | SolidField::Cylinder
                        | SolidField::Faces
                        | SolidField::TargetBody
                        | SolidField::ToolBodies
                        | SolidField::FirstPlane
                        | SolidField::SecondPlane
                        | SolidField::AxisEdge
                        | SolidField::DirectionEdge
                        | SolidField::SecondDirectionEdge
                        | SolidField::Bodies
                )
            ) && !e.form.is_busy()
        }) else {
            return Ok(false);
        };
        with_receipt(&services.bridge, &services.engine, owner, |receipt| {
            check_revision(editor, &receipt)?;
            if matches!(
                editor.pick_target,
                Some(SolidField::Path | SolidField::Guide | SolidField::AxisLine)
            ) {
                let candidate = point
                    .map(|p| {
                        if editor.pick_target == Some(SolidField::AxisLine) {
                            axis_line(world, editor, owner, p)
                        } else {
                            sketch_curve(world, editor, owner, p)
                        }
                    })
                    .transpose()?
                    .flatten();
                let next = candidate.map(|(sketch_name, entity_id)| PathRefDto {
                    sketch_name,
                    entity_ids: vec![entity_id],
                });
                if editor.hovered_path != next
                    || native_viewport::interface_preview_revision(world) != editor.preview_revision
                {
                    editor.hovered_path = next;
                    update_preview(editor, world)?;
                }
                return Ok(true);
            }
            if editor.pick_target.is_some_and(SolidField::is_move_point) {
                let next = point.and_then(|p| move_point(world, editor, owner, p));
                if next != editor.hovered_point {
                    editor.hovered_point = next;
                    update_preview(editor, world)?;
                }
                return Ok(true);
            }
            if editor.pick_target == Some(SolidField::HolePositions) {
                if let Some(placement) = editor.hole_placement {
                    placement.validate(world, &editor.snapshot)?;
                }
                let next = point.and_then(|p| {
                    hole_point(
                        world,
                        editor,
                        owner,
                        p,
                        editor.form.hole_support(),
                        editor.hole_placement,
                    )
                    .map(|(_, p)| {
                        editor
                            .hole_placement
                            .map_or(p, |placement| placement.world_point(p))
                    })
                });
                if next != editor.hovered_point
                    || native_viewport::interface_preview_revision(world) != editor.preview_revision
                {
                    editor.hovered_point = next;
                    update_preview(editor, world)?;
                }
                return Ok(true);
            }
            if matches!(
                editor.pick_target,
                Some(SolidField::FirstPlane | SolidField::SecondPlane)
            ) {
                let next = point
                    .map(|p| native_viewport::interface_support_pick(world, &owner.document_id, p))
                    .transpose()?
                    .flatten();
                if editor.hovered_plane != next {
                    editor.hovered_plane = next;
                    plane_view(world, owner, true, next)?;
                }
                return Ok(true);
            }
            let hit = point
                .map(|p| {
                    native_viewport::interface_pick(
                        world,
                        &owner.document_id,
                        p,
                        if editor
                            .pick_target
                            .is_some_and(SolidField::is_straight_reference)
                        {
                            NativePickPurpose::StraightEdge
                        } else if editor.pick_target == Some(SolidField::Edges) {
                            NativePickPurpose::RefinableEdge
                        } else {
                            NativePickPurpose::Geometry
                        },
                    )
                })
                .transpose()?
                .flatten();
            if editor.pick_target == Some(SolidField::Source) {
                let profile = point
                    .map(|p| source_profile(world, editor, owner, p, hit.as_ref()))
                    .transpose()?
                    .flatten();
                let face = hit
                    .as_ref()
                    .filter(|hit| {
                        profile.is_none()
                            && editor.form.kind() == SolidFormKind::Extrude
                            && editor.snapshot.source_local(hit.body_id, hit.occurrence_id)
                            && planar_face(
                                editor,
                                PlanarFaceSourceDto {
                                    body_id: BodyId(hit.body_id),
                                    face_id: FaceId(hit.face_id),
                                },
                            )
                            .is_some()
                    })
                    .map(|hit| (BodyId(hit.body_id), FaceId(hit.face_id)));
                if profile != editor.hovered_profile
                    || face != editor.hovered_face
                    || native_viewport::interface_preview_revision(world) != editor.preview_revision
                {
                    editor.hovered_profile = profile;
                    editor.hovered_face = face;
                    update_preview(editor, world)?;
                }
                return Ok(true);
            }
            if matches!(
                editor.pick_target,
                Some(
                    SolidField::TargetBody
                        | SolidField::ToolBodies
                        | SolidField::Bodies
                        | SolidField::Targets
                )
            ) {
                let hit = hit.filter(|hit| {
                    editor.form.move_is_component()
                        || editor.snapshot.source_local(hit.body_id, hit.occurrence_id)
                });
                let occurrence = hit.as_ref().and_then(|hit| hit.occurrence_id);
                let next = hit.map(|hit| BodyId(hit.body_id));
                let occurrence_changed = editor.hovered_occurrence != occurrence;
                editor.hovered_occurrence = occurrence;
                if occurrence_changed
                    || next != editor.hovered_body
                    || native_viewport::interface_preview_revision(world) != editor.preview_revision
                {
                    editor.hovered_body = next;
                    update_preview(editor, world)?;
                }
                return Ok(true);
            }
            if matches!(
                editor.pick_target,
                Some(
                    SolidField::Faces
                        | SolidField::Cylinder
                        | SolidField::HoleSupport
                        | SolidField::StopFace
                )
            ) {
                let hit = hit.filter(|hit| {
                    (if editor.pick_target == Some(SolidField::HoleSupport) {
                        hole_placement::Placement::capture(
                            world,
                            &editor.snapshot,
                            hit.body_id,
                            hit.occurrence_id,
                        )
                        .is_ok()
                    } else {
                        editor.snapshot.source_local(hit.body_id, hit.occurrence_id)
                    }) && (!matches!(
                        editor.pick_target,
                        Some(SolidField::HoleSupport | SolidField::StopFace)
                    ) || planar_face(
                        editor,
                        PlanarFaceSourceDto {
                            body_id: BodyId(hit.body_id),
                            face_id: FaceId(hit.face_id),
                        },
                    )
                    .is_some())
                });
                let occurrence = hit.as_ref().and_then(|hit| hit.occurrence_id);
                let next = hit.map(|hit| (BodyId(hit.body_id), FaceId(hit.face_id)));
                let snap = if editor.pick_target == Some(SolidField::HoleSupport) {
                    point.zip(next).and_then(|(p, (body_id, face_id))| {
                        let placement = hole_placement::Placement::capture(
                            world,
                            &editor.snapshot,
                            body_id.0,
                            occurrence,
                        )
                        .ok()?;
                        hole_point(
                            world,
                            editor,
                            owner,
                            p,
                            Some(PlanarFaceSourceDto { body_id, face_id }),
                            Some(placement),
                        )
                        .map(|(_, point)| placement.world_point(point))
                    })
                } else {
                    None
                };
                if next != editor.hovered_face
                    || occurrence != editor.hovered_occurrence
                    || snap != editor.hovered_point
                    || native_viewport::interface_preview_revision(world) != editor.preview_revision
                {
                    editor.hovered_face = next;
                    editor.hovered_occurrence = occurrence;
                    editor.hovered_point = snap;
                    update_preview(editor, world)?;
                }
                return Ok(true);
            }
            let next = hit
                .filter(|hit| editor.snapshot.source_local(hit.body_id, hit.occurrence_id))
                .and_then(|hit| {
                    let body = BodyId(hit.body_id);
                    let edge = limo_cad_core::EdgeId(hit.edge_id?);
                    editor
                        .snapshot
                        .viewport
                        .document
                        .scene
                        .bodies
                        .iter()
                        .find(|b| b.id == body)?
                        .edges
                        .iter()
                        .find(|e| {
                            e.id == edge
                                && (e.refinable
                                    || editor
                                        .pick_target
                                        .is_some_and(SolidField::is_straight_reference))
                        })?;
                    Some((body, edge))
                });
            if next == editor.hovered_edge
                && native_viewport::interface_preview_revision(world) == editor.preview_revision
            {
                return Ok(true);
            }
            editor.hovered_edge = next;
            update_preview(editor, world)?;
            Ok(true)
        })
    })();
    world.insert_resource(state);
    result
}

fn inside(point: limo_cad_sketch::Vec2, polygon: &[Point2Dto]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        if (a.y > point.y) != (b.y > point.y)
            && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
        {
            inside = !inside;
        }
    }
    inside
}

fn planar_face(editor: &Editor, source: PlanarFaceSourceDto) -> Option<limo_cad_core::PlaneBasis> {
    editor
        .snapshot
        .viewport
        .document
        .scene
        .bodies
        .iter()
        .find(|body| body.id == source.body_id)?
        .faces
        .iter()
        .find(|face| face.id == source.face_id)?
        .plane
}

pub(crate) fn handle_canvas_pick(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    point: [f32; 2],
) -> Result<Option<Value>, String> {
    let Some(panel) = panel(world) else {
        return Ok(None);
    };
    let Some(target) = panel.pick_target else {
        return Ok(None);
    };
    if panel.busy {
        return Err("Wait for the feature to finish".into());
    }
    let result = canvas_pick(world, services, owner, point, &panel, target);
    let reveal = {
        let mut state = world.resource_mut::<NativeFeature>();
        record_interaction_result(&mut state, owner, panel.form_id, &result)
    };
    if reveal {
        panel::reveal_error(world);
    }
    result
}

fn canvas_pick(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    point: [f32; 2],
    panel: &FeaturePanel,
    target: SolidField,
) -> Result<Option<Value>, String> {
    let pick =
        services
            .bridge
            .with_native_document_receipt(&services.engine, owner, |revision| {
                let editor = world
                    .resource::<NativeFeature>()
                    .editor
                    .as_ref()
                    .ok_or("The feature form is closed")?;
                if editor.id != panel.form_id
                    || editor.snapshot.receipt.owner != *owner
                    || editor.snapshot.receipt.revision != revision
                {
                    return Err(
                        "The model changed; reopen the feature before selecting references".into(),
                    );
                }
                if matches!(target, SolidField::FirstPlane | SolidField::SecondPlane) {
                    let plane =
                        native_viewport::interface_support_pick(world, &owner.document_id, point)?
                            .ok_or("Choose a visible reference plane or planar face")?;
                    if matches!(plane, limo_cad_core::PlaneRef::PlanarFace { .. }) {
                        let hit = native_viewport::interface_pick(
                            world,
                            &owner.document_id,
                            point,
                            NativePickPurpose::Geometry,
                        )?
                        .ok_or("The reference face is no longer visible")?;
                        if !editor.snapshot.source_local(hit.body_id, hit.occurrence_id) {
                            return Err("Open the component before selecting its references".into());
                        }
                    }
                    return Ok(FeaturePick::Plane(plane));
                }
                if target.is_move_point() {
                    return move_point(world, editor, owner, point)
                        .map(FeaturePick::MovePoint)
                        .ok_or_else(|| {
                            "Pick a visible sketch point, body vertex or surface".into()
                        });
                }
                if target == SolidField::HolePositions {
                    if let Some(placement) = editor.hole_placement {
                        placement.validate(world, &editor.snapshot)?;
                    }
                    if let Some((reference, point)) = hole_point(
                        world,
                        editor,
                        owner,
                        point,
                        editor.form.hole_support(),
                        editor.hole_placement,
                    ) {
                        return Ok(FeaturePick::HolePosition {
                            point,
                            reference: Some(reference),
                        });
                    }
                }
                let hit = native_viewport::interface_pick(
                    world,
                    &owner.document_id,
                    point,
                    if target.is_straight_reference() {
                        NativePickPurpose::StraightEdge
                    } else if target == SolidField::Edges {
                        NativePickPurpose::RefinableEdge
                    } else {
                        NativePickPurpose::Geometry
                    },
                )?;
                if target.is_straight_reference() {
                    let hit = hit.ok_or("Pick a straight edge")?;
                    if editor.form.move_is_component() {
                        return Ok(FeaturePick::OccurrenceEdge(
                            BodyId(hit.body_id),
                            limo_cad_core::EdgeId(hit.edge_id.ok_or("Choose a straight edge")?),
                            hit.occurrence_id.ok_or("Choose a component edge")?,
                        ));
                    }
                    if !editor.snapshot.source_local(hit.body_id, hit.occurrence_id) {
                        return Err("Open the component before selecting its axis".into());
                    }
                    return Ok(FeaturePick::AxisEdge(
                        BodyId(hit.body_id),
                        limo_cad_core::EdgeId(hit.edge_id.ok_or("Pick a straight edge")?),
                    ));
                }
                if target == SolidField::Edges {
                    let hit = hit.ok_or("Pick an edge on a visible body")?;
                    if !editor.snapshot.source_local(hit.body_id, hit.occurrence_id) {
                        return Err("Open the component before selecting its edges".into());
                    }
                    let id = limo_cad_core::EdgeId(
                        hit.edge_id.ok_or("Pick an edge, rather than a face")?,
                    );
                    let body = BodyId(hit.body_id);
                    let mut edges = editor
                        .form
                        .selected_edges()
                        .filter(|(selected, _)| *selected == body)
                        .map(|(_, e)| e.to_vec())
                        .unwrap_or_default();
                    if let Some(index) = edges.iter().position(|e| *e == id) {
                        edges.remove(index);
                    } else {
                        edges.push(id);
                    }
                    return Ok(FeaturePick::Edges {
                        body: Some(body),
                        edges,
                    });
                }
                if matches!(target, SolidField::Path | SolidField::Guide) {
                    let (name, id) = sketch_curve(world, editor, owner, point)?
                        .ok_or("Pick a visible sketch curve for this path")?;
                    let mut path = editor
                        .form
                        .path(target)
                        .filter(|p| p.sketch_name == name)
                        .cloned()
                        .unwrap_or(PathRefDto {
                            sketch_name: name,
                            entity_ids: vec![],
                        });
                    if let Some(index) = path.entity_ids.iter().position(|item| *item == id) {
                        path.entity_ids.remove(index);
                    } else {
                        path.entity_ids.push(id);
                    }
                    return Ok(FeaturePick::Path(path));
                }
                if target == SolidField::AxisLine {
                    return axis_line(world, editor, owner, point)?
                        .map(|(sketch_name, entity_id)| FeaturePick::AxisLine {
                            sketch_name,
                            entity_id,
                        })
                        .ok_or_else(|| "Pick a straight line on the profile's plane".into());
                }
                let profile = if target == SolidField::Source {
                    source_profile(world, editor, owner, point, hit.as_ref())?
                } else {
                    None
                };
                if let Some(profile) = profile {
                    let mut profiles = editor.form.selected_profiles();
                    if editor.form.kind() != SolidFormKind::Loft {
                        profiles.retain(|p| p.sketch_name == profile.sketch_name);
                    }
                    if editor.form.kind() == SolidFormKind::Sweep
                        && profiles.first() != Some(&profile)
                    {
                        profiles.clear();
                    }
                    if let Some(index) = profiles.iter().position(|p| p == &profile) {
                        profiles.remove(index);
                    } else {
                        profiles.push(profile);
                    }
                    return Ok(FeaturePick::Profiles(profiles));
                }
                let hit = hit.ok_or("No selectable feature reference at this point")?;
                if target == SolidField::Bodies && editor.form.move_is_component() {
                    return hit
                        .occurrence_id
                        .map(FeaturePick::Occurrence)
                        .ok_or_else(|| "Select an assembly component".into());
                }
                if !matches!(target, SolidField::HoleSupport | SolidField::HolePositions)
                    && !editor.snapshot.source_local(hit.body_id, hit.occurrence_id)
                {
                    return Err("Open the component before selecting its references".into());
                }
                match target {
                    SolidField::HoleSupport => {
                        let face = PlanarFaceSourceDto {
                            body_id: BodyId(hit.body_id),
                            face_id: FaceId(hit.face_id),
                        };
                        let placement = hole_placement::Placement::capture(
                            world,
                            &editor.snapshot,
                            hit.body_id,
                            hit.occurrence_id,
                        )?;
                        let snapped =
                            hole_point(world, editor, owner, point, Some(face), Some(placement));
                        Ok(FeaturePick::PlacedHoleSupport {
                            face,
                            point: Some(snapped.as_ref().map_or_else(
                                || placement.local_point(hit.point.map(f64::from)),
                                |(_, p)| *p,
                            )),
                            reference: snapped.map(|(r, _)| r),
                            placement,
                        })
                    }
                    SolidField::HolePositions => {
                        if editor.form.hole_support()
                            != Some(PlanarFaceSourceDto {
                                body_id: BodyId(hit.body_id),
                                face_id: FaceId(hit.face_id),
                            })
                        {
                            return Err("Click the support face or a visible sketch point".into());
                        }
                        let position = if let Some(placement) = editor.hole_placement {
                            placement.validate(world, &editor.snapshot)?;
                            if !placement.matches(hit.body_id, hit.occurrence_id) {
                                return Err(
                                    "Click the same component instance as the hole support".into(),
                                );
                            }
                            placement.local_point(hit.point.map(f64::from))
                        } else {
                            if !editor.snapshot.source_local(hit.body_id, hit.occurrence_id) {
                                return Err(
                                    "Select the hole support on this component instance first"
                                        .into(),
                                );
                            }
                            hit.point.map(f64::from)
                        };
                        Ok(FeaturePick::HolePosition {
                            point: position,
                            reference: None,
                        })
                    }
                    SolidField::Bodies => {
                        let id = BodyId(hit.body_id);
                        let mut bodies = editor.form.selected_bodies().to_vec();
                        if editor.form.kind() == SolidFormKind::SplitBody {
                            bodies = vec![id];
                        } else if let Some(index) = bodies.iter().position(|b| *b == id) {
                            bodies.remove(index);
                        } else {
                            bodies.push(id);
                        }
                        Ok(FeaturePick::Bodies(bodies))
                    }
                    SolidField::TargetBody | SolidField::ToolBodies => {
                        let id = BodyId(hit.body_id);
                        let mut bodies = editor.form.combine_bodies(target);
                        if target == SolidField::TargetBody {
                            bodies = vec![id];
                        } else if let Some(index) = bodies.iter().position(|b| *b == id) {
                            bodies.remove(index);
                        } else {
                            bodies.push(id);
                        }
                        Ok(FeaturePick::Bodies(bodies))
                    }
                    SolidField::Faces => {
                        let body = BodyId(hit.body_id);
                        let id = FaceId(hit.face_id);
                        let mut faces = editor
                            .form
                            .selected_faces()
                            .filter(|(selected, _)| *selected == body)
                            .map(|(_, f)| f.to_vec())
                            .unwrap_or_default();
                        if let Some(index) = faces.iter().position(|f| *f == id) {
                            faces.remove(index);
                        } else {
                            faces.push(id);
                        }
                        Ok(FeaturePick::Faces {
                            body: Some(body),
                            faces,
                        })
                    }
                    SolidField::Targets => {
                        let mut targets = editor.form.targets().to_vec();
                        let id = BodyId(hit.body_id);
                        if let Some(index) = targets.iter().position(|item| *item == id) {
                            targets.remove(index);
                        } else {
                            targets.push(id);
                        }
                        Ok(FeaturePick::Bodies(targets))
                    }
                    SolidField::Cylinder | SolidField::Source | SolidField::StopFace => {
                        Ok(FeaturePick::Face(PlanarFaceSourceDto {
                            body_id: BodyId(hit.body_id),
                            face_id: FaceId(hit.face_id),
                        }))
                    }
                    _ => Err("This feature field does not accept canvas references".into()),
                }
            })?;
    accept_pick(
        &services.engine,
        &services.bridge,
        world,
        owner,
        panel.form_id,
        pick,
        || Ok(()),
    )
    .map(|mut value| {
        value["handled"] = json!(true);
        Some(value)
    })
}

/// The catalog carries every engine-supported point kind, including arc ends and
/// spline fit points. Selection uses the same projection as the visible sketch.
fn hole_point(
    world: &World,
    editor: &Editor,
    owner: &DocumentContext,
    cursor: [f32; 2],
    support: Option<PlanarFaceSourceDto>,
    placement: Option<hole_placement::Placement>,
) -> Option<(limo_cad_solid::SketchPointRefDto, [f64; 3])> {
    let (_, _, view, _) = native_viewport::interface_view(world);
    let mut best: Option<(f32, limo_cad_solid::SketchPointRefDto, [f64; 3])> = None;
    for sketch in &editor.snapshot.viewport.document.profile_catalog {
        if view.hidden_sketch_names.contains(&sketch.sketch_name) {
            continue;
        }
        for p in &sketch.reference_points {
            let world_point = sketch.basis.to_3d([p.position.x, p.position.y]);
            // Hole references are projected onto the support plane, just as
            // the kernel resolves their associative positions. This also
            // snaps retained sketch points on the stock's base plane.
            let point = if let Some(face) = support {
                let basis = planar_face(editor, face)?;
                basis.to_3d(basis.to_2d(world_point))
            } else {
                world_point
            };
            let Some(pixel) = native_viewport::interface_world_point(
                world,
                &owner.document_id,
                placement.map_or(point, |placement| placement.world_point(point)),
            )
            .ok()
            .flatten() else {
                continue;
            };
            let distance = (pixel[0] - cursor[0]).hypot(pixel[1] - cursor[1]);
            if distance <= 9. && best.as_ref().is_none_or(|(d, _, _)| distance < *d) {
                best = Some((
                    distance,
                    limo_cad_solid::SketchPointRefDto {
                        sketch_name: sketch.sketch_name.clone(),
                        entity_id: p.entity_id,
                        point: p.point.clone(),
                    },
                    point,
                ));
            }
        }
    }
    best.map(|(_, r, p)| (r, p))
}

fn move_point(
    world: &World,
    editor: &Editor,
    owner: &DocumentContext,
    cursor: [f32; 2],
) -> Option<[f64; 3]> {
    if let Some((_, point)) = hole_point(world, editor, owner, cursor, None, None) {
        return Some(point);
    }
    let hit = native_viewport::interface_pick(
        world,
        &owner.document_id,
        cursor,
        NativePickPurpose::Vertex,
    )
    .ok()
    .flatten()
    .or_else(|| {
        native_viewport::interface_pick(
            world,
            &owner.document_id,
            cursor,
            NativePickPurpose::Geometry,
        )
        .ok()
        .flatten()
    })?;
    let body = editor
        .snapshot
        .viewport
        .document
        .scene
        .bodies
        .iter()
        .find(|b| b.id.0 == hit.body_id)?;
    let view = native_viewport::interface_view(world).2;
    let pose = hit
        .occurrence_id
        .and_then(|id| {
            view.instance_body_poses
                .iter()
                .find(|p| p.occurrence_id.0 == id && p.body_id.0 == hit.body_id)
                .map(|p| (p.translation, p.rotation))
        })
        .or_else(|| {
            view.body_poses
                .iter()
                .find(|p| p.body_id.0 == hit.body_id)
                .map(|p| (p.translation, p.rotation))
        })
        .unwrap_or(([0.; 3], [0., 0., 0., 1.]));
    let rotation = bevy::math::DQuat::from_array(pose.1);
    let translation = bevy::math::DVec3::from_array(pose.0);
    let mut best: Option<(f32, [f64; 3])> = None;
    for edge in &body.edges {
        for point in edge.points.first().into_iter().chain(edge.points.last()) {
            let p = (rotation * bevy::math::DVec3::new(point.x, point.y, point.z) + translation)
                .to_array();
            let Some(pixel) = native_viewport::interface_world_point(world, &owner.document_id, p)
                .ok()
                .flatten()
            else {
                continue;
            };
            let distance = (pixel[0] - cursor[0]).hypot(pixel[1] - cursor[1]);
            if distance <= 9. && best.as_ref().is_none_or(|(d, _)| distance < *d) {
                best = Some((distance, p));
            }
        }
    }
    Some(
        best.map(|(_, p)| p)
            .unwrap_or_else(|| hit.point.map(f64::from)),
    )
}

fn source_profile(
    world: &World,
    editor: &Editor,
    owner: &DocumentContext,
    point: [f32; 2],
    hit: Option<&native_viewport::NativePick>,
) -> Result<Option<ProfileRefDto>, String> {
    let (_, camera, presentation, _) = native_viewport::interface_view(world);
    let camera = bevy::math::DVec3::from_array(camera.position.map(f64::from));
    let mut nearest = hit.map(|hit| hit.distance).unwrap_or(f64::INFINITY);
    let mut profile = None;
    for sketch in &editor.snapshot.viewport.document.profile_catalog {
        if presentation
            .hidden_sketch_names
            .contains(&sketch.sketch_name)
        {
            continue;
        }
        let Some(local) = native_viewport::interface_sketch_point(
            world,
            &owner.document_id,
            point,
            sketch.basis,
        )?
        else {
            continue;
        };
        let distance = camera.distance(bevy::math::DVec3::from_array(
            sketch.basis.to_3d([local.x, local.y]),
        ));
        if distance > nearest + 1e-5 {
            continue;
        }
        for region in &sketch.profiles {
            if region.nesting_depth % 2 != 0 || !inside(local, &region.points) {
                continue;
            }
            if sketch
                .profiles
                .iter()
                .any(|hole| hole.parent_index == Some(region.index) && inside(local, &hole.points))
            {
                continue;
            }
            nearest = distance;
            profile = Some(ProfileRefDto {
                sketch_name: sketch.sketch_name.clone(),
                profile_index: region.index,
            });
        }
    }
    Ok(profile)
}

fn sketch_curve(
    world: &World,
    editor: &Editor,
    owner: &DocumentContext,
    point: [f32; 2],
) -> Result<Option<(String, u64)>, String> {
    let (_, camera, presentation, _) = native_viewport::interface_view(world);
    let camera = bevy::math::DVec3::from_array(camera.position.map(f64::from));
    let mut candidate = None;
    let mut distance = f64::INFINITY;
    for sketch in &editor.snapshot.viewport.document.finished_sketches {
        if presentation.hidden_sketch_names.contains(&sketch.name) {
            continue;
        }
        let Some(catalog) = editor
            .snapshot
            .viewport
            .document
            .profile_catalog
            .iter()
            .find(|s| s.sketch_name == sketch.name)
        else {
            continue;
        };
        let entities: Vec<_> = sketch
            .entities
            .iter()
            .filter(|entity| {
                catalog
                    .path_curves
                    .iter()
                    .any(|c| c.entity_id() == entity.id().0)
            })
            .cloned()
            .collect();
        let Some(id) = crate::native_editor::selection::hit(&entities, point, false, |p| {
            native_viewport::interface_world_point(
                world,
                &owner.document_id,
                sketch.basis.to_3d([p.x, p.y]),
            )
            .ok()
            .flatten()
        }) else {
            continue;
        };
        let Some(local) = native_viewport::interface_sketch_point(
            world,
            &owner.document_id,
            point,
            sketch.basis,
        )?
        else {
            continue;
        };
        let depth = camera.distance(bevy::math::DVec3::from_array(
            sketch.basis.to_3d([local.x, local.y]),
        ));
        if depth < distance {
            distance = depth;
            candidate = Some((sketch.name.clone(), id.0));
        }
    }
    Ok(candidate)
}

fn axis_line(
    world: &World,
    editor: &Editor,
    owner: &DocumentContext,
    point: [f32; 2],
) -> Result<Option<(String, u64)>, String> {
    let presentation = native_viewport::interface_view(world).2;
    let model = editor.snapshot.model(editor.form.parameter_sketch());
    let cursor = bevy::math::Vec2::from_array(point);
    let mut candidate = None;
    for sketch in &editor.snapshot.viewport.document.profile_catalog {
        if presentation
            .hidden_sketch_names
            .contains(&sketch.sketch_name)
        {
            continue;
        }
        for line in &sketch.lines {
            if !editor
                .form
                .accepts_axis(&sketch.sketch_name, line.entity_id, &model)
            {
                continue;
            }
            let Some(a) = native_viewport::interface_world_point(
                world,
                &owner.document_id,
                sketch.basis.to_3d([line.start.x, line.start.y]),
            )?
            else {
                continue;
            };
            let Some(b) = native_viewport::interface_world_point(
                world,
                &owner.document_id,
                sketch.basis.to_3d([line.end.x, line.end.y]),
            )?
            else {
                continue;
            };
            let a = bevy::math::Vec2::from_array(a);
            let b = bevy::math::Vec2::from_array(b);
            let delta = b - a;
            let t = if delta.length_squared() > 1e-10 {
                ((cursor - a).dot(delta) / delta.length_squared()).clamp(0., 1.)
            } else {
                0.
            };
            let distance = cursor.distance(a + delta * t);
            if distance <= 7.
                && candidate
                    .as_ref()
                    .is_none_or(|(best, _, _)| distance < *best)
            {
                candidate = Some((distance, sketch.sketch_name.clone(), line.entity_id));
            }
        }
    }
    Ok(candidate.map(|(_, name, id)| (name, id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_hit_test_handles_both_windings_and_concavity() {
        let mut polygon = vec![
            Point2Dto::new(0., 0.),
            Point2Dto::new(4., 0.),
            Point2Dto::new(4., 1.),
            Point2Dto::new(1., 1.),
            Point2Dto::new(1., 4.),
            Point2Dto::new(0., 4.),
        ];
        for _ in 0..2 {
            assert!(inside(limo_cad_sketch::Vec2::new(0.5, 3.), &polygon));
            assert!(!inside(limo_cad_sketch::Vec2::new(3., 3.), &polygon));
            polygon.reverse();
        }
    }
}
