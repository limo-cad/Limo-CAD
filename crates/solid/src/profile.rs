use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;

use crate::{Point2Dto, ProfileCurveDto};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment2 {
    pub id: u64,
    pub a: Point2Dto,
    pub b: Point2Dto,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileError {
    Empty,
    OpenChain(u64),
    Branch(Point2Dto),
    Degenerate,
    SelfIntersecting,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProfileError::Empty => write!(f, "sketch contains no closed profile curves"),
            ProfileError::OpenChain(id) => write!(f, "profile curve {id} belongs to an open chain"),
            ProfileError::Branch(p) => {
                write!(f, "profile branches near ({:.4}, {:.4})", p.x, p.y)
            }
            ProfileError::Degenerate => write!(f, "profile is degenerate"),
            ProfileError::SelfIntersecting => write!(f, "profile is self-intersecting"),
        }
    }
}

impl std::error::Error for ProfileError {}

/// Produce the minimal analytic boundary consumed by solid kernels while
/// leaving the editable sketch topology untouched.
///
/// Sketch solvers intentionally keep trimmed carriers and individual fillet
/// entities so dimensions, undo, and later edits remain reversible. At a
/// limiting condition those entities can resolve to null edges or several
/// consecutive pieces of the same analytic circle. B-rep kernels do better
/// with the equivalent canonical boundary: null pieces removed and compatible
/// co-circular arcs unified. Every returned curve retains all contributing
/// sketch entity IDs so callers can map the derived topology back to design
/// intent.
pub fn canonicalize_profile_curves(
    curves: &[ProfileCurveDto],
    linear_tolerance: f64,
) -> Vec<ProfileCurveDto> {
    let tolerance = linear_tolerance.max(1e-9);
    let mut canonical = curves
        .iter()
        .filter_map(|curve| normalize_profile_curve(curve, tolerance))
        .collect::<Vec<_>>();

    loop {
        let mut changed = false;
        let mut merged = Vec::with_capacity(canonical.len());
        for curve in canonical {
            if let Some(previous) = merged.pop() {
                if let Some(combined) = merge_compatible_curves(&previous, &curve, tolerance) {
                    merged.push(combined);
                    changed = true;
                } else {
                    merged.push(previous);
                    merged.push(curve);
                }
            } else {
                merged.push(curve);
            }
        }

        if merged.len() > 1 {
            let last = merged.last().cloned().unwrap();
            let first = merged.first().cloned().unwrap();
            if let Some(combined) = merge_compatible_curves(&last, &first, tolerance) {
                merged.pop();
                merged.remove(0);
                merged.insert(0, combined);
                changed = true;
            }
        }

        canonical = merged;
        if !changed {
            return canonical;
        }
    }
}

fn profile_curve_sources(entity_id: u64, sources: &[u64]) -> Vec<u64> {
    let mut result = if sources.is_empty() {
        vec![entity_id]
    } else {
        sources.to_vec()
    };
    if !result.contains(&entity_id) {
        result.push(entity_id);
    }
    result.sort_unstable();
    result.dedup();
    result
}

