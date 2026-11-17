//! Screw threads, as the grooves that cut them.
//!
//! A thread is one groove wound round an axis. Its section is a tooth: a
//! trapezoid for the 60-degree metric form, the same with rounded corners
//! for the 30-degree form meant for printing. Swept along the screw the
//! tooth is a solid of half a dozen faces, however many turns it makes,
//! and cutting it out of a rod or a bore leaves the thread.

use crate::boolean::{boolean, Op};
use crate::make::{self, PSeg};
use crate::math::*;
use crate::topo::Body;

/// Where a thread starts, which way it runs and how far.
#[derive(Clone, Copy, Debug)]
pub struct Run {
    /// On the axis, where the thread begins.
    pub start: V3,
    /// Along the axis, into the thread.
    pub along: V3,
    /// Across the axis: where the groove is as it passes a turn before
    /// `start`.
    pub zero: V3,
    pub pitch: f64,
    pub depth: f64,
    pub left_hand: bool,
}

/// A closed loop through `points`.
fn polygon(points: &[V2]) -> Vec<PSeg> {
    (0..points.len())
        .map(|i| PSeg::Line {
            a: points[i],
            b: points[(i + 1) % points.len()],
        })
        .collect()
}

/// How far a flank may be carried past the surface it has to cut through:
/// as far as `wanted`, short of where the turns of the cutter would run
/// into each other.
fn carry(wanted: f64, half_width: f64, pitch: f64, slope: f64) -> Result<f64, String> {
    let room = 0.5 * (0.499 * pitch - half_width) / slope;
    let by = wanted.min(room);
    if by < 1.0e-4 {
        return Err("the thread profile leaves no room between its turns".into());
    }
    Ok(by)
}

/// The groove of a 60-degree external thread, from its root at the minor
/// diameter out through the major diameter.
pub fn iso_external(
    major: f64,
    pitch_diameter: f64,
    minor: f64,
    pitch: f64,
) -> Result<Vec<PSeg>, String> {
    let slope = (PI / 6.0).tan();
    let (inner, middle, outer) = (0.5 * minor, 0.5 * pitch_diameter, 0.5 * major);
    let at_inner = 0.25 * pitch - (middle - inner) * slope;
    let at_outer = 0.25 * pitch + (outer - middle) * slope;
    if !(inner > 0.0 && middle > inner && outer > middle)
        || at_inner <= 0.0
        || at_outer >= 0.499 * pitch
    {
        return Err("ISO external thread limits do not form a valid 60-degree profile".into());
    }
    let wanted = (0.03 * major)
        .min(0.15 * pitch)
        .min(major - pitch_diameter)
        .max(5.0e-3);
    let by = carry(wanted, at_outer, pitch, slope)?;
    let (beyond, wide) = (outer + by, at_outer + by * slope);
    Ok(polygon(&[
        v2(inner, -at_inner),
        v2(beyond, -wide),
        v2(beyond, wide),
        v2(inner, at_inner),
    ]))
}

/// The groove of a 60-degree internal thread, from inside the bore out to
/// its root at the major diameter.
pub fn iso_internal(
    major: f64,
    pitch_diameter: f64,
    minor: f64,
    pitch: f64,
) -> Result<Vec<PSeg>, String> {
    let slope = (PI / 6.0).tan();
    let (inner, middle, outer) = (0.5 * minor, 0.5 * pitch_diameter, 0.5 * major);
    let at_outer = 0.25 * pitch - (outer - middle) * slope;
    let at_inner = 0.25 * pitch + (middle - inner) * slope;
    let wanted = (5.0e-3 * minor)
        .min(2.0e-2 * pitch)
        .min(2.5e-2 * (pitch_diameter - minor))
        .max(2.0e-3);
    if !(inner - wanted > 0.0 && middle > inner && outer > middle)
        || at_outer <= 0.0
        || at_inner >= 0.499 * pitch
    {
        return Err("ISO internal thread limits do not form a valid 60-degree profile".into());
    }
    let by = carry(wanted, at_inner, pitch, slope)?;
    let (within, wide) = (inner - by, at_inner + by * slope);
    Ok(polygon(&[
        v2(within, -wide),
        v2(outer, -at_outer),
        v2(outer, at_outer),
        v2(within, wide),
    ]))
}

