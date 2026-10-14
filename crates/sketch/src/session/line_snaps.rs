//! Finite edge acquisition for line starts and angle-locked endpoints.
use super::*;
use crate::geomops::trimext::{self, Circle, LineSeg};

impl SketchSession {
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
