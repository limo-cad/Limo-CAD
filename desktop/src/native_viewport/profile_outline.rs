//! Profile strokes must leave room for a directly highlighted sketch line.
//! This changes display geometry only, never sketch/profile coordinates.

use bevy::math::DVec2;
use limo_cad_sketch::EntityDto;
use limo_cad_solid::{Point2Dto, ProfileCurveDto, ProfileLoopDto};

type Segment = [Point2Dto; 2];

/// Remove only the parts of a closed profile outline already represented by
/// direct hover/selection feedback. Profiles can merge adjacent collinear
/// entities into one edge, so skipping a whole profile edge is not sufficient.
pub(super) fn profile_outline_segments(
    points: &[Point2Dto],
    replacements: &[Segment],
) -> Vec<Segment> {
    let mut result = Vec::new();
    if points.len() < 2 {
        return result;
    }
    for index in 0..points.len() {
        result.extend(line_remainder(
            [points[index], points[(index + 1) % points.len()]],
            replacements,
        ));
    }
    result
}

fn line_remainder(segment: Segment, replacements: &[Segment]) -> Vec<Segment> {
    let mut pieces = vec![segment];
    for replacement in replacements {
        pieces = pieces
            .into_iter()
            .flat_map(|piece| subtract_overlap(piece, *replacement))
            .collect();
    }
    pieces
}

/// The profile owns its thin border, so the ordinary, wider sketch stroke
/// must only draw the part outside that border. Open tails remain visible.
#[derive(Debug, PartialEq)]
pub(super) enum BaseCurveRemainder {
    Complete,
    Lines(Vec<Segment>),
    Circular(Vec<[f64; 2]>),
}

pub(super) fn base_curve_remainder(
    entity: &EntityDto,
    profiles: &[&ProfileLoopDto],
) -> BaseCurveRemainder {
    if profiles.is_empty() {
        return BaseCurveRemainder::Complete;
    }
    match entity {
        EntityDto::Line { start, end, .. } => {
            let outlines = profiles
                .iter()
                .flat_map(|profile| profile_outline_segments(&profile.points, &[]))
                .collect::<Vec<_>>();
            BaseCurveRemainder::Lines(line_remainder(
                [
                    Point2Dto::new(start.x, start.y),
                    Point2Dto::new(end.x, end.y),
                ],
                &outlines,
            ))
        }
        EntityDto::Spline { tessellation, .. } => {
            let outlines = profiles
                .iter()
                .flat_map(|profile| profile_outline_segments(&profile.points, &[]))
                .collect::<Vec<_>>();
            BaseCurveRemainder::Lines(
                tessellation
                    .windows(2)
                    .flat_map(|pair| {
                        line_remainder(
                            [
                                Point2Dto::new(pair[0].x, pair[0].y),
                                Point2Dto::new(pair[1].x, pair[1].y),
                            ],
                            &outlines,
                        )
                    })
                    .collect(),
            )
        }
        EntityDto::Arc { center, .. } | EntityDto::Circle { center, .. } => {
            let tau = std::f64::consts::TAU;
            let (start, end) = match entity {
                EntityDto::Arc {
                    start_angle,
                    end_angle,
                    ..
                } => {
                    let mut sweep = end_angle - start_angle;
                    while sweep <= 0.0 {
                        sweep += tau;
                    }
                    (*start_angle, start_angle + sweep)
                }
                _ => (0.0, tau),
            };
            let angle = |point: Point2Dto| (point.y - center.y).atan2(point.x - center.x);
            let mut pieces = vec![[start, end]];
            for curve in profiles.iter().flat_map(|profile| &profile.curves) {
                let (id, sources) = match curve {
                    ProfileCurveDto::Line {
                        entity_id,
                        source_entity_ids,
                        ..
                    }
                    | ProfileCurveDto::Arc {
                        entity_id,
                        source_entity_ids,
                        ..
                    }
                    | ProfileCurveDto::Circle {
                        entity_id,
                        source_entity_ids,
                        ..
                    }
                    | ProfileCurveDto::Polyline {
                        entity_id,
                        source_entity_ids,
                        ..
                    } => (*entity_id, source_entity_ids),
                };
                if id != entity.id().0 && !sources.contains(&entity.id().0) {
                    continue;
                }
                match curve {
                    ProfileCurveDto::Circle { .. } => return BaseCurveRemainder::Circular(vec![]),
                    ProfileCurveDto::Arc {
                        start: a,
                        mid,
                        end: b,
                        ..
                    } => {
                        let (mut a, b, mid) = (angle(*a), angle(*b), angle(*mid));
                        let mut sweep = (b - a).rem_euclid(tau);

                        if (mid - a).rem_euclid(tau) > sweep {
                            a = b;
                            sweep = tau - sweep;
                        }
                        let a = start + (a - start).rem_euclid(tau);
                        for shift in [-tau, 0.0, tau] {
                            pieces = subtract_interval(&pieces, [a + shift, a + shift + sweep]);
                        }
                    }
                    _ => {}
                }
            }
            if pieces == vec![[start, end]] {
                BaseCurveRemainder::Complete
            } else {
                BaseCurveRemainder::Circular(pieces)
            }
        }
        _ => BaseCurveRemainder::Complete,
    }
}

