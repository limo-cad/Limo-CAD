//! Intersection (G451-style) cutter-center geometry. Offset analytic lines
//! and circles first; tessellate only the resulting centerline for stock
//! sweeps. Unsupported/vanishing joins fail closed, never round over silently.
use crate::{CamPlanError, Point2Dto as P};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

const EPS: f64 = 1e-7;
const JOIN_EPS: f64 = 5e-6;
const CHORD_ERROR: f64 = 0.0025;
const MAX_CHORDS: usize = 200_000;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ProfileMove {
    Line(P),
    Arc { center: P, clockwise: bool, to: P },
}

#[derive(Clone, Copy, Debug)]
struct Primitive {
    start: P,
    end: P,
    circle: Option<(P, f64, bool)>,
}

fn add(a: P, b: P) -> P {
    P::new(a.x + b.x, a.y + b.y)
}
fn sub(a: P, b: P) -> P {
    P::new(a.x - b.x, a.y - b.y)
}
fn mul(a: P, t: f64) -> P {
    P::new(a.x * t, a.y * t)
}
fn dot(a: P, b: P) -> f64 {
    a.x * b.x + a.y * b.y
}
fn cross(a: P, b: P) -> f64 {
    a.x * b.y - a.y * b.x
}
fn len(a: P) -> f64 {
    a.x.hypot(a.y)
}
fn unit(a: P) -> P {
    mul(a, 1.0 / len(a))
}
fn normal(a: P) -> P {
    P::new(-a.y, a.x)
}
fn err(reason: &str) -> CamPlanError {
    CamPlanError(format!("Intersection cutter compensation: {reason}. Use in-computer compensation or adjust the contour/leads."))
}
fn angle(a: P, b: P) -> f64 {
    cross(a, b).atan2(dot(a, b))
}
fn sweep(a: P, b: P, cw: bool) -> f64 {
    let delta = if cw { angle(b, a) } else { angle(a, b) };
    if delta <= EPS {
        delta + TAU
    } else {
        delta
    }
}

impl Primitive {
    fn tangent(self, at_end: bool) -> P {
        if let Some((center, _, cw)) = self.circle {
            mul(
                normal(unit(sub(
                    if at_end { self.end } else { self.start },
                    center,
                ))),
                if cw { -1.0 } else { 1.0 },
            )
        } else {
            unit(sub(self.end, self.start))
        }
    }
    fn offset(self, distance: f64) -> Result<Self, CamPlanError> {
        if let Some((center, radius, cw)) = self.circle {
            let r = radius + if cw { distance } else { -distance };
            if r <= EPS {
                return Err(err("the tool radius consumes or inverts an arc"));
            }
            Ok(Self {
                start: add(center, mul(unit(sub(self.start, center)), r)),
                end: add(center, mul(unit(sub(self.end, center)), r)),
                circle: Some((center, r, cw)),
            })
        } else {
            let shift = mul(normal(self.tangent(false)), distance);
            Ok(Self {
                start: add(self.start, shift),
                end: add(self.end, shift),
                circle: None,
            })
        }
    }
}

fn line_circle(line: Primitive, center: P, radius: f64) -> Vec<P> {
    let d = unit(sub(line.end, line.start));
    let foot = add(line.start, mul(d, dot(sub(center, line.start), d)));
    let h2 = radius * radius - dot(sub(foot, center), sub(foot, center));
    if h2 < -EPS {
        return vec![];
    }
    let h = h2.max(0.0).sqrt();
    vec![add(foot, mul(d, h)), sub(foot, mul(d, h))]
}

fn intersections(a: Primitive, b: Primitive) -> Vec<P> {
    match (a.circle, b.circle) {
        (None, None) => {
            let u = unit(sub(a.end, a.start));
            let v = unit(sub(b.end, b.start));
            let det = cross(u, v);
            if det.abs() < EPS {
                return vec![];
            }
            vec![add(a.start, mul(u, cross(sub(b.start, a.start), v) / det))]
        }
        (None, Some((c, r, _))) => line_circle(a, c, r),
        (Some((c, r, _)), None) => line_circle(b, c, r),
        (Some((ca, ra, _)), Some((cb, rb, _))) => {
            let d = len(sub(cb, ca));
            if d <= EPS || d > ra + rb + EPS || d < (ra - rb).abs() - EPS {
                return vec![];
            }
            let x = (ra * ra - rb * rb + d * d) / (2.0 * d);
            let h2 = ra * ra - x * x;
            if h2 < -EPS {
                return vec![];
            }
            let u = unit(sub(cb, ca));
            let foot = add(ca, mul(u, x));
            let side = mul(normal(u), h2.max(0.0).sqrt());
            vec![add(foot, side), sub(foot, side)]
        }
    }
}

