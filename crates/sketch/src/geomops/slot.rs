//! Capsule-slot construction from a centerline and width.
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
pub enum SlotError {
    Degenerate,
    NotPositive,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotCapsule {
    pub line1: LineSeg,
    pub line2: LineSeg,
    pub arc1: ArcSeg,
    pub arc2: ArcSeg,
}

pub fn slot_capsule(c1: Pt, c2: Pt, width: f64) -> Result<SlotCapsule, SlotError> {
    if width <= 0.0 {
        return Err(SlotError::NotPositive);
    }

    let dx = c2.x - c1.x;
    let dy = c2.y - c1.y;
    let dist_sq = dx * dx + dy * dy;

    if dist_sq < (1e-9_f64).powi(2) {
        return Err(SlotError::Degenerate);
    }

    let dist = dist_sq.sqrt();
    let u = Pt {
        x: dx / dist,
        y: dy / dist,
    };

    let n = Pt { x: -u.y, y: u.x };

    let r = width / 2.0;

    let line1_a = Pt {
        x: c1.x + n.x * r,
        y: c1.y + n.y * r,
    };
    let line1_b = Pt {
        x: c2.x + n.x * r,
        y: c2.y + n.y * r,
    };

    let line2_a = Pt {
        x: c1.x - n.x * r,
        y: c1.y - n.y * r,
    };
    let line2_b = Pt {
        x: c2.x - n.x * r,
        y: c2.y - n.y * r,
    };

    let arc2_start = (-n.y).atan2(-n.x);
    let mut arc2_end = n.y.atan2(n.x);
    if arc2_end <= arc2_start {
        arc2_end += 2.0 * std::f64::consts::PI;
    }

    let arc1_start = n.y.atan2(n.x);
    let mut arc1_end = (-n.y).atan2(-n.x);
    if arc1_end <= arc1_start {
        arc1_end += 2.0 * std::f64::consts::PI;
    }

    Ok(SlotCapsule {
        line1: LineSeg {
            a: line1_a,
            b: line1_b,
        },
        line2: LineSeg {
            a: line2_a,
            b: line2_b,
        },
        arc1: ArcSeg {
            center: c1,
            radius: r,
            start_angle: arc1_start,
            end_angle: arc1_end,
            ccw: true,
        },
        arc2: ArcSeg {
            center: c2,
            radius: r,
            start_angle: arc2_start,
            end_angle: arc2_end,
            ccw: true,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    fn pt_approx_eq(a: Pt, b: Pt, tol: f64) -> bool {
        approx_eq(a.x, b.x, tol) && approx_eq(a.y, b.y, tol)
    }

    #[test]
    fn test_horizontal() {
        let c1 = Pt { x: 0.0, y: 0.0 };
        let c2 = Pt { x: 40.0, y: 0.0 };
        let width = 10.0;
        let slot = slot_capsule(c1, c2, width).unwrap();

        assert!(pt_approx_eq(slot.line1.a, Pt { x: 0.0, y: 5.0 }, 1e-9));
        assert!(pt_approx_eq(slot.line1.b, Pt { x: 40.0, y: 5.0 }, 1e-9));

        assert!(pt_approx_eq(slot.line2.a, Pt { x: 0.0, y: -5.0 }, 1e-9));
        assert!(pt_approx_eq(slot.line2.b, Pt { x: 40.0, y: -5.0 }, 1e-9));

        assert!(pt_approx_eq(slot.arc2.center, c2, 1e-9));
        assert!(approx_eq(slot.arc2.radius, 5.0, 1e-9));
        assert!(approx_eq(slot.arc2.start_angle, -PI / 2.0, 1e-9));
        assert!(approx_eq(slot.arc2.end_angle, PI / 2.0, 1e-9));
        assert!(slot.arc2.ccw);

        assert!(pt_approx_eq(slot.arc1.center, c1, 1e-9));
        assert!(approx_eq(slot.arc1.radius, 5.0, 1e-9));
        assert!(approx_eq(slot.arc1.start_angle, PI / 2.0, 1e-9));
        assert!(approx_eq(slot.arc1.end_angle, 3.0 * PI / 2.0, 1e-9));
        assert!(slot.arc1.ccw);
    }

    #[test]
    fn test_vertical() {
        let c1 = Pt { x: 0.0, y: 0.0 };
        let c2 = Pt { x: 0.0, y: 30.0 };
        let width = 6.0;
        let slot = slot_capsule(c1, c2, width).unwrap();

        assert!(pt_approx_eq(slot.line1.a, Pt { x: -3.0, y: 0.0 }, 1e-9));
        assert!(pt_approx_eq(slot.line1.b, Pt { x: -3.0, y: 30.0 }, 1e-9));

        assert!(pt_approx_eq(slot.line2.a, Pt { x: 3.0, y: 0.0 }, 1e-9));
        assert!(pt_approx_eq(slot.line2.b, Pt { x: 3.0, y: 30.0 }, 1e-9));

        assert!(pt_approx_eq(slot.arc2.center, c2, 1e-9));
        assert!(approx_eq(slot.arc2.radius, 3.0, 1e-9));
        assert!(approx_eq(slot.arc2.start_angle, 0.0, 1e-9));
        assert!(approx_eq(slot.arc2.end_angle, PI, 1e-9));
        assert!(slot.arc2.ccw);

        assert!(pt_approx_eq(slot.arc1.center, c1, 1e-9));
        assert!(approx_eq(slot.arc1.radius, 3.0, 1e-9));
        assert!(approx_eq(slot.arc1.start_angle, PI, 1e-9));
        assert!(approx_eq(slot.arc1.end_angle, 2.0 * PI, 1e-9));
        assert!(slot.arc1.ccw);
    }

    #[test]
    fn test_diagonal() {
        let c1 = Pt { x: 10.0, y: 5.0 };
        let c2 = Pt { x: 50.0, y: 35.0 };
        let width = 8.0;
        let slot = slot_capsule(c1, c2, width).unwrap();

        let r = 4.0;

        let dx = slot.line1.a.x - c1.x;
        let dy = slot.line1.a.y - c1.y;
        assert!(approx_eq((dx * dx + dy * dy).sqrt(), r, 1e-9));

        let dx = slot.line1.b.x - c2.x;
        let dy = slot.line1.b.y - c2.y;
        assert!(approx_eq((dx * dx + dy * dy).sqrt(), r, 1e-9));

        let dx = slot.line2.a.x - c1.x;
        let dy = slot.line2.a.y - c1.y;
        assert!(approx_eq((dx * dx + dy * dy).sqrt(), r, 1e-9));

        let dx = slot.line2.b.x - c2.x;
        let dy = slot.line2.b.y - c2.y;
        assert!(approx_eq((dx * dx + dy * dy).sqrt(), r, 1e-9));

        let line_dir_x = slot.line1.b.x - slot.line1.a.x;
        let line_dir_y = slot.line1.b.y - slot.line1.a.y;

        let vec_c1_to_l1a_x = slot.line1.a.x - c1.x;
        let vec_c1_to_l1a_y = slot.line1.a.y - c1.y;
        let dot1 = line_dir_x * vec_c1_to_l1a_x + line_dir_y * vec_c1_to_l1a_y;
        assert!(approx_eq(dot1, 0.0, 1e-9));

        let vec_c2_to_l1b_x = slot.line1.b.x - c2.x;
        let vec_c2_to_l1b_y = slot.line1.b.y - c2.y;
        let dot2 = line_dir_x * vec_c2_to_l1b_x + line_dir_y * vec_c2_to_l1b_y;
        assert!(approx_eq(dot2, 0.0, 1e-9));

        let vec_c1_to_l2a_x = slot.line2.a.x - c1.x;
        let vec_c1_to_l2a_y = slot.line2.a.y - c1.y;
        let dot3 = line_dir_x * vec_c1_to_l2a_x + line_dir_y * vec_c1_to_l2a_y;
        assert!(approx_eq(dot3, 0.0, 1e-9));

        let vec_c2_to_l2b_x = slot.line2.b.x - c2.x;
        let vec_c2_to_l2b_y = slot.line2.b.y - c2.y;
        let dot4 = line_dir_x * vec_c2_to_l2b_x + line_dir_y * vec_c2_to_l2b_y;
        assert!(approx_eq(dot4, 0.0, 1e-9));
    }

    #[test]
    fn test_errors() {
        let c1 = Pt { x: 0.0, y: 0.0 };
        let c2 = Pt { x: 10.0, y: 0.0 };

        assert_eq!(slot_capsule(c1, c2, 0.0), Err(SlotError::NotPositive));

        assert_eq!(slot_capsule(c1, c2, -2.0), Err(SlotError::NotPositive));

        assert_eq!(slot_capsule(c1, c1, 10.0), Err(SlotError::Degenerate));
    }
}
