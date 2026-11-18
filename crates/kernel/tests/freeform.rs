//! Free-form geometry: spline surfaces, and the solids that have them.
mod common;

use common::*;
use limo_cad_kernel::loft;
use limo_cad_kernel::make::{self, Seg};
use limo_cad_kernel::props::mass;
use limo_cad_kernel::step::{read, write};
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{boolean, v3, Body, Op, SurfKind, Xform, V3};
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
    limo_cad_kernel::audit::audit(body, limo_cad_kernel::audit::CLOSE).unwrap();
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
    // (Two meshes take most of the chord error out of each other, not all
    // of it: the number of chords round an arc is a whole number.)
    assert!(
        (m.volume - volume).abs() < 1.0e-5 * volume,
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

/// Cut, shared part and union of two solids: each a sound solid under a
/// clean skin, and together weighing what the two weigh.
fn combined(a: &Body, b: &Body) -> [Body; 3] {
    let done = |op: Op| boolean(a, b, op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    let out = [done(Op::Cut), done(Op::Intersect), done(Op::Union)];
    for body in &out {
        limo_cad_kernel::audit::audit(body, limo_cad_kernel::audit::CLOSE).unwrap();
        assert_mesh_is_a_clean_skin(body);
    }
    let (va, vb) = (mass(a).volume, mass(b).volume);
    let [cut, common, union] = [&out[0], &out[1], &out[2]].map(|body| mass(body).volume);
    let scale = va.max(vb);
    assert!((cut + common - va).abs() < 1.0e-8 * scale);
    assert!((union + common - va - vb).abs() < 1.0e-8 * scale);
    assert!(common > 1.0e-3 * scale, "the two were meant to overlap");
    out
}

/// Written out and read back, a body is the same body.
fn saved(body: &Body) {
    let text = body.to_text();
    let back = Body::from_text(&text).unwrap();
    assert_eq!(back.to_text(), text);
    back.check().unwrap();
    assert_eq!(mass(&back).volume, mass(body).volume);
}

#[test]
fn lofts_cut_by_blocks_and_rods() {
    let lofts = [
        loft::skin(&[square(10.0, 0.0, 0.0), square(6.0, 25.0, 0.5)], true).unwrap(),
        loft::skin(&[square(10.0, 0.0, 0.0), circle(8.0, 30.0)], true).unwrap(),
        loft::skin(
            &[circle(8.0, 0.0), circle(12.0, 15.0), circle(6.0, 30.0)],
            false,
        )
        .unwrap(),
    ];
    let tools = [
        make::block(v3(2.0, -30.0, 5.0), v3(30.0, 30.0, 12.0)).unwrap(),
        make::cylinder(v3(-30.0, 1.0, 12.0), V3::X, 3.0, 60.0).unwrap(),
        make::cylinder(v3(1.0, 0.5, -5.0), V3::Z, 3.0, 50.0).unwrap(),
        make::cylinder(v3(-25.0, -3.0, 2.0), v3(1.0, 0.2, 0.4).unit(), 2.5, 70.0).unwrap(),
    ];
    for skin in &lofts {
        saved(skin);
        for tool in &tools {
            let [cut, common, _] = combined(skin, tool);
            // A hole through a free-form face has an edge written on it.
            saved(&cut);
            saved(&common);
        }
    }
}

#[test]
fn free_form_solids_against_each_other() {
    let prism = loft::skin(&[square(10.0, 0.0, 0.0), square(6.0, 25.0, 0.5)], true).unwrap();
    let vase = loft::skin(
        &[circle(8.0, 0.0), circle(12.0, 15.0), circle(6.0, 30.0)],
        false,
    )
    .unwrap();
    combined(
        &prism,
        &vase.transformed(&Xform::translation(v3(9.0, 2.0, 3.0))),
    );
    let turned = Xform::rotation(V3::ZERO, v3(1.0, 0.3, 0.0).unit(), 0.9);
    combined(&prism, &prism.transformed(&turned));
    // In a mirror the skin is the same skin, seen the other way round.
    let mirror = Xform::mirror(v3(9.0, 0.0, 0.0), V3::X);
    let image = vase.transformed(&mirror);
    sound(&image);
    saved(&image);
    assert!((mass(&image).volume - mass(&vase).volume).abs() < 1.0e-9);
    let [_, common, _] = combined(&vase, &image);
    saved(&common);
}

/// What a body weighs after a trip through a STEP file, against what it
/// weighed before: volume and area, as fractions off.
fn through_step(name: &str, body: &Body) -> (f64, f64) {
    let text = write(&[(name.to_string(), body)], "free-form test");
    let back = read(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(back.len(), 1, "{name}");
    let back = &back[0];
    assert_eq!(back.face_count(), body.face_count(), "{name}");
    back.check().unwrap();
    assert_mesh_is_a_clean_skin(back);
    let (before, after) = (mass(body), mass(back));
    (
        (after.volume / before.volume - 1.0).abs(),
        (after.area / before.area - 1.0).abs(),
    )
}

#[test]
fn free_form_solids_come_back_from_step() {
    // A skin goes out as the spline it is and comes back the same spline.
    let prism = loft::skin(&[square(10.0, 0.0, 0.0), square(6.0, 25.0, 0.5)], true).unwrap();
    let vase = loft::skin(
        &[circle(8.0, 0.0), circle(12.0, 15.0), circle(6.0, 30.0)],
        false,
    )
    .unwrap();
    let image = vase.transformed(&Xform::mirror(v3(3.0, 0.0, 0.0), v3(1.0, 0.1, 0.2).unit()));
    for (name, body) in [("prism", &prism), ("vase", &vase), ("image", &image)] {
        let (volume, area) = through_step(name, body);
        assert!(
            volume < 1.0e-13 && area < 1.0e-13,
            "{name}: {volume} {area}"
        );
    }
    // The rim of a hole through one is written as a cubic that follows it
    // to a ten-millionth of a millimetre.
    let rod = make::cylinder(v3(-25.0, -3.0, 2.0), v3(1.0, 0.2, 0.4).unit(), 2.5, 70.0).unwrap();
    let drilled = boolean(&vase, &rod, Op::Cut).unwrap();
    let (volume, area) = through_step("drilled", &drilled);
    assert!(volume < 1.0e-9 && area < 1.0e-9, "{volume} {area}");
}

#[test]
fn threads_come_back_from_step() {
    use limo_cad_kernel::make::PSeg;
    use limo_cad_kernel::thread::{clipped, cutter, rounded, Run};
    use limo_cad_kernel::v2;
    use std::f64::consts::TAU;
    // A screw surface has no name in STEP and goes out as a spline that
    // follows it closely. Read back, the thread is a solid of spline
    // faces that weighs what the exact one does: what is in the file is
    // the thread.
    let plain = make::cylinder(V3::ZERO, V3::Z, 5.0, 9.0).unwrap();
    let points = [v2(4.0, -0.3), v2(5.2, -0.9), v2(5.2, 0.9), v2(4.0, 0.3)];
    let tooth: Vec<PSeg> = (0..4)
        .map(|i| PSeg::Line {
            a: points[i],
            b: points[(i + 1) % 4],
        })
        .collect();
    let groove = make::screw(&tooth, V3::ZERO, V3::Z, V3::X, 2.0 / TAU, -TAU, TAU * 5.5).unwrap();
    let cut = boolean(&plain, &groove, Op::Cut).unwrap();
    let (volume, area) = through_step("trapezoid thread", &cut);
    assert!(volume < 1.0e-8 && area < 1.0e-7, "{volume} {area}");
    // A rounded thread over seven and a half turns, each face of it one
    // long spline.
    let run = Run {
        start: V3::ZERO,
        along: V3::Z,
        zero: V3::X,
        pitch: 4.0,
        depth: 30.0,
        left_hand: false,
    };
    let rod = make::cylinder(V3::ZERO, V3::Z, 12.0, 30.0).unwrap();
    let profile = rounded(24.0, 20.0, 4.0, 0.3, 0.0, false).unwrap();
    let groove = clipped(&cutter(&profile, &run).unwrap(), &run, 12.0, true).unwrap();
    let long = boolean(&rod, &groove, Op::Cut).unwrap();
    let (volume, area) = through_step("rounded thread", &long);
    assert!(volume < 1.0e-7 && area < 1.0e-6, "{volume} {area}");
}

#[test]
fn a_profile_carried_without_turning() {
    // Whatever the path does on the way, every slice parallel to the
    // profile is a copy of it: the volume is its area times how far the
    // path gets along the profile's normal.
    let rectangle: Vec<Seg> = {
        let p = [
            v3(-4.0, -2.0, 0.0),
            v3(4.0, -2.0, 0.0),
            v3(4.0, 2.0, 0.0),
            v3(-4.0, 2.0, 0.0),
        ];
        (0..4)
            .map(|i| Seg::Line {
                a: p[i],
                b: p[(i + 1) % 4],
            })
            .collect()
    };
    let line = |a: V3, b: V3| Seg::Line { a, b };
    // A quarter turn in the plane x-z, bulging towards +x, then on.
    let bend = Seg::Arc {
        a: v3(0.0, 0.0, 10.0),
        m: v3(
            20.0 * (1.0 - (PI / 8.0).cos()),
            0.0,
            10.0 + 20.0 * (PI / 8.0).sin(),
        ),
        b: v3(
            20.0 * (1.0 - (PI / 4.0).cos()),
            0.0,
            10.0 + 20.0 * (PI / 4.0).sin(),
        ),
    };
    let end = v3(
        20.0 * (1.0 - (PI / 4.0).cos()),
        0.0,
        10.0 + 20.0 * (PI / 4.0).sin(),
    );
    let onward = end + v3(5.0, 3.0, 5.0);
    /// What is carried, along what, its area and where it ends up.
    type Case = (&'static str, Vec<Seg>, Vec<Seg>, f64, V3);
    let cases: Vec<Case> = vec![
        (
            "slanted bar",
            rectangle.clone(),
            vec![line(V3::ZERO, v3(3.0, 2.0, 12.0))],
            32.0,
            v3(3.0, 2.0, 12.0),
        ),
        (
            "bar round a bend",
            rectangle.clone(),
            vec![line(V3::ZERO, v3(0.0, 0.0, 10.0)), bend, line(end, onward)],
            32.0,
            onward,
        ),
        (
            "rod up a slope",
            circle(3.0, 0.0),
            vec![line(V3::ZERO, v3(4.0, -2.0, 9.0))],
            PI * 9.0,
            v3(4.0, -2.0, 9.0),
        ),
        (
            "rod round a bend",
            circle(3.0, 0.0),
            vec![line(V3::ZERO, v3(0.0, 0.0, 10.0)), bend, line(end, onward)],
            PI * 9.0,
            onward,
        ),
        (
            "slot round a bend",
            slot(5.0, 2.0, 0.0),
            vec![bend_from_origin()],
            PI * 4.0 + 40.0,
            v3(
                20.0 * (1.0 - (PI / 4.0).cos()),
                0.0,
                20.0 * (PI / 4.0).sin(),
            ),
        ),
    ];
    for (name, profile, path, area, to) in cases {
        let body = loft::carried(&profile, &path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (volume, _) = sound(&body);
        let exact = area * to.z;
        assert!(
            (volume - exact).abs() < 1.0e-9 * exact,
            "{name}: {volume} for {exact}"
        );
        saved(&body);
        // Every edge is a line or an arc.
        for e in 0..body.edge_count() as u32 {
            let kind = body.e_curve_ref(e).kind;
            assert!(
                matches!(
                    kind,
                    limo_cad_kernel::CurveKind::Line | limo_cad_kernel::CurveKind::Circle
                ),
                "{name}: {kind:?}"
            );
        }
    }
    // A bar straight up is the block it always was.
    let block = loft::carried(&rectangle, &[line(V3::ZERO, v3(0.0, 0.0, 7.0))]).unwrap();
    assert_eq!(count(&block, SurfKind::Plane), 6);
    // A path that doubles back through the profile's plane is refused.
    let back = Seg::Arc {
        a: V3::ZERO,
        m: v3(10.0, 0.0, 10.0),
        b: v3(20.0, 0.0, 0.0),
    };
    assert!(loft::carried(&rectangle, &[back]).is_err());
}

/// The same quarter-turn-less bend, starting at the origin.
fn bend_from_origin() -> Seg {
    Seg::Arc {
        a: V3::ZERO,
        m: v3(
            20.0 * (1.0 - (PI / 8.0).cos()),
            0.0,
            20.0 * (PI / 8.0).sin(),
        ),
        b: v3(
            20.0 * (1.0 - (PI / 4.0).cos()),
            0.0,
            20.0 * (PI / 4.0).sin(),
        ),
    }
}
