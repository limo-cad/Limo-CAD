//! Read-only dimension inference and automatic leader placement.
use super::*;
use crate::dto::DimensionPreviewRequest;

impl SketchSession {
    pub(super) fn reference_dimension_side(&self, constraint: Constraint) -> Option<f64> {
        let signed = match constraint {
            Constraint::ReferenceDistance { entity, edge, .. } => {
                let (a, b) = self.sketch.reference_edge(edge)?.straight_line()?;
                let point = self
                    .sketch
                    .point_position(entity)
                    .unwrap_or_else(|| self.line_mid(entity));
                (b - a).perp().dot(point - a)
            }
            Constraint::ReferenceAngle { entity, edge, .. } => {
                let (a, b) = self.sketch.reference_edge(edge)?.straight_line()?;
                let line = self.line_dir(entity)?;
                let reference = b - a;
                reference.x * line.y - reference.y * line.x
            }
            _ => return None,
        };
        Some(if signed < 0. { -1. } else { 1. })
    }

    /// No geometry, constraint, parameter, history or model revision changes.
    pub fn preview_dimension(
        &self,
        request: DimensionPreviewRequest,
    ) -> Result<DimensionDto, SessionError> {
        let (constraint, value, _) =
            self.dimension_spec(&request.entities, request.reference_edge)?;
        if let Some(id) = self.sketch.equivalent_driving_constraint(&constraint, None) {
            if let Some(dimension) = self
                .dimension_dtos()
                .into_iter()
                .find(|d| d.constraint_id == id)
            {
                return Ok(dimension);
            }
        }
        // Opposite sides of a constrained rectangle share one length driver.
        // Reuse that parameter only when authored H/V rails prove the span,
        // never merely because two measured lengths happen to be equal.
        if let Some(dimension) = self.dimension_dtos().into_iter().find(|d| {
            d.mode == DimensionMode::Driving
                && self
                    .sketch
                    .constraint(d.constraint_id)
                    .is_some_and(|driver| self.same_axis_length(constraint, *driver))
        }) {
            return Ok(dimension);
        }
        let mut probe = self.sketch.clone();
        let id = probe.add_constraint(constraint);
        if let Some(side) = self.reference_dimension_side(constraint) {
            probe.set_offset_side(id, side);
        }
        let reference = crate::solver::constraints_are_redundant(&probe, &[id]);
        let position = self.default_dimension_position(
            &request.entities,
            request.reference_edge,
            constraint.kind_str(),
        )?;
        let kind = constraint.kind_str();
        let text = match kind {
            "angle" => format!("{value:.2}°"),
            "diameter" => format!("Ø{value:.2}"),
            "radius" => format!("R{value:.2}"),
            _ => format!("{value:.2}"),
        };
        Ok(DimensionDto {
            constraint_id: ConstraintId(0),
            mode: if reference {
                DimensionMode::Reference
            } else {
                DimensionMode::Driving
            },
            kind: kind.into(),
            entities: request.entities,
            reference_edge: request.reference_edge,
            param_id: None,
            param_name: None,
            param_expression: None,
            value,
            text,
            text_pos: position,
        })
    }

