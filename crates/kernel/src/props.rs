//! Exact properties from boundary integrals.

use crate::geom::CurveKind;
use crate::math::*;
use crate::topo::Body;

/// Five-point Gauss–Legendre nodes and weights on `[-1, 1]`.
const GAUSS_X: [f64; 5] = [
    0.0,
    0.538_469_310_105_683_1,
    -0.538_469_310_105_683_1,
    0.906_179_845_938_664,
    -0.906_179_845_938_664,
];
const GAUSS_W: [f64; 5] = [
    0.568_888_888_888_888_9,
    0.478_628_670_499_366_5,
    0.478_628_670_499_366_5,
    0.236_926_885_056_189_08,
    0.236_926_885_056_189_08,
];

/// Integrate `f(t)` over the parameter range of edge `e`.
fn integrate_edge(body: &Body, e: u32, mut f: impl FnMut(f64) -> f64) -> f64 {
    let [t0, t1] = body.e_t[e as usize];
    let pieces = match body.e_curve_ref(e).kind {
        CurveKind::Line => 1,
        _ => (((t1 - t0) / (PI / 8.0)).ceil() as usize).max(1),
    };
    let step = (t1 - t0) / pieces as f64;
    let mut sum = 0.0;
    for i in 0..pieces {
        let (a, half) = (t0 + step * i as f64, 0.5 * step);
        for k in 0..5 {
            sum += GAUSS_W[k] * f(a + half * (1.0 + GAUSS_X[k]));
        }
    }
    sum * 0.5 * step
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanarProps {
    pub centroid: V3,
    pub area: f64,
    pub perimeter: f64,
    pub loops: u32,
    pub edges: u32,
}

/// Area, centroid and perimeter of a planar face, to quadrature precision.
pub fn planar_props(body: &Body, f: u32) -> Option<PlanarProps> {
    let s = body.f_surface(f);
    if !s.is_plane() {
        return None;
    }
    // Integrate in plane coordinates oriented with the outward normal.
    let frame = s.f;
    let outward = body.f_sense[f as usize];
    let (mut area2, mut mu, mut mv, mut perimeter) = (0.0, 0.0, 0.0, 0.0);
    let mut edges: Vec<u32> = Vec::new();
    let mut loops = 0;
    for l in body.face_loops(f) {
        loops += 1;
        for h in body.loop_hes(l) {
            let e = h >> 1;
            edges.push(e);
            let c = *body.e_curve_ref(e);
            // Travel direction of the half-edge relative to the edge, and of
            // the plane coordinates relative to the outward normal.
            let sign = if (h & 1 == 0) == outward { 1.0 } else { -1.0 };
            let local = |t: f64| {
                let p = frame.local(c.point(t));
                let d = c.deriv(t);
                (p.x, p.y, d.dot(frame.x), d.dot(frame.y))
            };
            area2 += sign
                * integrate_edge(body, e, |t| {
                    let (u, v, du, dv) = local(t);
                    u * dv - v * du
                });
            mu += sign
                * integrate_edge(body, e, |t| {
                    let (u, _, _, dv) = local(t);
                    0.5 * u * u * dv
                });
            mv -= sign
                * integrate_edge(body, e, |t| {
                    let (_, v, du, _) = local(t);
                    0.5 * v * v * du
                });
            perimeter += match c.kind {
                CurveKind::Ellipse => integrate_edge(body, e, |t| c.deriv(t).len()),
                _ => body.e_length(e),
            };
        }
    }
    let area = 0.5 * area2;
    if area.abs() < 1.0e-18 {
        return None;
    }
    edges.sort_unstable();
    edges.dedup();
    let centroid = frame.o + frame.x * (mu / area) + frame.y * (mv / area);
    Some(PlanarProps {
        centroid,
        area: area.abs(),
        perimeter,
        loops,
        edges: edges.len() as u32,
    })
}
