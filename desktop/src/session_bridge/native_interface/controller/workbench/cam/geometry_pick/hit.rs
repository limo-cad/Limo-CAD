use super::*;

pub(super) struct Projected {
    pub points: Vec<Option<Vec2>>,
    /// Camera-forward depth at each sample. Coincident silhouettes must pick
    /// the front edge instead of depending on the B-rep's iteration order.
    pub depths: Vec<f64>,
}
fn fraction(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let ratio = ((point - a).dot(ab) / ab.length_squared()).clamp(0., 1.);
    if ratio.is_finite() {
        ratio
    } else {
        0.
    }
}
pub(super) fn distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    point.distance(a.lerp(b, fraction(point, a, b)))
}
pub(super) fn closest(
    candidates: &[worker::Candidate],
    projected: &[Projected],
    point: Vec2,
    closed_mode: bool,
    individual: bool,
) -> Option<usize> {
    let mut best = 10.0;
    let mut best_depth = f64::INFINITY;
    let mut result = None;
    for (index, (candidate, projected)) in candidates.iter().zip(projected).enumerate() {
        let n = projected.points.len();
        if n < 2 {
            continue;
        }
        let penalty = if closed_mode && !individual && !candidate.planar {
            0.5
        } else {
            0.
        };
        for i in 0..(n - 1 + usize::from(candidate.closed)) {
            let (Some(a), Some(b)) = (projected.points[i], projected.points[(i + 1) % n]) else {
                continue;
            };
            let score = distance(point, a, b) + penalty;
            let ratio = f64::from(fraction(point, a, b));
            let depth =
                1. / ((1. - ratio) / projected.depths[i] + ratio / projected.depths[(i + 1) % n]);
            if score <= 10.
                && (score < best - 0.01 || ((score - best).abs() <= 0.01 && depth <= best_depth))
            {
                best = score;
                best_depth = depth;
                result = Some(index);
            }
        }
    }
    result
}
pub(super) fn project(
    world: &World,
    owner: &DocumentContext,
    viewport: InterfaceRect,
    candidates: &[worker::Candidate],
) -> Result<Vec<Projected>, String> {
    let (_, camera) = native_viewport::interface_camera_snapshot(world);
    let origin = Vec3::from_array(camera.position).as_dvec3();
    let forward = (Vec3::from_array(camera.target).as_dvec3() - origin).normalize_or_zero();
    candidates
        .iter()
        .map(|candidate| {
            let points = candidate
                .points
                .iter()
                .map(|point| {
                    native_viewport::interface_world_point(world, &owner.document_id, *point).map(
                        |point| {
                            point.map(|p| {
                                Vec2::new(p[0] + viewport.x as f32, p[1] + viewport.y as f32)
                            })
                        },
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let depths = candidate
                .points
                .iter()
                .map(|point| (DVec3::from_array(*point) - origin).dot(forward))
                .collect();
            Ok(Projected { points, depths })
        })
        .collect()
}
