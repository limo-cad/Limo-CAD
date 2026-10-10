//! Sketch modify tools: Fillet, Chamfer, Offset, Trim, Extend, Break, Mirror,
//! Move/Copy, Sketch Scale, and the Polygon create tool. Geometry kernels
//! come from the geomops leaf modules; this module owns entity-model integration:
//! shared-point retargeting, structural coincident, auto-dimension (D9),
//! and one-undo-command-per-tool semantics.

use std::collections::{BTreeMap, BTreeSet};

use crate::constraint::{ArcEndpoint, Constraint};
use crate::dto::{
    BreakRequest, ChamferRequest, CircularPatternRequest, ExtendRequest, FilletPreviewDto,
    FilletRequest, MirrorRequest, MoveCopyRequest, OffsetPreviewDto, OffsetRequest, PolygonRequest,
    PreviewCurve, RectangularPatternRequest, ScaleRequest, ToolResult, TrimPreviewDto, TrimRequest,
};
use crate::entity::{Entity, EntityId};
use crate::geometry::Vec2;
use crate::geomops::{chamfer, fillet, offset, polygon, trimext, xform};
use crate::params::ParamKind;
use crate::session::{SessionError, SketchSession};

const EPS: f64 = 1e-9;
const CORNER_EPS: f64 = 1e-6;
const TAU: f64 = std::f64::consts::TAU;

#[derive(Debug)]
struct SweepTrim {
    kept: Vec<(f64, f64)>,
    removed: (f64, f64),
}

/// Entity arc ↔ geomops sweep conversions (entity arcs sweep CCW from
/// start_angle to end_angle; geomops carries an explicit `ccw` flag).
fn entity_sweep_to_geom(start: f64, end: f64) -> (f64, f64, bool) {
    (start, end, true)
}

fn geom_sweep_to_entity(start: f64, end: f64, ccw: bool) -> (f64, f64) {
    if ccw {
        (start, end)
    } else {
        (end, start)
    }
}

fn angle_of(center: Vec2, p: Vec2) -> f64 {
    (p.y - center.y).atan2(p.x - center.x)
}

fn sweep_contains(a0: f64, a1: f64, a: f64) -> bool {
    let span = (a1 - a0).rem_euclid(TAU);
    let off = (a - a0).rem_euclid(TAU);
    off <= span + 1e-9
}

impl SketchSession {
    fn arc_endpoint_points(&self, arc: EntityId) -> Vec<EntityId> {
        self.sketch
            .constraints()
            .filter_map(|(_, constraint)| match *constraint {
                Constraint::ArcEndpointCoincident {
                    point, arc: owner, ..
                } if owner == arc => Some(point),
                _ => None,
            })
            .collect()
    }

    /// A generated center is transform-owned only when every curve/diagonal
    /// using it is carried too. Circle and rectangle centers may be the same
    /// point; checking each relation family separately loses that topology.
    fn is_selected_center(
        &self,
        point: EntityId,
        selection: &BTreeSet<EntityId>,
        points: &BTreeSet<EntityId>,
    ) -> bool {
        if !self.sketch.is_generated_point(point) || self.sketch.is_referenced_by_entity(point) {
            return false;
        }
        let relations: Vec<&Constraint> = self.sketch.relations_pointing_at(point).collect();
        !relations.is_empty()
            && relations.iter().all(|constraint| match **constraint {
                Constraint::CenterCoincident {
                    point: center,
                    curve,
                } => center == point && selection.contains(&curve),
                Constraint::SpanMidpoint {
                    point: center,
                    start,
                    end,
                } => center == point && points.contains(&start) && points.contains(&end),
                _ => false,
            })
    }

    /// An acquired point may follow a selected curve, but it must not carry
    /// unselected lines, other curves or rectangle diagonals with it.
    fn center_has_unselected_geometry(
        &self,
        point: EntityId,
        selection: &BTreeSet<EntityId>,
        points: &BTreeSet<EntityId>,
    ) -> bool {
        self.sketch.is_referenced_by_entity(point)
            || self
                .sketch
                .relations_pointing_at(point)
                .any(|constraint| match *constraint {
                    Constraint::CenterCoincident { curve, .. } => !selection.contains(&curve),
                    Constraint::SpanMidpoint { start, end, .. } => {
                        !points.contains(&start) || !points.contains(&end)
                    }
                    Constraint::ArcEndpointCoincident { arc, .. } => !selection.contains(&arc),
                    _ => false,
                })
    }

    /// Include the centers needed to preserve incidences inside a selection.
    /// Copy may duplicate a shared/acquired center without copying any external
    /// owner; an in-place transform must additionally exclude external anchors.
    fn include_selection_centers(
        &self,
        selection: &BTreeSet<EntityId>,
        points: &mut BTreeSet<EntityId>,
    ) {
        loop {
            let centers: Vec<_> = self
                .sketch
                .constraints()
                .filter_map(|(_, constraint)| match *constraint {
                    Constraint::CenterCoincident { point, curve } if selection.contains(&curve) => {
                        Some(point)
                    }
                    Constraint::SpanMidpoint { point, start, end }
                        if points.contains(&start) && points.contains(&end) =>
                    {
                        Some(point)
                    }
                    _ => None,
                })
                .collect();
            let previous_len = points.len();
            points.extend(centers);
            if points.len() == previous_len {
                break;
            }
        }
    }

    /// Every point a transform of `ids` has to carry with it, so shared geometry
    /// stays together and the solver never has to split the difference between a
    /// curve and the handle that defines it. Move and Scale share this list;
    /// Copy also materializes independent counterparts of external centers.
    ///
    /// Which center a rectangle owns is a property of the relation, not of any
    /// one selected edge. All its diagonal corners and any other owners must
    /// be carried; selecting just one of two centered rectangles is not enough.
    fn owned_points(&self, ids: &BTreeSet<EntityId>) -> BTreeSet<EntityId> {
        let mut points = BTreeSet::new();
        for id in ids {
            match self.sketch.entity(*id) {
                Some(Entity::Point { .. }) => {
                    points.insert(*id);
                }
                Some(Entity::Line { start, end }) => {
                    points.insert(*start);
                    points.insert(*end);
                }
                Some(Entity::Arc { .. }) => {
                    points.extend(self.arc_endpoint_points(*id));
                }
                Some(Entity::Circle { .. } | Entity::Spline { .. }) | None => {}
            }
        }
        let direct_points = points.clone();
        self.include_selection_centers(ids, &mut points);

        loop {
            let external: Vec<_> = points
                .difference(&direct_points)
                .filter(|point| !self.is_selected_center(**point, ids, &points))
                .copied()
                .collect();
            if external.is_empty() {
                break;
            }
            for point in external {
                points.remove(&point);
            }
        }
        points
    }

    /// Trim/Break change the finite sweep. Keep original endpoint references
    /// on the surviving pieces and give each cut a new point, shared across
    /// adjacent pieces. Stale anchors must not pull the edited angles back.
    fn rebind_arc_endpoints(&mut self, original: EntityId, parts: &[EntityId]) {
        let bindings: Vec<_> = self
            .sketch
            .constraints()
            .filter_map(|(id, c)| match *c {
                Constraint::ArcEndpointCoincident { point, arc, .. } if arc == original => {
                    Some((id, point))
                }
                _ => None,
            })
            .collect();
        let mut points: Vec<_> = bindings
            .iter()
            .filter_map(|(_, point)| {
                self.sketch
                    .point_position(*point)
                    .map(|position| (*point, position))
            })
            .collect();
        for (id, _) in bindings {
            self.sketch.remove_constraint(id);
        }
        for &arc in parts {
            let Some(Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) = self.sketch.entity(arc).cloned()
            else {
                continue;
            };
            let mut used = Vec::new();
            for (end, angle) in [
                (ArcEndpoint::Start, start_angle),
                (ArcEndpoint::End, end_angle),
            ] {
                let position = center + Vec2::new(radius * angle.cos(), radius * angle.sin());
                let point = points
                    .iter()
                    .find(|(point, p)| !used.contains(point) && p.distance(position) <= EPS)
                    .map(|(point, _)| *point)
                    .unwrap_or_else(|| {
                        let point = self.sketch.add_generated_point(position);
                        points.push((point, position));
                        point
                    });
                self.sketch
                    .add_constraint(Constraint::ArcEndpointCoincident { point, arc, end });
                used.push(point);
            }
        }
    }

    fn line_seg(&self, id: EntityId) -> Result<fillet::LineSeg, SessionError> {
        let (a, b) = self
            .sketch
            .resolved_line(id)
            .ok_or(SessionError::InvalidConstraint(
                "expected a line entity".to_string(),
            ))?;
        Ok(fillet::LineSeg { a, b })
    }

    fn circle_of(&self, id: EntityId) -> Option<(Vec2, f64)> {
        match self.sketch.entity(id) {
            Some(Entity::Circle { center, radius }) | Some(Entity::Arc { center, radius, .. }) => {
                Some((*center, *radius))
            }
            _ => None,
        }
    }

