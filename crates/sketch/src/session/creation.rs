//! Stateless creation resolution, shared by hover previews and mutations.
use super::*;
use crate::dto::{CreationPreviewDto, CreationPreviewRequest, PreviewCurve};
use crate::geomops::slot::{slot_capsule, SlotCapsule};

pub(super) struct ResolvedRectangle {
    pub anchor: Vec2,
    pub anchor_target: SnapTarget,
    pub corner: Vec2,
    pub corner_target: SnapTarget,
    pub width: Option<f64>,
    pub height: Option<f64>,
}
pub(super) struct ResolvedCircle {
    pub anchor: Vec2,
    pub anchor_target: SnapTarget,
    pub edge: Vec2,
    pub edge_target: SnapTarget,
    pub diameter: Option<f64>,
    pub center: Vec2,
    pub radius: f64,
}

pub(super) struct ResolvedArc {
    pub center: Vec2,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub start_pick: (Vec2, SnapTarget),
    pub end_pick: (Vec2, SnapTarget),
    pub center_target: SnapTarget,
    pub mid_pick: Option<(Vec2, SnapTarget)>,
    pub signed_sweep: f64,
    pub lock: Option<f64>,
}

impl SketchSession {
    pub(super) fn resolve_center_arc(
        &self,
        request: &crate::dto::ArcCenterRequest,
    ) -> Result<ResolvedArc, SessionError> {
        let crate::dto::ArcCenterRequest {
            center,
            start,
            sweep,
            ctrl_held,
            radius_mm: locked_radius,
            sweep_rad,
            ..
        } = *request;
        let radius_text = request.radius_text.as_deref();
        let angle_text = request.angle_text.as_deref();
        let (center, center_target) = self.snap_creation(center, ctrl_held);
        let lock = self.positive_input(radius_text, locked_radius)?;
        let (start, start_target) = match lock {
            Some(radius) => self.radius_locked_point(center, radius, start, ctrl_held),
            None => self.snap_creation(start, ctrl_held),
        };
        let (sweep, sweep_target) = match lock {
            Some(radius) => self.radius_locked_point(center, radius, sweep, ctrl_held),
            None => self.snap_creation(sweep, ctrl_held),
        };
        let radius = center.distance(start);
        if !radius.is_finite() || radius < MIN_LINE_LENGTH_MM {
            return Err(SessionError::DegenerateSegment);
        }
        let start_ray = (start.y - center.y).atan2(start.x - center.x);
        let sweep_ray = (sweep.y - center.y).atan2(sweep.x - center.x);

        let sweep_rad = if let Some(text) = angle_text {
            let degrees = self.eval_text(text)?;
            if !degrees.is_finite()
                || degrees.abs() > 360.0
                || degrees.abs().to_radians() <= MIN_ARC_TRAVEL_RAD
            {
                return Err(SessionError::InvalidConstraint(
                    "Arc angle must be nonzero and between -360 and 360 degrees".into(),
                ));
            }
            Some(degrees.to_radians())
        } else {
            if sweep_rad.is_some_and(|v| !v.is_finite()) {
                return Err(SessionError::DegenerateSegment);
            }
            sweep_rad
        };
        let signed_sweep = match sweep_rad {
            Some(travel) if travel.abs() > MIN_ARC_TRAVEL_RAD => {
                let magnitude = travel.abs().min(std::f64::consts::TAU);
                if travel < 0.0 {
                    -magnitude
                } else {
                    magnitude
                }
            }

            Some(_) => return Err(SessionError::DegenerateSegment),

            None => {
                let mut ccw = sweep_ray - start_ray;
                if ccw <= 0.0 {
                    ccw += std::f64::consts::TAU;
                }
                ccw
            }
        };
        let (start_angle, end_angle, sweep_is_start) = if signed_sweep < 0.0 {
            (start_ray + signed_sweep, start_ray, true)
        } else {
            (start_ray, start_ray + signed_sweep, false)
        };
        let start_position =
            center + Vec2::new(radius * start_angle.cos(), radius * start_angle.sin());
        let end_position = center + Vec2::new(radius * end_angle.cos(), radius * end_angle.sin());

        let sweep_position = if sweep_is_start {
            start_position
        } else {
            end_position
        };
        let sweep_target = if sweep.distance(sweep_position) <= MERGE_EPS {
            sweep_target
        } else {
            SnapTarget::None
        };
        let (stored_start_target, stored_end_target) = if sweep_is_start {
            (sweep_target, start_target)
        } else {
            (start_target, sweep_target)
        };

        Ok(ResolvedArc {
            center,
            radius,
            start_angle,
            end_angle,
            start_pick: (start_position, stored_start_target),
            end_pick: (end_position, stored_end_target),
            center_target,
            mid_pick: None,
            signed_sweep,
            lock,
        })
    }