    fn same_axis_length(&self, first: Constraint, second: Constraint) -> bool {
        let pair = |constraint| match constraint {
            Constraint::Distance { from, to: None, .. } => self.sketch.line_endpoint_ids(from),
            Constraint::Distance {
                from, to: Some(to), ..
            } if self.sketch.point_position(from).is_some()
                && self.sketch.point_position(to).is_some() =>
            {
                Some((from, to))
            }
            _ => None,
        };
        let (Some((a, b)), Some((c, d))) = (pair(first), pair(second)) else {
            return false;
        };
        let same_pair =
            |p: (EntityId, EntityId), q: (EntityId, EntityId)| p == q || p == (q.1, q.0);
        let rails = |constraint| match constraint {
            Constraint::Horizontal { entity } => {
                self.sketch.line_endpoint_ids(entity).map(|p| (true, p))
            }
            Constraint::Vertical { entity } => {
                self.sketch.line_endpoint_ids(entity).map(|p| (false, p))
            }
            Constraint::HorizontalPoints { a, b } => Some((true, (a, b))),
            Constraint::VerticalPoints { a, b } => Some((false, (a, b))),
            _ => None,
        };
        let axis = |pair| {
            self.sketch.constraints().find_map(|(_, constraint)| {
                let (horizontal, points) = rails(*constraint)?;
                same_pair(pair, points).then_some(horizontal)
            })
        };
        let (Some(horizontal), Some(other)) = (axis((a, b)), axis((c, d))) else {
            return false;
        };
        if horizontal != other {
            return false;
        }
        let connected = |start, target| {
            let mut visited = vec![start];
            let mut index = 0;
            while index < visited.len() {
                let point = visited[index];
                index += 1;
                if point == target {
                    return true;
                }
                for (_, constraint) in self.sketch.constraints() {
                    let Some((axis, (a, b))) = rails(*constraint) else {
                        continue;
                    };
                    if axis == horizontal {
                        continue;
                    }
                    let next = if a == point {
                        b
                    } else if b == point {
                        a
                    } else {
                        continue;
                    };
                    if !visited.contains(&next) {
                        visited.push(next);
                    }
                }
            }
            false
        };
        (connected(a, c) && connected(b, d)) || (connected(a, d) && connected(b, c))
    }

    fn default_dimension_position(
        &self,
        ids: &[EntityId],
        edge: Option<EdgeId>,
        kind: &str,
    ) -> Result<Vec2, SessionError> {
        let anchor = |id| {
            self.sketch
                .point_position(id)
                .unwrap_or_else(|| self.line_mid(id))
        };
        let other_line = edge
            .and_then(|edge| self.sketch.reference_edge(edge)?.straight_line())
            .or_else(|| ids.get(1).and_then(|id| self.sketch.resolved_line(*id)));
        if let (Some((a, b)), Some((c, d))) = (self.sketch.resolved_line(ids[0]), other_line) {
            let u = b - a;
            let v = d - c;
            if kind == "angle" {
                let cross = u.x * v.y - u.y * v.x;
                let t = ((c.x - a.x) * v.y - (c.y - a.y) * v.x) / cross;
                let direction = u * (1. / u.length()) + v * (1. / v.length());
                return Ok(a
                    + u * t
                    + direction * (u.length().min(v.length()) * 0.35).clamp(1., 12.));
            }
            let midpoint = (a + b) * 0.5;
            let foot = c + v * ((midpoint - c).dot(v) / v.dot(v));
            return Ok((midpoint + foot) * 0.5
                + v * (default_linear_dimension_offset(u.length()) / v.length()));
        }
        if let Some((a, b)) = self.sketch.resolved_line(ids[0]) {
            let d = b - a;
            return Ok((a + b) * 0.5
                + d.perp() * (default_linear_dimension_offset(d.length()) / d.length()));
        }
        if let Some((center, radius)) = self.circle_spec(ids[0]) {
            let gap = default_radial_dimension_gap(radius * 2.);
            return Ok(center + Vec2::new(radius + gap, radius + gap));
        }
        let first = anchor(ids[0]);
        if let Some((a, b)) = other_line {
            let d = b - a;
            let foot = a + d * ((first - a).dot(d) / d.dot(d));
            return Ok((first + foot) * 0.5
                + d * (default_linear_dimension_offset(first.distance(foot)) / d.length()));
        }
        let second = ids.get(1).map_or(first, |id| anchor(*id));
        let d = second - first;
        let offset = if d.length() > 1e-9 {
            d.perp() * (default_linear_dimension_offset(d.length()) / d.length())
        } else {
            Vec2::new(0., 1.)
        };
        Ok((first + second) * 0.5 + offset)
    }
}