fn normalize_profile_curve(curve: &ProfileCurveDto, tolerance: f64) -> Option<ProfileCurveDto> {
    match curve {
        ProfileCurveDto::Line {
            entity_id,
            source_entity_ids,
            start,
            end,
        } => (point_is_finite(*start)
            && point_is_finite(*end)
            && dist2(*start, *end) > tolerance * tolerance)
            .then(|| ProfileCurveDto::Line {
                entity_id: *entity_id,
                source_entity_ids: profile_curve_sources(*entity_id, source_entity_ids),
                start: *start,
                end: *end,
            }),
        ProfileCurveDto::Arc {
            entity_id,
            source_entity_ids,
            start,
            mid,
            end,
        } => (point_is_finite(*start)
            && point_is_finite(*mid)
            && point_is_finite(*end)
            && (dist2(*start, *mid) > tolerance * tolerance
                || dist2(*mid, *end) > tolerance * tolerance))
            .then(|| ProfileCurveDto::Arc {
                entity_id: *entity_id,
                source_entity_ids: profile_curve_sources(*entity_id, source_entity_ids),
                start: *start,
                mid: *mid,
                end: *end,
            }),
        ProfileCurveDto::Circle {
            entity_id,
            source_entity_ids,
            center,
            radius,
        } => (point_is_finite(*center) && radius.is_finite() && *radius > tolerance).then(|| {
            ProfileCurveDto::Circle {
                entity_id: *entity_id,
                source_entity_ids: profile_curve_sources(*entity_id, source_entity_ids),
                center: *center,
                radius: *radius,
            }
        }),
        ProfileCurveDto::Polyline {
            entity_id,
            source_entity_ids,
            points,
        } => {
            if points.iter().any(|point| !point_is_finite(*point)) {
                return None;
            }
            let mut cleaned = Vec::with_capacity(points.len());
            for point in points {
                if cleaned
                    .last()
                    .is_none_or(|previous| dist2(*previous, *point) > tolerance * tolerance)
                {
                    cleaned.push(*point);
                }
            }
            (cleaned.len() >= 2).then(|| ProfileCurveDto::Polyline {
                entity_id: *entity_id,
                source_entity_ids: profile_curve_sources(*entity_id, source_entity_ids),
                points: cleaned,
            })
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ArcCircle {
    center: Point2Dto,
    radius: f64,
    start_angle: f64,
    sweep: f64,
}

fn arc_circle(start: Point2Dto, mid: Point2Dto, end: Point2Dto) -> Option<ArcCircle> {
    let determinant =
        2.0 * (start.x * (mid.y - end.y) + mid.x * (end.y - start.y) + end.x * (start.y - mid.y));
    if !determinant.is_finite() || determinant.abs() <= 1e-15 {
        return None;
    }
    let start_norm = start.x * start.x + start.y * start.y;
    let mid_norm = mid.x * mid.x + mid.y * mid.y;
    let end_norm = end.x * end.x + end.y * end.y;
    let center = Point2Dto::new(
        (start_norm * (mid.y - end.y)
            + mid_norm * (end.y - start.y)
            + end_norm * (start.y - mid.y))
            / determinant,
        (start_norm * (end.x - mid.x)
            + mid_norm * (start.x - end.x)
            + end_norm * (mid.x - start.x))
            / determinant,
    );
    let radius = dist2(center, start).sqrt();
    if !point_is_finite(center) || !radius.is_finite() || radius <= 1e-12 {
        return None;
    }

    let start_angle = (start.y - center.y).atan2(start.x - center.x);
    let mid_angle = (mid.y - center.y).atan2(mid.x - center.x);
    let end_angle = (end.y - center.y).atan2(end.x - center.x);
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
    let ccw_sweep = ccw(start_angle, end_angle);
    let mid_ccw = ccw(start_angle, mid_angle);
    let angular_epsilon = 1e-10;
    let sweep = if mid_ccw <= ccw_sweep + angular_epsilon {
        ccw_sweep
    } else {
        -ccw(end_angle, start_angle)
    };
    (sweep.abs() > angular_epsilon).then_some(ArcCircle {
        center,
        radius,
        start_angle,
        sweep,
    })
}

fn merge_same_circle_arcs(
    left: &ProfileCurveDto,
    right: &ProfileCurveDto,
    tolerance: f64,
) -> Option<ProfileCurveDto> {
    let (
        ProfileCurveDto::Arc {
            entity_id: left_id,
            source_entity_ids: left_sources,
            start: left_start,
            mid: left_mid,
            end: left_end,
        },
        ProfileCurveDto::Arc {
            entity_id: right_id,
            source_entity_ids: right_sources,
            start: right_start,
            mid: right_mid,
            end: right_end,
        },
    ) = (left, right)
    else {
        return None;
    };
    if dist2(*left_end, *right_start) > tolerance * tolerance {
        return None;
    }

    let left_circle = arc_circle(*left_start, *left_mid, *left_end)?;
    let right_circle = arc_circle(*right_start, *right_mid, *right_end)?;
    let geometry_tolerance = tolerance.max(left_circle.radius.max(right_circle.radius) * 1e-9);
    if dist2(left_circle.center, right_circle.center) > geometry_tolerance * geometry_tolerance
        || (left_circle.radius - right_circle.radius).abs() > geometry_tolerance
        || left_circle.sweep.signum() != right_circle.sweep.signum()
    {
        return None;
    }

    let combined_sweep = left_circle.sweep + right_circle.sweep;
    let radius = (left_circle.radius + right_circle.radius) * 0.5;
    let angular_tolerance = (geometry_tolerance / radius).clamp(1e-10, 1e-3);
    if combined_sweep.abs() > std::f64::consts::TAU + angular_tolerance {
        return None;
    }
    let center = Point2Dto::new(
        (left_circle.center.x + right_circle.center.x) * 0.5,
        (left_circle.center.y + right_circle.center.y) * 0.5,
    );
    let mut sources = profile_curve_sources(*left_id, left_sources);
    sources.extend(profile_curve_sources(*right_id, right_sources));
    sources.sort_unstable();
    sources.dedup();
    let entity_id = *sources.first()?;

    if (combined_sweep.abs() - std::f64::consts::TAU).abs() <= angular_tolerance {
        return Some(ProfileCurveDto::Circle {
            entity_id,
            source_entity_ids: sources,
            center,
            radius,
        });
    }

    let mid_angle = left_circle.start_angle + combined_sweep * 0.5;
    Some(ProfileCurveDto::Arc {
        entity_id,
        source_entity_ids: sources,
        start: *left_start,
        mid: Point2Dto::new(
            center.x + radius * mid_angle.cos(),
            center.y + radius * mid_angle.sin(),
        ),
        end: *right_end,
    })
}

fn merge_compatible_curves(
    left: &ProfileCurveDto,
    right: &ProfileCurveDto,
    tolerance: f64,
) -> Option<ProfileCurveDto> {
    merge_collinear_lines(left, right, tolerance)
        .or_else(|| merge_same_circle_arcs(left, right, tolerance))
}

fn merge_collinear_lines(
    left: &ProfileCurveDto,
    right: &ProfileCurveDto,
    tolerance: f64,
) -> Option<ProfileCurveDto> {
    let (
        ProfileCurveDto::Line {
            entity_id: left_id,
            source_entity_ids: left_sources,
            start: left_start,
            end: left_end,
        },
        ProfileCurveDto::Line {
            entity_id: right_id,
            source_entity_ids: right_sources,
            start: right_start,
            end: right_end,
        },
    ) = (left, right)
    else {
        return None;
    };
    if dist2(*left_end, *right_start) > tolerance * tolerance {
        return None;
    }
    let left_vector = Point2Dto::new(left_end.x - left_start.x, left_end.y - left_start.y);
    let right_vector = Point2Dto::new(right_end.x - right_start.x, right_end.y - right_start.y);
    let left_length = dist2(*left_start, *left_end).sqrt();
    let right_length = dist2(*right_start, *right_end).sqrt();
    if left_length <= tolerance || right_length <= tolerance {
        return None;
    }

    let normalized_cross = cross(left_vector, right_vector) / (left_length * right_length);
    let normalized_dot = (left_vector.x * right_vector.x + left_vector.y * right_vector.y)
        / (left_length * right_length);
    let angular_tolerance = (tolerance / left_length.min(right_length)).clamp(1e-10, 1e-3);
    if normalized_cross.abs() > angular_tolerance || normalized_dot <= 0.0 {
        return None;
    }
    let offset = orientation(*left_start, *left_end, *right_end).abs() / left_length;
    if offset > tolerance {
        return None;
    }

    let mut sources = profile_curve_sources(*left_id, left_sources);
    sources.extend(profile_curve_sources(*right_id, right_sources));
    sources.sort_unstable();
    sources.dedup();
    Some(ProfileCurveDto::Line {
        entity_id: *sources.first()?,
        source_entity_ids: sources,
        start: *left_start,
        end: *right_end,
    })
}

fn dist2(a: Point2Dto, b: Point2Dto) -> f64 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2)
}

fn point_cmp(a: Point2Dto, b: Point2Dto) -> Ordering {
    a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y))
}

fn signed_area(points: &[Point2Dto]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>()
        * 0.5
}

fn orientation(a: Point2Dto, b: Point2Dto, c: Point2Dto) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn on_segment(a: Point2Dto, b: Point2Dto, p: Point2Dto, eps: f64) -> bool {
    orientation(a, b, p).abs() <= eps
        && p.x >= a.x.min(b.x) - eps
        && p.x <= a.x.max(b.x) + eps
        && p.y >= a.y.min(b.y) - eps
        && p.y <= a.y.max(b.y) + eps
}

fn segments_intersect(a: Point2Dto, b: Point2Dto, c: Point2Dto, d: Point2Dto, eps: f64) -> bool {
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);
    (o1 * o2 < -eps && o3 * o4 < -eps)
        || (o1.abs() <= eps && on_segment(a, b, c, eps))
        || (o2.abs() <= eps && on_segment(a, b, d, eps))
        || (o3.abs() <= eps && on_segment(c, d, a, eps))
        || (o4.abs() <= eps && on_segment(c, d, b, eps))
}

