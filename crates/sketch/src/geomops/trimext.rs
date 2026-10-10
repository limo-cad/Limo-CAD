//! Trim and extend intersections for sketch curves.
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

#[derive(Clone, Debug, PartialEq)]
pub struct LineTrim {
    /// Surviving connected pieces, ordered from the original `a` end toward
    /// `b`. A middle trim has two pieces.
    pub kept: Vec<LineSeg>,
    pub removed: LineSeg,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Line(LineSeg),
    Circle(Circle),
}

const EPS: f64 = 1e-9;

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

pub fn line_line(l1: &LineSeg, l2: &LineSeg) -> Option<(Pt, f64, f64)> {
    let dx1 = l1.b.x - l1.a.x;
    let dy1 = l1.b.y - l1.a.y;
    let dx2 = l2.b.x - l2.a.x;
    let dy2 = l2.b.y - l2.a.y;

    let det = dx1 * dy2 - dy1 * dx2;
    if approx_eq(det, 0.0) {
        return None;
    }

    let rhs_x = l2.a.x - l1.a.x;
    let rhs_y = l2.a.y - l1.a.y;

    let det_val = dx1 * dy2 - dy1 * dx2;

    if approx_eq(det_val, 0.0) {
        return None;
    }

    let t = (rhs_x * dy2 - rhs_y * dx2) / det_val;
    let u = (rhs_x * dy1 - rhs_y * dx1) / det_val;

    let pt = Pt {
        x: l1.a.x + t * dx1,
        y: l1.a.y + t * dy1,
    };

    Some((pt, t, u))
}

pub fn line_circle(l: &LineSeg, c: &Circle) -> Vec<(Pt, f64)> {
    let dx = l.b.x - l.a.x;
    let dy = l.b.y - l.a.y;

    let vx = l.a.x - c.center.x;
    let vy = l.a.y - c.center.y;

    let a = dx * dx + dy * dy;
    let b = 2.0 * (vx * dx + vy * dy);
    let cc = vx * vx + vy * vy - c.radius * c.radius;

    let discriminant = b * b - 4.0 * a * cc;

    if discriminant < -EPS {
        return Vec::new();
    }

    let sqrt_disc = discriminant.max(0.0).sqrt();
    let mut results = Vec::new();

    let t1 = (-b - sqrt_disc) / (2.0 * a);
    let t2 = (-b + sqrt_disc) / (2.0 * a);

    let pt1 = Pt {
        x: l.a.x + t1 * dx,
        y: l.a.y + t1 * dy,
    };
    results.push((pt1, t1));

    if !approx_eq(t1, t2) {
        let pt2 = Pt {
            x: l.a.x + t2 * dx,
            y: l.a.y + t2 * dy,
        };
        results.push((pt2, t2));
    }

    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    results
}

pub fn circle_circle(c1: &Circle, c2: &Circle) -> Vec<Pt> {
    let dx = c2.center.x - c1.center.x;
    let dy = c2.center.y - c1.center.y;
    let d_sq = dx * dx + dy * dy;
    let d = d_sq.sqrt();

    if approx_eq(d, 0.0) {
        return Vec::new();
    }

    let r1 = c1.radius;
    let r2 = c2.radius;

    if d > r1 + r2 + EPS {
        return Vec::new();
    }
    if d < (r1 - r2).abs() - EPS {
        return Vec::new();
    }

    let a_val = (r1 * r1 - r2 * r2 + d_sq) / (2.0 * d);
    let h_sq = r1 * r1 - a_val * a_val;

    let x2 = c1.center.x + a_val * (dx / d);
    let y2 = c1.center.y + a_val * (dy / d);

    let h = if h_sq < 0.0 { 0.0 } else { h_sq.sqrt() };

    let rx = -dy / d * h;
    let ry = dx / d * h;

    if h < EPS {
        vec![Pt { x: x2, y: y2 }]
    } else {
        vec![
            Pt {
                x: x2 + rx,
                y: y2 + ry,
            },
            Pt {
                x: x2 - rx,
                y: y2 - ry,
            },
        ]
    }
}

pub fn nearest_param_on_line(l: &LineSeg, p: Pt) -> f64 {
    let dx = l.b.x - l.a.x;
    let dy = l.b.y - l.a.y;
    let len_sq = dx * dx + dy * dy;

    if approx_eq(len_sq, 0.0) {
        return 0.0;
    }

    ((p.x - l.a.x) * dx + (p.y - l.a.y) * dy) / len_sq
}

