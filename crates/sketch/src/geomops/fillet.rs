//! Tangent fillet construction between line segments.
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
pub struct ArcSeg {
    pub center: Pt,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub ccw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilletError {
    Parallel,
    Degenerate,
    RadiusTooLarge,
    NotPositive,
}

pub struct FilletResult {
    pub arc: ArcSeg,
    pub tangent_on_l1: Pt,
    pub tangent_on_l2: Pt,
}

const EPS: f64 = 1e-9;
const PI: f64 = std::f64::consts::PI;
const TWO_PI: f64 = 2.0 * PI;

fn cross2d(a: Pt, b: Pt) -> f64 {
    a.x * b.y - a.y * b.x
}

fn dot2d(a: Pt, b: Pt) -> f64 {
    a.x * b.x + a.y * b.y
}

fn sub(a: Pt, b: Pt) -> Pt {
    Pt {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}

fn add(a: Pt, b: Pt) -> Pt {
    Pt {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}

fn scale(p: Pt, s: f64) -> Pt {
    Pt {
        x: p.x * s,
        y: p.y * s,
    }
}

fn len_sq(p: Pt) -> f64 {
    p.x * p.x + p.y * p.y
}

fn len(p: Pt) -> f64 {
    len_sq(p).sqrt()
}

fn normalize(p: Pt) -> Pt {
    let l = len(p);
    if l < EPS {
        return Pt { x: 0.0, y: 0.0 };
    }
    scale(p, 1.0 / l)
}

#[cfg(test)]
fn compute_arc_midpoint(arc: &ArcSeg) -> Pt {
    let start = arc.start_angle;
    let end = arc.end_angle;

    let sweep = if arc.ccw {
        let mut diff = end - start;
        while diff < 0.0 {
            diff += TWO_PI;
        }
        diff
    } else {
        let mut diff = start - end;
        while diff < 0.0 {
            diff += TWO_PI;
        }
        diff
    };

    let half_sweep = sweep / 2.0;
    let mid_angle = if arc.ccw {
        start + half_sweep
    } else {
        start - half_sweep
    };

    Pt {
        x: arc.center.x + arc.radius * mid_angle.cos(),
        y: arc.center.y + arc.radius * mid_angle.sin(),
    }
}

pub fn fillet_lines(l1: &LineSeg, l2: &LineSeg, radius: f64) -> Result<FilletResult, FilletError> {
    if radius <= 0.0 {
        return Err(FilletError::NotPositive);
    }

    let d1 = sub(l1.b, l1.a);
    let d2 = sub(l2.b, l2.a);

    if len_sq(d1) < EPS * EPS || len_sq(d2) < EPS * EPS {
        return Err(FilletError::Degenerate);
    }

    let cross = cross2d(d1, d2);
    if cross.abs() < EPS {
        return Err(FilletError::Parallel);
    }

    let rhs = sub(l2.a, l1.a);
    let t = cross2d(rhs, d2) / cross;
    let v = add(l1.a, scale(d1, t));

    let va1 = sub(l1.a, v);
    let vb1 = sub(l1.b, v);
    let dist_a1 = len(va1);
    let dist_b1 = len(vb1);

    let dir1 = if dist_b1 > dist_a1 + EPS {
        normalize(vb1)
    } else if dist_a1 > dist_b1 + EPS {
        normalize(va1)
    } else {
        normalize(vb1)
    };

    let va2 = sub(l2.a, v);
    let vb2 = sub(l2.b, v);
    let dist_a2 = len(va2);
    let dist_b2 = len(vb2);

    let dir2 = if dist_b2 > dist_a2 + EPS {
        normalize(vb2)
    } else if dist_a2 > dist_b2 + EPS {
        normalize(va2)
    } else {
        normalize(vb2)
    };

    let cos_theta = dot2d(dir1, dir2).clamp(-1.0, 1.0);
    let theta = cos_theta.acos();

    if !(EPS..=PI - EPS).contains(&theta) {
        return Err(FilletError::Parallel);
    }

    let half_theta = theta / 2.0;
    let tan_half = half_theta.tan();
    if tan_half.abs() < EPS {
        return Err(FilletError::Parallel);
    }

    let t_dist = radius / tan_half;

    if t_dist > dist_b1.max(dist_a1) + EPS {
        return Err(FilletError::RadiusTooLarge);
    }
    if t_dist > dist_b2.max(dist_a2) + EPS {
        return Err(FilletError::RadiusTooLarge);
    }

    let tangent_on_l1 = add(v, scale(dir1, t_dist));
    let tangent_on_l2 = add(v, scale(dir2, t_dist));

    let sin_half = half_theta.sin();
    if sin_half.abs() < EPS {
        return Err(FilletError::Parallel);
    }

    let center_dist = radius / sin_half;
    let bisector = normalize(add(dir1, dir2));
    let center = add(v, scale(bisector, center_dist));

    let start_angle = (tangent_on_l1.y - center.y).atan2(tangent_on_l1.x - center.x);
    let end_angle = (tangent_on_l2.y - center.y).atan2(tangent_on_l2.x - center.x);

    let minor_sweep = PI - theta;

    let mut diff_ccw = end_angle - start_angle;
    while diff_ccw < 0.0 {
        diff_ccw += TWO_PI;
    }
    while diff_ccw >= TWO_PI {
        diff_ccw -= TWO_PI;
    }

    let mut diff_cw = start_angle - end_angle;
    while diff_cw < 0.0 {
        diff_cw += TWO_PI;
    }
    while diff_cw >= TWO_PI {
        diff_cw -= TWO_PI;
    }

    let ccw = (diff_ccw - minor_sweep).abs() < (diff_cw - minor_sweep).abs();

    Ok(FilletResult {
        arc: ArcSeg {
            center,
            radius,
            start_angle,
            end_angle,
            ccw,
        },
        tangent_on_l1,
        tangent_on_l2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_pt_eq(a: Pt, b: Pt, eps: f64) {
        assert!(
            (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps,
            "Points not equal: {:?} vs {:?}, diff x={}, y={}",
            a,
            b,
            a.x - b.x,
            a.y - b.y
        );
    }

    #[test]
    fn test_fillet_90_deg_origin() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 10.0 },
        };
        let r = 2.0;

        let result = fillet_lines(&l1, &l2, r).unwrap();

        assert_pt_eq(result.tangent_on_l1, Pt { x: 2.0, y: 0.0 }, 1e-9);
        assert_pt_eq(result.tangent_on_l2, Pt { x: 0.0, y: 2.0 }, 1e-9);
        assert_pt_eq(result.arc.center, Pt { x: 2.0, y: 2.0 }, 1e-9);
        assert!(!result.arc.ccw, "ccw should be false");

        let mid = compute_arc_midpoint(&result.arc);
        assert_pt_eq(
            mid,
            Pt {
                x: 0.585786437627,
                y: 0.585786437627,
            },
            1e-6,
        );
    }

    #[test]
    fn test_fillet_90_deg_corner() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 10.0, y: 0.0 },
            b: Pt { x: 10.0, y: 10.0 },
        };
        let r = 3.0;

        let result = fillet_lines(&l1, &l2, r).unwrap();

        assert_pt_eq(result.tangent_on_l1, Pt { x: 7.0, y: 0.0 }, 1e-9);
        assert_pt_eq(result.tangent_on_l2, Pt { x: 10.0, y: 3.0 }, 1e-9);
        assert_pt_eq(result.arc.center, Pt { x: 7.0, y: 3.0 }, 1e-9);
        assert!(result.arc.ccw, "ccw should be true");

        let mid = compute_arc_midpoint(&result.arc);
        assert_pt_eq(
            mid,
            Pt {
                x: 9.121320343560,
                y: 0.878679656440,
            },
            1e-6,
        );
    }

    #[test]
    fn test_radius_too_large() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 1.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 1.0 },
        };
        let r = 2.0;

        let result = fillet_lines(&l1, &l2, r);
        assert!(matches!(result, Err(FilletError::RadiusTooLarge)));
    }

    #[test]
    fn test_parallel() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 5.0 },
            b: Pt { x: 10.0, y: 5.0 },
        };
        let r = 1.0;

        let result = fillet_lines(&l1, &l2, r);
        assert!(matches!(result, Err(FilletError::Parallel)));
    }

    #[test]
    fn test_not_positive() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 10.0 },
        };
        let r = 0.0;

        let result = fillet_lines(&l1, &l2, r);
        assert!(matches!(result, Err(FilletError::NotPositive)));
    }

    #[test]
    fn test_degenerate() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let r = 1.0;

        let result = fillet_lines(&l1, &l2, r);
        assert!(matches!(result, Err(FilletError::Degenerate)));
    }
}