fn validate_simple(points: &[Point2Dto], eps: f64) -> Result<(), ProfileError> {
    let n = points.len();
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        for j in (i + 1)..n {
            if j == i || j == (i + 1) % n || (i == 0 && j == n - 1) {
                continue;
            }
            let c = points[j];
            let d = points[(j + 1) % n];
            if segments_intersect(a, b, c, d, eps) {
                return Err(ProfileError::SelfIntersecting);
            }
        }
    }
    Ok(())
}

fn point_is_finite(point: Point2Dto) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

fn cross(a: Point2Dto, b: Point2Dto) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Split each carrier segment wherever another sketch-segment endpoint lies
/// on its interior. For face discovery, also node proper interior crossings:
/// two crossing sketch curves form selectable planar regions even when the
/// sketch solver has not created an explicit coincident point there.
///
/// Sketch entities remain stable and unsplit; this is only the planar graph
/// used for profile discovery. Retaining the source segment ID on every piece
/// lets the caller recover the original analytic curve.
fn node_segments_impl(
    segments: &[Segment2],
    tolerance: f64,
    split_crossings: bool,
    ignore_degenerate: bool,
) -> Result<Vec<Segment2>, ProfileError> {
    let tolerance = tolerance.max(1e-9);
    let tol2 = tolerance * tolerance;
    let valid_segments = segments
        .iter()
        .copied()
        .filter_map(|segment| {
            let valid = point_is_finite(segment.a)
                && point_is_finite(segment.b)
                && dist2(segment.a, segment.b) > tol2;
            if valid {
                Some(Ok(segment))
            } else if ignore_degenerate {
                None
            } else {
                Some(Err(ProfileError::Degenerate))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if valid_segments.is_empty() {
        return Err(ProfileError::Empty);
    }
    let candidate_points = valid_segments
        .iter()
        .flat_map(|segment| [segment.a, segment.b])
        .collect::<Vec<_>>();
    let mut parameters = valid_segments
        .iter()
        .map(|_| vec![0.0, 1.0])
        .collect::<Vec<_>>();

    for (segment_index, segment) in valid_segments.iter().enumerate() {
        let dx = segment.b.x - segment.a.x;
        let dy = segment.b.y - segment.a.y;
        let length2 = dx * dx + dy * dy;
        let length = length2.sqrt();
        let parameter_tolerance = (tolerance / length).min(0.25);
        for point in &candidate_points {
            let parameter = ((point.x - segment.a.x) * dx + (point.y - segment.a.y) * dy) / length2;
            if parameter <= parameter_tolerance || parameter >= 1.0 - parameter_tolerance {
                continue;
            }
            let projected =
                Point2Dto::new(segment.a.x + parameter * dx, segment.a.y + parameter * dy);
            if dist2(projected, *point) <= tol2 {
                parameters[segment_index].push(parameter);
            }
        }
    }

    if split_crossings {
        for left_index in 0..valid_segments.len() {
            let left = valid_segments[left_index];
            let left_vector = Point2Dto::new(left.b.x - left.a.x, left.b.y - left.a.y);
            let left_length = dist2(left.a, left.b).sqrt();
            for right_index in (left_index + 1)..valid_segments.len() {
                let right = valid_segments[right_index];
                let right_vector = Point2Dto::new(right.b.x - right.a.x, right.b.y - right.a.y);
                let right_length = dist2(right.a, right.b).sqrt();
                let denominator = cross(left_vector, right_vector);

                let parallel_tolerance = tolerance * (left_length + right_length);
                if denominator.abs() <= parallel_tolerance {
                    continue;
                }
                let delta = Point2Dto::new(right.a.x - left.a.x, right.a.y - left.a.y);
                let left_parameter = cross(delta, right_vector) / denominator;
                let right_parameter = cross(delta, left_vector) / denominator;
                let left_parameter_tolerance = (tolerance / left_length).min(0.25);
                let right_parameter_tolerance = (tolerance / right_length).min(0.25);
                if left_parameter < -left_parameter_tolerance
                    || left_parameter > 1.0 + left_parameter_tolerance
                    || right_parameter < -right_parameter_tolerance
                    || right_parameter > 1.0 + right_parameter_tolerance
                {
                    continue;
                }
                let left_point = Point2Dto::new(
                    left.a.x + left_parameter * left_vector.x,
                    left.a.y + left_parameter * left_vector.y,
                );
                let right_point = Point2Dto::new(
                    right.a.x + right_parameter * right_vector.x,
                    right.a.y + right_parameter * right_vector.y,
                );
                if dist2(left_point, right_point) > tol2 * 4.0 {
                    continue;
                }
                if left_parameter > left_parameter_tolerance
                    && left_parameter < 1.0 - left_parameter_tolerance
                {
                    parameters[left_index].push(left_parameter.clamp(0.0, 1.0));
                }
                if right_parameter > right_parameter_tolerance
                    && right_parameter < 1.0 - right_parameter_tolerance
                {
                    parameters[right_index].push(right_parameter.clamp(0.0, 1.0));
                }
            }
        }
    }

    let mut noded = Vec::new();
    for (segment, parameters) in valid_segments.iter().zip(parameters.iter_mut()) {
        let dx = segment.b.x - segment.a.x;
        let dy = segment.b.y - segment.a.y;
        let length = dist2(segment.a, segment.b).sqrt();
        parameters.sort_by(f64::total_cmp);
        parameters.dedup_by(|left, right| (*left - *right).abs() * length <= tolerance);

        for pair in parameters.windows(2) {
            let a = Point2Dto::new(segment.a.x + pair[0] * dx, segment.a.y + pair[0] * dy);
            let b = Point2Dto::new(segment.a.x + pair[1] * dx, segment.a.y + pair[1] * dy);
            if dist2(a, b) > tol2 {
                noded.push(Segment2 {
                    id: segment.id,
                    a,
                    b,
                });
            }
        }
    }

    let mut unique = Vec::<Segment2>::new();
    for segment in noded {
        let duplicate = unique.iter_mut().find(|candidate| {
            (dist2(candidate.a, segment.a) <= tol2 && dist2(candidate.b, segment.b) <= tol2)
                || (dist2(candidate.a, segment.b) <= tol2 && dist2(candidate.b, segment.a) <= tol2)
        });
        if let Some(existing) = duplicate {
            existing.id = existing.id.min(segment.id);
        } else {
            unique.push(segment);
        }
    }
    Ok(unique)
}

fn node_segments_at_endpoints(
    segments: &[Segment2],
    tolerance: f64,
) -> Result<Vec<Segment2>, ProfileError> {
    node_segments_impl(segments, tolerance, false, false)
}

fn node_segments_for_faces(
    segments: &[Segment2],
    tolerance: f64,
) -> Result<Vec<Segment2>, ProfileError> {
    node_segments_impl(segments, tolerance, true, true)
}

/// Remove every edge that does not belong to a cycle in the active embedded
/// graph. Degree peeling alone is insufficient: a construction line can join
/// two otherwise valid closed profiles at both ends, making a bridge whose
/// endpoints each have degree three. A face walk then traverses that bridge
/// twice and incorrectly reports one self-intersecting loop instead of two
/// profile boundaries.
fn remove_bridges(active: &mut [bool], endpoints: &[[usize; 2]], vertex_count: usize) {
    fn visit(
        vertex: usize,
        parent_edge: Option<usize>,
        adjacency: &[Vec<(usize, usize)>],
        discovery: &mut [usize],
        low: &mut [usize],
        next_time: &mut usize,
        bridges: &mut [bool],
    ) {
        discovery[vertex] = *next_time;
        low[vertex] = *next_time;
        *next_time += 1;
        for &(next, edge) in &adjacency[vertex] {
            if Some(edge) == parent_edge {
                continue;
            }
            if discovery[next] == usize::MAX {
                visit(
                    next,
                    Some(edge),
                    adjacency,
                    discovery,
                    low,
                    next_time,
                    bridges,
                );
                low[vertex] = low[vertex].min(low[next]);
                if low[next] > discovery[vertex] {
                    bridges[edge] = true;
                }
            } else {
                low[vertex] = low[vertex].min(discovery[next]);
            }
        }
    }

    let mut adjacency = vec![Vec::<(usize, usize)>::new(); vertex_count];
    for (edge, [a, b]) in endpoints.iter().copied().enumerate() {
        if active[edge] {
            adjacency[a].push((b, edge));
            adjacency[b].push((a, edge));
        }
    }
    let mut discovery = vec![usize::MAX; vertex_count];
    let mut low = vec![usize::MAX; vertex_count];
    let mut bridges = vec![false; active.len()];
    let mut next_time = 0;
    for vertex in 0..vertex_count {
        if discovery[vertex] == usize::MAX && !adjacency[vertex].is_empty() {
            visit(
                vertex,
                None,
                &adjacency,
                &mut discovery,
                &mut low,
                &mut next_time,
                &mut bridges,
            );
        }
    }
    for (keep, bridge) in active.iter_mut().zip(bridges) {
        if bridge {
            *keep = false;
        }
    }
}

/// Extract deterministic CCW loops from unordered, possibly reversed curve
/// segments. Every clustered endpoint must have degree two.
pub fn extract_closed_loops(
    segments: &[Segment2],
    tolerance: f64,
) -> Result<Vec<Vec<Point2Dto>>, ProfileError> {
    if segments.is_empty() {
        return Err(ProfileError::Empty);
    }
    let tolerance = tolerance.max(1e-9);
    let tol2 = tolerance * tolerance;
    let mut ordered = node_segments_at_endpoints(segments, tolerance)?;
    ordered.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| point_cmp(left.a, right.a))
            .then_with(|| point_cmp(left.b, right.b))
    });
    if ordered
        .iter()
        .any(|segment| dist2(segment.a, segment.b) <= tol2)
    {
        return Err(ProfileError::Degenerate);
    }

    let mut vertices: Vec<Point2Dto> = Vec::new();
    let mut endpoints = Vec::with_capacity(ordered.len());
    for segment in &ordered {
        let mut ids = [0usize; 2];
        for (slot, point) in [segment.a, segment.b].into_iter().enumerate() {
            let index = vertices
                .iter()
                .position(|existing| dist2(*existing, point) <= tol2)
                .unwrap_or_else(|| {
                    vertices.push(point);
                    vertices.len() - 1
                });
            ids[slot] = index;
        }
        if ids[0] == ids[1] {
            return Err(ProfileError::Degenerate);
        }
        endpoints.push(ids);
    }

    let mut adjacency = vec![Vec::<usize>::new(); vertices.len()];
    for (segment_index, [a, b]) in endpoints.iter().copied().enumerate() {
        adjacency[a].push(segment_index);
        adjacency[b].push(segment_index);
    }
    for (vertex_index, incident) in adjacency.iter().enumerate() {
        match incident.len() {
            2 => {}
            0 => unreachable!(),
            1 => return Err(ProfileError::OpenChain(ordered[incident[0]].id)),
            _ => return Err(ProfileError::Branch(vertices[vertex_index])),
        }
    }

    let mut used = vec![false; ordered.len()];
    let mut loops = Vec::new();
    while let Some(first_segment) = used.iter().position(|value| !*value) {
        let [a, b] = endpoints[first_segment];
        let (start, mut current) = if point_cmp(vertices[a], vertices[b]).is_le() {
            (a, b)
        } else {
            (b, a)
        };
        let mut previous_segment = first_segment;
        used[first_segment] = true;
        let mut points = vec![vertices[start], vertices[current]];

        while current != start {
            let incident = &adjacency[current];
            let next_segment = if incident[0] == previous_segment {
                incident[1]
            } else {
                incident[0]
            };
            if used[next_segment] {
                return Err(ProfileError::Degenerate);
            }
            used[next_segment] = true;
            let [x, y] = endpoints[next_segment];
            current = if x == current { y } else { x };
            previous_segment = next_segment;
            if current != start {
                points.push(vertices[current]);
            }
        }

        if points.len() < 3 {
            return Err(ProfileError::Degenerate);
        }
        validate_simple(&points, tolerance)?;
        let area = signed_area(&points);
        if area.abs() <= tolerance * tolerance {
            return Err(ProfileError::Degenerate);
        }
        if area < 0.0 {
            points.reverse();
        }
        let first = (0..points.len())
            .min_by(|a, b| point_cmp(points[*a], points[*b]))
            .unwrap();
        points.rotate_left(first);
        loops.push(points);
    }
    loops.sort_by(|a, b| point_cmp(a[0], b[0]));
    Ok(loops)
}

