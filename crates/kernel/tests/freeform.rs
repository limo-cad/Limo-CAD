//! Free-form geometry: spline surfaces, and the solids that have them.
mod common;

use common::*;
use limo_cad_kernel::loft;
use limo_cad_kernel::make::Seg;
use limo_cad_kernel::props::mass;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{v3, Body, SurfKind, V3};
use std::f64::consts::PI;

/// The corners of a square about the `z` axis, turned by `turn`.
fn corners(half: f64, z: f64, turn: f64) -> Vec<V3> {
    (0..4)
        .map(|i| {
            let a = turn + PI / 4.0 + PI / 2.0 * i as f64;
            let r = half * 2f64.sqrt();
            v3(r * a.cos(), r * a.sin(), z)
        })
        .collect()
}

fn square(half: f64, z: f64, turn: f64) -> Vec<Seg> {
    let p = corners(half, z, turn);
    (0..4)
        .map(|i| Seg::Line {
            a: p[i],
            b: p[(i + 1) % 4],
        })
        .collect()
}

fn circle(r: f64, z: f64) -> Vec<Seg> {
    vec![Seg::Circle {
        c: v3(0.0, 0.0, z),
        p: v3(r, 0.0, z),
        n: V3::Z,
    }]
}

/// A slot: two lines joined by two half circles.
fn slot(half_length: f64, r: f64, z: f64) -> Vec<Seg> {
    let (a, b, c, d) = (
        v3(-half_length, -r, z),
        v3(half_length, -r, z),
        v3(half_length, r, z),
        v3(-half_length, r, z),
    );
    vec![
        Seg::Line { a, b },
        Seg::Arc {
            a: b,
            m: v3(half_length + r, 0.0, z),
            b: c,
        },
        Seg::Line { a: c, b: d },
        Seg::Arc {
            a: d,
            m: v3(-half_length - r, 0.0, z),
            b: a,
        },
    ]
}

/// A sound solid whose display mesh is a clean skin at three finenesses,
/// and whose exact volume and area are what ever finer meshes tend to.
fn sound(body: &Body) -> (f64, f64) {
    body.check().unwrap();
    assert_mesh_is_a_clean_skin(body);
    let at = |linear: f64, angular: f64| {
        let options = TessOptions { linear, angular };
        let mesh = tessellate(body, &options);
        mesh.check(body, &options)
            .unwrap_or_else(|flaw| panic!("meshed to {linear}: {flaw}"));
        (mesh.volume(), mesh.area())
    };
    let (coarse, fine) = (at(0.002, 0.04), at(0.0005, 0.02));
    let volume = (4.0 * fine.0 - coarse.0) / 3.0;
    let area = (4.0 * fine.1 - coarse.1) / 3.0;
    let m = mass(body);
    assert!(m.exact);
    assert!(
        (m.volume - volume).abs() < 2.0e-6 * volume,
        "volume {} against {volume} from meshes",
        m.volume
    );
    assert!(
        (m.area - area).abs() < 2.0e-5 * area,
        "area {} against {area} from meshes",
        m.area
    );
    (m.volume, m.area)
}

#[test]
fn two_squares_turned_against_each_other() {
    let (low, high) = (corners(10.0, 0.0, 0.0), corners(6.0, 25.0, 0.5));
    let body = loft::skin(&[square(10.0, 0.0, 0.0), square(6.0, 25.0, 0.5)], true).unwrap();
    assert_eq!(count(&body, SurfKind::Spline), 4);
    assert_eq!(count(&body, SurfKind::Plane), 2);
    let (volume, _) = sound(&body);
    // A slice at height `t` is the polygon a fraction `t` of the way from
    // one square to the other, whose area is a parabola in `t`.
    let cross = |p: &[V3], q: &[V3]| -> f64 {
        (0..4)
            .map(|i| p[i].x * q[(i + 1) % 4].y - p[i].y * q[(i + 1) % 4].x)
            .sum::<f64>()
            * 0.5
    };
    let mixed = cross(&low, &high) + cross(&high, &low);
    let exact = 25.0 * (cross(&low, &low) + cross(&high, &high) + 0.5 * mixed) / 3.0;
    assert!((volume - exact).abs() < 1.0e-9 * exact, "{volume} {exact}");
}

