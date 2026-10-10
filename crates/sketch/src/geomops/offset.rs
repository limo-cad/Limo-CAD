//! Offset construction for sketch lines, arcs, and circles.
//!
//! The module-local `Pt` aliases the crate's `Vec2` so sketch operations share
//! the engine's geometry type.

pub type Pt = crate::geometry::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSeg {
    pub a: Pt,
    pub b: Pt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub center: Pt,
    pub radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcSeg {
    pub circle: Circle,
    pub start_angle: f64,
    pub end_angle: f64,
    pub ccw: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Line(LineSeg),
    Circle(Circle),
    Arc(ArcSeg),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetError {
    Degenerate,
    CollapseToPoint,
}

pub fn offset_curve(c: &Curve, distance: f64) -> Result<Curve, OffsetError> {
    match c {
        Curve::Line(line) => {
            let dx = line.b.x - line.a.x;
            let dy = line.b.y - line.a.y;
            let len_sq = dx * dx + dy * dy;

            if len_sq == 0.0 {
                return Err(OffsetError::Degenerate);
            }

            let len = len_sq.sqrt();

            let nx = -dy / len;
            let ny = dx / len;

            let offset_x = nx * distance;
            let offset_y = ny * distance;

            let new_a = Pt {
                x: line.a.x + offset_x,
                y: line.a.y + offset_y,
            };
            let new_b = Pt {
                x: line.b.x + offset_x,
                y: line.b.y + offset_y,
            };

            Ok(Curve::Line(LineSeg { a: new_a, b: new_b }))
        }
        Curve::Circle(circle) => {
            let new_radius = circle.radius + distance;
            if new_radius <= 1e-12 {
                return Err(OffsetError::CollapseToPoint);
            }
            Ok(Curve::Circle(Circle {
                center: circle.center,
                radius: new_radius,
            }))
        }
        Curve::Arc(arc) => {
            let new_radius = arc.circle.radius + distance;
            if new_radius <= 1e-12 {
                return Err(OffsetError::CollapseToPoint);
            }
            let new_circle = Circle {
                center: arc.circle.center,
                radius: new_radius,
            };
            Ok(Curve::Arc(ArcSeg {
                circle: new_circle,
                start_angle: arc.start_angle,
                end_angle: arc.end_angle,
                ccw: arc.ccw,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_pt_eq(a: Pt, b: Pt, eps: f64) {
        assert!((a.x - b.x).abs() < eps, "x mismatch: {} vs {}", a.x, b.x);
        assert!((a.y - b.y).abs() < eps, "y mismatch: {} vs {}", a.y, b.y);
    }

    fn assert_f64_eq(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() < eps, "{} vs {}", a, b);
    }

    #[test]
    fn test_line_offset_positive() {
        let line = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let curve = Curve::Line(line);
        let result = offset_curve(&curve, 2.0).unwrap();
        match result {
            Curve::Line(l) => {
                assert_pt_eq(l.a, Pt { x: 0.0, y: 2.0 }, 1e-9);
                assert_pt_eq(l.b, Pt { x: 10.0, y: 2.0 }, 1e-9);
            }
            _ => panic!("Expected Line"),
        }
    }

    #[test]
    fn test_line_offset_negative() {
        let line = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let curve = Curve::Line(line);
        let result = offset_curve(&curve, -2.0).unwrap();
        match result {
            Curve::Line(l) => {
                assert_pt_eq(l.a, Pt { x: 0.0, y: -2.0 }, 1e-9);
                assert_pt_eq(l.b, Pt { x: 10.0, y: -2.0 }, 1e-9);
            }
            _ => panic!("Expected Line"),
        }
    }

    #[test]
    fn test_line_vertical_offset() {
        let line = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 5.0 },
        };
        let curve = Curve::Line(line);
        let result = offset_curve(&curve, 1.0).unwrap();
        match result {
            Curve::Line(l) => {
                assert_pt_eq(l.a, Pt { x: -1.0, y: 0.0 }, 1e-9);
                assert_pt_eq(l.b, Pt { x: -1.0, y: 5.0 }, 1e-9);
            }
            _ => panic!("Expected Line"),
        }
    }

    #[test]
    fn test_circle_offset_outward() {
        let circle = Circle {
            center: Pt { x: 0.0, y: 0.0 },
            radius: 5.0,
        };
        let curve = Curve::Circle(circle);
        let result = offset_curve(&curve, 1.0).unwrap();
        match result {
            Curve::Circle(c) => {
                assert_f64_eq(c.radius, 6.0, 1e-9);
            }
            _ => panic!("Expected Circle"),
        }
    }

    #[test]
    fn test_circle_offset_inward() {
        let circle = Circle {
            center: Pt { x: 0.0, y: 0.0 },
            radius: 5.0,
        };
        let curve = Curve::Circle(circle);
        let result = offset_curve(&curve, -2.0).unwrap();
        match result {
            Curve::Circle(c) => {
                assert_f64_eq(c.radius, 3.0, 1e-9);
            }
            _ => panic!("Expected Circle"),
        }
    }

    #[test]
    fn test_circle_collapse() {
        let circle = Circle {
            center: Pt { x: 0.0, y: 0.0 },
            radius: 5.0,
        };
        let curve = Curve::Circle(circle);
        let result = offset_curve(&curve, -5.0);
        assert_eq!(result, Err(OffsetError::CollapseToPoint));
    }

    #[test]
    fn test_arc_offset() {
        let arc = ArcSeg {
            circle: Circle {
                center: Pt { x: 1.0, y: 1.0 },
                radius: 4.0,
            },
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
            ccw: true,
        };
        let curve = Curve::Arc(arc);
        let result = offset_curve(&curve, -1.0).unwrap();
        match result {
            Curve::Arc(a) => {
                assert_f64_eq(a.circle.radius, 3.0, 1e-9);
                assert_pt_eq(a.circle.center, Pt { x: 1.0, y: 1.0 }, 1e-9);
                assert_f64_eq(a.start_angle, 0.0, 1e-9);
                assert_f64_eq(a.end_angle, std::f64::consts::FRAC_PI_2, 1e-9);
                assert!(a.ccw);
            }
            _ => panic!("Expected Arc"),
        }
    }

    #[test]
    fn test_degenerate_line() {
        let line = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 0.0 },
        };
        let curve = Curve::Line(line);
        let result = offset_curve(&curve, 1.0);
        assert_eq!(result, Err(OffsetError::Degenerate));
    }
}