/// One bounded face of the embedded segment graph together with the
/// provenance needed to decide whether it is a user profile.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedFace {
    /// CCW boundary polygon, rotated to start at its lexicographic minimum.
    pub points: Vec<Point2Dto>,
    /// Boundary edges carried by an authored (non-projected) segment. A face
    /// whose boundary is made only of projected support geometry is a real
    /// face of the planar subdivision, but the user never drew it: it must not
    /// become a selectable profile, and it must not turn authored geometry
    /// nested inside it into a hole.
    pub authored_edges: usize,
}

/// Extract bounded planar faces while permitting unrelated open sketch
/// geometry. Peeling vertices with degree below two removes line/path/rib
/// chains. The remaining embedded graph may still contain vertices of degree
/// three or more when adjacent regions share an edge or vertex, so each
/// directed half-edge is walked with the bounded face on its left.
///
/// `projected` marks the segment ids that came from projected support
/// geometry instead of an authored sketch entity. They participate in the
/// walk so a support edge can seal a region the user drew against it, and each
/// returned face reports how much authored geometry bounds it.
pub fn extract_bounded_faces(
    segments: &[Segment2],
    tolerance: f64,
    projected: &BTreeSet<u64>,
) -> Result<Vec<BoundedFace>, ProfileError> {
    if segments.is_empty() {
        return Err(ProfileError::Empty);
    }
    let tolerance = tolerance.max(1e-9);
    let tol2 = tolerance * tolerance;
    let mut ordered = node_segments_for_faces(segments, tolerance)?;
    ordered.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| point_cmp(left.a, right.a))
            .then_with(|| point_cmp(left.b, right.b))
    });
    let mut vertices = Vec::<Point2Dto>::new();
    let mut endpoints = Vec::with_capacity(ordered.len());
    for segment in &ordered {
        if dist2(segment.a, segment.b) <= tol2 {
            return Err(ProfileError::Degenerate);
        }
        let mut ids = [0usize; 2];
        for (slot, point) in [segment.a, segment.b].into_iter().enumerate() {
            ids[slot] = vertices
                .iter()
                .position(|existing| dist2(*existing, point) <= tol2)
                .unwrap_or_else(|| {
                    vertices.push(point);
                    vertices.len() - 1
                });
        }
        endpoints.push(ids);
    }

    let mut active = vec![true; ordered.len()];
    loop {
        let mut degree = vec![0usize; vertices.len()];
        for (index, [a, b]) in endpoints.iter().copied().enumerate() {
            if active[index] {
                degree[a] += 1;
                degree[b] += 1;
            }
        }
        let mut changed = false;
        for (index, [a, b]) in endpoints.iter().copied().enumerate() {
            if active[index] && (degree[a] < 2 || degree[b] < 2) {
                active[index] = false;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    remove_bridges(&mut active, &endpoints, vertices.len());

    if !active.iter().any(|keep| *keep) {
        return Err(ProfileError::Empty);
    }

    let half_endpoints = |half_edge: usize| {
        let [a, b] = endpoints[half_edge / 2];
        if half_edge.is_multiple_of(2) {
            (a, b)
        } else {
            (b, a)
        }
    };
    let mut outgoing = vec![Vec::<usize>::new(); vertices.len()];
    for (segment_index, [a, b]) in endpoints.iter().copied().enumerate() {
        if active[segment_index] {
            outgoing[a].push(segment_index * 2);
            outgoing[b].push(segment_index * 2 + 1);
        }
    }
    for (vertex_index, incident) in outgoing.iter_mut().enumerate() {
        incident.sort_by(|left, right| {
            let (_, left_to) = half_endpoints(*left);
            let (_, right_to) = half_endpoints(*right);
            let left_angle = (vertices[left_to].y - vertices[vertex_index].y)
                .atan2(vertices[left_to].x - vertices[vertex_index].x);
            let right_angle = (vertices[right_to].y - vertices[vertex_index].y)
                .atan2(vertices[right_to].x - vertices[vertex_index].x);
            left_angle
                .total_cmp(&right_angle)
                .then_with(|| ordered[*left / 2].id.cmp(&ordered[*right / 2].id))
                .then_with(|| left.cmp(right))
        });
    }

    let mut visited = vec![false; ordered.len() * 2];
    let mut loops = Vec::new();
    for start in 0..visited.len() {
        if !active[start / 2] || visited[start] {
            continue;
        }
        let mut current = start;
        let mut points = Vec::new();
        let mut authored_edges = 0usize;
        for _ in 0..=visited.len() {
            if visited[current] {
                if current != start {
                    return Err(ProfileError::Degenerate);
                }
                break;
            }
            visited[current] = true;
            if !projected.contains(&ordered[current / 2].id) {
                authored_edges += 1;
            }
            let (from, to) = half_endpoints(current);
            points.push(vertices[from]);

            let reverse = current ^ 1;
            let incident = &outgoing[to];
            let reverse_index = incident
                .iter()
                .position(|candidate| *candidate == reverse)
                .ok_or(ProfileError::Degenerate)?;
            current = incident[(reverse_index + incident.len() - 1) % incident.len()];
        }
        if current != start {
            return Err(ProfileError::Degenerate);
        }
        if points.len() < 3 {
            continue;
        }

        let area = signed_area(&points);

        if area <= tol2 {
            continue;
        }
        validate_simple(&points, tolerance)?;
        let first = (0..points.len())
            .min_by(|a, b| point_cmp(points[*a], points[*b]))
            .unwrap();
        points.rotate_left(first);
        loops.push(BoundedFace {
            points,
            authored_edges,
        });
    }

    if loops.is_empty() {
        return Err(ProfileError::Empty);
    }
    loops.sort_by(|a, b| {
        point_cmp(a.points[0], b.points[0])
            .then_with(|| signed_area(&b.points).total_cmp(&signed_area(&a.points)))
            .then_with(|| a.points.len().cmp(&b.points.len()))
    });
    Ok(loops)
}

/// Extract bounded planar faces from authored sketch geometry alone. Every
/// segment is authored, so the provenance carried by [`extract_bounded_faces`]
/// is not observable here.
pub fn extract_closed_loops_allow_open(
    segments: &[Segment2],
    tolerance: f64,
) -> Result<Vec<Vec<Point2Dto>>, ProfileError> {
    Ok(
        extract_bounded_faces(segments, tolerance, &BTreeSet::new())?
            .into_iter()
            .map(|face| face.points)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point2Dto {
        Point2Dto::new(x, y)
    }

    fn arc(
        entity_id: u64,
        center: Point2Dto,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> ProfileCurveDto {
        let point = |angle: f64| {
            p(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        };
        ProfileCurveDto::Arc {
            entity_id,
            source_entity_ids: vec![entity_id],
            start: point(start_angle),
            mid: point((start_angle + end_angle) * 0.5),
            end: point(end_angle),
        }
    }

    #[test]
    fn canonicalization_merges_adjacent_co_circular_fillets_with_provenance() {
        let curves = vec![
            ProfileCurveDto::Line {
                entity_id: 1,
                source_entity_ids: vec![1],
                start: p(0.0, 40.0),
                end: p(5e-7, 40.0),
            },
            arc(16, p(0.0, 30.0), 10.0, 0.0, std::f64::consts::FRAC_PI_2),
            arc(
                19,
                p(0.0, 30.0),
                10.0,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
            ),
        ];

        let result = canonicalize_profile_curves(&curves, 1e-3);
        assert_eq!(result.len(), 1);
        let ProfileCurveDto::Arc {
            source_entity_ids,
            start,
            mid,
            end,
            ..
        } = &result[0]
        else {
            panic!("two quarter arcs should become one semicircle");
        };
        assert_eq!(source_entity_ids, &[16, 19]);
        assert!(dist2(*start, p(10.0, 30.0)) < 1e-12);
        assert!(dist2(*mid, p(0.0, 40.0)) < 1e-12);
        assert!(dist2(*end, p(-10.0, 30.0)) < 1e-12);
    }

    #[test]
    fn canonicalization_merges_across_closed_loop_boundary() {
        let curves = vec![
            arc(
                2,
                p(0.0, 0.0),
                5.0,
                std::f64::consts::PI,
                std::f64::consts::PI * 1.5,
            ),
            ProfileCurveDto::Line {
                entity_id: 3,
                source_entity_ids: vec![3],
                start: p(0.0, -5.0),
                end: p(8.0, -5.0),
            },
            ProfileCurveDto::Line {
                entity_id: 4,
                source_entity_ids: vec![4],
                start: p(8.0, -5.0),
                end: p(8.0, 0.0),
            },
            ProfileCurveDto::Line {
                entity_id: 5,
                source_entity_ids: vec![5],
                start: p(8.0, 0.0),
                end: p(5.0, 0.0),
            },
            arc(1, p(0.0, 0.0), 5.0, 0.0, std::f64::consts::PI),
        ];

        let result = canonicalize_profile_curves(&curves, 1e-6);
        assert_eq!(result.len(), 4);
        assert!(result.iter().any(|curve| matches!(
            curve,
            ProfileCurveDto::Arc { source_entity_ids, .. } if source_entity_ids == &vec![1, 2]
        )));
    }

    #[test]
    fn canonicalization_does_not_merge_different_radii_or_directions() {
        let different_radius = canonicalize_profile_curves(
            &[
                arc(1, p(0.0, 0.0), 10.0, 0.0, std::f64::consts::FRAC_PI_2),
                arc(
                    2,
                    p(0.0, 0.001),
                    9.999,
                    std::f64::consts::FRAC_PI_2,
                    std::f64::consts::PI,
                ),
            ],
            1e-5,
        );
        assert_eq!(different_radius.len(), 2);

        let reverse = canonicalize_profile_curves(
            &[
                arc(1, p(0.0, 0.0), 10.0, 0.0, std::f64::consts::FRAC_PI_2),
                arc(2, p(0.0, 0.0), 10.0, std::f64::consts::FRAC_PI_2, 0.0),
            ],
            1e-5,
        );
        assert_eq!(reverse.len(), 2);
    }

    #[test]
    fn canonicalization_merges_collinear_lines_but_not_corners() {
        let line = |entity_id, start, end| ProfileCurveDto::Line {
            entity_id,
            source_entity_ids: vec![entity_id],
            start,
            end,
        };
        let result = canonicalize_profile_curves(
            &[
                line(7, p(0.0, 0.0), p(5.0, 0.0)),
                line(8, p(5.0, 0.0), p(10.0, 0.0)),
                line(9, p(10.0, 0.0), p(10.0, 4.0)),
            ],
            1e-6,
        );
        assert_eq!(result.len(), 2);
        assert!(matches!(
            &result[0],
            ProfileCurveDto::Line {
                source_entity_ids,
                start,
                end,
                ..
            } if source_entity_ids == &vec![7, 8]
                && dist2(*start, p(0.0, 0.0)) < 1e-12
                && dist2(*end, p(10.0, 0.0)) < 1e-12
        ));
        assert!(matches!(
            &result[1],
            ProfileCurveDto::Line { source_entity_ids, .. } if source_entity_ids == &vec![9]
        ));
    }

    #[test]
    fn four_quarter_arcs_become_one_full_circle() {
        let result = canonicalize_profile_curves(
            &[
                arc(1, p(0.0, 0.0), 4.0, 0.0, std::f64::consts::FRAC_PI_2),
                arc(
                    2,
                    p(0.0, 0.0),
                    4.0,
                    std::f64::consts::FRAC_PI_2,
                    std::f64::consts::PI,
                ),
                arc(
                    3,
                    p(0.0, 0.0),
                    4.0,
                    std::f64::consts::PI,
                    std::f64::consts::PI * 1.5,
                ),
                arc(
                    4,
                    p(0.0, 0.0),
                    4.0,
                    std::f64::consts::PI * 1.5,
                    std::f64::consts::TAU,
                ),
            ],
            1e-6,
        );
        assert_eq!(result.len(), 1);
        assert!(matches!(
            &result[0],
            ProfileCurveDto::Circle { source_entity_ids, .. }
                if source_entity_ids == &vec![1, 2, 3, 4]
        ));
    }

    fn s(id: u64, a: Point2Dto, b: Point2Dto) -> Segment2 {
        Segment2 { id, a, b }
    }

    #[test]
    fn shuffled_reversed_square_is_deterministic_ccw() {
        let loops = extract_closed_loops(
            &[
                s(4, p(1.0, 0.0), p(0.0, 0.0)),
                s(1, p(1.0, 1.0), p(1.0, 0.0)),
                s(3, p(0.0, 0.0), p(0.0, 1.0)),
                s(2, p(0.0, 1.0), p(1.0, 1.0)),
            ],
            1e-6,
        )
        .unwrap();
        assert_eq!(
            loops,
            vec![vec![p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)]]
        );
    }

    #[test]
    fn two_loops_sort_and_invalid_graphs_fail() {
        let loops = extract_closed_loops(
            &[
                s(5, p(10.0, 0.0), p(11.0, 0.0)),
                s(6, p(11.0, 0.0), p(11.0, 1.0)),
                s(7, p(11.0, 1.0), p(10.0, 1.0)),
                s(8, p(10.0, 1.0), p(10.0, 0.0)),
                s(1, p(0.0, 0.0), p(1.0, 0.0)),
                s(2, p(1.0, 0.0), p(1.0, 1.0)),
                s(3, p(1.0, 1.0), p(0.0, 1.0)),
                s(4, p(0.0, 1.0), p(0.0, 0.0)),
            ],
            1e-6,
        )
        .unwrap();
        assert_eq!(loops.len(), 2);
        assert_eq!(loops[0][0], p(0.0, 0.0));
        assert_eq!(loops[1][0], p(10.0, 0.0));

        assert!(matches!(
            extract_closed_loops(&[s(1, p(0.0, 0.0), p(1.0, 0.0))], 1e-6),
            Err(ProfileError::OpenChain(1))
        ));
        assert!(matches!(
            extract_closed_loops(
                &[
                    s(1, p(0.0, 0.0), p(1.0, 0.0)),
                    s(2, p(1.0, 0.0), p(0.0, 1.0)),
                    s(3, p(0.0, 1.0), p(0.0, 0.0)),
                    s(4, p(0.0, 0.0), p(-1.0, 0.0)),
                ],
                1e-6,
            ),
            Err(ProfileError::Branch(_))
        ));
    }

    #[test]
    fn bow_tie_is_rejected() {
        let result = extract_closed_loops(
            &[
                s(1, p(0.0, 0.0), p(1.0, 1.0)),
                s(2, p(1.0, 1.0), p(0.0, 1.0)),
                s(3, p(0.0, 1.0), p(1.0, 0.0)),
                s(4, p(1.0, 0.0), p(0.0, 0.0)),
            ],
            1e-6,
        );
        assert_eq!(result, Err(ProfileError::SelfIntersecting));
    }

    #[test]
    fn closed_loop_survives_unrelated_open_axis_line() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(1.0, 0.0), p(2.0, 0.0)),
                s(2, p(2.0, 0.0), p(2.0, 1.0)),
                s(3, p(2.0, 1.0), p(1.0, 1.0)),
                s(4, p(1.0, 1.0), p(1.0, 0.0)),
                s(5, p(0.0, -2.0), p(0.0, 2.0)),
            ],
            1e-6,
        )
        .unwrap();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0][0], p(1.0, 0.0));
    }

    #[test]
    fn closed_loop_survives_a_partially_coincident_attached_chain() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(0.0, 0.0), p(-15.0, 0.0)),
                s(2, p(-15.0, 0.0), p(-15.0, -7.5)),
                s(3, p(-15.0, -7.5), p(15.0, -7.5)),
                s(4, p(15.0, -7.5), p(15.0, 7.5)),
                s(5, p(15.0, 7.5), p(-15.0, 7.5)),
                s(6, p(-15.0, 7.5), p(-15.0, -7.5)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 1);
        assert!((signed_area(&loops[0]) - 450.0).abs() < 1e-9);
    }

    #[test]
    fn adjacent_regions_sharing_an_edge_are_distinct_faces() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(0.0, 1.0), p(1.0, 2.0)),
                s(2, p(1.0, 2.0), p(2.0, 1.0)),
                s(3, p(2.0, 1.0), p(0.0, 1.0)),
                s(4, p(0.0, 1.0), p(0.0, 0.0)),
                s(5, p(0.0, 0.0), p(2.0, 0.0)),
                s(6, p(2.0, 0.0), p(2.0, 1.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 2);
        assert!(loops.iter().all(|points| signed_area(points) > 0.0));
        assert_eq!(
            loops
                .iter()
                .map(|points| signed_area(points))
                .collect::<Vec<_>>(),
            vec![2.0, 1.0],
        );
    }

    fn projected(ids: &[u64]) -> BTreeSet<u64> {
        ids.iter().copied().collect()
    }

    /// Tessellate an arc into segments the way the sketch manager does.
    fn arc_segments(
        id: u64,
        center: Point2Dto,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> Vec<Segment2> {
        let steps = 16;
        let point = |index: usize| {
            let angle = start_angle + (end_angle - start_angle) * index as f64 / steps as f64;
            p(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        };
        (0..steps)
            .map(|index| s(id * 1_000 + index as u64, point(index), point(index + 1)))
            .collect()
    }

    fn boundary_square() -> Vec<Segment2> {
        vec![
            s(900, p(0.0, 0.0), p(10.0, 0.0)),
            s(901, p(10.0, 0.0), p(10.0, 10.0)),
            s(902, p(10.0, 10.0), p(0.0, 10.0)),
            s(903, p(0.0, 10.0), p(0.0, 0.0)),
        ]
    }

    #[test]
    fn projected_support_edges_seal_loops_without_becoming_profiles() {
        let square = boundary_square();
        let boundary = projected(&[900, 901, 902, 903]);

        let boundary_only = extract_bounded_faces(&square, 1e-6, &boundary).unwrap();
        assert_eq!(boundary_only.len(), 1);
        assert_eq!(boundary_only[0].authored_edges, 0);

        let authored = extract_bounded_faces(&square, 1e-6, &projected(&[])).unwrap();
        assert_eq!(authored.len(), 1);
        assert_eq!(authored[0].authored_edges, 4);

        let mut sealed = square;
        sealed.extend(arc_segments(
            4,
            p(5.0, 10.0),
            3.0,
            std::f64::consts::PI,
            std::f64::consts::TAU,
        ));
        let faces = extract_bounded_faces(&sealed, 1e-6, &boundary).unwrap();
        assert_eq!(faces.len(), 2);
        assert!(faces.iter().all(|face| face.authored_edges > 0));
        let areas = faces
            .iter()
            .map(|face| signed_area(&face.points).abs())
            .collect::<Vec<_>>();
        assert!(
            areas
                .iter()
                .any(|area| (area - std::f64::consts::PI * 9.0 / 2.0).abs() < 0.1),
            "the sealed half disc must be one of the faces: {areas:?}"
        );
    }

    #[test]
    fn authored_geometry_overlapping_a_projected_edge_stays_authored() {
        let mut segments = boundary_square();

        segments.push(s(4, p(0.0, 0.0), p(5.0, 0.0)));
        let faces =
            extract_bounded_faces(&segments, 1e-6, &projected(&[900, 901, 902, 903])).unwrap();
        assert_eq!(faces.len(), 1);
        assert!(
            faces[0].authored_edges > 0,
            "a coincident authored piece must keep authored provenance"
        );
    }

    #[test]
    fn regions_touching_at_one_vertex_are_distinct_faces() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(-2.0, -2.0), p(0.0, -2.0)),
                s(2, p(0.0, -2.0), p(0.0, 0.0)),
                s(3, p(0.0, 0.0), p(-2.0, 0.0)),
                s(4, p(-2.0, 0.0), p(-2.0, -2.0)),
                s(5, p(0.0, 0.0), p(2.0, 0.0)),
                s(6, p(2.0, 0.0), p(2.0, 2.0)),
                s(7, p(2.0, 2.0), p(0.0, 2.0)),
                s(8, p(0.0, 2.0), p(0.0, 0.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 2);
        assert!(loops
            .iter()
            .all(|points| (signed_area(points) - 4.0).abs() < 1e-9));
    }

    #[test]
    fn endpoint_on_edge_junctions_subdivide_an_outer_profile() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(0.0, 0.0), p(4.0, 0.0)),
                s(2, p(4.0, 0.0), p(4.0, 4.0)),
                s(3, p(4.0, 4.0), p(0.0, 4.0)),
                s(4, p(0.0, 4.0), p(0.0, 0.0)),
                s(5, p(2.0, 4.0), p(2.0, 2.0)),
                s(6, p(2.0, 2.0), p(4.0, 2.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 2);
        let mut areas = loops
            .iter()
            .map(|points| signed_area(points))
            .collect::<Vec<_>>();
        areas.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(areas, vec![4.0, 12.0]);
    }

    #[test]
    fn interior_curve_crossings_are_noded_into_selectable_regions() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(0.0, 0.0), p(4.0, 0.0)),
                s(2, p(4.0, 0.0), p(4.0, 4.0)),
                s(3, p(4.0, 4.0), p(0.0, 4.0)),
                s(4, p(0.0, 4.0), p(0.0, 0.0)),
                s(5, p(2.0, 0.0), p(2.0, 4.0)),
                s(6, p(0.0, 2.0), p(4.0, 2.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 4);
        assert!(loops
            .iter()
            .all(|points| (signed_area(points) - 4.0).abs() < 1e-9));
    }

    #[test]
    fn self_crossed_contour_is_resolved_into_its_bounded_faces() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(0.0, 0.0), p(2.0, 2.0)),
                s(2, p(2.0, 2.0), p(0.0, 2.0)),
                s(3, p(0.0, 2.0), p(2.0, 0.0)),
                s(4, p(2.0, 0.0), p(0.0, 0.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 2);
        assert!(loops
            .iter()
            .all(|points| (signed_area(points) - 1.0).abs() < 1e-9));
    }

    #[test]
    fn tiny_or_non_finite_strays_do_not_hide_valid_profiles() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(0.0, 0.0), p(3.0, 0.0)),
                s(2, p(3.0, 0.0), p(3.0, 2.0)),
                s(3, p(3.0, 2.0), p(0.0, 2.0)),
                s(4, p(0.0, 2.0), p(0.0, 0.0)),
                s(5, p(10.0, 10.0), p(10.0 + 1e-12, 10.0)),
                s(6, p(f64::NAN, 0.0), p(1.0, 0.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 1);
        assert!((signed_area(&loops[0]) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn bridge_between_two_closed_regions_is_not_a_profile_boundary() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(-4.0, -4.0), p(4.0, -4.0)),
                s(2, p(4.0, -4.0), p(4.0, 4.0)),
                s(3, p(4.0, 4.0), p(-4.0, 4.0)),
                s(4, p(-4.0, 4.0), p(-4.0, -4.0)),
                s(5, p(-1.0, -1.0), p(1.0, -1.0)),
                s(6, p(1.0, -1.0), p(1.0, 1.0)),
                s(7, p(1.0, 1.0), p(-1.0, 1.0)),
                s(8, p(-1.0, 1.0), p(-1.0, -1.0)),
                s(9, p(0.0, 4.0), p(0.0, 1.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 2);
        let mut areas = loops
            .iter()
            .map(|points| signed_area(points))
            .collect::<Vec<_>>();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas, vec![4.0, 64.0]);
    }

    #[test]
    fn chain_connecting_separate_closed_regions_at_both_ends_is_removed() {
        let loops = extract_closed_loops_allow_open(
            &[
                s(1, p(-5.0, -2.0), p(-1.0, -2.0)),
                s(2, p(-1.0, -2.0), p(-1.0, 2.0)),
                s(3, p(-1.0, 2.0), p(-5.0, 2.0)),
                s(4, p(-5.0, 2.0), p(-5.0, -2.0)),
                s(5, p(1.0, -2.0), p(5.0, -2.0)),
                s(6, p(5.0, -2.0), p(5.0, 2.0)),
                s(7, p(5.0, 2.0), p(1.0, 2.0)),
                s(8, p(1.0, 2.0), p(1.0, -2.0)),
                s(9, p(-1.0, 0.0), p(0.0, 0.0)),
                s(10, p(0.0, 0.0), p(1.0, 0.0)),
            ],
            1e-6,
        )
        .unwrap();

        assert_eq!(loops.len(), 2);
        assert!(loops
            .iter()
            .all(|points| (signed_area(points) - 16.0).abs() < 1e-9));
    }

    /// Exact reproduction of the profile graph published by the live desktop
    /// debug bridge for the 2026-08-12 arch-with-hole report. Two R10 fillets
    /// fully consume a 20 mm carrier, an open center line hangs from the arc
    /// junction, and a redundant line overlaps half the bottom edge. None of
    /// those non-material curves may hide either the outer region or its hole.
    #[test]
    fn arch_with_consumed_carrier_open_centerline_and_hole_has_two_loops() {
        let mut segments = vec![
            s(3_000, p(0.0, 0.0), p(-10.0, 0.0)),
            s(7_000, p(-10.0, 0.0), p(10.0, 0.0)),
            s(8_000, p(10.0, 0.0), p(10.0, 30.0)),
            s(10_000, p(-10.0, 30.0), p(-10.0, 0.0)),
            s(13_000, p(0.0, 40.0), p(0.0, 30.0)),
        ];
        let mut push_arc = |entity: u64, start: f64, end: f64, radius: f64| {
            let steps = 16;
            let points = (0..=steps)
                .map(|index| {
                    let angle = start + (end - start) * index as f64 / steps as f64;
                    p(radius * angle.cos(), 30.0 + radius * angle.sin())
                })
                .collect::<Vec<_>>();
            for (index, pair) in points.windows(2).enumerate() {
                segments.push(s(entity * 1_000 + index as u64, pair[0], pair[1]));
            }
        };
        push_arc(16, 0.0, std::f64::consts::FRAC_PI_2, 10.0);
        push_arc(19, std::f64::consts::FRAC_PI_2, std::f64::consts::PI, 10.0);
        let circle = (0..64)
            .map(|index| {
                let angle = std::f64::consts::TAU * index as f64 / 64.0;
                p(5.0 * angle.cos(), 30.0 + 5.0 * angle.sin())
            })
            .collect::<Vec<_>>();
        for (index, (a, b)) in circle
            .iter()
            .copied()
            .zip(circle.iter().copied().cycle().skip(1))
            .take(circle.len())
            .enumerate()
        {
            segments.push(s(20_000 + index as u64, a, b));
        }

        let loops = extract_closed_loops_allow_open(&segments, 1e-5).unwrap();
        assert_eq!(loops.len(), 2);
        let mut areas = loops
            .iter()
            .map(|points| signed_area(points))
            .collect::<Vec<_>>();
        areas.sort_by(f64::total_cmp);
        assert!(
            areas[0] > 70.0 && areas[0] < 80.0,
            "hole area: {}",
            areas[0]
        );
        assert!(
            areas[1] > 750.0 && areas[1] < 760.0,
            "outer area: {}",
            areas[1]
        );
    }
}