    fn arc_sweep(&self, id: EntityId) -> Option<(f64, f64)> {
        match self.sketch.entity(id) {
            Some(Entity::Arc {
                start_angle,
                end_angle,
                ..
            }) => Some((*start_angle, *end_angle)),
            Some(Entity::Circle { .. }) => Some((0.0, TAU)),
            _ => None,
        }
    }

    /// Retarget a line endpoint (the end nearest `near`) to an existing or
    /// new point at `pos`. The abandoned corner point is KEPT as a
    /// persistent constraint reference (2026-07-19 PM owner design ask) —
    /// the caller anchors it at the edges' intersection
    /// with Coincident incidences.
    fn retarget_line_end(&mut self, line: EntityId, near: Vec2, pos: Vec2) -> EntityId {
        let (start, end) = self.sketch.line_endpoint_ids(line).unwrap();
        let (sa, sb) = (
            self.sketch.point_position(start).unwrap(),
            self.sketch.point_position(end).unwrap(),
        );
        let at_start = sa.distance(near) <= sb.distance(near);
        let old = if at_start { start } else { end };
        let new_point = self.sketch.add_generated_point(pos);
        if let Some(Entity::Line { start, end }) = self.sketch.entity_mut(line) {
            if at_start {
                *start = new_point;
            } else {
                *end = new_point;
            }
        }
        let _ = old;
        new_point
    }

    /// The vertex-side endpoint id of `line` near `near` (BEFORE retarget).
    fn vertex_endpoint(&self, line: EntityId, near: Vec2) -> EntityId {
        let (start, end) = self.sketch.line_endpoint_ids(line).unwrap();
        let (sa, sb) = (
            self.sketch.point_position(start).unwrap(),
            self.sketch.point_position(end).unwrap(),
        );
        if sa.distance(near) <= sb.distance(near) {
            start
        } else {
            end
        }
    }

    fn endpoint_at(&self, line: EntityId, position: Vec2) -> Option<EntityId> {
        let (start, end) = self.sketch.line_endpoint_ids(line)?;
        [start, end].into_iter().find(|point| {
            self.sketch
                .point_position(*point)
                .is_some_and(|candidate| candidate.distance(position) <= CORNER_EPS)
        })
    }

    fn shared_corner_reference(
        &self,
        l1: EntityId,
        l2: EntityId,
        position: Vec2,
    ) -> Option<EntityId> {
        let c1 = self.endpoint_at(l1, position)?;
        let c2 = self.endpoint_at(l2, position)?;
        if c1 == c2 {
            return Some(c1);
        }
        self.sketch
            .constraints()
            .any(|(_, constraint)| {
                matches!(
                    constraint,
                    Constraint::Coincident { a, b }
                        if (*a == c1 && *b == c2) || (*a == c2 && *b == c1)
                )
            })
            .then_some(c1)
    }

    fn collinear_overlap(&self, selected: EntityId, candidate: EntityId) -> Option<f64> {
        let (a, b) = self.sketch.resolved_line(selected)?;
        let (c, d) = self.sketch.resolved_line(candidate)?;
        let direction = b - a;
        let length = direction.length();
        if length <= CORNER_EPS {
            return None;
        }
        let unit = direction * (1.0 / length);
        let cross = |value: Vec2| direction.x * value.y - direction.y * value.x;
        if cross(c - a).abs() > CORNER_EPS * length || cross(d - a).abs() > CORNER_EPS * length {
            return None;
        }
        let tc = (c - a).dot(unit);
        let td = (d - a).dot(unit);
        let overlap = length.min(tc.max(td)) - 0.0_f64.max(tc.min(td));
        (overlap > CORNER_EPS).then_some(overlap)
    }

    /// A click can land on a shorter line that overlaps the visible outline.
    /// When that support does not own the selected corner, prefer the unique
    /// collinear carrier that overlaps it and structurally shares the corner
    /// point with the other selected line. This keeps pick ambiguity from
    /// turning into a false persistent-corner constraint at commit time.
    fn resolve_adjacent_carrier(
        &self,
        selected: EntityId,
        other: EntityId,
        intersection: Vec2,
    ) -> EntityId {
        if self.endpoint_at(selected, intersection).is_some() {
            return selected;
        }
        let Some(other_corner) = self.endpoint_at(other, intersection) else {
            return selected;
        };
        self.sketch
            .entities()
            .filter_map(|(candidate, entity)| {
                if candidate == selected || candidate == other {
                    return None;
                }
                if !matches!(entity, Entity::Line { .. })
                    || self.endpoint_at(candidate, intersection) != Some(other_corner)
                {
                    return None;
                }
                self.collinear_overlap(selected, candidate)
                    .map(|overlap| (candidate, overlap))
            })
            .max_by(|(a_id, a_overlap), (b_id, b_overlap)| {
                a_overlap.total_cmp(b_overlap).then_with(|| b_id.cmp(a_id))
            })
            .map(|(candidate, _)| candidate)
            .unwrap_or(selected)
    }

    fn resolve_corner_lines(&self, l1: EntityId, l2: EntityId) -> (EntityId, EntityId) {
        let (Ok(first), Ok(second)) = (self.line_seg(l1), self.line_seg(l2)) else {
            return (l1, l2);
        };
        let Some(intersection) = line_vertex(&first, &second) else {
            return (l1, l2);
        };
        let l1 = self.resolve_adjacent_carrier(l1, l2, intersection);
        let l2 = self.resolve_adjacent_carrier(l2, l1, intersection);
        (l1, l2)
    }

    /// Anchor a persistent corner point at the intersection of its two
    /// original edges via Coincident incidences (idempotent for re-ops).
    fn anchor_corner_reference(&mut self, corner: EntityId, l1: EntityId, l2: EntityId) {
        let existing: Vec<(EntityId, EntityId)> = self
            .sketch
            .constraints()
            .filter_map(|(_, c)| match c {
                Constraint::Coincident { a, b } => Some((*a, *b)),
                _ => None,
            })
            .collect();
        let has =
            |a: EntityId, b: EntityId| existing.contains(&(a, b)) || existing.contains(&(b, a));
        if !has(corner, l1) {
            self.sketch
                .add_constraint(Constraint::Coincident { a: corner, b: l1 });
        }
        if !has(corner, l2) {
            self.sketch
                .add_constraint(Constraint::Coincident { a: corner, b: l2 });
        }
    }

    /// A midpoint attached to a finite carrier must retain the original
    /// corner-to-corner span when a corner operation shortens that carrier.
    /// Capture and retarget those relations before the line endpoint changes.
    fn preserve_midpoint_span(&mut self, line: EntityId, corner: EntityId) {
        let Some((start, end)) = self.sketch.line_endpoint_ids(line) else {
            return;
        };
        let opposite = if start == corner {
            end
        } else if end == corner {
            start
        } else {
            return;
        };
        let midpoint_constraints = self
            .sketch
            .constraints()
            .filter_map(|(id, constraint)| match *constraint {
                Constraint::Midpoint { a, b } if b == line => Some((id, a)),
                _ => None,
            })
            .collect::<Vec<_>>();
        for (id, point) in midpoint_constraints {
            self.sketch.replace_constraint(
                id,
                Constraint::SpanMidpoint {
                    point,
                    start: corner,
                    end: opposite,
                },
            );
        }
    }