/// The groove of a 30-degree thread with rounded root and crest, for a
/// screw (`internal` false) or for the nut that takes it. Both are cut
/// from the same numbers; the nut's groove is the screw's ridge, wider by
/// the clearance along the axis.
pub fn rounded(
    major: f64,
    minor: f64,
    pitch: f64,
    radius: f64,
    axial_clearance: f64,
    internal: bool,
) -> Result<Vec<PSeg>, String> {
    let (lo, hi) = (0.5 * minor, 0.5 * major);
    let beta = PI / 12.0;
    let (sin, cos, slope) = (beta.sin(), beta.cos(), beta.tan());
    let h0 = 0.25 * pitch - 0.5 * (hi - lo) * slope;
    let h1 = 0.25 * pitch + 0.5 * (hi - lo) * slope;
    let corner = radius * (1.0 / cos - slope);
    let (z0, z1) = (h0 - corner, h1 + corner);
    let overlap = (0.02 * pitch).max(0.005);
    if lo <= overlap
        || radius <= 0.0
        || 2.0 * radius * (1.0 - sin) >= hi - lo
        || z0 <= 0.5 * axial_clearance
        || z1 + 0.5 * axial_clearance >= 0.5 * pitch
    {
        return Err("rounded trapezoidal thread profile is invalid".into());
    }
    // The upper half, from the bore outwards; the lower half mirrors it.
    // Past the surface it has to cut through, the tooth narrows a little
    // rather than running on square, which no screw surface here does.
    let upper: Vec<PSeg> = if internal {
        // Wide at the bore, narrow at the root of the nut's groove.
        let (wide, narrow) = (
            0.5 * pitch - z0 + 0.5 * axial_clearance,
            0.5 * pitch - z1 + 0.5 * axial_clearance,
        );
        let (bore, bore_off) = (
            v2(lo, wide),
            v2(lo + radius - radius * sin, wide - radius * cos),
        );
        let (root_off, root) = (
            v2(hi - radius + radius * sin, narrow + radius * cos),
            v2(hi, narrow),
        );
        vec![
            PSeg::Line {
                a: v2(lo - overlap, wide - overlap * slope),
                b: bore,
            },
            PSeg::Arc {
                a: bore,
                b: bore_off,
                c: v2(lo + radius, wide),
                ccw: true,
            },
            PSeg::Line {
                a: bore_off,
                b: root_off,
            },
            PSeg::Arc {
                a: root_off,
                b: root,
                c: v2(hi - radius, narrow),
                ccw: false,
            },
        ]
    } else {
        let (root, root_off) = (
            v2(lo, z0),
            v2(lo + radius - radius * sin, z0 + radius * cos),
        );
        let (crest_off, crest) = (
            v2(hi - radius + radius * sin, z1 - radius * cos),
            v2(hi, z1),
        );
        vec![
            PSeg::Arc {
                a: root,
                b: root_off,
                c: v2(lo + radius, z0),
                ccw: false,
            },
            PSeg::Line {
                a: root_off,
                b: crest_off,
            },
            PSeg::Arc {
                a: crest_off,
                b: crest,
                c: v2(hi - radius, z1),
                ccw: true,
            },
            PSeg::Line {
                a: crest,
                b: v2(hi + overlap, z1 - overlap * slope),
            },
        ]
    };
    // Up the outer side as given, back down the inner side mirrored: the
    // lower half first so that the loop runs outwards below and inwards
    // above, material to its left.
    let mirror = |q: V2| v2(q.x, -q.y);
    let start = match upper[0] {
        PSeg::Line { a, .. } | PSeg::Arc { a, .. } => a,
        PSeg::Circle { .. } => unreachable!(),
    };
    let end = match upper[upper.len() - 1] {
        PSeg::Line { b, .. } | PSeg::Arc { b, .. } => b,
        PSeg::Circle { .. } => unreachable!(),
    };
    let mut lp: Vec<PSeg> = upper
        .iter()
        .map(|seg| match *seg {
            PSeg::Line { a, b } => PSeg::Line {
                a: mirror(a),
                b: mirror(b),
            },
            PSeg::Arc { a, b, c, ccw } => PSeg::Arc {
                a: mirror(a),
                b: mirror(b),
                c: mirror(c),
                ccw: !ccw,
            },
            circle => circle,
        })
        .collect();
    lp.push(PSeg::Line {
        a: mirror(end),
        b: end,
    });
    for seg in upper.iter().rev() {
        lp.push(match *seg {
            PSeg::Line { a, b } => PSeg::Line { a: b, b: a },
            PSeg::Arc { a, b, c, ccw } => PSeg::Arc {
                a: b,
                b: a,
                c,
                ccw: !ccw,
            },
            circle => circle,
        });
    }
    lp.push(PSeg::Line {
        a: start,
        b: mirror(start),
    });
    Ok(lp)
}

/// The groove of `profile` wound from a turn before the start of the run
/// to a turn past its end.
pub fn cutter(profile: &[PSeg], run: &Run) -> Result<Body, String> {
    let turns = (run.depth + 2.0 * run.pitch) / run.pitch;
    if !(turns.is_finite() && turns > 0.0 && turns <= 256.0) {
        return Err(
            "thread interval is too short or exceeds 256 turns; use simplified representation"
                .into(),
        );
    }
    let lead = run.pitch / TAU;
    let origin = run.start - run.along * run.pitch;
    if run.left_hand {
        make::screw(
            profile,
            origin,
            run.along,
            run.zero,
            -lead,
            -TAU * turns,
            0.0,
        )
    } else {
        make::screw(profile, origin, run.along, run.zero, lead, 0.0, TAU * turns)
    }
}

/// The cutter with what lies past the end of the run taken off, and with
/// `at_start` what lies before its start as well.
pub fn clipped(cutter: &Body, run: &Run, radius: f64, at_start: bool) -> Result<Body, String> {
    let from = if at_start { 0.0 } else { -run.pitch };
    let clip = make::cylinder(
        run.start + run.along * from,
        run.along,
        radius + run.pitch,
        run.depth - from,
    )?;
    boolean(cutter, &clip, Op::Intersect)
        .map_err(|e| format!("could not trim thread to its requested depth ({e})"))
}