    pub(super) fn resolve_three_point_arc(
        &self,
        request: &crate::dto::Arc3PointRequest,
    ) -> Result<ResolvedArc, SessionError> {
        let crate::dto::Arc3PointRequest {
            p1,
            p2,
            p3,
            ctrl_held,
        } = *request;
        let (p1, p1_target) = self.snap_creation(p1, ctrl_held);
        let (p2, p2_target) = self.snap_creation(p2, ctrl_held);
        let (p3, p3_target) = self.snap_creation(p3, ctrl_held);
        let d = 2.0 * (p1.x * (p2.y - p3.y) + p2.x * (p3.y - p1.y) + p3.x * (p1.y - p2.y));
        if !d.is_finite() || d.abs() < MERGE_EPS {
            return Err(SessionError::InvalidConstraint(
                "Choose three distinct, non-collinear points for the arc".into(),
            ));
        }
        let (a2, b2, c2) = (p1.dot(p1), p2.dot(p2), p3.dot(p3));
        let ux = (a2 * (p2.y - p3.y) + b2 * (p3.y - p1.y) + c2 * (p1.y - p2.y)) / d;
        let uy = (a2 * (p3.x - p2.x) + b2 * (p1.x - p3.x) + c2 * (p2.x - p1.x)) / d;
        let center = Vec2::new(ux, uy);
        let radius = center.distance(p1);
        if !radius.is_finite() || radius < MIN_LINE_LENGTH_MM {
            return Err(SessionError::DegenerateSegment);
        }
        let ang = |p: Vec2| (p.y - center.y).atan2(p.x - center.x);
        let (a0, a1, am) = (ang(p1), ang(p3), ang(p2));

        let ccw_contains = |s: f64, e: f64, m: f64| {
            let span = (e - s).rem_euclid(std::f64::consts::TAU);
            let off = (m - s).rem_euclid(std::f64::consts::TAU);
            off <= span
        };
        let (start_angle, end_angle, start_pick, end_pick) = if ccw_contains(a0, a1, am) {
            (a0, a1, (p1, p1_target), (p3, p3_target))
        } else {
            (a1, a0, (p3, p3_target), (p1, p1_target))
        };

        Ok(ResolvedArc {
            center,
            radius,
            start_angle,
            end_angle,
            start_pick,
            end_pick,
            center_target: SnapTarget::None,
            mid_pick: Some((p2, p2_target)),
            signed_sweep: crate::geometry::arc_span(start_angle, end_angle),
            lock: None,
        })
    }

    pub(crate) fn positive_input(
        &self,
        text: Option<&str>,
        number: Option<f64>,
    ) -> Result<Option<f64>, SessionError> {
        let value = match text {
            Some(text) => Some(self.eval_text(text)?),
            None => number,
        };
        if value.is_some_and(|value| !value.is_finite() || value < MIN_LINE_LENGTH_MM) {
            return Err(SessionError::InvalidConstraint(
                "Enter a finite positive length".into(),
            ));
        }
        Ok(value)
    }