#[test]
fn a_square_that_becomes_a_circle() {
    let body = loft::skin(&[square(10.0, 0.0, 0.0), circle(8.0, 30.0)], true).unwrap();
    assert_eq!(count(&body, SurfKind::Spline), 4);
    let (volume, _) = sound(&body);
    // Between a prism on the circle's inscribed square and one on the
    // square itself.
    assert!(volume > 30.0 * 128.0 && volume < 30.0 * 400.0);
    // The flat ends keep their true outlines: lines one end, arcs of the
    // circle the other.
    let arcs = (0..body.edge_count() as u32)
        .filter(|&e| {
            let c = body.e_curve_ref(e);
            c.kind == limo_cad_kernel::CurveKind::Circle && (c.r - 8.0).abs() < 1.0e-12
        })
        .count();
    assert_eq!(arcs, 4);
}

#[test]
fn a_smooth_skin_through_three_circles() {
    let body = loft::skin(
        &[circle(8.0, 0.0), circle(12.0, 15.0), circle(6.0, 30.0)],
        false,
    )
    .unwrap();
    assert_eq!(count(&body, SurfKind::Spline), 4);
    let (volume, _) = sound(&body);
    // Every slice is a circle, since the weights of the arcs agree. Radius
    // and height are the parabolas through the three sections, each at its
    // place along the skin: by how far its points are from the last one's.
    let (first, second) = (4.0f64.hypot(15.0), 6.0f64.hypot(15.0));
    let mid = first / (first + second);
    let through = |a: f64, b: f64, c: f64, t: f64| {
        a * (t - mid) * (t - 1.0) / mid
            + b * t * (t - 1.0) / (mid * (mid - 1.0))
            + c * t * (t - mid) / (1.0 - mid)
    };
    let radius = |t: f64| through(8.0, 12.0, 6.0, t);
    let height = |t: f64| through(0.0, 15.0, 30.0, t);
    let n = 2000;
    let exact: f64 = (0..n)
        .map(|i| {
            let (a, b) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            let f = |t: f64| {
                let rise = (height(t + 1.0e-6) - height(t - 1.0e-6)) / 2.0e-6;
                PI * radius(t).powi(2) * rise
            };
            (b - a) / 6.0 * (f(a) + 4.0 * f(0.5 * (a + b)) + f(b))
        })
        .sum();
    assert!((volume - exact).abs() < 1.0e-8 * exact, "{volume} {exact}");
}

#[test]
fn a_smooth_skin_through_unlike_sections() {
    let body = loft::skin(
        &[
            square(10.0, 0.0, 0.0),
            circle(8.0, 20.0),
            square(7.0, 40.0, 0.3),
        ],
        false,
    )
    .unwrap();
    assert_eq!(count(&body, SurfKind::Spline), 4);
    sound(&body);
    // Slots of different lengths: half circles are cut in two, so that no
    // piece is asked to carry more than a rational curve can.
    let body = loft::skin(&[slot(10.0, 4.0, 0.0), slot(4.0, 6.0, 18.0)], true).unwrap();
    assert_eq!(body.face_count(), 8);
    sound(&body);
    let body = loft::skin(
        &[
            slot(10.0, 4.0, 0.0),
            slot(4.0, 6.0, 18.0),
            slot(8.0, 3.0, 30.0),
        ],
        false,
    )
    .unwrap();
    sound(&body);
}

#[test]
fn sections_that_cannot_be_joined_are_refused() {
    let triangle = |z: f64| {
        let p = [v3(0.0, 0.0, z), v3(10.0, 0.0, z), v3(0.0, 10.0, z)];
        (0..3)
            .map(|i| Seg::Line {
                a: p[i],
                b: p[(i + 1) % 3],
            })
            .collect::<Vec<_>>()
    };
    let refused = loft::skin(&[square(10.0, 0.0, 0.0), triangle(20.0)], true).unwrap_err();
    assert!(refused.contains("same number of segments"), "{refused}");
    let refused = loft::skin(&[square(10.0, 0.0, 0.0), square(10.0, 0.0, 0.2)], true).unwrap_err();
    assert!(refused.contains("on top of each other"), "{refused}");
}
