//! Line-segment chamfer construction.
//!
//! The module-local `Pt` is aliased to the crate's `Vec2`, and the
//! finite-segment boundary uses the application's modeling tolerance so
//! constrained equality is not rejected as round-off.

pub type Pt = crate::geometry::Vec2;

/// Geometric comparisons must tolerate solver round-off. Values inside this
/// band are treated as the exact finite-segment boundary and clamped to the
/// endpoint; values beyond it are still rejected as oversized.
const DISTANCE_EPS: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineSeg {
    pub a: Pt,
    pub b: Pt,
}

#[derive(Debug, PartialEq)]
pub enum ChamferError {
    Parallel,
    Degenerate,
    DistanceTooLarge,
    NotPositive,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChamferResult {
    pub point_on_l1: Pt,
    pub point_on_l2: Pt,
}

fn pt_sub(a: Pt, b: Pt) -> Pt {
    Pt {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}

fn pt_add(a: Pt, b: Pt) -> Pt {
    Pt {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}

fn pt_mul(p: Pt, s: f64) -> Pt {
    Pt {
        x: p.x * s,
        y: p.y * s,
    }
}

fn pt_len(p: Pt) -> f64 {
    (p.x * p.x + p.y * p.y).sqrt()
}

fn pt_norm(p: Pt) -> Pt {
    let l = pt_len(p);
    if l < 1e-15 {
        return Pt { x: 0.0, y: 0.0 };
    }
    Pt {
        x: p.x / l,
        y: p.y / l,
    }
}

fn cross2d(a: Pt, b: Pt) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Compute intersection of two infinite lines defined by segments l1 and l2.
/// Returns None if parallel.
fn line_intersection(l1: &LineSeg, l2: &LineSeg) -> Option<Pt> {
    let p1 = l1.a;
    let r = pt_sub(l1.b, l1.a);
    let p2 = l2.a;
    let s = pt_sub(l2.b, l2.a);

    let rxs = cross2d(r, s);
    if rxs.abs() < 1e-15 {
        return None;
    }

    let q = pt_sub(p2, p1);
    let t = cross2d(q, s) / rxs;

    Some(pt_add(p1, pt_mul(r, t)))
}

pub fn chamfer_lines(
    l1: &LineSeg,
    l2: &LineSeg,
    d1: f64,
    d2: f64,
) -> Result<ChamferResult, ChamferError> {
    if d1 <= 0.0 || d2 <= 0.0 {
        return Err(ChamferError::NotPositive);
    }

    let len1 = pt_len(pt_sub(l1.b, l1.a));
    let len2 = pt_len(pt_sub(l2.b, l2.a));
    if len1 < 1e-15 || len2 < 1e-15 {
        return Err(ChamferError::Degenerate);
    }

    let v = match line_intersection(l1, l2) {
        Some(v) => v,
        None => return Err(ChamferError::Parallel),
    };

    let dist_va = pt_len(pt_sub(l1.a, v));
    let dist_vb = pt_len(pt_sub(l1.b, v));

    let dir1 = if dist_vb >= dist_va - 1e-15 {
        pt_norm(pt_sub(l1.b, v))
    } else {
        pt_norm(pt_sub(l1.a, v))
    };

    let dist_va2 = pt_len(pt_sub(l2.a, v));
    let dist_vb2 = pt_len(pt_sub(l2.b, v));

    let dir2 = if dist_vb2 >= dist_va2 - 1e-15 {
        pt_norm(pt_sub(l2.b, v))
    } else {
        pt_norm(pt_sub(l2.a, v))
    };

    let max_dist1 = if dist_vb >= dist_va - 1e-15 {
        dist_vb
    } else {
        dist_va
    };
    let max_dist2 = if dist_vb2 >= dist_va2 - 1e-15 {
        dist_vb2
    } else {
        dist_va2
    };

    if d1 > max_dist1 + DISTANCE_EPS {
        return Err(ChamferError::DistanceTooLarge);
    }
    if d2 > max_dist2 + DISTANCE_EPS {
        return Err(ChamferError::DistanceTooLarge);
    }

    let p1 = pt_add(v, pt_mul(dir1, d1.min(max_dist1)));
    let p2 = pt_add(v, pt_mul(dir2, d2.min(max_dist2)));

    Ok(ChamferResult {
        point_on_l1: p1,
        point_on_l2: p2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: Pt, b: Pt, eps: f64) -> bool {
        (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps
    }

    #[test]
    fn test_chamfer_basic() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 10.0 },
        };
        let res = chamfer_lines(&l1, &l2, 2.0, 3.0).unwrap();
        assert!(approx_eq(res.point_on_l1, Pt { x: 2.0, y: 0.0 }, 1e-9));
        assert!(approx_eq(res.point_on_l2, Pt { x: 0.0, y: 3.0 }, 1e-9));
    }

    #[test]
    fn test_chamfer_tiebreak() {
        let l1 = LineSeg {
            a: Pt { x: -5.0, y: 0.0 },
            b: Pt { x: 5.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: -5.0 },
            b: Pt { x: 0.0, y: 5.0 },
        };
        let res = chamfer_lines(&l1, &l2, 1.0, 1.0).unwrap();
        assert!(approx_eq(res.point_on_l1, Pt { x: 1.0, y: 0.0 }, 1e-9));
        assert!(approx_eq(res.point_on_l2, Pt { x: 0.0, y: 1.0 }, 1e-9));
    }

    #[test]
    fn test_chamfer_distance_too_large() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 1.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 10.0 },
        };
        let res = chamfer_lines(&l1, &l2, 2.0, 1.0);
        assert_eq!(res, Err(ChamferError::DistanceTooLarge));
    }

    #[test]
    fn test_chamfer_accepts_roundoff_at_exact_boundary() {
        let almost_twenty = 20.0 - 4.0e-15;
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt {
                x: almost_twenty,
                y: 0.0,
            },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 40.0 },
        };
        let result = chamfer_lines(&l1, &l2, 20.0, 20.0).unwrap();
        assert!(approx_eq(
            result.point_on_l1,
            Pt {
                x: almost_twenty,
                y: 0.0,
            },
            1e-12,
        ));
    }

    #[test]
    fn test_chamfer_parallel() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 1.0 },
            b: Pt { x: 10.0, y: 1.0 },
        };
        let res = chamfer_lines(&l1, &l2, 1.0, 1.0);
        assert_eq!(res, Err(ChamferError::Parallel));
    }

    #[test]
    fn test_chamfer_not_positive() {
        let l1 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 10.0, y: 0.0 },
        };
        let l2 = LineSeg {
            a: Pt { x: 0.0, y: 0.0 },
            b: Pt { x: 0.0, y: 10.0 },
        };
        let res = chamfer_lines(&l1, &l2, 0.0, 1.0);
        assert_eq!(res, Err(ChamferError::NotPositive));
    }
}