pub fn trim_line_parts(l: &LineSeg, click: Pt, cuts: &[Pt]) -> Option<LineTrim> {
    let click_t = nearest_param_on_line(l, click);

    let mut valid_cuts: Vec<f64> = cuts
        .iter()
        .map(|c| nearest_param_on_line(l, *c))
        .filter(|&t| t > EPS && t < 1.0 - EPS)
        .collect();

    if valid_cuts.is_empty() {
        return None;
    }

    valid_cuts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    valid_cuts.dedup_by(|a, b| (*a - *b).abs() <= EPS);

    let lower_bound = valid_cuts
        .iter()
        .rev()
        .find(|&&t| t <= click_t + EPS)
        .copied()
        .unwrap_or(0.0);

    let upper_bound = valid_cuts
        .iter()
        .find(|&&t| t >= click_t - EPS)
        .copied()
        .unwrap_or(1.0);

    let at = |t: f64| Pt {
        x: l.a.x + t * (l.b.x - l.a.x),
        y: l.a.y + t * (l.b.y - l.a.y),
    };
    if upper_bound - lower_bound <= EPS {
        return None;
    }

    let removed = LineSeg {
        a: at(lower_bound),
        b: at(upper_bound),
    };
    let mut kept = Vec::with_capacity(2);
    if lower_bound > EPS {
        kept.push(LineSeg {
            a: l.a,
            b: removed.a,
        });
    }
    if upper_bound < 1.0 - EPS {
        kept.push(LineSeg {
            a: removed.b,
            b: l.b,
        });
    }
    Some(LineTrim { kept, removed })
}

/// Backward-compatible single-piece helper used by the leaf-module tests.
/// Integrators that need correct CAD trim semantics should use
/// [`trim_line_parts`].
#[cfg(test)]
pub fn trim_line(l: &LineSeg, click: Pt, cuts: &[Pt]) -> Option<LineSeg> {
    let result = trim_line_parts(l, click, cuts)?;
    result.kept.into_iter().max_by(|a, b| {
        let la = a.a.distance(a.b);
        let lb = b.a.distance(b.b);
        la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
    })
}

