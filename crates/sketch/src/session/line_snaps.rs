//! Finite edge acquisition for line starts and angle-locked endpoints.
use super::*;
use crate::geomops::trimext::{self, Circle, LineSeg};

impl SketchSession {
    /// A small angular cone plus a viewport capture radius: long lines must
    /// not pull the cursor many pixels sideways just to reach an axis.
    pub(super) fn line_axis(
        &self,
        from: Vec2,
        cursor: Vec2,
        ctrl_held: bool,
    ) -> Option<TrackingAxis> {
        let delta = cursor - from;
        if ctrl_held || delta.length() < MIN_LINE_LENGTH_MM {
            return None;
        }
        let tolerance = INFERENCE_ANGLE_TOL_DEG.to_radians().tan();
        let (axis, along, across) = if delta.x.abs() >= delta.y.abs() {
            (TrackingAxis::Horizontal, delta.x.abs(), delta.y.abs())
        } else {
            (TrackingAxis::Vertical, delta.y.abs(), delta.x.abs())
        };
        (across <= tolerance * along && self.grid_capture_mm.is_none_or(|radius| across <= radius))
            .then_some(axis)
    }

    pub(super) fn snap_line_acquisition(
        &self,
        raw: Vec2,
        ctrl_held: bool,
        exclude: Option<Vec2>,
    ) -> (Vec2, SnapTarget) {
        let acquired = self.snap_inner(raw, !ctrl_held, !ctrl_held, exclude);
        if ctrl_held
            || !self.point_snap
            || matches!(
                acquired.1,
                SnapTarget::Point { .. }
                    | SnapTarget::Origin
                    | SnapTarget::Midpoint { .. }
                    | SnapTarget::ReferenceMidpoint { .. }
            )
        {
            return acquired;
        }
        let nearest = self
            .sketch
            .entities()
            .filter_map(|(id, entity)| {
                let Entity::Line { .. } = entity else {
                    return None;
                };
                let (a, b) = self.sketch.resolved_line(id)?;
                let d = b - a;
                let n = d.dot(d);
                if n <= MERGE_EPS * MERGE_EPS {
                    return None;
                }
                let point = a + d * ((raw - a).dot(d) / n).clamp(0., 1.);
                let distance = point.distance(raw);
                (distance <= self.snap_tolerance
                    && !exclude.is_some_and(|p| p.distance(point) <= MERGE_EPS))
                .then_some((point, id, distance))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((point, entity, distance)) = nearest {
            if !matches!(acquired.1, SnapTarget::ProjectedEdge { .. })
                || distance <= acquired.0.distance(raw)
            {
                return (point, SnapTarget::Curve { entity });
            }
        }
        acquired
    }

    /// Intersect the typed ray with finite carriers before considering grid
    /// increments. An off-ray midpoint can never override the user's angle.
    pub(super) fn snap_line_ray(
        &self,
        from: Vec2,
        direction: Vec2,
        cursor: Vec2,
    ) -> Option<(Vec2, SnapTarget)> {
        if !self.point_snap {
            return None;
        }
        let ray = LineSeg {
            a: from,
            b: from + direction,
        };
        let intersection = |a: Vec2, b: Vec2| {
            let (point, t, u) = trimext::line_line(&ray, &LineSeg { a, b })?;
            (t >= MIN_LINE_LENGTH_MM && (-MERGE_EPS..=1. + MERGE_EPS).contains(&u)).then_some(point)
        };
        let mut candidates = Vec::new();
        for (entity, geometry) in self.sketch.entities() {
            if !matches!(geometry, Entity::Line { .. }) {
                continue;
            }
            let Some((a, b)) = self.sketch.resolved_line(entity) else {
                continue;
            };
            if let Some(point) = intersection(a, b) {
                let target = if point.distance((a + b) * 0.5) <= MERGE_EPS {
                    SnapTarget::Midpoint { entity }
                } else {
                    SnapTarget::Curve { entity }
                };
                candidates.push((point, target));
            }
        }
        for edge in &self.projected_edges {
            let points: Vec<_> = if let Some(circle) = edge.circle {
                trimext::line_circle(
                    &ray,
                    &Circle {
                        center: circle.center,
                        radius: circle.radius,
                    },
                )
                .into_iter()
                .filter(|(_, t)| *t >= MIN_LINE_LENGTH_MM)
                .map(|(point, _)| point)
                .filter(|point| {
                    edge.closest_point(*point)
                        .is_some_and(|finite| finite.distance(*point) <= MERGE_EPS)
                })
                .collect()
            } else {
                edge.points
                    .windows(2)
                    .filter_map(|p| intersection(p[0], p[1]))
                    .collect()
            };
            for point in points {
                let target = if edge
                    .midpoint()
                    .is_some_and(|mid| mid.distance(point) <= MERGE_EPS)
                {
                    SnapTarget::ReferenceMidpoint { edge: edge.edge_id }
                } else {
                    SnapTarget::ProjectedEdge {
                        edge: edge.edge_id,
                        position: point,
                    }
                };
                candidates.push((point, target));
            }
        }
        candidates
            .into_iter()
            .filter(|(point, _)| point.distance(cursor) <= self.snap_tolerance)
            .min_by(|a, b| a.0.distance(cursor).total_cmp(&b.0.distance(cursor)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OriginPlane, PlaneRef, ProjectedEdgeDto, ViewportSnapContext};
    use limo_cad_core::EdgeId;

    fn fixture(pixel: f64) -> SketchSession {
        let plane = PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        };
        let mut session = SketchSession::new(
            "Axis snapping",
            PlaneRef::PlanarFace {
                face_id: limo_cad_core::FaceId(1),
            },
            plane.basis().unwrap(),
            false,
        );
        session
            .replace_viewport_snap(ViewportSnapContext {
                grid_step_mm: pixel * 24.,
                point_tolerance_mm: pixel * 14.,
                grid_capture_mm: pixel * 7.,
            })
            .unwrap();
        session
    }

    #[test]
    fn axis_snap_captures_both_directions_and_releases_at_angle_or_pixel_limit() {
        for pixel in [0.01, 0.1, 1.] {
            for direction in [
                Vec2::new(1., 0.),
                Vec2::new(-1., 0.),
                Vec2::new(0., 1.),
                Vec2::new(0., -1.),
            ] {
                for side in [-1., 1.] {
                    let mut session = fixture(pixel);
                    let from = Vec2::new(100., 100.);
                    let normal = direction.perp() * side;
                    let raw = from
                        + (direction * 100. + normal * (100. * 2.5_f64.to_radians().tan())) * pixel;
                    let preview = session.preview_segment(from, raw, false);
                    let expected = from + direction * (100. * pixel);
                    assert!(preview.snapped_to.distance(expected) < 1e-9);
                    assert!(preview.inferences.contains(&if direction.x != 0. {
                        Inference::Horizontal
                    } else {
                        Inference::Vertical
                    }));
                    let committed = session.add_line(from, raw, false).unwrap();
                    assert!(
                        session
                            .sketch
                            .point_position(committed.end_point_id)
                            .unwrap()
                            .distance(expected)
                            < 1e-9
                    );
                    // Use a fresh session so endpoint attraction is not being tested here.
                    let session = fixture(pixel);
                    for (length, degrees) in [(100., 3.5_f64), (400., 2.)] {
                        let raw = from
                            + (direction * length + normal * (length * degrees.to_radians().tan()))
                                * pixel;
                        let preview = session.preview_segment(from, raw, false);
                        assert!(preview.snapped_to.distance(raw) < 1e-9);
                        assert!(
                            preview.inferences.is_empty(),
                            "{pixel}, {length}, {degrees}: {preview:?}"
                        );
                    }
                    let suppressed = session.preview_segment(from, raw, true);
                    assert_eq!(suppressed.snapped_to, raw);
                    assert!(suppressed.inferences.is_empty());
                    let locked = session.preview_segment_locked(
                        from,
                        (Some(100. * pixel), None),
                        raw,
                        false,
                        (None, None, None, None),
                    );
                    assert!(locked.snapped_to.distance(expected) < 1e-9);
                }
            }
        }
    }

    #[test]
    fn inferred_axis_intersects_support_edge_and_stays_attached_after_dimensioning() {
        let mut session = fixture(0.1);
        let edge = EdgeId(55);
        session.set_projected_edges(vec![ProjectedEdgeDto {
            id: 1 << 40,
            edge_id: edge,
            points: vec![Vec2::new(0., 20.), Vec2::new(40., 20.)],
            circle: None,
        }]);
        let from = Vec2::new(12., 5.);
        let raw = Vec2::new(12.2, 20.);
        let preview = session.preview_segment(from, raw, false);
        assert_eq!(preview.snapped_to, Vec2::new(12., 20.));
        assert!(preview.inferences.contains(&Inference::Vertical));
        assert!(matches!(preview.snap, SnapTarget::ProjectedEdge { edge: id, .. } if id == edge));
        let result = session.add_line(from, raw, false).unwrap();
        assert!(result.sketch.constraints.iter().any(|c| matches!(c.constraint, Constraint::ReferenceOnEdge { point, edge: id } if point == result.end_point_id && id == edge)));
        session
            .add_dimension(crate::DimensionRequest {
                entities: vec![result.entity_id],
                text_pos: Vec2::new(15., 10.),
                value_text: Some("25".into()),
            })
            .unwrap();
        let start = session
            .sketch
            .point_position(result.start_point_id)
            .unwrap();
        let end = session.sketch.point_position(result.end_point_id).unwrap();
        assert!((start.x - end.x).abs() < 1e-8);
        assert!((end.y - 20.).abs() < 1e-8);
        assert!((start.distance(end) - 25.).abs() < 1e-8);
        let saved = session.sketch.snapshot();
        session.undo().unwrap();
        session.redo().unwrap();
        assert_eq!(
            serde_json::to_value(session.sketch.snapshot()).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }

    #[test]
    fn inferred_axis_intersects_sketch_carrier_instead_of_its_nearest_projection() {
        let mut session = fixture(0.1);
        let carrier = session
            .add_line(Vec2::new(-10., 12.), Vec2::new(40., 22.), true)
            .unwrap();
        let from = Vec2::new(12., 0.);
        let raw = Vec2::new(12.2, 16.5);
        let preview = session.preview_segment(from, raw, false);
        assert!(preview.snapped_to.distance(Vec2::new(12., 16.4)) < 1e-9);
        assert_eq!(
            preview.snap,
            SnapTarget::Curve {
                entity: carrier.entity_id
            }
        );
        assert!(preview.inferences.contains(&Inference::Vertical));
        let result = session.add_line(from, raw, false).unwrap();
        assert!(result.sketch.constraints.iter().any(|c| matches!(c.constraint, Constraint::Coincident { a, b } if a == result.end_point_id && b == carrier.entity_id)));
        assert!(
            (session
                .sketch
                .point_position(result.end_point_id)
                .unwrap()
                .x
                - 12.)
                .abs()
                < 1e-9
        );
        let angle = 89_f64.to_radians();
        let locked = session.preview_segment_locked(
            from,
            (None, Some(89.)),
            raw,
            false,
            (None, None, None, None),
        );
        let delta = locked.snapped_to - from;
        assert!((delta.x * angle.sin() - delta.y * angle.cos()).abs() < 1e-9);
    }

    #[test]
    fn exact_points_win_and_axis_snapping_does_not_extend_finite_carriers() {
        let mut session = fixture(0.1);
        let target = session.add_point(Vec2::new(12.4, 20.)).unwrap().entities[0];
        let point = session.preview_segment(Vec2::new(12., 5.), Vec2::new(12.39, 20.), false);
        assert_eq!(point.snap, SnapTarget::Point { entity: target });
        assert!(!point.inferences.contains(&Inference::Vertical));
        session.set_projected_edges(vec![ProjectedEdgeDto {
            id: 1 << 40,
            edge_id: EdgeId(55),
            points: vec![Vec2::new(0., 20.), Vec2::new(20., 20.)],
            circle: None,
        }]);
        let beyond = session.preview_segment(Vec2::new(40., 5.), Vec2::new(40.2, 20.), false);
        assert_eq!(beyond.snapped_to, Vec2::new(40., 20.));
        assert_eq!(beyond.snap, SnapTarget::None);
    }
}