/// Returns centerline vertices and the source move index for every chord.
pub(crate) fn intersection_path(
    start: P,
    moves: &[ProfileMove],
    radius: f64,
    left: bool,
) -> Result<(Vec<P>, Vec<usize>), CamPlanError> {
    if moves.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return Err(err("missing contour or radius"));
    }
    let mut previous = start;
    let mut originals = Vec::with_capacity(moves.len());
    for motion in moves {
        let (end, circle) = match *motion {
            ProfileMove::Line(to) => {
                if len(sub(to, previous)) <= EPS {
                    return Err(err("zero-length compensated line"));
                }
                (to, None)
            }
            ProfileMove::Arc {
                center,
                clockwise,
                to,
            } => {
                let r = len(sub(previous, center));
                if !r.is_finite() || r <= EPS || (len(sub(to, center)) - r).abs() > JOIN_EPS {
                    return Err(err("inconsistent compensated arc radius"));
                }
                (to, Some((center, r, clockwise)))
            }
        };
        if ![previous.x, previous.y, end.x, end.y]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(err("non-finite path"));
        }
        originals.push(Primitive {
            start: previous,
            end,
            circle,
        });
        previous = end;
    }
    let distance = if left { radius } else { -radius };
    let mut offsets = originals
        .iter()
        .map(|p| p.offset(distance))
        .collect::<Result<Vec<_>, _>>()?;
    for i in 1..offsets.len() {
        let turn = angle(originals[i - 1].tangent(true), originals[i].tangent(false));

        if turn * distance < 0.0 && turn.abs() > FRAC_PI_2 + 1e-5 {
            return Err(err(
                "outside turn exceeds the supported 90-degree intersection policy",
            ));
        }
        let a = offsets[i - 1];
        let b = offsets[i];
        let join = if len(sub(a.end, b.start)) <= JOIN_EPS && turn.abs() < 1e-5 {
            if b.circle.is_some() {
                b.start
            } else {
                a.end
            }
        } else {
            intersections(a, b)
                .into_iter()
                .filter(|p| p.x.is_finite() && p.y.is_finite())
                .min_by(|p, q| {
                    len(sub(*p, originals[i].start)).total_cmp(&len(sub(*q, originals[i].start)))
                })
                .ok_or_else(|| err("adjacent offset elements have no intersection"))?
        };
        if len(sub(join, originals[i].start)) > radius * 4.0 + JOIN_EPS {
            return Err(err("unbounded intersection join"));
        }
        offsets[i - 1].end = join;
        offsets[i].start = join;
    }
    let mut points = vec![offsets[0].start];
    let mut source = Vec::new();
    for (i, p) in offsets.iter().enumerate() {
        if let Some((center, r, cw)) = p.circle {
            let original = originals[i];
            let direction = if cw { -1.0 } else { 1.0 };
            let original_sweep = sweep(sub(original.start, center), sub(original.end, center), cw);
            let trim_start = direction * angle(sub(original.start, center), sub(p.start, center));
            let trim_end = direction * angle(sub(original.end, center), sub(p.end, center));
            let extent = original_sweep + trim_end - trim_start;
            if extent <= EPS
                || extent > TAU + EPS
                || trim_start.abs() > PI / 2.0 + EPS
                || trim_end.abs() > PI / 2.0 + EPS
            {
                return Err(err("intersection erases, reverses or wraps an arc"));
            }
            let step = (2.0 * (1.0 - CHORD_ERROR / r).clamp(-1.0, 1.0).acos()).min(0.5 / r);
            let count = (extent / step).ceil().max(1.0) as usize;
            if count > MAX_CHORDS || source.len().saturating_add(count) > MAX_CHORDS {
                return Err(err("centerline sampling exceeds its safety budget"));
            }
            let start_angle = (p.start.y - center.y).atan2(p.start.x - center.x);
            for j in 1..=count {
                let t = start_angle + direction * extent * j as f64 / count as f64;
                points.push(if j == count {
                    p.end
                } else {
                    P::new(center.x + r * t.cos(), center.y + r * t.sin())
                });
                source.push(i);
            }
        } else {
            if dot(sub(p.end, p.start), originals[i].tangent(false)) <= EPS {
                return Err(err("intersection consumes or reverses a line"));
            }
            if source.len() >= MAX_CHORDS {
                return Err(err("centerline sampling exceeds its safety budget"));
            }
            points.push(p.end);
            source.push(i);
        }
    }
    Ok((points, source))
}