    fn mutate_with_undo<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<R, SessionError>,
    ) -> Result<ToolResult, SessionError> {
        self.mutate_with_undo_stays(|session| {
            f(session)?;
            Ok(crate::solver::SolveStays::default())
        })
    }

    fn mutate_with_undo_stays(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<crate::solver::SolveStays, SessionError>,
    ) -> Result<ToolResult, SessionError> {
        let before = self.sketch.snapshot();
        let stays = match f(self) {
            Ok(stays) => stays,
            Err(error) => {
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
        };
        self.sketch.cleanup_generated_points(&before);
        let analysis = crate::solver::solve_with_stays(&mut self.sketch, &[], &stays);
        if !analysis.converged {
            self.sketch.restore(before);
            self.recompute();
            return Err(SessionError::InvalidConstraint(
                "operation conflicts with existing sketch constraints".to_string(),
            ));
        }
        self.analysis = Some(analysis);
        self.push_command(before);
        Ok(ToolResult {
            entities: Vec::new(),
            sketch: self.dto(),
        })
    }

    pub fn fillet_preview(
        &self,
        request: &FilletRequest,
    ) -> Result<FilletPreviewDto, SessionError> {
        let (_, _, arc, t1, t2) = self.compute_fillet(request)?;
        Ok(FilletPreviewDto {
            center: arc.center,
            radius: arc.radius,
            start_angle: arc.start_angle,
            end_angle: arc.end_angle,
            ccw: arc.ccw,
            tangent_on_l1: t1,
            tangent_on_l2: t2,
        })
    }

    fn compute_fillet(
        &self,
        request: &FilletRequest,
    ) -> Result<(EntityId, EntityId, fillet::ArcSeg, Vec2, Vec2), SessionError> {
        if !self.is_line_id(request.l1) || !self.is_line_id(request.l2) {
            return Err(SessionError::InvalidConstraint(
                "Fillet needs two lines".to_string(),
            ));
        }
        let (l1_id, l2_id) = self.resolve_corner_lines(request.l1, request.l2);
        let radius = self.eval_text(&request.radius_text)?;
        let l1 = self.line_seg(l1_id)?;
        let l2 = self.line_seg(l2_id)?;
        let result = fillet::fillet_lines(&l1, &l2, radius).map_err(|error| match error {
            fillet::FilletError::NotPositive => SessionError::InvalidConstraint(
                "Fillet radius must be greater than zero. Enter a finite positive radius.".into(),
            ),
            error => SessionError::InvalidConstraint(format!("fillet: {error:?}").to_lowercase()),
        })?;
        let v = line_vertex(&l1, &l2).unwrap_or(result.arc.center);
        let _ = v;
        Ok((
            l1_id,
            l2_id,
            result.arc,
            result.tangent_on_l1,
            result.tangent_on_l2,
        ))
    }

    pub fn fillet_lines(&mut self, request: &FilletRequest) -> Result<ToolResult, SessionError> {
        let (l1, l2, arc, t1, t2) = self.compute_fillet(request)?;
        let radius_text = request.radius_text.clone();
        self.mutate_with_undo(move |s| {
            let v = line_vertex(&s.line_seg(l1)?, &s.line_seg(l2)?).unwrap();
            let corner1 = s.vertex_endpoint(l1, v);
            let corner2 = s.vertex_endpoint(l2, v);
            let shared_corner = s.shared_corner_reference(l1, l2, v);
            s.preserve_midpoint_span(l1, corner1);
            s.preserve_midpoint_span(l2, corner2);
            let p1 = s.retarget_line_end(l1, v, t1);
            let p2 = s.retarget_line_end(l2, v, t2);

            let corner = shared_corner.unwrap_or_else(|| s.sketch.add_generated_point(v));
            s.anchor_corner_reference(corner, l1, l2);
            let (a0, a1) = geom_sweep_to_entity(arc.start_angle, arc.end_angle, arc.ccw);
            let arc_id = s.sketch.add_entity(Entity::Arc {
                center: arc.center,
                radius: arc.radius,
                start_angle: a0,
                end_angle: a1,
            });
            s.sketch
                .add_constraint(Constraint::Tangent { a: l1, b: arc_id });
            s.sketch
                .add_constraint(Constraint::Tangent { a: l2, b: arc_id });

            let arc_start = arc.center + Vec2::new(arc.radius * a0.cos(), arc.radius * a0.sin());
            let (end1, end2) = if arc_start.distance(t1) <= arc_start.distance(t2) {
                (
                    crate::constraint::ArcEndpoint::Start,
                    crate::constraint::ArcEndpoint::End,
                )
            } else {
                (
                    crate::constraint::ArcEndpoint::End,
                    crate::constraint::ArcEndpoint::Start,
                )
            };
            s.sketch.add_constraint(Constraint::ArcEndpointCoincident {
                point: p1,
                arc: arc_id,
                end: end1,
            });
            s.sketch.add_constraint(Constraint::ArcEndpointCoincident {
                point: p2,
                arc: arc_id,
                end: end2,
            });

            let param = s.param_from_text_pub(ParamKind::Length, Some(&radius_text), arc.radius)?;
            let pos = arc.center
                + Vec2::new(
                    arc.radius * ((a0 + a1) / 2.0).cos() * 1.2,
                    arc.radius * ((a0 + a1) / 2.0).sin() * 1.2,
                );
            s.add_constraint_bound(
                Constraint::Radius {
                    entity: arc_id,
                    value: arc.radius,
                },
                param,
                pos,
                false,
            )?;
            Ok(())
        })
    }

    pub(super) fn resolve_chamfer(
        &self,
        request: &ChamferRequest,
    ) -> Result<(EntityId, EntityId, f64, Vec2, Vec2), SessionError> {
        if !self.is_line_id(request.l1) || !self.is_line_id(request.l2) {
            return Err(SessionError::InvalidConstraint(
                "Chamfer needs two lines".to_string(),
            ));
        }
        let (l1, l2) = self.resolve_corner_lines(request.l1, request.l2);
        let distance = self
            .positive_input(Some(&request.distance_text), None)?
            .unwrap();
        let l1_geom = self.line_seg(l1)?;
        let l2_geom = self.line_seg(l2)?;
        let result = chamfer::chamfer_lines(
            &chamfer::LineSeg {
                a: l1_geom.a,
                b: l1_geom.b,
            },
            &chamfer::LineSeg {
                a: l2_geom.a,
                b: l2_geom.b,
            },
            distance,
            distance,
        )
        .map_err(|e| SessionError::InvalidConstraint(format!("chamfer: {e:?}").to_lowercase()))?;
        Ok((l1, l2, distance, result.point_on_l1, result.point_on_l2))
    }

    pub fn chamfer_preview(&self, request: &ChamferRequest) -> Result<PreviewCurve, SessionError> {
        let (_, _, _, a, b) = self.resolve_chamfer(request)?;
        Ok(PreviewCurve::Line { a, b })
    }

    pub fn chamfer_lines(&mut self, request: &ChamferRequest) -> Result<ToolResult, SessionError> {
        let (l1, l2, distance, p1, p2) = self.resolve_chamfer(request)?;
        self.mutate_with_undo(move |s| {
            let v = line_vertex(&s.line_seg(l1)?, &s.line_seg(l2)?).unwrap();
            let corner1 = s.vertex_endpoint(l1, v);
            let corner2 = s.vertex_endpoint(l2, v);
            let shared_corner = s.shared_corner_reference(l1, l2, v);
            s.preserve_midpoint_span(l1, corner1);
            s.preserve_midpoint_span(l2, corner2);
            let t1 = s.retarget_line_end(l1, v, p1);
            let t2 = s.retarget_line_end(l2, v, p2);
            let corner = shared_corner.unwrap_or_else(|| s.sketch.add_generated_point(v));
            s.anchor_corner_reference(corner, l1, l2);
            let line_id = s.sketch.add_entity(Entity::line(t1, t2));

            s.sketch.add_constraint(Constraint::EqualDistance {
                origin: corner,
                a: t1,
                b: t2,
            });
            let param =
                s.param_from_text_pub(ParamKind::Length, Some(&request.distance_text), distance)?;
            let corner_position = s.sketch.point_position(corner).unwrap_or(v);
            let arm = p1 - corner_position;
            let span = arm.x.hypot(arm.y);
            let mut text_pos = (corner_position + p1) * 0.5;
            if span.is_finite() && span > 0.0 {
                let mut normal = Vec2::new(-arm.y / span, arm.x / span);
                if normal.dot(p2 - corner_position) > 0.0 {
                    normal = normal * -1.0;
                }
                // New labels start 6 mm outside the corner, away from relation glyphs.
                // This is not a collision guarantee for all fonts/zoom; saved labels stay put.
                text_pos = text_pos + normal * 6.0;
            }
            s.add_constraint_bound(
                Constraint::Distance {
                    from: corner,
                    to: Some(t1),
                    value: distance,
                },
                param,
                text_pos,
                false,
            )?;
            let _ = line_id;
            Ok(())
        })
    }

    fn offset_side(&self, entity: EntityId, cursor: Vec2) -> Result<f64, SessionError> {
        match self.sketch.entity(entity) {
            Some(Entity::Line { .. }) => {
                let (a, b) = self.sketch.resolved_line(entity).unwrap();
                let d = b - a;
                let cross = d.x * (cursor.y - a.y) - d.y * (cursor.x - a.x);
                Ok(if cross >= 0.0 { 1.0 } else { -1.0 })
            }
            Some(Entity::Circle { center, radius, .. })
            | Some(Entity::Arc { center, radius, .. }) => {
                let dc = center.distance(cursor);
                Ok(if dc >= *radius { 1.0 } else { -1.0 })
            }
            _ => Err(SessionError::InvalidConstraint(
                "Offset needs a line, circle, or arc".to_string(),
            )),
        }
    }

    fn offset_compute(
        &self,
        entity: EntityId,
        distance_text: &str,
        cursor: Vec2,
    ) -> Result<offset::Curve, SessionError> {
        let side = self.offset_side(entity, cursor)?;
        let magnitude = self.eval_text(distance_text)?;
        if !magnitude.is_finite() || magnitude.abs() <= EPS {
            return Err(SessionError::InvalidConstraint(
                "offset distance must be finite and nonzero".to_string(),
            ));
        }
        let distance = magnitude * side;
        let curve = self.to_offset_curve(entity)?;
        offset::offset_curve(&curve, distance)
            .map_err(|error| match error {
                offset::OffsetError::CollapseToPoint => SessionError::InvalidConstraint(
                    "Inward offset distance must be smaller than the circle or arc radius. Reduce the distance or reverse the offset direction.".into(),
                ),
                error => SessionError::InvalidConstraint(format!("offset: {error:?}").to_lowercase()),
            })
    }

    fn to_offset_curve(&self, entity: EntityId) -> Result<offset::Curve, SessionError> {
        match self.sketch.entity(entity) {
            Some(Entity::Line { .. }) => {
                let seg = self.line_seg(entity)?;
                Ok(offset::Curve::Line(offset::LineSeg { a: seg.a, b: seg.b }))
            }
            Some(Entity::Circle { center, radius }) => Ok(offset::Curve::Circle(offset::Circle {
                center: *center,
                radius: *radius,
            })),
            Some(Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) => {
                let (a0, a1, ccw) = entity_sweep_to_geom(*start_angle, *end_angle);
                Ok(offset::Curve::Arc(offset::ArcSeg {
                    circle: offset::Circle {
                        center: *center,
                        radius: *radius,
                    },
                    start_angle: a0,
                    end_angle: a1,
                    ccw,
                }))
            }
            _ => Err(SessionError::InvalidConstraint(
                "Offset needs a line, circle, or arc".to_string(),
            )),
        }
    }

    pub fn offset_preview(
        &self,
        request: &OffsetRequest,
    ) -> Result<OffsetPreviewDto, SessionError> {
        let curve = self.offset_compute(request.entity, &request.distance_text, request.cursor)?;
        Ok(OffsetPreviewDto {
            curve: preview_of_offset(&curve),
        })
    }

    pub fn offset_curve_op(&mut self, request: &OffsetRequest) -> Result<ToolResult, SessionError> {
        let curve = self.offset_compute(request.entity, &request.distance_text, request.cursor)?;
        let source = request.entity;
        let distance = self.eval_text(&request.distance_text)?;
        let side = self.offset_side(request.entity, request.cursor)?;
        let distance_text = request.distance_text.clone();
        self.mutate_with_undo(move |s| {
            match curve {
                offset::Curve::Line(seg) => {
                    let a = s.sketch.add_generated_point(seg.a);
                    let b = s.sketch.add_generated_point(seg.b);
                    let line_id = s.sketch.add_entity(Entity::line(a, b));

                    if s.is_line_id(source) {
                        s.sketch.add_constraint(Constraint::Parallel {
                            a: source,
                            b: line_id,
                        });

                        s.sketch.add_constraint(Constraint::Equal {
                            a: source,
                            b: line_id,
                        });
                        let param = s.param_from_text_pub(
                            ParamKind::Length,
                            Some(&distance_text),
                            distance,
                        )?;
                        let mid = (seg.a + seg.b) * 0.5;
                        s.add_constraint_bound_offset(
                            Constraint::Distance {
                                from: source,
                                to: Some(line_id),
                                value: distance,
                            },
                            param,
                            mid,
                            false,
                            Some(side),
                        )?;
                    }
                }
                offset::Curve::Circle(c) => {
                    let id = s.sketch.add_entity(Entity::Circle {
                        center: c.center,
                        radius: c.radius,
                    });

                    s.attach_owned_center(id, c.center);
                    s.constrain_radial_offset(
                        source,
                        id,
                        distance,
                        &distance_text,
                        c.center,
                        c.radius,
                    )?;
                }
                offset::Curve::Arc(a) => {
                    let (a0, a1) = geom_sweep_to_entity(a.start_angle, a.end_angle, a.ccw);
                    let id = s.sketch.add_entity(Entity::Arc {
                        center: a.circle.center,
                        radius: a.circle.radius,
                        start_angle: a0,
                        end_angle: a1,
                    });
                    s.constrain_radial_offset(
                        source,
                        id,
                        distance,
                        &distance_text,
                        a.circle.center,
                        a.circle.radius,
                    )?;
                }
            }
            Ok(())
        })
    }

    fn constrain_radial_offset(
        &mut self,
        source: EntityId,
        target: EntityId,
        distance: f64,
        distance_text: &str,
        center: Vec2,
        target_radius: f64,
    ) -> Result<(), SessionError> {
        let source_radius = self
            .circle_of(source)
            .map(|(_, radius)| radius)
            .ok_or_else(|| {
                SessionError::InvalidConstraint(
                    "radial offset source must be a circle or arc".to_string(),
                )
            })?;
        self.sketch.add_constraint(Constraint::Concentric {
            a: source,
            b: target,
        });

        let (from, to) = if (target_radius - source_radius) * distance >= 0.0 {
            (source, target)
        } else {
            (target, source)
        };
        let magnitude = distance;
        let param = self.param_from_text_pub(ParamKind::Length, Some(distance_text), magnitude)?;
        let text_pos =
            self.radial_offset_text_position(center, (source_radius + target_radius) * 0.5);
        self.add_constraint_bound(
            Constraint::Distance {
                from,
                to: Some(to),
                value: magnitude,
            },
            param,
            text_pos,
            false,
        )?;
        Ok(())
    }

    fn radial_offset_text_position(&self, center: Vec2, radius: f64) -> Vec2 {
        let candidates = [
            center + Vec2::new(radius, 0.0),
            center + Vec2::new(0.0, radius),
            center + Vec2::new(-radius, 0.0),
            center + Vec2::new(0.0, -radius),
        ];
        let anchors: Vec<_> = self
            .sketch
            .constraints()
            .filter_map(|(id, _)| self.sketch.dim_placement(&id))
            .filter(|point| point.x.is_finite() && point.y.is_finite())
            .collect();
        let clearance = |candidate: Vec2| {
            anchors
                .iter()
                .map(|anchor| candidate.distance(*anchor))
                .fold(f64::INFINITY, f64::min)
        };
        let mut best = candidates[0];
        let mut best_clearance = clearance(best);
        // This 4 mm creation-only heuristic spaces saved anchors, not text bounds.
        // It cannot guarantee separation at every zoom/font; existing placements stay put.
        if best_clearance >= 4.0 {
            return best;
        }
        for candidate in candidates.into_iter().skip(1) {
            let candidate_clearance = clearance(candidate);
            if candidate_clearance > best_clearance {
                best = candidate;
                best_clearance = candidate_clearance;
            }
        }
        best
    }

    /// Intersections of `entity` with every other entity (angle-filtered
    /// for arcs).
    fn cuts_for(&self, entity: EntityId) -> Vec<Vec2> {
        let mut cuts = Vec::new();
        for (other_id, other) in self.sketch.entities() {
            if other_id == entity {
                continue;
            }
            match (self.sketch.entity(entity), other) {
                (Some(Entity::Line { .. }), Entity::Line { .. }) => {
                    if let (Ok(l1), Ok(l2)) = (self.line_seg(entity), self.line_seg(other_id)) {
                        let a = trimext::LineSeg { a: l1.a, b: l1.b };
                        let b = trimext::LineSeg { a: l2.a, b: l2.b };
                        if let Some((pt, _, u)) = trimext::line_line(&a, &b) {
                            if (-EPS..=1.0 + EPS).contains(&u) {
                                cuts.push(pt);
                            }
                        }
                    }
                }
                (Some(Entity::Line { .. }), Entity::Circle { .. } | Entity::Arc { .. }) => {
                    if let Ok(l) = self.line_seg(entity) {
                        let (center, radius) = match other {
                            Entity::Circle { center, radius }
                            | Entity::Arc { center, radius, .. } => (*center, *radius),
                            _ => unreachable!(),
                        };
                        let seg = trimext::LineSeg { a: l.a, b: l.b };
                        let circle = trimext::Circle { center, radius };
                        for (pt, _) in trimext::line_circle(&seg, &circle) {
                            if self.point_on_sweep(other_id, center, pt) {
                                cuts.push(pt);
                            }
                        }
                    }
                }
                (Some(Entity::Circle { .. } | Entity::Arc { .. }), Entity::Line { .. }) => {
                    if let Ok(l) = self.line_seg(other_id) {
                        let (center, radius) = self.circle_of(entity).unwrap();
                        let seg = trimext::LineSeg { a: l.a, b: l.b };
                        let circle = trimext::Circle { center, radius };
                        for (pt, t) in trimext::line_circle(&seg, &circle) {
                            if (-EPS..=1.0 + EPS).contains(&t)
                                && self.point_on_sweep(entity, center, pt)
                            {
                                cuts.push(pt);
                            }
                        }
                    }
                }
                (
                    Some(Entity::Circle { .. } | Entity::Arc { .. }),
                    Entity::Circle { .. } | Entity::Arc { .. },
                ) => {
                    let (c1, r1) = self.circle_of(entity).unwrap();
                    let (c2, r2) = match other {
                        Entity::Circle { center, radius } | Entity::Arc { center, radius, .. } => {
                            (*center, *radius)
                        }
                        _ => unreachable!(),
                    };
                    for pt in trimext::circle_circle(
                        &trimext::Circle {
                            center: c1,
                            radius: r1,
                        },
                        &trimext::Circle {
                            center: c2,
                            radius: r2,
                        },
                    ) {
                        if self.point_on_sweep(entity, c1, pt)
                            && self.point_on_sweep(other_id, c2, pt)
                        {
                            cuts.push(pt);
                        }
                    }
                }
                _ => {}
            }
        }
        cuts
    }

    fn point_on_sweep(&self, id: EntityId, center: Vec2, p: Vec2) -> bool {
        match self.arc_sweep(id) {
            Some((a0, a1)) => sweep_contains(a0, a1, angle_of(center, p)),
            None => true,
        }
    }

    fn is_line_id(&self, id: EntityId) -> bool {
        matches!(self.sketch.entity(id), Some(Entity::Line { .. }))
    }

    /// Split a sweep by the interval containing `click_angle`. Open arcs can
    /// retain two disconnected pieces; a closed circle retains one wrapped
    /// complement arc.
    fn trim_sweep_parts(
        &self,
        a0: f64,
        a1: f64,
        cuts: &[f64],
        click: f64,
        closed: bool,
    ) -> Option<SweepTrim> {
        let span = if closed {
            TAU
        } else {
            (a1 - a0).rem_euclid(TAU)
        };
        if span <= EPS {
            return None;
        }
        let off = |a: f64| (a - a0).rem_euclid(TAU);
        let click_off = off(click);
        let mut sorted: Vec<f64> = cuts
            .iter()
            .map(|a| off(*a))
            .filter(|o| *o > 1e-9 && *o < span - 1e-9)
            .collect();
        if sorted.is_empty() {
            return None;
        }
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted.dedup_by(|a, b| (*a - *b).abs() <= EPS);
        let lower = sorted
            .iter()
            .rev()
            .find(|o| **o <= click_off)
            .cloned()
            .unwrap_or(0.0);
        let upper = sorted
            .iter()
            .find(|o| **o >= click_off)
            .cloned()
            .unwrap_or(span);
        if upper - lower <= EPS {
            return None;
        }
        let removed = (a0 + lower, a0 + upper);
        let kept = if closed {
            vec![(a0 + upper, a0 + span + lower)]
        } else {
            let mut pieces = Vec::with_capacity(2);
            if lower > EPS {
                pieces.push((a0, a0 + lower));
            }
            if upper < span - EPS {
                pieces.push((a0 + upper, a0 + span));
            }
            pieces
        };
        Some(SweepTrim { kept, removed })
    }

    pub fn trim_preview(&self, request: &TrimRequest) -> Result<TrimPreviewDto, SessionError> {
        let (kept, removed) = self.trim_compute(request.entity, request.click)?;
        Ok(TrimPreviewDto { kept, removed })
    }

    fn trim_compute(
        &self,
        entity: EntityId,
        click: Vec2,
    ) -> Result<(Vec<PreviewCurve>, PreviewCurve), SessionError> {
        let cuts = self.cuts_for(entity);
        match self.sketch.entity(entity) {
            Some(Entity::Line { .. }) => {
                let l = self.line_seg(entity)?;
                let seg = trimext::LineSeg { a: l.a, b: l.b };
                let trim = trimext::trim_line_parts(&seg, click, &cuts).ok_or(
                    SessionError::InvalidConstraint("nothing to trim here".to_string()),
                )?;
                Ok((
                    trim.kept
                        .into_iter()
                        .map(|piece| PreviewCurve::Line {
                            a: piece.a,
                            b: piece.b,
                        })
                        .collect(),
                    PreviewCurve::Line {
                        a: trim.removed.a,
                        b: trim.removed.b,
                    },
                ))
            }
            Some(Entity::Circle { center, radius }) => {
                let (a0, a1) = self.arc_sweep(entity).unwrap();
                let cut_angles: Vec<f64> = cuts.iter().map(|c| angle_of(*center, *c)).collect();
                let trim = self
                    .trim_sweep_parts(a0, a1, &cut_angles, angle_of(*center, click), true)
                    .ok_or(SessionError::InvalidConstraint(
                        "nothing to trim here".to_string(),
                    ))?;
                Ok((
                    trim.kept
                        .into_iter()
                        .map(|(start_angle, end_angle)| PreviewCurve::Arc {
                            center: *center,
                            radius: *radius,
                            start_angle,
                            end_angle,
                        })
                        .collect(),
                    PreviewCurve::Arc {
                        center: *center,
                        radius: *radius,
                        start_angle: trim.removed.0,
                        end_angle: trim.removed.1,
                    },
                ))
            }
            Some(Entity::Arc { center, radius, .. }) => {
                let (a0, a1) = self.arc_sweep(entity).unwrap();
                let cut_angles: Vec<f64> = cuts.iter().map(|c| angle_of(*center, *c)).collect();
                let trim = self
                    .trim_sweep_parts(a0, a1, &cut_angles, angle_of(*center, click), false)
                    .ok_or(SessionError::InvalidConstraint(
                        "nothing to trim here".to_string(),
                    ))?;
                Ok((
                    trim.kept
                        .into_iter()
                        .map(|(start_angle, end_angle)| PreviewCurve::Arc {
                            center: *center,
                            radius: *radius,
                            start_angle,
                            end_angle,
                        })
                        .collect(),
                    PreviewCurve::Arc {
                        center: *center,
                        radius: *radius,
                        start_angle: trim.removed.0,
                        end_angle: trim.removed.1,
                    },
                ))
            }
            _ => Err(SessionError::InvalidConstraint(
                "Trim needs a line, circle, or arc".to_string(),
            )),
        }
    }

    pub fn trim_entity(&mut self, request: &TrimRequest) -> Result<ToolResult, SessionError> {
        let entity = request.entity;
        let click = request.click;
        let cuts = self.cuts_for(entity);
        self.mutate_with_undo(move |s| match s.sketch.entity(entity).cloned() {
            Some(Entity::Line { .. }) => {
                let l = s.line_seg(entity)?;
                let seg = trimext::LineSeg { a: l.a, b: l.b };
                let trim = trimext::trim_line_parts(&seg, click, &cuts).ok_or(
                    SessionError::InvalidConstraint("nothing to trim here".to_string()),
                )?;
                let (original_start, original_end) = s
                    .sketch
                    .line_endpoint_ids(entity)
                    .ok_or(SessionError::EntityNotFound(entity))?;
                let horizontal = s.sketch.has_constraint_on(entity, |constraint| {
                    matches!(constraint, Constraint::Horizontal { .. })
                });
                let vertical = s.sketch.has_constraint_on(entity, |constraint| {
                    matches!(constraint, Constraint::Vertical { .. })
                });
                match trim.kept.as_slice() {
                    [piece] if piece.a.distance(l.a) <= EPS => {
                        let cut = s.sketch.add_generated_point(piece.b);
                        if let Some(Entity::Line { end, .. }) = s.sketch.entity_mut(entity) {
                            *end = cut;
                        }
                        s.sketch.remove_unused_generated_points([original_end]);
                    }
                    [piece] => {
                        let cut = s.sketch.add_generated_point(piece.a);
                        if let Some(Entity::Line { start, .. }) = s.sketch.entity_mut(entity) {
                            *start = cut;
                        }
                        s.sketch.remove_unused_generated_points([original_start]);
                    }
                    [first, second] => {
                        let first_cut = s.sketch.add_generated_point(first.b);
                        let second_cut = s.sketch.add_generated_point(second.a);
                        if let Some(Entity::Line { end, .. }) = s.sketch.entity_mut(entity) {
                            *end = first_cut;
                        }
                        let second_line =
                            s.sketch.add_entity(Entity::line(second_cut, original_end));
                        if horizontal {
                            s.sketch.add_constraint(Constraint::Horizontal {
                                entity: second_line,
                            });
                        }
                        if vertical {
                            s.sketch.add_constraint(Constraint::Vertical {
                                entity: second_line,
                            });
                        }
                    }
                    _ => {
                        return Err(SessionError::InvalidConstraint(
                            "trim would remove the entire line".to_string(),
                        ))
                    }
                }
                Ok(())
            }
            Some(Entity::Circle { center, radius }) => {
                let (a0, a1) = (0.0, TAU);
                let cut_angles: Vec<f64> = cuts.iter().map(|c| angle_of(center, *c)).collect();
                let trim = s
                    .trim_sweep_parts(a0, a1, &cut_angles, angle_of(center, click), true)
                    .ok_or(SessionError::InvalidConstraint(
                        "nothing to trim here".to_string(),
                    ))?;
                let (k0, k1) = trim.kept[0];

                if let Some(slot) = s.sketch.entity_mut(entity) {
                    *slot = Entity::Arc {
                        center,
                        radius,
                        start_angle: k0,
                        end_angle: k1,
                    };
                }
                s.rebind_arc_endpoints(entity, &[entity]);
                Ok(())
            }
            Some(Entity::Arc { .. }) => {
                let (center, radius) = s.circle_of(entity).unwrap();
                let (a0, a1) = s.arc_sweep(entity).unwrap();
                let cut_angles: Vec<f64> = cuts.iter().map(|c| angle_of(center, *c)).collect();
                let trim = s
                    .trim_sweep_parts(a0, a1, &cut_angles, angle_of(center, click), false)
                    .ok_or(SessionError::InvalidConstraint(
                        "nothing to trim here".to_string(),
                    ))?;
                let Some(&(k0, k1)) = trim.kept.first() else {
                    return Err(SessionError::InvalidConstraint(
                        "trim would remove the entire arc".to_string(),
                    ));
                };
                if let Some(Entity::Arc {
                    start_angle,
                    end_angle,
                    ..
                }) = s.sketch.entity_mut(entity)
                {
                    *start_angle = k0;
                    *end_angle = k1;
                }
                let mut parts = vec![entity];
                if let Some(&(start_angle, end_angle)) = trim.kept.get(1) {
                    parts.push(s.sketch.add_entity(Entity::Arc {
                        center,
                        radius,
                        start_angle,
                        end_angle,
                    }));
                }
                s.rebind_arc_endpoints(entity, &parts);
                Ok(())
            }
            _ => Err(SessionError::InvalidConstraint(
                "Trim needs a line, circle, or arc".to_string(),
            )),
        })
    }

    pub fn extend_entity(&mut self, request: &ExtendRequest) -> Result<ToolResult, SessionError> {
        if !self.is_line_id(request.entity) {
            return Err(SessionError::InvalidConstraint(
                "Extend needs a line (arcs later)".to_string(),
            ));
        }
        let entity = request.entity;
        let click = request.click;
        self.mutate_with_undo(move |s| {
            let l = s.line_seg(entity)?;
            let source = trimext::LineSeg { a: l.a, b: l.b };
            let extend_start = click.distance(l.a) <= click.distance(l.b);
            let length = l.a.distance(l.b);
            let mut best: Option<(f64, Vec2, EntityId)> = None;
            let mut consider = |point: Vec2, t: f64, boundary: EntityId| {
                let extension = if extend_start {
                    if t >= -EPS {
                        return;
                    }
                    -t * length
                } else {
                    if t <= 1.0 + EPS {
                        return;
                    }
                    (t - 1.0) * length
                };
                if extension > length * 100.0 {
                    return;
                }
                if best
                    .as_ref()
                    .is_none_or(|(current, _, _)| extension < *current - EPS)
                {
                    best = Some((extension, point, boundary));
                }
            };

            for (other_id, other) in s.sketch.entities() {
                if other_id == entity {
                    continue;
                }
                match other {
                    Entity::Line { .. } => {
                        let seg = s.line_seg(other_id)?;
                        if let Some((point, t, u)) =
                            trimext::line_line(&source, &trimext::LineSeg { a: seg.a, b: seg.b })
                        {
                            if (-EPS..=1.0 + EPS).contains(&u) {
                                consider(point, t, other_id);
                            }
                        }
                    }
                    Entity::Circle { center, radius } => {
                        for (point, t) in trimext::line_circle(
                            &source,
                            &trimext::Circle {
                                center: *center,
                                radius: *radius,
                            },
                        ) {
                            consider(point, t, other_id);
                        }
                    }
                    Entity::Arc { center, radius, .. } => {
                        for (point, t) in trimext::line_circle(
                            &source,
                            &trimext::Circle {
                                center: *center,
                                radius: *radius,
                            },
                        ) {
                            if s.point_on_sweep(other_id, *center, point) {
                                consider(point, t, other_id);
                            }
                        }
                    }
                    _ => {}
                }
            }

            let (_, target, boundary) = best.ok_or_else(|| {
                SessionError::InvalidConstraint(
                    "no boundary intersects the selected end".to_string(),
                )
            })?;
            let (start, end) = s
                .sketch
                .line_endpoint_ids(entity)
                .ok_or(SessionError::EntityNotFound(entity))?;
            let point_id = if extend_start { start } else { end };
            let extended_point = if s.sketch.lines_connected_to(point_id).len() > 1 {
                let new_point = s.sketch.add_generated_point(target);
                if let Some(Entity::Line { start, end }) = s.sketch.entity_mut(entity) {
                    if extend_start {
                        *start = new_point;
                    } else {
                        *end = new_point;
                    }
                }
                new_point
            } else if let Some(Entity::Point { position }) = s.sketch.entity_mut(point_id) {
                *position = target;
                point_id
            } else {
                return Err(SessionError::EntityNotFound(point_id));
            };

            s.sketch.add_constraint(Constraint::Coincident {
                a: extended_point,
                b: boundary,
            });
            Ok(())
        })
    }

    pub fn break_curve(&mut self, request: &BreakRequest) -> Result<ToolResult, SessionError> {
        let entity = request.entity;
        let at = request.at;
        self.mutate_with_undo(move |s| match s.sketch.entity(entity).cloned() {
            Some(Entity::Line { start, end }) => {
                let l = s.line_seg(entity)?;
                let t = nearest_param(&l, at).clamp(0.02, 0.98);
                let p = Vec2::new(l.a.x + t * (l.b.x - l.a.x), l.a.y + t * (l.b.y - l.a.y));
                let (sa, sb) = (
                    s.sketch.point_position(start).unwrap(),
                    s.sketch.point_position(end).unwrap(),
                );

                let mid = s.sketch.add_generated_point(p);

                let line2 = s.sketch.add_entity(Entity::line(mid, end));

                if let Some(Entity::Line { end: e, .. }) = s.sketch.entity_mut(entity) {
                    *e = mid;
                }

                let hv: Vec<Constraint> = s
                    .sketch
                    .constraints()
                    .filter(|(_, c)| {
                        matches!(c, Constraint::Horizontal { entity: e } if *e == entity)
                            || matches!(c, Constraint::Vertical { entity: e } if *e == entity)
                    })
                    .map(|(_, c)| *c)
                    .collect();
                for c in hv {
                    let nc = match c {
                        Constraint::Horizontal { .. } => Constraint::Horizontal { entity: line2 },
                        Constraint::Vertical { .. } => Constraint::Vertical { entity: line2 },
                        _ => unreachable!(),
                    };
                    s.sketch.add_constraint(nc);
                }
                let _ = (sa, sb);
                Ok(())
            }
            Some(Entity::Circle { center, radius }) => {
                let a = angle_of(center, at);
                if let Some(slot) = s.sketch.entity_mut(entity) {
                    *slot = Entity::Arc {
                        center,
                        radius,
                        start_angle: a,

                        end_angle: a + TAU,
                    };
                }
                s.rebind_arc_endpoints(entity, &[entity]);
                Ok(())
            }
            Some(Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) => {
                let a = angle_of(center, at);
                if !sweep_contains(start_angle, end_angle, a) {
                    return Err(SessionError::InvalidConstraint(
                        "break point is not on the arc".to_string(),
                    ));
                }
                let span = (end_angle - start_angle).rem_euclid(TAU);
                let offset = (a - start_angle).rem_euclid(TAU);
                if offset <= EPS || span - offset <= EPS {
                    return Err(SessionError::InvalidConstraint(
                        "break point must be inside the arc".to_string(),
                    ));
                }
                if let Some(Entity::Arc { end_angle: e, .. }) = s.sketch.entity_mut(entity) {
                    *e = a;
                }
                let second = s.sketch.add_entity(Entity::Arc {
                    center,
                    radius,
                    start_angle: a,
                    end_angle,
                });
                s.rebind_arc_endpoints(entity, &[entity, second]);
                Ok(())
            }
            _ => Err(SessionError::InvalidConstraint(
                "Break needs a line, circle, or arc".to_string(),
            )),
        })
    }

    pub fn mirror_entities(&mut self, request: &MirrorRequest) -> Result<ToolResult, SessionError> {
        if !self.is_line_id(request.axis_line) {
            return Err(SessionError::InvalidConstraint(
                "Mirror needs a sketch line as the axis".to_string(),
            ));
        }
        let axis = {
            let l = self.line_seg(request.axis_line)?;
            xform::LineSeg { a: l.a, b: l.b }
        };
        let ids: BTreeSet<EntityId> = request.entity_ids.iter().copied().collect();
        self.mutate_with_undo(move |s| {
            s.copy_entities_transformed(&ids, |point| xform::mirror_point(point, &axis), true)
        })
    }

    /// All copy/mirror/pattern tools share one topology-preserving constructor.
    /// An occurrence owns its handles, but lines and arcs within it reuse the
    /// same mapped points. External references and driving dimensions are not
    /// copied: an occurrence is independent of the source sketch's anchors.
    fn copy_entities_transformed(
        &mut self,
        ids: &BTreeSet<EntityId>,
        transform: impl Fn(Vec2) -> Vec2,
        reflected: bool,
    ) -> Result<(), SessionError> {
        for id in ids {
            if self.sketch.entity(*id).is_none() {
                return Err(SessionError::EntityNotFound(*id));
            }
        }
        let mut source_points = self.owned_points(ids);

        self.include_selection_centers(ids, &mut source_points);

        let mut point_map = BTreeMap::new();
        for source in source_points {
            let position = self
                .sketch
                .point_position(source)
                .ok_or(SessionError::EntityNotFound(source))?;
            let copied = self.sketch.add_entity(Entity::Point {
                position: transform(position),
            });

            if !ids.contains(&source) || self.sketch.is_generated_point(source) {
                self.sketch.mark_generated_point(copied);
            }
            point_map.insert(source, copied);
        }

        let mut entity_map = point_map.clone();
        for id in ids {
            match self.sketch.entity(*id).cloned() {
                Some(Entity::Point { .. }) => {}
                Some(Entity::Line { start, end }) => {
                    let copied = self.sketch.add_entity(Entity::line(
                        *point_map.get(&start).expect("mapped line start"),
                        *point_map.get(&end).expect("mapped line end"),
                    ));
                    entity_map.insert(*id, copied);
                }
                Some(Entity::Circle { center, radius }) => {
                    let copied = self.sketch.add_entity(Entity::Circle {
                        center: transform(center),
                        radius,
                    });
                    entity_map.insert(*id, copied);
                }
                Some(Entity::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                }) => {
                    let copied_center = transform(center);
                    let original_angle = if reflected { end_angle } else { start_angle };
                    let ray =
                        transform(center + Vec2::new(original_angle.cos(), original_angle.sin()))
                            - copied_center;
                    let copied_start = ray.y.atan2(ray.x);
                    let copied = self.sketch.add_entity(Entity::Arc {
                        center: copied_center,
                        radius,
                        start_angle: copied_start,
                        end_angle: copied_start + crate::geometry::arc_span(start_angle, end_angle),
                    });
                    entity_map.insert(*id, copied);
                    let bindings: Vec<_> = self
                        .sketch
                        .constraints()
                        .filter_map(|(_, relation)| match *relation {
                            Constraint::ArcEndpointCoincident { point, arc, end } if arc == *id => {
                                Some((point, end))
                            }
                            _ => None,
                        })
                        .collect();
                    for (point, end) in bindings {
                        let end = if reflected {
                            match end {
                                ArcEndpoint::Start => ArcEndpoint::End,
                                ArcEndpoint::End => ArcEndpoint::Start,
                            }
                        } else {
                            end
                        };
                        self.sketch
                            .add_constraint(Constraint::ArcEndpointCoincident {
                                point: point_map[&point],
                                arc: copied,
                                end,
                            });
                    }

                    for (end, angle) in [
                        (ArcEndpoint::Start, copied_start),
                        (
                            ArcEndpoint::End,
                            copied_start + crate::geometry::arc_span(start_angle, end_angle),
                        ),
                    ] {
                        if !self.sketch.constraints().any(|(_, relation)| matches!(relation,
                            Constraint::ArcEndpointCoincident { arc, end: bound, .. } if *arc == copied && *bound == end)) {
                            let point = self.sketch.add_generated_point(copied_center + Vec2::new(radius * angle.cos(), radius * angle.sin()));
                            self.sketch.add_constraint(Constraint::ArcEndpointCoincident { point, arc: copied, end });
                        }
                    }
                }
                Some(Entity::Spline { points }) => {
                    let copied = self.sketch.add_entity(Entity::Spline {
                        points: points.into_iter().map(&transform).collect(),
                    });
                    entity_map.insert(*id, copied);
                }
                None => return Err(SessionError::EntityNotFound(*id)),
            }
        }

        let relations: Vec<_> = self
            .sketch
            .constraints()
            .filter_map(|(_, relation)| match *relation {
                Constraint::Coincident { a, b } => Some(Constraint::Coincident {
                    a: *entity_map.get(&a)?,
                    b: *entity_map.get(&b)?,
                }),
                Constraint::CenterCoincident { point, curve } => {
                    Some(Constraint::CenterCoincident {
                        point: *entity_map.get(&point)?,
                        curve: *entity_map.get(&curve)?,
                    })
                }

                Constraint::SpanMidpoint { point, start, end } => Some(Constraint::SpanMidpoint {
                    point: *entity_map.get(&point)?,
                    start: *entity_map.get(&start)?,
                    end: *entity_map.get(&end)?,
                }),
                Constraint::Tangent { a, b } => Some(Constraint::Tangent {
                    a: *entity_map.get(&a)?,
                    b: *entity_map.get(&b)?,
                }),
                _ => None,
            })
            .collect();
        for relation in relations {
            self.sketch.add_constraint(relation);
        }

        let copied_circles: Vec<(EntityId, Vec2)> = ids
            .iter()
            .filter_map(|id| match self.sketch.entity(*id) {
                Some(Entity::Circle { center, .. }) => Some((*id, transform(*center))),
                _ => None,
            })
            .collect();
        for (source, center) in copied_circles {
            let copied = entity_map
                .get(&source)
                .copied()
                .expect("every selected entity was copied");
            let bound = self.sketch.constraints().any(|(_, relation)| {
                matches!(relation, Constraint::CenterCoincident { curve, .. } if *curve == copied)
            });
            if bound {
                continue;
            }
            let point = self.sketch.add_generated_point(center);
            self.sketch.add_constraint(Constraint::CenterCoincident {
                point,
                curve: copied,
            });
        }
        Ok(())
    }

    /// Create a one- or two-direction rectangular pattern as one undoable
    /// sketch command. Both counts include the selected source occurrence.
    pub fn rectangular_pattern(
        &mut self,
        request: &RectangularPatternRequest,
    ) -> Result<ToolResult, SessionError> {
        if request.entity_ids.is_empty() {
            return Err(SessionError::InvalidConstraint(
                "Rectangular pattern needs at least one selected entity".to_string(),
            ));
        }
        if !(2..=1000).contains(&request.count) || !(1..=1000).contains(&request.second_count) {
            return Err(SessionError::InvalidConstraint(
                "Pattern counts must be between 1 and 1000; the first count must be at least 2"
                    .to_string(),
            ));
        }
        if !request.spacing.is_finite() || request.spacing.abs() <= EPS {
            return Err(SessionError::InvalidConstraint(
                "Pattern spacing must be a finite non-zero distance".to_string(),
            ));
        }
        if request.second_count > 1
            && (!request.second_spacing.is_finite() || request.second_spacing.abs() <= EPS)
        {
            return Err(SessionError::InvalidConstraint(
                "Second-direction spacing must be a finite non-zero distance".to_string(),
            ));
        }
        let direction_length = request.direction.length();
        if !direction_length.is_finite() || direction_length <= EPS {
            return Err(SessionError::InvalidConstraint(
                "Pattern direction must be non-zero".to_string(),
            ));
        }
        let direction = request.direction * (1.0 / direction_length);
        let second_direction = match request.second_direction {
            Some(candidate) => {
                let length = candidate.length();
                if !length.is_finite() || length <= EPS {
                    return Err(SessionError::InvalidConstraint(
                        "Second pattern direction must be non-zero".to_string(),
                    ));
                }
                candidate * (1.0 / length)
            }
            None => Vec2::new(-direction.y, direction.x),
        };
        let spacing = request.spacing;
        let second_spacing = request.second_spacing;
        let first_count = request.count;
        let second_count = request.second_count;
        let ids: BTreeSet<EntityId> = request.entity_ids.iter().copied().collect();
        self.mutate_with_undo(move |session| {
            for second_index in 0..second_count {
                for first_index in 0..first_count {
                    if first_index == 0 && second_index == 0 {
                        continue;
                    }
                    let delta = direction * (spacing * f64::from(first_index))
                        + second_direction * (second_spacing * f64::from(second_index));
                    session.copy_entities_transformed(&ids, |point| point + delta, false)?;
                }
            }
            Ok(())
        })
    }

    /// Create a circular pattern as one undoable sketch command.
    pub fn circular_pattern(
        &mut self,
        request: &CircularPatternRequest,
    ) -> Result<ToolResult, SessionError> {
        if request.entity_ids.is_empty() {
            return Err(SessionError::InvalidConstraint(
                "Circular pattern needs at least one selected entity".to_string(),
            ));
        }
        if !(2..=1000).contains(&request.count) {
            return Err(SessionError::InvalidConstraint(
                "Circular pattern count must be between 2 and 1000".to_string(),
            ));
        }
        if !request.center.x.is_finite()
            || !request.center.y.is_finite()
            || !request.total_angle_deg.is_finite()
            || request.total_angle_deg.abs() <= EPS
        {
            return Err(SessionError::InvalidConstraint(
                "Circular pattern center and angle must be finite; angle must be non-zero"
                    .to_string(),
            ));
        }
        let center = request.center;
        let total_angle = request.total_angle_deg.to_radians();
        let full_circle = (request.total_angle_deg.abs() - 360.0).abs() <= 1e-9;
        let divisor = if full_circle {
            request.count
        } else {
            request.count - 1
        };
        let step = total_angle / f64::from(divisor);
        let count = request.count;
        let ids: BTreeSet<EntityId> = request.entity_ids.iter().copied().collect();
        self.mutate_with_undo(move |session| {
            for index in 1..count {
                let angle = step * f64::from(index);
                let (sin, cos) = angle.sin_cos();
                session.copy_entities_transformed(
                    &ids,
                    |point| {
                        let relative = point - center;
                        center
                            + Vec2::new(
                                relative.x * cos - relative.y * sin,
                                relative.x * sin + relative.y * cos,
                            )
                    },
                    false,
                )?;
            }
            Ok(())
        })
    }

    pub fn move_copy_entities(
        &mut self,
        request: &MoveCopyRequest,
    ) -> Result<ToolResult, SessionError> {
        let (dx, dy) = (request.dx, request.dy);
        let ids: BTreeSet<EntityId> = request.entity_ids.iter().copied().collect();
        if ids.is_empty() || !dx.is_finite() || !dy.is_finite() {
            return Err(SessionError::InvalidConstraint(
                "Move/Copy requires selected geometry and finite distances".into(),
            ));
        }
        for &id in &ids {
            if self.sketch.entity(id).is_none() {
                return Err(SessionError::EntityNotFound(id));
            }
        }
        let copy = request.copy;
        if !copy && dx == 0. && dy == 0. {
            return Ok(ToolResult {
                entities: Vec::new(),
                sketch: self.dto(),
            });
        }
        self.mutate_with_undo_stays(move |s| {
            if copy {
                let delta = Vec2::new(dx, dy);
                s.copy_entities_transformed(&ids, |point| point + delta, false)?;
                Ok(crate::solver::SolveStays::default())
            } else {
                let point_ids = s.owned_points(&ids);
                let mut direct_ids = Vec::new();
                for id in &ids {
                    match s.sketch.entity(*id) {
                        Some(Entity::Line { .. }) | Some(Entity::Point { .. }) => {}
                        Some(
                            Entity::Circle { .. } | Entity::Arc { .. } | Entity::Spline { .. },
                        ) => {
                            direct_ids.push(*id);
                        }
                        None => return Err(SessionError::EntityNotFound(*id)),
                    }
                }
                for &point_id in &point_ids {
                    s.translate_entity(point_id, dx, dy)?;
                }
                for id in direct_ids {
                    s.translate_entity(id, dx, dy)?;
                }
                s.rigid_transform_stays(&ids, &point_ids)
            }
        })
    }

    fn translate_entity(&mut self, id: EntityId, dx: f64, dy: f64) -> Result<(), SessionError> {
        let entity = self
            .sketch
            .entity_mut(id)
            .ok_or(SessionError::EntityNotFound(id))?;
        match entity {
            Entity::Point { position } => {
                *position = Vec2::new(position.x + dx, position.y + dy);
            }
            Entity::Line { start, end } => {
                let points = [*start, *end];
                for pid in points {
                    if let Some(Entity::Point { position }) = self.sketch.entity_mut(pid) {
                        *position = Vec2::new(position.x + dx, position.y + dy);
                    }
                }
            }
            Entity::Circle { center, .. } | Entity::Arc { center, .. } => {
                *center = Vec2::new(center.x + dx, center.y + dy);
            }
            Entity::Spline { points } => {
                for p in points {
                    *p = Vec2::new(p.x + dx, p.y + dy);
                }
            }
        }
        Ok(())
    }

    fn rigid_transform_stays(
        &self,
        ids: &BTreeSet<EntityId>,
        points: &BTreeSet<EntityId>,
    ) -> Result<crate::solver::SolveStays, SessionError> {
        let mut rigid_entities: Vec<_> = points.iter().copied().collect();
        for &id in ids {
            match self.sketch.entity(id) {
                Some(Entity::Circle { .. } | Entity::Arc { .. } | Entity::Spline { .. }) => {
                    rigid_entities.push(id);
                }
                Some(_) => {}
                None => return Err(SessionError::EntityNotFound(id)),
            }
        }
        rigid_entities.extend(self.sketch.constraints().filter_map(|(_, constraint)| {
            match *constraint {
                Constraint::CenterCoincident { point, curve }
                    if ids.contains(&curve)
                        && !points.contains(&point)
                        && self.center_has_unselected_geometry(point, ids, points) =>
                {
                    Some(point)
                }
                _ => None,
            }
        }));
        rigid_entities.sort_unstable();
        rigid_entities.dedup();
        let finite_point = |p: &Vec2| p.x.is_finite() && p.y.is_finite();
        for &id in &rigid_entities {
            let finite = match self.sketch.entity(id) {
                Some(Entity::Point { position }) => finite_point(position),
                Some(Entity::Circle { center, radius }) => {
                    finite_point(center) && radius.is_finite()
                }
                Some(Entity::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                }) => {
                    finite_point(center)
                        && radius.is_finite()
                        && start_angle.is_finite()
                        && end_angle.is_finite()
                }
                Some(Entity::Spline { points }) => points.iter().all(finite_point),
                Some(Entity::Line { .. }) => true,
                None => return Err(SessionError::EntityNotFound(id)),
            };
            if !finite {
                return Err(SessionError::InvalidConstraint(
                    "Transform would put geometry outside finite coordinates".into(),
                ));
            }
        }
        Ok(crate::solver::SolveStays {
            rigid_entities,
            ..Default::default()
        })
    }

    pub fn scale_entities(&mut self, request: &ScaleRequest) -> Result<ToolResult, SessionError> {
        let factor = self.eval_text(&request.factor_text)?;
        if !factor.is_finite() || factor.abs() < 1e-12 {
            return Err(SessionError::InvalidConstraint(
                "scale factor must be finite and non-zero".to_string(),
            ));
        }
        let origin = request.origin;
        let ids: BTreeSet<EntityId> = request.entity_ids.iter().copied().collect();
        if ids.is_empty() || !origin.x.is_finite() || !origin.y.is_finite() {
            return Err(SessionError::InvalidConstraint(
                "Scale requires selected geometry and a finite origin".into(),
            ));
        }
        for &id in &ids {
            if self.sketch.entity(id).is_none() {
                return Err(SessionError::EntityNotFound(id));
            }
        }
        if factor == 1. {
            return Ok(ToolResult {
                entities: Vec::new(),
                sketch: self.dto(),
            });
        }
        self.mutate_with_undo_stays(move |s| {
            let point_ids = s.owned_points(&ids);
            for &pid in &point_ids {
                if let Some(Entity::Point { position }) = s.sketch.entity_mut(pid) {
                    *position = origin + (*position - origin) * factor;
                }
            }
            for id in &ids {
                match s.sketch.entity_mut(*id) {
                    Some(Entity::Circle { center, radius }) => {
                        *center = origin + (*center - origin) * factor;
                        *radius *= factor.abs();
                    }
                    Some(Entity::Arc {
                        center,
                        radius,
                        start_angle,
                        end_angle,
                    }) => {
                        *center = origin + (*center - origin) * factor;
                        *radius *= factor.abs();
                        if factor < 0.0 {
                            *start_angle += std::f64::consts::PI;
                            *end_angle += std::f64::consts::PI;
                        }
                    }
                    Some(Entity::Spline { points }) => {
                        for p in points.iter_mut() {
                            *p = origin + (*p - origin) * factor;
                        }
                    }
                    _ => {}
                }
            }
            s.rigid_transform_stays(&ids, &point_ids)
        })
    }

    pub fn polygon_create(&mut self, request: &PolygonRequest) -> Result<ToolResult, SessionError> {
        let radius = self.eval_text(&request.radius_text)?;
        let mode = match request.mode.as_str() {
            "circumscribed" => polygon::PolyMode::Circumscribed,
            _ => polygon::PolyMode::Inscribed,
        };
        let vertices = polygon::regular_polygon(
            request.center,
            request.edge_count as usize,
            radius,
            request.rotation_deg.to_radians(),
            mode,
        )
        .map_err(|error| match error {
            polygon::PolyError::NotPositive => SessionError::InvalidConstraint(
                "Polygon radius must be greater than zero. Enter a positive radius.".into(),
            ),
            error => SessionError::InvalidConstraint(format!("polygon: {error:?}").to_lowercase()),
        })?;
        self.mutate_with_undo(move |s| {
            let mut point_ids = Vec::with_capacity(vertices.len());
            for v in &vertices {
                point_ids.push(s.sketch.add_generated_point(*v));
            }
            for i in 0..point_ids.len() {
                s.sketch.add_entity(Entity::line(
                    point_ids[i],
                    point_ids[(i + 1) % point_ids.len()],
                ));
            }
            Ok(())
        })
    }
}

fn line_vertex(l1: &fillet::LineSeg, l2: &fillet::LineSeg) -> Option<Vec2> {
    trimext::line_line(
        &trimext::LineSeg { a: l1.a, b: l1.b },
        &trimext::LineSeg { a: l2.a, b: l2.b },
    )
    .map(|(pt, _, _)| pt)
}

fn nearest_param(l: &fillet::LineSeg, p: Vec2) -> f64 {
    trimext::nearest_param_on_line(&trimext::LineSeg { a: l.a, b: l.b }, p)
}

fn preview_of_offset(curve: &offset::Curve) -> PreviewCurve {
    match curve {
        offset::Curve::Line(seg) => PreviewCurve::Line { a: seg.a, b: seg.b },
        offset::Curve::Circle(c) => PreviewCurve::Circle {
            center: c.center,
            radius: c.radius,
        },
        offset::Curve::Arc(a) => {
            let (a0, a1) = geom_sweep_to_entity(a.start_angle, a.end_angle, a.ccw);
            PreviewCurve::Arc {
                center: a.circle.center,
                radius: a.circle.radius,
                start_angle: a0,
                end_angle: a1,
            }
        }
    }
}