    pub(super) fn resolve_rectangle(
        &self,
        request: &LockedRectangleRequest,
    ) -> Result<ResolvedRectangle, SessionError> {
        let width = self.positive_input(request.width_text.as_deref(), request.width_mm)?;
        let height = self.positive_input(request.height_text.as_deref(), request.height_mm)?;
        let (anchor, anchor_target) = self.snap_creation(request.anchor, request.ctrl_held);
        let (hint, target) = self.snap_creation(request.corner_hint, request.ctrl_held);
        let divisor = if request.mode == RectangleMode::Center {
            2.0
        } else {
            1.0
        };
        let corner = Vec2::new(
            width.map_or(hint.x, |w| {
                anchor.x
                    + if hint.x >= anchor.x {
                        w / divisor
                    } else {
                        -w / divisor
                    }
            }),
            height.map_or(hint.y, |h| {
                anchor.y
                    + if hint.y >= anchor.y {
                        h / divisor
                    } else {
                        -h / divisor
                    }
            }),
        );
        Self::rectangle_corners(request.mode, anchor, corner)?;
        Ok(ResolvedRectangle {
            anchor,
            anchor_target,
            corner,
            corner_target: if corner.distance(hint) <= MERGE_EPS {
                target
            } else {
                SnapTarget::None
            },
            width,
            height,
        })
    }

