//! Dimension ops: Sketch Dimension tool creation, typed-input auto-dimensions,
//! edit/move/delete, placement defaults, and snapshot DTO assembly. Child
//! module of `session` — it uses the
//! session's private fields directly.

use std::collections::HashMap;

use crate::constraint::{Constraint, ConstraintId};
use crate::dto::{
    AddConstraintResult, DimensionDto, DimensionRequest, EditDimensionRequest,
    MoveDimensionRequest, SetDimensionModeRequest, ToolResult,
};
use crate::entity::{Entity, EntityId, AXIS_SENTINEL};
use crate::expr::{self, ExprError};
use crate::geometry::Vec2;
use crate::params::{ParamId, ParamKind};
use crate::session::{SessionError, SketchSession};
use crate::sketch::DimensionMode;

impl From<ExprError> for SessionError {
    fn from(e: ExprError) -> Self {
        SessionError::Expression(e.to_string())
    }
}

impl SketchSession {
    fn line_length(&self, id: EntityId) -> Option<f64> {
        let (a, b) = self.sketch.resolved_line(id)?;
        Some(a.distance(b))
    }

    fn line_mid(&self, id: EntityId) -> Vec2 {
        self.sketch
            .resolved_line(id)
            .map(|(a, b)| (a + b) * 0.5)
            .unwrap_or(Vec2::ZERO)
    }

    fn line_dir(&self, id: EntityId) -> Option<Vec2> {
        let (a, b) = self.sketch.resolved_line(id)?;
        Some(b - a)
    }

    fn point_of_line_start(&self, id: EntityId) -> Option<Vec2> {
        let (a, _) = self.sketch.resolved_line(id)?;
        Some(a)
    }

    fn circle_spec(&self, id: EntityId) -> Option<(Vec2, f64)> {
        match self.sketch.entity(id) {
            Some(Entity::Circle { center, radius }) | Some(Entity::Arc { center, radius, .. }) => {
                Some((*center, *radius))
            }
            _ => None,
        }
    }

    /// Direction of the arc's own midpoint, which is where its radius leader
    /// touches it. A full turn uses the opposite ray from its stored start.
    fn arc_mid_angle(&self, id: EntityId) -> Option<f64> {
        match self.sketch.entity(id) {
            Some(Entity::Arc {
                start_angle,
                end_angle,
                ..
            }) => {
                let span = crate::geometry::arc_span(*start_angle, *end_angle);
                Some(start_angle + span / 2.0)
            }
            _ => None,
        }
    }

