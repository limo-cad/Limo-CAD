//! Regular-polygon construction for inscribed and circumscribed modes.
//!
//! The module-local `Pt` aliases the crate's `Vec2` so sketch operations share
//! the engine's geometry type.

pub type Pt = crate::geometry::Vec2;

#[derive(Debug, PartialEq)]
pub enum PolyMode {
    Inscribed,
    Circumscribed,
}

#[derive(Debug, PartialEq)]
pub enum PolyError {
    TooFewSides,
    NotPositive,
}

pub fn regular_polygon(
    center: Pt,
    n: usize,
    radius: f64,
    rotation: f64,
    mode: PolyMode,
) -> Result<Vec<Pt>, PolyError> {
    if n < 3 {
        return Err(PolyError::TooFewSides);
    }
    if radius <= 0.0 {
        return Err(PolyError::NotPositive);
    }

    let effective_radius = match mode {
        PolyMode::Inscribed => radius,
        PolyMode::Circumscribed => radius / (std::f64::consts::PI / n as f64).cos(),
    };

    let mut vertices = Vec::with_capacity(n);
    for k in 0..n {
        let angle = rotation + 2.0 * std::f64::consts::PI * (k as f64) / (n as f64);
        let x = center.x + effective_radius * angle.cos();
        let y = center.y + effective_radius * angle.sin();
        vertices.push(Pt { x, y });
    }

    Ok(vertices)
}

#[cfg(test)]
pub fn polygon_from_edge(a: Pt, b: Pt, n: usize) -> Result<Vec<Pt>, PolyError> {
    if n < 3 {
        return Err(PolyError::TooFewSides);
    }
    if a == b {
        return Err(PolyError::NotPositive);
    }

    let edge_len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    let angle_ab = (b.y - a.y).atan2(b.x - a.x);

    let turn_angle = 2.0 * std::f64::consts::PI / n as f64;

    let mut vertices = Vec::with_capacity(n);
    vertices.push(a);
    vertices.push(b);

    let mut current_pt = b;
    let mut current_angle = angle_ab;

    for _ in 2..n {
        current_angle += turn_angle;
        let next_pt = Pt {
            x: current_pt.x + edge_len * current_angle.cos(),
            y: current_pt.y + edge_len * current_angle.sin(),
        };
        vertices.push(next_pt);
        current_pt = next_pt;
    }

    Ok(vertices)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-6;

    fn assert_pt_eq(actual: Pt, expected: Pt) {
        assert!(
            (actual.x - expected.x).abs() < EPS,
            "x mismatch: {} vs {}",
            actual.x,
            expected.x
        );
        assert!(
            (actual.y - expected.y).abs() < EPS,
            "y mismatch: {} vs {}",
            actual.y,
            expected.y
        );
    }

    #[test]
    fn test_inscribed_hexagon() {
        let v = regular_polygon(Pt { x: 0.0, y: 0.0 }, 6, 2.0, 0.0, PolyMode::Inscribed).unwrap();
        assert_eq!(v.len(), 6);
        assert_pt_eq(v[0], Pt { x: 2.0, y: 0.0 });
        assert_pt_eq(
            v[1],
            Pt {
                x: 1.0,
                y: 1.732050807568877,
            },
        );
    }

    #[test]
    fn test_circumscribed_square() {
        let v =
            regular_polygon(Pt { x: 0.0, y: 0.0 }, 4, 1.0, 0.0, PolyMode::Circumscribed).unwrap();
        let expected_r = std::f64::consts::SQRT_2;
        assert_pt_eq(
            v[0],
            Pt {
                x: expected_r,
                y: 0.0,
            },
        );

        for pt in &v {
            let r = (pt.x * pt.x + pt.y * pt.y).sqrt();
            assert!((r - expected_r).abs() < EPS);
        }
    }

    #[test]
    fn test_inscribed_triangle_rotated() {
        let v = regular_polygon(
            Pt { x: 1.0, y: 1.0 },
            3,
            1.0,
            std::f64::consts::FRAC_PI_2,
            PolyMode::Inscribed,
        )
        .unwrap();
        assert_pt_eq(v[0], Pt { x: 1.0, y: 2.0 });
    }

    #[test]
    fn test_polygon_from_edge_square() {
        let v = polygon_from_edge(Pt { x: 0.0, y: 0.0 }, Pt { x: 1.0, y: 0.0 }, 4).unwrap();
        assert_eq!(v.len(), 4);
        assert_pt_eq(v[0], Pt { x: 0.0, y: 0.0 });
        assert_pt_eq(v[1], Pt { x: 1.0, y: 0.0 });
        assert_pt_eq(v[2], Pt { x: 1.0, y: 1.0 });
        assert_pt_eq(v[3], Pt { x: 0.0, y: 1.0 });
    }

    #[test]
    fn test_polygon_from_edge_triangle() {
        let v = polygon_from_edge(Pt { x: 0.0, y: 0.0 }, Pt { x: 0.0, y: 2.0 }, 3).unwrap();
        assert_eq!(v.len(), 3);
        assert_pt_eq(v[0], Pt { x: 0.0, y: 0.0 });
        assert_pt_eq(v[1], Pt { x: 0.0, y: 2.0 });
        assert_pt_eq(
            v[2],
            Pt {
                x: -1.732050807568877,
                y: 1.0,
            },
        );
    }

    #[test]
    fn test_errors() {
        assert_eq!(
            regular_polygon(Pt { x: 0.0, y: 0.0 }, 2, 1.0, 0.0, PolyMode::Inscribed),
            Err(PolyError::TooFewSides)
        );
        assert_eq!(
            regular_polygon(Pt { x: 0.0, y: 0.0 }, 3, 0.0, 0.0, PolyMode::Inscribed),
            Err(PolyError::NotPositive)
        );
        assert_eq!(
            polygon_from_edge(Pt { x: 0.0, y: 0.0 }, Pt { x: 0.0, y: 0.0 }, 3),
            Err(PolyError::NotPositive)
        );
        assert_eq!(
            polygon_from_edge(Pt { x: 0.0, y: 0.0 }, Pt { x: 1.0, y: 0.0 }, 2),
            Err(PolyError::TooFewSides)
        );
    }
}