    pub(super) fn rectangle_corners(
        mode: RectangleMode,
        p1: Vec2,
        p2: Vec2,
    ) -> Result<[Vec2; 4], SessionError> {
        let (min, max) = match mode {
            RectangleMode::TwoPoint => (
                Vec2::new(p1.x.min(p2.x), p1.y.min(p2.y)),
                Vec2::new(p1.x.max(p2.x), p1.y.max(p2.y)),
            ),
            RectangleMode::Center => {
                let extent = Vec2::new((p2.x - p1.x).abs(), (p2.y - p1.y).abs());
                (p1 - extent, p1 + extent)
            }
        };
        if ![p1.x, p1.y, p2.x, p2.y, min.x, min.y, max.x, max.y]
            .iter()
            .all(|v| v.is_finite())
            || max.x - min.x < MIN_LINE_LENGTH_MM
            || max.y - min.y < MIN_LINE_LENGTH_MM
        {
            return Err(SessionError::DegenerateSegment);
        }
        Ok([min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)])
    }

    pub(super) fn resolve_circle(
        &self,
        request: &LockedCircleRequest,
    ) -> Result<ResolvedCircle, SessionError> {
        let diameter =
            self.positive_input(request.diameter_text.as_deref(), request.diameter_mm)?;
        let (anchor, anchor_target) = self.snap_creation(request.anchor, request.ctrl_held);
        let divisor = if request.mode == CircleMode::CenterDiameter {
            2.0
        } else {
            1.0
        };
        let (edge, edge_target) = match diameter {
            Some(d) => {
                self.radius_locked_point(anchor, d / divisor, request.edge_hint, request.ctrl_held)
            }
            None => self.snap_creation(request.edge_hint, request.ctrl_held),
        };
        let center = if request.mode == CircleMode::CenterDiameter {
            anchor
        } else {
            (anchor + edge) * 0.5
        };
        let radius = center.distance(edge);
        if ![center.x, center.y, radius].iter().all(|v| v.is_finite())
            || radius < MIN_LINE_LENGTH_MM
        {
            return Err(SessionError::DegenerateSegment);
        }
        Ok(ResolvedCircle {
            anchor,
            anchor_target,
            edge,
            edge_target,
            diameter,
            center,
            radius,
        })
    }

    pub(super) fn resolve_slot(
        &self,
        request: &SlotRequest,
    ) -> Result<(SlotCapsule, Vec2, SnapTarget, f64), SessionError> {
        let (p1, _) = self.snap_creation(request.p1, request.ctrl_held);
        let (p2, _) = self.snap_creation(request.p2, request.ctrl_held);
        let (cursor, target) = self.snap_creation(request.cursor, request.ctrl_held);
        let locked = self.positive_input(request.width_text.as_deref(), request.width_mm)?;
        let d = p2 - p1;
        let len = d.length();
        if !len.is_finite() || len < MERGE_EPS {
            return Err(SessionError::DegenerateSegment);
        }
        let width = locked.unwrap_or(2.0 * d.perp().dot(cursor - p1).abs() / len);
        if !width.is_finite() || width < MIN_LINE_LENGTH_MM {
            return Err(SessionError::DegenerateSegment);
        }
        let (c1, c2) = match request.mode {
            SlotMode::CenterToCenter => (p1, p2),
            SlotMode::Overall => {
                if len <= width {
                    return Err(SessionError::InvalidConstraint(
                        "Overall slot width must be smaller than its outside length. Reduce the width or move the endpoints farther apart.".into(),
                    ));
                }
                let offset = d * (width / (2.0 * len));
                (p1 + offset, p2 - offset)
            }
            SlotMode::CenterPoint => (p2, p1 * 2.0 - p2),
        };
        let cap = slot_capsule(c1, c2, width).map_err(|_| SessionError::DegenerateSegment)?;

        let on_side = (d.perp().dot(cursor - p1).abs() / len - width / 2.0).abs() <= MERGE_EPS;
        Ok((
            cap,
            cursor,
            if on_side { target } else { SnapTarget::None },
            width,
        ))
    }

    pub fn preview_creation(
        &self,
        request: &CreationPreviewRequest,
    ) -> Result<CreationPreviewDto, SessionError> {
        let (curves, snapped_to, snap, values) = match request {
            CreationPreviewRequest::ArcCenter(_) | CreationPreviewRequest::Arc3Point(_) => {
                let r = match request {
                    CreationPreviewRequest::ArcCenter(request) => {
                        self.resolve_center_arc(request)?
                    }
                    CreationPreviewRequest::Arc3Point(request) => {
                        self.resolve_three_point_arc(request)?
                    }
                    _ => unreachable!(),
                };
                let pick = match request {
                    CreationPreviewRequest::Arc3Point(request) => {
                        self.snap_creation(request.p3, request.ctrl_held)
                    }
                    _ => {
                        if r.signed_sweep < 0.0 {
                            r.start_pick
                        } else {
                            r.end_pick
                        }
                    }
                };
                (
                    vec![PreviewCurve::Arc {
                        center: r.center,
                        radius: r.radius,
                        start_angle: r.start_angle,
                        end_angle: r.end_angle,
                    }],
                    pick.0,
                    pick.1,
                    vec![("radius", r.radius), ("angle", r.signed_sweep.to_degrees())],
                )
            }
            CreationPreviewRequest::Chamfer(request) => {
                let (_, _, distance, a, b) = self.resolve_chamfer(request)?;
                (
                    vec![PreviewCurve::Line { a, b }],
                    b,
                    SnapTarget::None,
                    vec![("distance", distance)],
                )
            }
            CreationPreviewRequest::Rectangle(request) => {
                let r = self.resolve_rectangle(request)?;
                let corners = Self::rectangle_corners(request.mode, r.anchor, r.corner)?;
                let curves = (0..4)
                    .map(|i| PreviewCurve::Line {
                        a: corners[i],
                        b: corners[(i + 1) % 4],
                    })
                    .collect();
                (
                    curves,
                    r.corner,
                    r.corner_target,
                    vec![
                        ("width", corners[1].x - corners[0].x),
                        ("height", corners[3].y - corners[0].y),
                    ],
                )
            }
            CreationPreviewRequest::Circle(request) => {
                let r = self.resolve_circle(request)?;
                (
                    vec![PreviewCurve::Circle {
                        center: r.center,
                        radius: r.radius,
                    }],
                    r.edge,
                    r.edge_target,
                    vec![("diameter", r.radius * 2.0)],
                )
            }
            CreationPreviewRequest::Slot(request) => {
                let (cap, cursor, target, width) = self.resolve_slot(request)?;
                let mut curves = vec![
                    PreviewCurve::Line {
                        a: cap.line1.a,
                        b: cap.line1.b,
                    },
                    PreviewCurve::Line {
                        a: cap.line2.a,
                        b: cap.line2.b,
                    },
                ];
                for arc in [cap.arc1, cap.arc2] {
                    curves.push(PreviewCurve::Arc {
                        center: arc.center,
                        radius: arc.radius,
                        start_angle: arc.start_angle,
                        end_angle: arc.end_angle,
                    });
                }
                (curves, cursor, target, vec![("width", width)])
            }
        };
        Ok(CreationPreviewDto {
            curves,
            snapped_to,
            snap,
            values: values
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        })
    }
}