    fn kind_of(&self, id: EntityId) -> Option<&'static str> {
        match self.sketch.entity(id) {
            Some(Entity::Point { .. }) => Some("point"),
            Some(Entity::Line { .. }) => Some("line"),
            Some(Entity::Circle { .. }) => Some("circle"),
            Some(Entity::Arc { .. }) => Some("arc"),
            Some(Entity::Spline { .. }) => Some("spline"),
            None => None,
        }
    }

    /// Signed perpendicular distance from `p` to the line through `id`.
    fn signed_dist_to_line(&self, p: Vec2, id: EntityId) -> Option<f64> {
        let (a, b) = self.sketch.resolved_line(id)?;
        let d = b - a;
        let len = d.length();
        if len < 1e-12 {
            return None;
        }
        Some((d.x * (p.y - a.y) - d.y * (p.x - a.x)) / len)
    }

    /// Evaluate typed text against the sketch's current parameters.
    pub(crate) fn eval_text(&self, text: &str) -> Result<f64, SessionError> {
        let params = self.sketch.params();
        Ok(expr::eval_expression(text, &mut |name| {
            params
                .by_name(name)
                .map(|p| p.value)
                .ok_or_else(|| ExprError::UnknownParameter(name.to_string()))
        })?)
    }

    /// Create the driving parameter for a dimension. Plain numbers become
    /// literals; formulas keep their expression (D9).
    pub(crate) fn param_from_text(
        &mut self,
        kind: ParamKind,
        text: Option<&str>,
        measured: f64,
    ) -> Result<ParamId, SessionError> {
        let trimmed = text.map(|t| t.trim().trim_start_matches('=').trim().to_string());
        match trimmed {
            None => Ok(self.sketch.params_mut().add(kind, None, measured)?),
            Some(t) if t.parse::<f64>().is_ok() => {
                let v = t.parse::<f64>().unwrap();
                Ok(self.sketch.params_mut().add(kind, None, v)?)
            }
            Some(t) => {
                self.eval_text(&t)?;
                Ok(self.sketch.params_mut().add(kind, Some(&t), measured)?)
            }
        }
    }

    /// Add a constraint with a param binding + placement, going through
    /// the same over-constraint rejection as panel application (D4.2).
    /// `record_undo: false` for auto-dims inside a tool op (the outer op
    /// pushes one command covering geometry + dimensions); on rejection
    /// the whole mutation (including the new parameter) is rolled back.
    pub(crate) fn add_constraint_bound(
        &mut self,
        constraint: Constraint,
        param: ParamId,
        text_pos: Vec2,
        record_undo: bool,
    ) -> Result<ConstraintId, SessionError> {
        self.add_constraint_bound_offset(constraint, param, text_pos, record_undo, None)
    }

    pub(super) fn add_constraint_bound_offset(
        &mut self,
        mut constraint: Constraint,
        param: ParamId,
        text_pos: Vec2,
        record_undo: bool,
        offset_side: Option<f64>,
    ) -> Result<ConstraintId, SessionError> {
        if let Err(error) = self.reject_duplicate_relation(&constraint) {
            self.sketch.params_mut().remove(param);
            return Err(error);
        }
        if let Some(value) = self
            .sketch
            .params()
            .get(param)
            .map(|parameter| parameter.value)
        {
            constraint.set_dimension_value(value);
        }
        let before = self.sketch.snapshot();
        let cid = self.sketch.add_constraint(constraint);
        self.sketch.bind_dimension(cid, param, text_pos);
        if let Some(side) = offset_side {
            self.sketch.set_offset_side(cid, side);
        }

        let analysis = self.solve_constraint_operation_with_recovery(&[constraint]);
        let new_residual = crate::solver::constraint_residual(&self.sketch, cid);
        if !analysis.converged || new_residual > 1e-6 {
            let error = self.classify_constraint_failure(cid, constraint);
            self.sketch.restore(before);
            self.sketch.params_mut().remove(param);
            self.recompute();
            return Err(error);
        }
        self.analysis = Some(analysis);
        if record_undo {
            self.push_command(before);
        }
        Ok(cid)
    }

    /// Sketch Dimension tool creation and entity-picking rules.
    pub fn add_dimension(&mut self, request: DimensionRequest) -> Result<ToolResult, SessionError> {
        let ids = request.entities.clone();
        let kinds: Vec<&str> = ids
            .iter()
            .map(|id| self.kind_of(*id).unwrap_or("missing"))
            .collect();
        let text_pos = request.text_pos;

        let (constraint, measured, kind) = match kinds.as_slice() {
            ["line"] => {
                let len = self
                    .line_length(ids[0])
                    .ok_or(SessionError::DegenerateSegment)?;
                (
                    Constraint::Distance {
                        from: ids[0],
                        to: None,
                        value: len,
                    },
                    len,
                    ParamKind::Length,
                )
            }
            ["point", "point"] => {
                let (Some(a), Some(b)) = (
                    self.sketch.point_position(ids[0]),
                    self.sketch.point_position(ids[1]),
                ) else {
                    return Err(SessionError::DegenerateSegment);
                };
                let d = a.distance(b);
                (
                    Constraint::Distance {
                        from: ids[0],
                        to: Some(ids[1]),
                        value: d,
                    },
                    d,
                    ParamKind::Length,
                )
            }
            ["point", "line"] | ["line", "point"] => {
                let (p, l) = if kinds[0] == "point" {
                    (ids[0], ids[1])
                } else {
                    (ids[1], ids[0])
                };
                let point = self.sketch.point_position(p).unwrap();
                let d = self
                    .signed_dist_to_line(point, l)
                    .ok_or(SessionError::DegenerateSegment)?;
                (
                    Constraint::Distance {
                        from: p,
                        to: Some(l),
                        value: d,
                    },
                    d,
                    ParamKind::Length,
                )
            }
            ["line", "line"] => {
                let da = self.line_dir(ids[0]).unwrap();
                let db = self.line_dir(ids[1]).unwrap();
                let cross = da.x * db.y - da.y * db.x;
                if cross.abs() < 1e-9 * da.length() * db.length() {
                    let qb = self.point_of_line_start(ids[1]).unwrap();
                    let d = self
                        .signed_dist_to_line(qb, ids[0])
                        .ok_or(SessionError::DegenerateSegment)?;
                    (
                        Constraint::Distance {
                            from: ids[0],
                            to: Some(ids[1]),
                            value: d,
                        },
                        d,
                        ParamKind::Length,
                    )
                } else {
                    let angle = angle_between(da, db);
                    (
                        Constraint::Angle {
                            a: ids[0],
                            b: ids[1],
                            value: angle,
                        },
                        angle,
                        ParamKind::Angle,
                    )
                }
            }
            ["circle"] => {
                let (_, r) = self
                    .circle_spec(ids[0])
                    .ok_or(SessionError::DegenerateSegment)?;
                let d = r * 2.0;
                (
                    Constraint::Diameter {
                        entity: ids[0],
                        value: d,
                    },
                    d,
                    ParamKind::Length,
                )
            }
            ["arc"] => {
                let (_, r) = self
                    .circle_spec(ids[0])
                    .ok_or(SessionError::DegenerateSegment)?;
                (
                    Constraint::Radius {
                        entity: ids[0],
                        value: r,
                    },
                    r,
                    ParamKind::Length,
                )
            }
            _ => return Err(SessionError::InvalidConstraint(
                "Dimension needs a line, two points, point+line, two lines, a circle, or an arc"
                    .to_string(),
            )),
        };

        if self
            .sketch
            .equivalent_driving_constraint(&constraint, None)
            .is_some()
        {
            if request.value_text.is_some() {
                return Err(SessionError::InvalidConstraint(
                    "This measurement is already driven; a reference dimension reports geometry and cannot accept a target value"
                        .to_string(),
                ));
            }
            if self
                .sketch
                .equivalent_reference_constraint(&constraint, None)
                .is_some()
            {
                return Err(SessionError::InvalidConstraint(
                    "A reference dimension already reports this measurement".to_string(),
                ));
            }
            let before = self.sketch.snapshot();
            let mut reference = constraint;
            reference.set_dimension_value(measured);
            let cid = self.sketch.add_constraint(reference);
            self.sketch.bind_reference_dimension(cid, text_pos);
            self.sketch.sync_dimension_constraint_values();
            self.push_command(before);
            return Ok(ToolResult {
                entities: ids,
                sketch: self.dto(),
            });
        }

        let before = self.sketch.snapshot();
        let param = self.param_from_text(kind, request.value_text.as_deref(), measured)?;
        let cid = match self.add_constraint_bound(constraint, param, text_pos, false) {
            Ok(cid) => cid,
            Err(error) => {
                self.sketch.restore(before);
                self.recompute();
                return Err(error);
            }
        };
        let redundant = crate::solver::constraints_are_redundant(&self.sketch, &[cid]);
        if redundant {
            if request.value_text.is_some() {
                self.sketch.restore(before);
                self.recompute();
                return Err(SessionError::InvalidConstraint(
                    "This measurement is already determined by existing constraints; create a reference dimension instead"
                        .to_string(),
                ));
            }
            self.sketch.unbind_dim_param(&cid);
            self.sketch.set_dim_mode(cid, DimensionMode::Reference);
            self.sketch.params_mut().remove(param);
            self.recompute();
            self.sketch.sync_dimension_constraint_values();
        }
        self.push_command(before);
        Ok(ToolResult {
            entities: ids,
            sketch: self.dto(),
        })
    }

    /// Double-click edit: set the driving parameter's value/expression and
    /// re-solve (dependents update live, D9).
    pub fn edit_dimension(
        &mut self,
        request: EditDimensionRequest,
    ) -> Result<AddConstraintResult, SessionError> {
        if self.sketch.is_reference_dimension(&request.constraint_id) {
            return Err(SessionError::InvalidConstraint(
                "Reference dimensions report solved geometry and cannot be edited; make the dimension driving first"
                    .to_string(),
            ));
        }
        let before = self.sketch.snapshot();
        let Some(pid) = self.sketch.dim_param(&request.constraint_id) else {
            return Err(SessionError::InvalidConstraint(
                "entity is not a dimension".to_string(),
            ));
        };
        let constraint = self
            .sketch
            .constraint(request.constraint_id)
            .copied()
            .ok_or_else(|| {
                SessionError::InvalidConstraint("dimension constraint is missing".to_string())
            })?;
        let before_dimensions = self
            .sketch
            .constraints()
            .filter(|(cid, constraint)| {
                !self.sketch.is_reference_dimension(cid)
                    && constraint.kind() == crate::constraint::ConstraintKind::Dimensional
            })
            .map(|(cid, constraint)| (cid, *constraint))
            .collect::<HashMap<_, _>>();
        if let Err(error) = self
            .sketch
            .params_mut()
            .set_expression(pid, &request.text)
            .map_err(SessionError::from)
        {
            self.sketch.restore(before);
            self.recompute();
            return Err(error);
        }

        self.sketch.sync_dimension_constraint_values();

        let changed_dimensions = self
            .sketch
            .constraints()
            .filter_map(|(cid, current)| {
                (!self.sketch.is_reference_dimension(&cid)
                    && current.kind() == crate::constraint::ConstraintKind::Dimensional
                    && before_dimensions.get(&cid).copied() != Some(*current))
                .then_some(*current)
            })
            .collect::<Vec<_>>();
        let analysis = self.solve_dimension_edit_with_recovery(&changed_dimensions);
        let residual = crate::solver::constraint_residual(&self.sketch, request.constraint_id);
        if !analysis.converged || residual > 1e-6 {
            let error = self.classify_constraint_failure(request.constraint_id, constraint);
            self.sketch.restore(before);
            self.recompute();
            return Err(error);
        }

        self.analysis = Some(analysis);
        self.push_command(before);
        Ok(AddConstraintResult {
            constraint_id: request.constraint_id,
            sketch: self.dto(),
        })
    }

    /// Convert a dimension between solver-driving and read-only reference
    /// modes. Conversion is undoable and never leaves orphan parameters.
    pub fn set_dimension_mode(
        &mut self,
        request: SetDimensionModeRequest,
    ) -> Result<AddConstraintResult, SessionError> {
        let cid = request.constraint_id;
        let constraint = self
            .sketch
            .constraint(cid)
            .copied()
            .filter(|constraint| {
                constraint.kind() == crate::constraint::ConstraintKind::Dimensional
                    && self.sketch.dim_placement(&cid).is_some()
            })
            .ok_or_else(|| {
                SessionError::InvalidConstraint("entity is not a dimension".to_string())
            })?;
        if self.sketch.dim_mode(&cid) == request.mode {
            return Ok(AddConstraintResult {
                constraint_id: cid,
                sketch: self.dto(),
            });
        }

        match request.mode {
            DimensionMode::Reference => {
                let pid = self.sketch.dim_param(&cid).ok_or_else(|| {
                    SessionError::InvalidConstraint(
                        "driving dimension has no parameter binding".to_string(),
                    )
                })?;
                let parameter = self.sketch.params().get(pid).ok_or_else(|| {
                    SessionError::InvalidConstraint(
                        "driving dimension parameter is missing".to_string(),
                    )
                })?;
                let name = parameter.name.clone();
                let used_by = self
                    .sketch
                    .params()
                    .all()
                    .iter()
                    .filter(|candidate| candidate.id != pid)
                    .filter_map(|candidate| {
                        let expression = candidate.expression.as_deref()?;
                        let ast = expr::parse(expression).ok()?;
                        expr::referenced_idents(&ast)
                            .contains(&name)
                            .then_some(candidate.name.as_str())
                    })
                    .collect::<Vec<_>>();
                if !used_by.is_empty() {
                    return Err(SessionError::InvalidConstraint(format!(
                        "Cannot make {name} a reference dimension because it is used by {}",
                        used_by.join(", ")
                    )));
                }

                let before = self.sketch.snapshot();
                self.sketch.unbind_dim_param(&cid);
                self.sketch.set_dim_mode(cid, DimensionMode::Reference);
                self.sketch.params_mut().remove(pid);
                self.recompute();
                self.sketch.sync_dimension_constraint_values();
                self.push_command(before);
            }
            DimensionMode::Driving => {
                if self
                    .sketch
                    .equivalent_driving_constraint(&constraint, Some(cid))
                    .is_some()
                {
                    return Err(SessionError::InvalidConstraint(
                        "Another driving dimension already controls this measurement; remove or make it reference first"
                            .to_string(),
                    ));
                }
                let measured = self
                    .sketch
                    .measure_dimension(cid, constraint)
                    .ok_or_else(|| {
                        SessionError::InvalidConstraint(
                            "Cannot measure this reference dimension".to_string(),
                        )
                    })?;
                let kind = if matches!(
                    constraint,
                    Constraint::Angle { .. } | Constraint::ArcAngle { .. }
                ) {
                    ParamKind::Angle
                } else {
                    ParamKind::Length
                };
                let before = self.sketch.snapshot();
                let pid = self.param_from_text(kind, None, measured)?;
                let mut driving = constraint;
                driving.set_dimension_value(measured);
                self.sketch.replace_constraint(cid, driving);
                let placement = self.sketch.dim_placement(&cid).unwrap_or(Vec2::ZERO);
                self.sketch.bind_dimension(cid, pid, placement);
                let analysis = crate::solver::solve(&mut self.sketch, &[]);
                let residual = crate::solver::constraint_residual(&self.sketch, cid);
                let redundant = analysis.converged
                    && residual <= 1e-6
                    && crate::solver::constraints_are_redundant(&self.sketch, &[cid]);
                if !analysis.converged || residual > 1e-6 || redundant {
                    if redundant {
                        self.sketch.restore(before);
                        self.recompute();
                        return Err(SessionError::InvalidConstraint(
                            "Existing constraints already determine this measurement, so it must remain a reference dimension"
                                .to_string(),
                        ));
                    }
                    let error = self.classify_constraint_failure(cid, driving);
                    self.sketch.restore(before);
                    self.recompute();
                    return Err(error);
                }
                self.analysis = Some(analysis);
                self.sketch.sync_dimension_constraint_values();
                self.push_command(before);
            }
        }

        Ok(AddConstraintResult {
            constraint_id: cid,
            sketch: self.dto(),
        })
    }

    /// Drag a dimension's text to a new position (undoable).
    pub fn move_dimension(
        &mut self,
        request: MoveDimensionRequest,
    ) -> Result<AddConstraintResult, SessionError> {
        if self.sketch.dim_placement(&request.constraint_id).is_none() {
            return Err(SessionError::InvalidConstraint(
                "entity is not a dimension".to_string(),
            ));
        }
        let before = self.sketch.snapshot();
        self.sketch
            .set_dim_placement(request.constraint_id, request.text_pos);
        self.push_command(before);
        Ok(AddConstraintResult {
            constraint_id: request.constraint_id,
            sketch: self.dto(),
        })
    }

    /// Delete a dimension (constraint + its parameter when orphaned).
    pub fn delete_dimension(
        &mut self,
        cid: ConstraintId,
    ) -> Result<AddConstraintResult, SessionError> {
        let param = self.sketch.dim_param(&cid);
        let before = self.sketch.snapshot();
        self.sketch.remove_constraint(cid);

        if let Some(pid) = param {
            let still_bound = self.sketch.dimension_of_param(pid).is_some();
            if !still_bound {
                let name = self.sketch.params().get(pid).map(|p| p.name.clone());
                let referenced = name.map(|n| {
                    self.sketch.params().all().iter().any(|p| {
                        p.id != pid
                            && p.expression
                                .as_deref()
                                .and_then(|text| expr::parse(text).ok())
                                .map(|ast| expr::referenced_idents(&ast).contains(&n))
                                .unwrap_or(false)
                    })
                });
                if referenced == Some(false) {
                    self.sketch.params_mut().remove(pid);
                }
            }
        }
        self.recompute();
        self.push_command(before);
        Ok(AddConstraintResult {
            constraint_id: cid,
            sketch: self.dto(),
        })
    }

    /// Typed length while drawing a line → Distance dim + annotation.
    pub(crate) fn auto_dim_line_length(&mut self, line: EntityId, text: &str) {
        let Some(len) = self.line_length(line) else {
            return;
        };
        let mid = self.line_mid(line);
        let dir = self.line_dir(line).unwrap_or(Vec2::new(1.0, 0.0));
        let pos = mid + perp_unit(dir) * default_linear_dimension_offset(len);
        let Ok(param) = self.param_from_text(ParamKind::Length, Some(text), len) else {
            return;
        };
        let _ = self.add_constraint_bound(
            Constraint::Distance {
                from: line,
                to: None,
                value: len,
            },
            param,
            pos,
            false,
        );
    }

    /// Typed angle while drawing a line → axis Angle dim (from +u).
    pub(crate) fn auto_dim_line_angle(&mut self, line: EntityId, text: &str) {
        let Some(dir) = self.line_dir(line) else {
            return;
        };
        let deg = dir.y.atan2(dir.x).to_degrees();
        let mid = self.line_mid(line);
        // New typed angle labels start opposite the automatic length label.
        // Existing/manual placements are retained; this is not a general
        // collision guarantee across adjacent geometry, fonts or zoom levels.
        let pos = mid - perp_unit(dir) * default_angular_dimension_offset(dir.length());
        let Ok(param) = self.param_from_text(ParamKind::Angle, Some(text), deg) else {
            return;
        };
        let _ = self.add_constraint_bound(
            Constraint::Angle {
                a: line,
                b: AXIS_SENTINEL,
                value: deg,
            },
            param,
            pos,
            false,
        );
    }

    /// Typed width/height while drawing a rectangle → Distance dims across
    /// the CORNER POINTS (bottom pair / left pair). Corner-to-corner
    /// reference (2026-07-19 PM): the dimensions then survive
    /// corner ops (fillet/chamfer keep the corner point as a persistent
    /// reference) instead of fighting the trim.
    pub(crate) fn auto_dim_rect(
        &mut self,
        bl: EntityId,
        br: EntityId,
        tl: EntityId,
        width_text: Option<&str>,
        height_text: Option<&str>,
    ) {
        if let Some(text) = width_text {
            if let (Some(a), Some(b)) = (
                self.sketch.point_position(bl),
                self.sketch.point_position(br),
            ) {
                let len = a.distance(b);
                let mid = (a + b) * 0.5;
                if let Ok(param) = self.param_from_text(ParamKind::Length, Some(text), len) {
                    let _ = self.add_constraint_bound(
                        Constraint::Distance {
                            from: bl,
                            to: Some(br),
                            value: len,
                        },
                        param,
                        mid + Vec2::new(0.0, -default_linear_dimension_offset(len)),
                        false,
                    );
                }
            }
        }
        if let Some(text) = height_text {
            if let (Some(a), Some(b)) = (
                self.sketch.point_position(bl),
                self.sketch.point_position(tl),
            ) {
                let len = a.distance(b);
                let mid = (a + b) * 0.5;
                if let Ok(param) = self.param_from_text(ParamKind::Length, Some(text), len) {
                    let _ = self.add_constraint_bound(
                        Constraint::Distance {
                            from: bl,
                            to: Some(tl),
                            value: len,
                        },
                        param,
                        mid + Vec2::new(-default_linear_dimension_offset(len), 0.0),
                        false,
                    );
                }
            }
        }
    }

    /// Typed diameter while drawing a circle → Diameter dim.
    pub(crate) fn auto_dim_circle(&mut self, circle: EntityId, text: &str) {
        let Some((center, r)) = self.circle_spec(circle) else {
            return;
        };
        let d = r * 2.0;
        let Ok(param) = self.param_from_text(ParamKind::Length, Some(text), d) else {
            return;
        };
        let _ = self.add_constraint_bound(
            Constraint::Diameter {
                entity: circle,
                value: d,
            },
            param,
            center
                + Vec2::new(
                    r + default_radial_dimension_gap(d),
                    r + default_radial_dimension_gap(d),
                ),
            false,
        );
    }

    /// Typed radius while drawing a center arc → Radius dim. The arc's radius
    /// is already resolved by the locked pick, so the dimension only records
    /// the driving value and stays editable like any other radius.
    pub(crate) fn auto_dim_arc_radius(&mut self, arc: EntityId, text: &str) {
        let Some((center, r)) = self.circle_spec(arc) else {
            return;
        };
        let Ok(param) = self.param_from_text(ParamKind::Length, Some(text), r) else {
            return;
        };

        let mid = self
            .arc_mid_angle(arc)
            .unwrap_or(std::f64::consts::FRAC_PI_4);
        let gap = default_radial_dimension_gap(r * 2.0);
        let text_pos = center + Vec2::new(mid.cos(), mid.sin()) * (r + gap);
        let _ = self.add_constraint_bound(
            Constraint::Radius {
                entity: arc,
                value: r,
            },
            param,
            text_pos,
            false,
        );
    }

    /// A typed sweep angle becomes a driving `ArcAngle` dimension, placed like
    /// an angular dimension: inside the arc, centred on the span it measures.
    pub(crate) fn auto_dim_arc_angle(&mut self, arc: EntityId, text: &str) {
        let Some((center, r)) = self.circle_spec(arc) else {
            return;
        };
        let Some(mid) = self.arc_mid_angle(arc) else {
            return;
        };
        let Ok(param) = self.param_from_text(ParamKind::Angle, Some(text), 0.0) else {
            return;
        };
        let span = self
            .sketch
            .entity(arc)
            .and_then(|entity| match entity {
                Entity::Arc {
                    start_angle,
                    end_angle,
                    ..
                } => Some(crate::geometry::arc_span(*start_angle, *end_angle)),
                _ => None,
            })
            .unwrap_or(0.0);
        let _ = self.add_constraint_bound(
            Constraint::ArcAngle {
                entity: arc,
                value: span.to_degrees(),
            },
            param,
            center + Vec2::new(mid.cos(), mid.sin()) * (r * 0.55),
            false,
        );
    }

    pub(crate) fn dimension_dtos(&self) -> Vec<DimensionDto> {
        self.sketch
            .constraints()
            .filter_map(|(cid, c)| {
                let kind = c.kind_str();
                if !matches!(kind, "distance" | "radius" | "diameter" | "angle") {
                    return None;
                }
                let mode = self.sketch.dim_mode(&cid);
                let (pid, param_name, param_expression, value) = match mode {
                    DimensionMode::Driving => {
                        let pid = self.sketch.dim_param(&cid)?;
                        let param = self.sketch.params().get(pid)?;
                        (
                            Some(pid),
                            Some(param.name.clone()),
                            param.expression.clone(),
                            param.value,
                        )
                    }
                    DimensionMode::Reference => {
                        (None, None, None, self.sketch.measure_dimension(cid, *c)?)
                    }
                };

                let display_value = if matches!(c, Constraint::ArcAngle { .. }) {
                    value.abs()
                } else {
                    value
                };
                let value_text = match kind {
                    "diameter" => format!("Ø{display_value:.2}"),
                    "radius" => format!("R{display_value:.2}"),
                    "angle" => format!("{display_value:.2}°"),
                    _ => format!("{display_value:.2}"),
                };
                let text = if mode == DimensionMode::Reference {
                    format!("({value_text})")
                } else {
                    value_text
                };
                Some(DimensionDto {
                    constraint_id: cid,
                    mode,
                    kind: kind.to_string(),
                    entities: c.referenced_entities(),
                    param_id: pid,
                    param_name,
                    param_expression,
                    value,
                    text,
                    text_pos: self.sketch.dim_placement(&cid).unwrap_or(Vec2::ZERO),
                })
            })
            .collect()
    }
}

/// Dimension annotations are born near the geometry they describe. A fixed
/// 15 mm offset overwhelms sub-millimetre details and creates unnecessarily
/// long extension lines, while a pure percentage can land on top of very
/// small geometry. These bounded, span-aware defaults remain user-adjustable
/// through `move_dimension`.
fn default_linear_dimension_offset(span: f64) -> f64 {
    (span.abs() * 0.35).clamp(1.5, 10.0)
}

fn default_angular_dimension_offset(span: f64) -> f64 {
    (span.abs() * 0.40).clamp(2.0, 12.0)
}

fn default_radial_dimension_gap(diameter: f64) -> f64 {
    (diameter.abs() * 0.25).clamp(2.0, 8.0)
}

fn angle_between(a: Vec2, b: Vec2) -> f64 {
    let cross = a.x * b.y - a.y * b.x;
    let dot = a.x * b.x + a.y * b.y;
    cross.atan2(dot).to_degrees().abs()
}

fn perp_unit(d: Vec2) -> Vec2 {
    let len = d.length();
    if len < 1e-12 {
        Vec2::new(0.0, 1.0)
    } else {
        d.perp() * (1.0 / len)
    }
}