#[cfg(test)]
pub fn extend_line_to(l: &LineSeg, targets: &[Curve]) -> Option<LineSeg> {
    let len = ((l.b.x - l.a.x).powi(2) + (l.b.y - l.a.y).powi(2)).sqrt();
    if approx_eq(len, 0.0) {
        return None;
    }

    let max_ext = 100.0 * len;

    let dx = l.b.x - l.a.x;
    let dy = l.b.y - l.a.y;

    let mut best_extension: Option<(f64, f64, f64)> = None;

    for target in targets {
        match target {
            Curve::Line(l2) => {
                if let Some((_, t, _)) = line_line(l, l2) {
                    if t > 1.0 + EPS {
                        let ext_len = (t - 1.0) * len;
                        if ext_len <= max_ext + EPS
                            && (best_extension.is_none()
                                || ext_len < best_extension.unwrap().0 - EPS)
                        {
                            best_extension = Some((ext_len, 0.0, t));
                        }
                    }
                }
            }
            Curve::Circle(c) => {
                for (_pt, t) in line_circle(l, c) {
                    if t > 1.0 + EPS {
                        let ext_len = (t - 1.0) * len;
                        if ext_len <= max_ext + EPS
                            && (best_extension.is_none()
                                || ext_len < best_extension.unwrap().0 - EPS)
                        {
                            best_extension = Some((ext_len, 0.0, t));
                        }
                    }
                }
            }
        }
    }

    for target in targets {
        match target {
            Curve::Line(l2) => {
                if let Some((_, t, _)) = line_line(l, l2) {
                    if t < -EPS {
                        let ext_len = (-t) * len;
                        if ext_len <= max_ext + EPS {
                            let current_best = best_extension.unwrap_or((f64::MAX, 0.0, 1.0));
                            if ext_len < current_best.0 - EPS {
                                best_extension = Some((ext_len, t, 1.0));
                            }
                        }
                    }
                }
            }
            Curve::Circle(c) => {
                for (_pt, t) in line_circle(l, c) {
                    if t < -EPS {
                        let ext_len = (-t) * len;
                        if ext_len <= max_ext + EPS {
                            let current_best = best_extension.unwrap_or((f64::MAX, 0.0, 1.0));
                            if ext_len < current_best.0 - EPS {
                                best_extension = Some((ext_len, t, 1.0));
                            }
                        }
                    }
                }
            }
        }
    }

    match best_extension {
        Some((_, t_a, t_b)) => {
            let new_a = Pt {
                x: l.a.x + t_a * dx,
                y: l.a.y + t_a * dy,
            };
            let new_b = Pt {
                x: l.a.x + t_b * dx,
                y: l.a.y + t_b * dy,
            };
            Some(LineSeg { a: new_a, b: new_b })
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_pt_eq(p1: Pt, p2: Pt) {
        assert!(
            (p1.x - p2.x).abs() < EPS,
            "x mismatch: {} vs {}",
            p1.x,
            p2.x
        );
        assert!(
            (p1.y - p2.y).abs() < EPS,
            "y mismatch: {} vs {}",
            p1.y,
            p2.y
        );
    }

    fn assert_f64_eq(a: f64, b: f64) {
        assert!((a - b).abs() < EPS, "f64 mismatch: {} vs {}", a, b);
    }

    #[test]
    fn test_line_line_intersection() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 5.0, y: -1.0 },
            b: Pt { x: 5.0, y: 1.0 },
        };

        let res = line_line(&l1, &l2);
        assert!(res.is_some());
        let (pt, t1, t2) = res.unwrap();
        assert_pt_eq(pt, Pt { x: 5.0, y: 0.0 });
        assert_f64_eq(t1, 0.5);
        assert_f64_eq(t2, 0.5);
    }

    #[test]
    fn test_line_line_parallel() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 1.0 },
            b: Pt { x: 10.0, y: 1.0 },
        };

        assert!(line_line(&l1, &l2).is_none());
    }

    #[test]
    fn test_line_circle() {
        let l = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let c = Circle {
            center: Pt { x: 0.0, y: 0.0 },
            radius: 2.0,
        };

        let res = line_circle(&l, &c);
        assert_eq!(res.len(), 2);

        let (pt1, t1) = res[0];
        let (pt2, t2) = res[1];

        assert_pt_eq(pt1, Pt { x: -2.0, y: 0.0 });
        assert_f64_eq(t1, -0.2);

        assert_pt_eq(pt2, Pt { x: 2.0, y: 0.0 });
        assert_f64_eq(t2, 0.2);
    }

    #[test]
    fn line_circle_tolerance_keeps_near_tangent_intersections_finite() {
        let circle = Circle {
            center: Pt { x: 0.0, y: 0.0 },
            radius: 1.0,
        };
        for y in [1.0, 1.0 + EPS / 64.0] {
            let line = LineSeg {
                a: Pt { x: -1.0, y },
                b: Pt { x: 1.0, y },
            };
            let intersections = line_circle(&line, &circle);
            assert_eq!(intersections.len(), 1);
            let (point, parameter) = intersections[0];
            assert_pt_eq(point, Pt { x: 0.0, y });
            assert_f64_eq(parameter, 0.5);
        }
        let miss = LineSeg {
            a: Pt {
                x: -1.0,
                y: 1.0 + EPS,
            },
            b: Pt {
                x: 1.0,
                y: 1.0 + EPS,
            },
        };
        assert!(line_circle(&miss, &circle).is_empty());
    }

    #[test]
    fn test_circle_circle_intersections() {
        let c1 = Circle {
            center: Pt { x: 0.0, y: 0.0 },
            radius: 5.0,
        };
        let c2 = Circle {
            center: Pt { x: 6.0, y: 0.0 },
            radius: 5.0,
        };

        let res = circle_circle(&c1, &c2);
        assert_eq!(res.len(), 2);

        let mut pts = res;
        pts.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());

        assert_pt_eq(pts[0], Pt { x: 3.0, y: -4.0 });
        assert_pt_eq(pts[1], Pt { x: 3.0, y: 4.0 });
    }

    #[test]
    fn test_circle_circle_tangent() {
        let c1 = Circle {
            center: Pt { x: 0.0, y: 0.0 },
            radius: 2.0,
        };
        let c2 = Circle {
            center: Pt { x: 4.0, y: 0.0 },
            radius: 2.0,
        };

        let res = circle_circle(&c1, &c2);
        assert_eq!(res.len(), 1);
        assert_pt_eq(res[0], Pt { x: 2.0, y: 0.0 });
    }

    #[test]
    fn test_trim_line_case1() {
        let l = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let click = Pt { x: 5.0, y: 0.1 };
        let cuts = vec![Pt { x: 4.0, y: 0.0 }, Pt { x: 7.0, y: 0.0 }];

        let res = trim_line(&l, click, &cuts);
        assert!(res.is_some());
        let seg = res.unwrap();
        assert_pt_eq(seg.a, Pt { x: 0.0, y: 0.0 });
        assert_pt_eq(seg.b, Pt { x: 4.0, y: 0.0 });
    }

    #[test]
    fn test_trim_line_case2() {
        let l = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let click = Pt { x: 9.0, y: -0.1 };
        let cuts = vec![Pt { x: 4.0, y: 0.0 }, Pt { x: 7.0, y: 0.0 }];

        let res = trim_line(&l, click, &cuts);
        assert!(res.is_some());
        let seg = res.unwrap();
        assert_pt_eq(seg.a, Pt { x: 0.0, y: 0.0 });
        assert_pt_eq(seg.b, Pt { x: 7.0, y: 0.0 });
    }

    #[test]
    fn test_extend_line_to_circle() {
        let l = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 5.0, y: 0.0 },
        };
        let c = Circle {
            center: Pt { x: 10.0, y: 0.0 },
            radius: 2.0,
        };
        let targets = vec![Curve::Circle(c)];

        let res = extend_line_to(&l, &targets);
        assert!(res.is_some());
        let seg = res.unwrap();
        assert_pt_eq(seg.a, Pt { x: 0.0, y: 0.0 });
        assert_pt_eq(seg.b, Pt { x: 8.0, y: 0.0 });
    }

    #[test]
    fn test_extend_line_to_line() {
        let l = LineSeg {
            a: Pt { x: 5.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: -5.0 },
            b: Pt { x: 0.0, y: 5.0 },
        };
        let targets = vec![Curve::Line(l2)];

        let res = extend_line_to(&l, &targets);
        assert!(res.is_some());
        let seg = res.unwrap();
        assert_pt_eq(seg.a, Pt { x: 0.0, y: 0.0 });
        assert_pt_eq(seg.b, Pt { x: 10.0, y: 0.0 });
    }
}