fn subtract_interval(pieces: &[[f64; 2]], cut: [f64; 2]) -> Vec<[f64; 2]> {
    let mut result = Vec::new();
    for &[start, end] in pieces {
        let from = start.max(cut[0]);
        let to = end.min(cut[1]);
        if to <= from {
            result.push([start, end]);
            continue;
        }
        if from - start > 1.0e-10 {
            result.push([start, from]);
        }
        if end - to > 1.0e-10 {
            result.push([to, end]);
        }
    }
    result
}

fn subtract_overlap(segment: Segment, replacement: Segment) -> Vec<Segment> {
    let vector = |point: Point2Dto| DVec2::new(point.x, point.y);
    let start = vector(segment[0]);
    let delta = vector(segment[1]) - start;
    let replacement_start = vector(replacement[0]) - start;
    let replacement_end = vector(replacement[1]) - start;
    let length = delta.length();
    let replacement_length = (replacement_end - replacement_start).length();
    if length <= f64::EPSILON || replacement_length <= f64::EPSILON {
        return vec![segment];
    }

    let tolerance = length.max(replacement_length).max(1.0) * 1.0e-10;
    if delta.perp_dot(replacement_start).abs() > tolerance * length
        || delta.perp_dot(replacement_end).abs() > tolerance * length
    {
        return vec![segment];
    }
    let first = replacement_start.dot(delta) / delta.length_squared();
    let last = replacement_end.dot(delta) / delta.length_squared();
    let from = first.min(last).max(0.0);
    let to = first.max(last).min(1.0);
    if to <= from {
        return vec![segment];
    }
    let point_at = |ratio: f64| {
        let point = start + delta * ratio;
        Point2Dto::new(point.x, point.y)
    };
    let mut remaining = Vec::with_capacity(2);
    if from > 0.0 {
        remaining.push([segment[0], point_at(from)]);
    }
    if to < 1.0 {
        remaining.push([point_at(to), segment[1]]);
    }
    remaining
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Segment {
        [Point2Dto::new(x1, y1), Point2Dto::new(x2, y2)]
    }

    fn rectangle() -> Vec<Point2Dto> {
        vec![
            Point2Dto::new(0.0, 0.0),
            Point2Dto::new(10.0, 0.0),
            Point2Dto::new(10.0, 5.0),
            Point2Dto::new(0.0, 5.0),
        ]
    }

    #[test]
    fn inactive_feedback_keeps_the_complete_closed_outline() {
        assert_eq!(profile_outline_segments(&rectangle(), &[]).len(), 4);
    }

    #[test]
    fn direct_boundary_line_replaces_the_profile_stroke_in_either_direction() {
        for replacement in [line(0.0, 0.0, 10.0, 0.0), line(10.0, 0.0, 0.0, 0.0)] {
            assert_eq!(
                profile_outline_segments(&rectangle(), &[replacement]),
                vec![
                    line(10.0, 0.0, 10.0, 5.0),
                    line(10.0, 5.0, 0.0, 5.0),
                    line(0.0, 5.0, 0.0, 0.0),
                ],
            );
        }
    }

    #[test]
    fn selecting_part_of_a_merged_boundary_keeps_its_neighbors() {
        assert_eq!(
            subtract_overlap(line(0.0, 0.0, 10.0, 0.0), line(3.0, 0.0, 7.0, 0.0)),
            vec![line(0.0, 0.0, 3.0, 0.0), line(7.0, 0.0, 10.0, 0.0)],
        );
    }

    #[test]
    fn adjacent_hover_and_selection_leave_no_duplicate_stroke_between_them() {
        let segments = profile_outline_segments(
            &rectangle(),
            &[line(2.0, 0.0, 4.0, 0.0), line(4.0, 0.0, 6.0, 0.0)],
        );
        assert_eq!(segments[0], line(0.0, 0.0, 2.0, 0.0));
        assert_eq!(segments[1], line(6.0, 0.0, 10.0, 0.0));
        assert_eq!(segments.len(), 5);
    }

    #[test]
    fn crossing_nearby_touching_and_degenerate_lines_do_not_erase_an_edge() {
        let edge = line(0.0, 0.0, 10.0, 0.0);
        for replacement in [
            line(5.0, -2.0, 5.0, 2.0),
            line(0.0, 0.001, 10.0, 0.001),
            line(10.0, 0.0, 20.0, 0.0),
            line(5.0, 0.0, 5.0, 0.0),
        ] {
            assert_eq!(subtract_overlap(edge, replacement), vec![edge]);
        }
    }

    #[test]
    fn oblique_reversed_overlap_preserves_exact_uncovered_endpoints() {
        assert_eq!(
            subtract_overlap(line(10.0, 15.0, 0.0, 5.0), line(7.0, 12.0, 3.0, 8.0)),
            vec![line(10.0, 15.0, 7.0, 12.0), line(3.0, 8.0, 0.0, 5.0)],
        );
    }

    fn profile(curves: Vec<ProfileCurveDto>) -> ProfileLoopDto {
        ProfileLoopDto {
            index: 0,
            points: rectangle(),
            area: 50.0,
            parent_index: None,
            nesting_depth: 0,
            curves,
        }
    }

    fn entity(value: serde_json::Value) -> EntityDto {
        serde_json::from_value(value).unwrap()
    }

    fn circle() -> EntityDto {
        entity(serde_json::json!({
            "kind": "circle", "id": 7, "center": {"x": 0, "y": 0},
            "radius": 5, "fully_defined": false
        }))
    }

    fn arc_boundary(start: f64, mid: f64, end: f64) -> ProfileCurveDto {
        let point = |degrees: f64| {
            let angle = degrees.to_radians();
            Point2Dto::new(5.0 * angle.cos(), 5.0 * angle.sin())
        };
        ProfileCurveDto::Arc {
            entity_id: 7,
            source_entity_ids: vec![7],
            start: point(start),
            mid: point(mid),
            end: point(end),
        }
    }

    #[test]
    fn profile_border_removes_only_the_covered_part_of_an_extended_base_line() {
        let source = entity(serde_json::json!({
            "kind": "line", "id": 7, "start_id": 1, "end_id": 2,
            "start": {"x": -5, "y": 0}, "end": {"x": 15, "y": 0},
            "fully_defined": false
        }));
        let profile = profile(vec![]);
        assert_eq!(
            base_curve_remainder(&source, &[&profile]),
            BaseCurveRemainder::Lines(vec![line(-5.0, 0.0, 0.0, 0.0), line(10.0, 0.0, 15.0, 0.0)]),
        );
        assert_eq!(
            base_curve_remainder(&source, &[]),
            BaseCurveRemainder::Complete
        );
    }

    #[test]
    fn full_circular_profile_has_no_duplicate_base_circle_stroke() {
        let profile = profile(vec![ProfileCurveDto::Circle {
            entity_id: 7,
            source_entity_ids: vec![7],
            center: Point2Dto::new(0.0, 0.0),
            radius: 5.0,
        }]);
        assert_eq!(
            base_curve_remainder(&circle(), &[&profile]),
            BaseCurveRemainder::Circular(vec![])
        );
    }

    #[test]
    fn reversed_partial_arc_preserves_the_uncovered_half_of_its_base_circle() {
        let profile = profile(vec![arc_boundary(180.0, 90.0, 0.0)]);
        let BaseCurveRemainder::Circular(ranges) = base_curve_remainder(&circle(), &[&profile])
        else {
            panic!("expected the uncovered half-circle");
        };
        assert_eq!(ranges.len(), 1);
        assert!((ranges[0][0] - std::f64::consts::PI).abs() < 1.0e-10);
        assert!((ranges[0][1] - std::f64::consts::TAU).abs() < 1.0e-10);
    }

    #[test]
    fn adjacent_profile_arcs_collectively_replace_the_whole_base_circle() {
        let first = profile(vec![arc_boundary(0.0, 90.0, 180.0)]);
        let second = profile(vec![arc_boundary(180.0, 270.0, 360.0)]);
        assert_eq!(
            base_curve_remainder(&circle(), &[&first, &second]),
            BaseCurveRemainder::Circular(vec![])
        );
    }

    #[test]
    fn wrapped_arc_and_merged_source_ids_preserve_only_the_open_tail() {
        let source = entity(serde_json::json!({
            "kind": "arc", "id": 7, "center": {"x": 0, "y": 0}, "radius": 5,
            "start_angle": -std::f64::consts::FRAC_PI_2,
            "end_angle": std::f64::consts::FRAC_PI_2,
            "fully_defined": false
        }));
        let mut boundary = arc_boundary(270.0, 315.0, 360.0);
        if let ProfileCurveDto::Arc {
            entity_id,
            source_entity_ids,
            ..
        } = &mut boundary
        {
            *entity_id = 9;
            *source_entity_ids = vec![9, 7];
        }
        let profile = profile(vec![boundary]);
        let BaseCurveRemainder::Circular(ranges) = base_curve_remainder(&source, &[&profile])
        else {
            panic!("expected the open tail of the source arc");
        };
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0][0].abs() < 1.0e-10);
        assert!((ranges[0][1] - std::f64::consts::FRAC_PI_2).abs() < 1.0e-10);
    }

    #[test]
    fn unrelated_circle_keeps_its_original_stroke() {
        let profile = profile(vec![ProfileCurveDto::Circle {
            entity_id: 88,
            source_entity_ids: vec![88],
            center: Point2Dto::new(0.0, 0.0),
            radius: 5.0,
        }]);
        assert_eq!(
            base_curve_remainder(&circle(), &[&profile]),
            BaseCurveRemainder::Complete
        );
    }

    #[test]
    fn profile_border_preserves_open_spline_tails() {
        let source = entity(serde_json::json!({
            "kind": "spline", "id": 7, "points": [],
            "tessellation": [
                {"x": -5, "y": 0}, {"x": 5, "y": 0}, {"x": 15, "y": 0}
            ], "fully_defined": false
        }));
        let profile = profile(vec![]);
        assert_eq!(
            base_curve_remainder(&source, &[&profile]),
            BaseCurveRemainder::Lines(vec![line(-5.0, 0.0, 0.0, 0.0), line(10.0, 0.0, 15.0, 0.0)])
        );
    }
}
