//! A body checked against its own geometry.
//!
//! [`Body::check`] reads the topology alone: every edge used once each
//! way, every loop closed. That is what a Boolean can get wrong by
//! miscounting. This asks the questions the numbers have to answer: does
//! every edge lie in both of its faces, does it end at its vertices, and
//! is the skin the right way out.

use crate::math::*;
use crate::props;
use crate::topo::Body;

/// How many places along an edge are looked at.
const ALONG: usize = 9;

/// What is wrong with the geometry of `body`, if anything: an edge more
/// than `tolerance` off one of its faces, an edge that ends further from
/// its vertex than two points may be and still count as one, or a skin
/// that encloses nothing.
///
/// A vertex is where edges were found to meet, each by a calculation of
/// its own, and stands for all of their ends: a Boolean takes ends within
/// four times [`TOL`] of each other for one vertex. So that is what an
/// edge is held to end within, not `tolerance`.
pub fn audit(body: &Body, tolerance: f64) -> Result<(), String> {
    body.check()?;
    for e in 0..body.edge_count() as u32 {
        let c = body.e_curve_ref(e);
        let [t0, t1] = body.e_t[e as usize];
        let [v0, v1] = body.e_v[e as usize];
        for (t, v) in [(t0, v0), (t1, v1)] {
            let gap = c.point(t).dist(body.verts[v as usize]);
            if gap.is_nan() || gap > (4.0 * TOL).max(tolerance) {
                return Err(format!(
                    "edge {e} ({:?}) ends {gap:.3e} from its vertex {v}",
                    c.kind
                ));
            }
        }
        for h in [2 * e, 2 * e + 1] {
            let f = body.h_face(h);
            let s = body.f_surface(f);
            for i in 0..=ALONG {
                let p = c.point(t0 + (t1 - t0) * i as f64 / ALONG as f64);
                let off = s.sdist(p).abs();
                if off.is_nan() || off > tolerance {
                    return Err(format!(
                        "edge {e} ({:?}) is {off:.3e} off face {f} ({:?}) at {p:?}",
                        c.kind, s.kind
                    ));
                }
            }
        }
    }
    let weighed = props::mass(body);
    if weighed.exact && weighed.volume <= 0.0 {
        return Err(format!(
            "the skin encloses {:.3e}: it is inside out, or flat",
            weighed.volume
        ));
    }
    Ok(())
}

/// The tolerance [`audit`] is usually asked to hold: a hundredth of what
/// the kernel takes for one point.
pub const CLOSE: f64 = 0.01 * TOL;
