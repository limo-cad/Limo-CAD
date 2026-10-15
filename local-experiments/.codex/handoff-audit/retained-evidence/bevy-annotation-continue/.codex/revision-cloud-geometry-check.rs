mod geometry {
pub(super) fn edge_distance(point: [f64; 2], [a, b]: [[f64; 2]; 2]) -> Option<f64> {
    if point.iter().chain(&a).chain(&b).any(|v| !v.is_finite()) {
        return None;
    }
    let delta = [b[0] - a[0], b[1] - a[1]];
    let length = delta[0].hypot(delta[1]);
    if !length.is_finite() || length < 1e-8 {
        return None;
    }
    let direction = delta.map(|v| v / length);
    let offset = [point[0] - a[0], point[1] - a[1]];
    let x = offset[0] * direction[0] + offset[1] * direction[1];
    let y = -offset[0] * direction[1] + offset[1] * direction[0];
    let count = (length / 5.).ceil().max(1.);
    let step = length / count;
    let radius = (step * 0.58).max(1.4);
    let height = (radius * radius - step * step * 0.25).sqrt();
    let half_angle = (step / (2. * radius)).asin();
    let nearest = (x / step).floor().clamp(0., count - 1.);
    let mut distance = f64::INFINITY;
    for i in [-1., 0., 1.].map(|d| (nearest + d).clamp(0., count - 1.)) {
        let left = i * step;
        let dx = x - (left + step * 0.5);
        let dy = y - height;
        distance = distance
            .min((x - left).hypot(y))
            .min((x - left - step).hypot(y));
        let angle = dy.atan2(dx);
        if (angle + std::f64::consts::FRAC_PI_2).abs() <= half_angle {
            distance = distance.min((dx.hypot(dy) - radius).abs());
        }
    }
    distance.is_finite().then_some(distance)
}

}
mod tests {
use super::geometry::*;
#[test]
fn revision_cloud_scallop_hit_uses_curved_stroke_and_rejects_empty_polygon_interior() {
    let edge = [[0., 0.], [20., 0.]];
    // 5 mm chord, radius 2.9 mm: the middle of the first scallop is
    // 2.9 - sqrt(2.9² - 2.5²) below the original straight polygon edge.
    let sagitta = 2.9 - (2.9_f64.powi(2) - 2.5_f64.powi(2)).sqrt();
    assert!(edge_distance([2.5, -sagitta], edge).unwrap() < 1e-10);
    assert!((edge_distance([2.5, -sagitta - 3.01], edge).unwrap() - 3.01).abs() < 1e-10);
    assert!(edge_distance([2.5, 4.], edge).unwrap() > 3.);
    assert!((edge_distance([-2., 0.], edge).unwrap() - 2.).abs() < 1e-10);
    assert!(edge_distance([20. + sagitta, 2.5], [[20., 0.], [20., 20.]]).unwrap() < 1e-10);
    assert!(edge_distance([17.5, sagitta], [[20., 0.], [0., 0.]]).unwrap() < 1e-10);
    for edge in [
        [[10., 10.], [80., 10.]],
        [[80., 10.], [80., 70.]],
        [[80., 70.], [10., 70.]],
        [[10., 70.], [10., 10.]],
    ] {
        assert!(edge_distance([45., 40.], edge).unwrap() > 3.);
    }
    assert!(edge_distance([1e11, 20.], [[0., 0.], [1e12, 0.]]).unwrap() > 3.);
    assert!(edge_distance([0., 0.], [[0., 0.], [0., 0.]]).is_none());
    assert!(edge_distance([f64::INFINITY, 0.], edge).is_none());
}


}