/// Validate the rounded coordinates/centers that the native post will emit.
/// Formatting alone must not approve unsupported intersections or consumed
/// arcs that would later fail workpiece verification.
pub(crate) fn validate_posted_intersections(
    document: &crate::CamDocumentDto,
    program: &crate::CamProgramDto,
) -> Result<(), CamPlanError> {
    use crate::{CamArcPlane, CamCommandDto as C, Point3Dto, PostDialect};
    let has_arcs = program
        .commands
        .iter()
        .any(|c| matches!(c, C::Circular { .. }));
    let round =
        |p| crate::post::output_point_mm(p, document.units, PostDialect::Siemens828d, has_arcs);
    let mut original_position = None::<Point3Dto>;
    let mut tool = None;
    let mut side = None;
    let mut start = None::<Point3Dto>;
    let mut moves = Vec::new();
    for command in &program.commands {
        match command {
            C::ToolChange { tool_id, .. } => tool = document.tool(*tool_id),
            C::CutterCompensationOn { left } => {
                side = Some(*left);
                start = None;
                moves.clear();
            }
            C::CutterCompensationOff => {
                if let (Some(left), Some(start), Some(tool)) = (side.take(), start, tool) {
                    intersection_path(P::new(start.x, start.y), &moves, tool.diameter * 0.5, left)?;
                } else {
                    return Err(err("missing activation or tool"));
                }
            }
            _ => {}
        }
        if let Some(to_original) = command.endpoint() {
            let to = round(to_original);
            if side.is_some() {
                if let Some(start) = start {
                    if (to.z - start.z).abs() > JOIN_EPS {
                        return Err(err("compensated motion changes depth"));
                    }
                    moves.push(match command {
                        C::Linear { .. } => ProfileMove::Line(P::new(to.x, to.y)),
                        C::Circular {
                            center,
                            clockwise,
                            plane: CamArcPlane::Xy,
                            ..
                        } => {
                            let from = original_position.ok_or_else(|| err("arc has no start"))?;
                            let offsets =
                                round(Point3Dto::new(center.x - from.x, center.y - from.y, 0.0));
                            let from = round(from);
                            ProfileMove::Arc {
                                center: P::new(from.x + offsets.x, from.y + offsets.y),
                                clockwise: *clockwise,
                                to: P::new(to.x, to.y),
                            }
                        }
                        _ => return Err(err("only planar lines/circles are supported")),
                    });
                } else {
                    start = Some(to);
                }
            }
            original_position = Some(to_original);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn near(a: P, b: P) {
        assert!(len(sub(a, b)) < 1e-6, "{a:?} != {b:?}");
    }
    #[test]
    fn line_intersections_match_inside_and_outside_equations() {
        let path = [
            ProfileMove::Line(P::new(10.0, 0.0)),
            ProfileMove::Line(P::new(10.0, 10.0)),
        ];
        let (left, _) = intersection_path(P::new(0.0, 0.0), &path, 1.0, true).unwrap();
        near(left[1], P::new(9.0, 1.0));
        let (right, _) = intersection_path(P::new(0.0, 0.0), &path, 1.0, false).unwrap();
        near(right[1], P::new(11.0, -1.0));
        let path = [
            ProfileMove::Line(P::new(10.0, 0.0)),
            ProfileMove::Line(P::new(20.0, 10.0)),
        ];
        let (p, _) = intersection_path(P::new(0.0, 0.0), &path, 6.0, false).unwrap();
        near(p[1], P::new(10.0 + 6.0 * (PI / 8.0).tan(), -6.0));
        assert!((len(sub(p[1], P::new(10.0, 0.0))) - 6.0 / (PI / 8.0).cos()).abs() < 1e-8);
    }
    #[test]
    fn tangent_line_circle_line_offsets_preserve_exact_arc_radius() {
        let center = P::new(10.0, 5.0);
        let path = [
            ProfileMove::Line(P::new(10.0, 0.0)),
            ProfileMove::Arc {
                center,
                clockwise: false,
                to: P::new(15.0, 5.0),
            },
            ProfileMove::Line(P::new(15.0, 15.0)),
        ];
        let (p, source) = intersection_path(P::new(0.0, 0.0), &path, 1.0, true).unwrap();
        near(p[1], P::new(10.0, 1.0));
        near(*p.last().unwrap(), P::new(14.0, 15.0));
        for (i, src) in source.iter().enumerate().filter(|(_, src)| **src == 1) {
            assert_eq!(*src, 1);
            assert!((len(sub(p[i], center)) - 4.0).abs() < 1e-8);
            assert!((len(sub(p[i + 1], center)) - 4.0).abs() < 1e-8);
            let midpoint = mul(add(p[i], p[i + 1]), 0.5);
            assert!(4.0 - len(sub(midpoint, center)) <= CHORD_ERROR + 1e-8);
        }
    }
    #[test]
    fn non_tangent_line_circle_joins_use_circle_intersection_not_chord_miter() {
        let center = P::new(2.0, 0.0);
        let path = [
            ProfileMove::Line(P::new(0.0, 0.0)),
            ProfileMove::Arc {
                center,
                clockwise: true,
                to: P::new(2.0, 2.0),
            },
        ];
        let (p, _) = intersection_path(P::new(-5.0, 0.0), &path, 0.5, true).unwrap();
        near(p[1], P::new(2.0 - 6.0_f64.sqrt(), 0.5));
        for point in &p[1..] {
            assert!((len(sub(*point, center)) - 2.5).abs() < 1e-8);
        }
    }
    #[test]
    fn concentric_tangent_arcs_and_a_full_circle_keep_their_winding() {
        let center = P::new(0.0, 0.0);
        for path in [
            vec![
                ProfileMove::Arc {
                    center,
                    clockwise: false,
                    to: P::new(0.0, 5.0),
                },
                ProfileMove::Arc {
                    center,
                    clockwise: false,
                    to: P::new(-5.0, 0.0),
                },
            ],
            vec![ProfileMove::Arc {
                center,
                clockwise: false,
                to: P::new(5.0, 0.0),
            }],
        ] {
            let (p, _) = intersection_path(P::new(5.0, 0.0), &path, 1.0, true).unwrap();
            for point in p {
                assert!((len(point) - 4.0).abs() < 1e-8);
            }
        }
    }
    #[test]
    fn nonconcentric_circle_joins_satisfy_both_offset_circle_equations() {
        let centers = [P::new(0.0, -5.0), P::new(5.0, 0.0)];
        let path = [
            ProfileMove::Arc {
                center: centers[0],
                clockwise: false,
                to: P::new(0.0, 0.0),
            },
            ProfileMove::Arc {
                center: centers[1],
                clockwise: false,
                to: P::new(5.0, -5.0),
            },
        ];
        let (points, source) = intersection_path(P::new(5.0, -5.0), &path, 1.0, true).unwrap();

        let x = (5.0 - 7.0_f64.sqrt()) / 2.0;
        let joint = source.iter().position(|index| *index == 1).unwrap();
        near(points[joint], P::new(x, -x));
        for (i, index) in source.iter().enumerate() {
            assert!((len(sub(points[i], centers[*index])) - 4.0).abs() < 1e-8);
            assert!((len(sub(points[i + 1], centers[*index])) - 4.0).abs() < 1e-8);
        }
    }
    #[test]
    fn collapsed_arcs_and_pointed_outside_turns_fail_closed() {
        let arc = [ProfileMove::Arc {
            center: P::new(0.0, 0.0),
            clockwise: false,
            to: P::new(0.0, 1.0),
        }];
        assert!(intersection_path(P::new(1.0, 0.0), &arc, 1.0, true)
            .unwrap_err()
            .to_string()
            .contains("consumes"));
        let pointed = [
            ProfileMove::Line(P::new(10.0, 0.0)),
            ProfileMove::Line(P::new(0.0, 10.0)),
        ];
        assert!(intersection_path(P::new(0.0, 0.0), &pointed, 1.0, false)
            .unwrap_err()
            .to_string()
            .contains("90-degree"));
        let invalid = [ProfileMove::Arc {
            center: P::new(f64::NAN, 0.0),
            clockwise: false,
            to: P::new(0.0, 1.0),
        }];
        assert!(intersection_path(P::new(1.0, 0.0), &invalid, 0.5, true).is_err());
    }
}
