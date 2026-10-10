//! Transient outlines only. Slot caps and fit splines reuse engine geometry;
//! committed entities always arrive from the authoritative model snapshot.

use super::{CreateTool, Draft};
use limo_cad_sketch::{slot_capsule, tessellate_spline, CircleMode, RectangleMode, SlotMode, Vec2};
use std::f64::consts::TAU;

type Segment = [Vec2; 2];

fn polyline(points: &[Vec2], out: &mut Vec<Segment>) {
    out.extend(points.windows(2).map(|p| [p[0], p[1]]));
}

fn arc(center: Vec2, radius: f64, start: f64, sweep: f64, out: &mut Vec<Segment>) {
    if !radius.is_finite() || radius <= 1e-9 || !sweep.is_finite() {
        return;
    }
    let count = ((sweep.abs() / TAU * 96.).ceil() as usize).clamp(8, 96);
    let points = (0..=count)
        .map(|i| {
            let a = start + sweep * i as f64 / count as f64;
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect::<Vec<_>>();
    polyline(&points, out);
}

impl Draft {
    pub(crate) fn outline(&self, cursor: Vec2) -> Vec<Segment> {
        self.outline_with_points(&self.points, cursor)
    }

    pub(crate) fn outline_with_points(&self, points: &[Vec2], cursor: Vec2) -> Vec<Segment> {
        let mut out = vec![];
        if !cursor.x.is_finite() || !cursor.y.is_finite() {
            return out;
        }
        let (Some(tool), Some(&p1)) = (self.tool, points.first()) else {
            return out;
        };
        match tool {
            CreateTool::Line => out.push([p1, cursor]),
            CreateTool::MidpointLine => out.push([p1 * 2. - cursor, cursor]),
            CreateTool::Point => {}
            CreateTool::Rectangle(mode) => {
                let p0 = if mode == RectangleMode::Center {
                    p1 * 2. - cursor
                } else {
                    p1
                };
                polyline(
                    &[
                        p0,
                        Vec2::new(cursor.x, p0.y),
                        cursor,
                        Vec2::new(p0.x, cursor.y),
                        p0,
                    ],
                    &mut out,
                );
            }
            CreateTool::Circle(mode) => {
                let (center, radius) = match mode {
                    CircleMode::CenterDiameter => (p1, p1.distance(cursor)),
                    CircleMode::TwoPoint => ((p1 + cursor) * 0.5, p1.distance(cursor) * 0.5),
                };
                arc(center, radius, 0., TAU, &mut out);
            }
            CreateTool::Arc3Point => {
                let Some(&p2) = points.get(1) else {
                    return vec![[p1, cursor]];
                };
                let p3 = cursor;
                let d = 2. * (p1.x * (p2.y - p3.y) + p2.x * (p3.y - p1.y) + p3.x * (p1.y - p2.y));
                if d.abs() < 1e-6 {
                    return vec![[p1, p2], [p2, p3]];
                }
                let (a, b, c) = (p1.dot(p1), p2.dot(p2), p3.dot(p3));
                let center = Vec2::new(
                    (a * (p2.y - p3.y) + b * (p3.y - p1.y) + c * (p1.y - p2.y)) / d,
                    (a * (p3.x - p2.x) + b * (p1.x - p3.x) + c * (p2.x - p1.x)) / d,
                );
                let angle = |p: Vec2| (p.y - center.y).atan2(p.x - center.x);
                let (start, end, middle) = (angle(p1), angle(p3), angle(p2));
                let ccw = (end - start).rem_euclid(TAU);
                let sweep = if (middle - start).rem_euclid(TAU) <= ccw {
                    ccw
                } else {
                    ccw - TAU
                };
                arc(center, center.distance(p1), start, sweep, &mut out);
            }
            CreateTool::ArcCenter => {
                let Some(&start) = points.get(1) else {
                    return vec![[p1, cursor]];
                };
                let a0 = (start.y - p1.y).atan2(start.x - p1.x);
                let a1 = (cursor.y - p1.y).atan2(cursor.x - p1.x);
                arc(
                    p1,
                    p1.distance(start),
                    a0,
                    (a1 - a0).rem_euclid(TAU),
                    &mut out,
                );
            }
            CreateTool::Slot(mode) => {
                let Some(&p2) = points.get(1) else {
                    return vec![[p1, cursor]];
                };
                let axis = p2 - p1;
                let length = axis.length();
                if length < 1e-9 {
                    return out;
                }
                let width =
                    2. * (axis.x * (cursor.y - p1.y) - axis.y * (cursor.x - p1.x)).abs() / length;
                let (c1, c2) = match mode {
                    SlotMode::CenterToCenter => (p1, p2),
                    SlotMode::CenterPoint => (p2, p1 * 2. - p2),
                    SlotMode::Overall if length > width => (
                        p1 + axis * (width / (2. * length)),
                        p2 - axis * (width / (2. * length)),
                    ),
                    SlotMode::Overall => return out,
                };
                if let Ok(cap) = slot_capsule(c1, c2, width) {
                    out.extend([[cap.line1.a, cap.line1.b], [cap.line2.a, cap.line2.b]]);
                    for cap in [cap.arc1, cap.arc2] {
                        arc(
                            cap.center,
                            cap.radius,
                            cap.start_angle,
                            cap.end_angle - cap.start_angle,
                            &mut out,
                        );
                    }
                }
            }
            CreateTool::Spline => {
                let mut points = points.to_vec();
                if points.last() != Some(&cursor) {
                    points.push(cursor);
                }
                polyline(&tessellate_spline(&points, 16), &mut out);
            }
        }
        out.retain(|line| line.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_point_arc_preview_follows_the_middle_pick_in_both_directions() {
        for mid in [Vec2::new(0., 10.), Vec2::new(0., -10.)] {
            let mut draft = Draft::default();
            draft.select(Some(CreateTool::Arc3Point));
            draft.prepare(Vec2::new(10., 0.), true).unwrap();
            draft.prepare(mid, true).unwrap();
            let outline = draft.outline(Vec2::new(-10., 0.));
            assert!(outline.iter().flatten().any(|p| p.distance(mid) < 1e-8));
            assert!(outline
                .iter()
                .flatten()
                .all(|p| (p.length() - 10.).abs() < 1e-8));
            assert!(outline.iter().flatten().all(|p| p.y * mid.y >= -1e-8));
        }
    }
    #[test]
    fn overall_slot_outline_keeps_the_requested_extents_and_closed_cap_junctions() {
        let mut draft = Draft::default();
        draft.select(Some(CreateTool::Slot(SlotMode::Overall)));
        draft.prepare(Vec2::ZERO, true).unwrap();
        draft.prepare(Vec2::new(40., 0.), true).unwrap();
        let outline = draft.outline(Vec2::new(20., 5.));
        for p in outline.iter().flatten() {
            assert!(p.x >= -1e-8 && p.x <= 40. + 1e-8 && p.y.abs() <= 5. + 1e-8);
        }
        for end in outline.iter().flatten() {
            assert!(
                outline
                    .iter()
                    .flatten()
                    .filter(|other| other.distance(*end) < 1e-8)
                    .count()
                    >= 2
            );
        }
        assert!(draft.outline(Vec2::new(20., 30.)).is_empty());
    }
}
