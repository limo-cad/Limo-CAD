use limo_cad_kernel::blend::{chamfer, fillet};
use limo_cad_kernel::make::{self, PSeg, Region, Seg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{boolean, Body, CurveKind, Op, SurfKind};

fn volume(body: &Body) -> f64 {
    tessellate(
        body,
        &TessOptions {
            linear: 0.0005,
            angular: 0.02,
        },
    )
    .volume()
}

fn close(actual: f64, expected: f64, rel: f64) {
    assert!(
        (actual - expected).abs() <= rel * expected.abs().max(1.0),
        "{actual} vs {expected}"
    );
}

fn count(body: &Body, kind: SurfKind) -> usize {
    (0..body.face_count() as u32)
        .filter(|&f| body.f_surface(f).kind == kind)
        .count()
}

/// Straight edges whose midpoint satisfies `pick`.
fn edges_where(body: &Body, pick: impl Fn(V3, V3) -> bool) -> Vec<u32> {
    (0..body.edge_count() as u32)
        .filter(|&e| {
            let c = body.e_curve_ref(e);
            c.kind == CurveKind::Line && pick(body.e_mid(e), c.f.x)
        })
        .collect()
}

#[test]
fn one_rounded_edge() {
    let block = make::block(V3::ZERO, v3(60.0, 30.0, 12.0)).unwrap();
    let edge = edges_where(&block, |m, _| m.y < 1e-9 && m.z > 12.0 - 1e-9);
    assert_eq!(edge.len(), 1);
    let r = fillet(&block, &edge, 2.0).unwrap();
    assert_eq!((r.face_count(), count(&r, SurfKind::Cylinder)), (7, 1));
    close(volume(&r), 21600.0 - (4.0 - PI) * 60.0, 1e-5);
}

#[test]
fn rounded_top_rim_has_mitred_corners() {
    let block = make::block(V3::ZERO, v3(60.0, 30.0, 12.0)).unwrap();
    let top = edges_where(&block, |m, d| m.z > 12.0 - 1e-9 && d.z.abs() < 1e-9);
    assert_eq!(top.len(), 4);
    let r = fillet(&block, &top, 2.0).unwrap();
    // Six flat faces and four rounds, meeting in elliptical mitres.
    assert_eq!((r.face_count(), count(&r, SurfKind::Cylinder)), (10, 4));
    let mitres = (0..r.edge_count() as u32)
        .filter(|&e| r.e_curve_ref(e).kind == CurveKind::Ellipse)
        .count();
    assert_eq!(mitres, 4);
    // Removed: four spandrel prisms, counting each corner overlap once.
    let r3 = 8.0;
    let removed = (4.0 - PI) * 180.0 - 4.0 * r3 * (5.0 / 3.0 - 0.5 * PI);
    close(volume(&r), 21600.0 - removed, 1e-5);
    r.check().unwrap();
}

#[test]
fn every_edge_rounded_gives_spherical_corners() {
    let block = make::block(V3::ZERO, v3(20.0, 16.0, 12.0)).unwrap();
    let all: Vec<u32> = (0..block.edge_count() as u32).collect();
    let r = fillet(&block, &all, 3.0).unwrap();
    assert_eq!(count(&r, SurfKind::Plane), 6);
    assert_eq!(count(&r, SurfKind::Cylinder), 12);
    assert_eq!(count(&r, SurfKind::Sphere), 8);
    // Rounded box: inner box plus slabs, quarter cylinders and sphere octants.
    let (a, b, c, rad) = (14.0, 10.0, 6.0, 3.0);
    let exact = a * b * c
        + 2.0 * rad * (a * b + b * c + a * c)
        + PI * rad * rad * (a + b + c)
        + 4.0 / 3.0 * PI * rad * rad * rad;
    close(volume(&r), exact, 2e-4);
}

#[test]
fn chamfers_equal_and_by_angle() {
    let block = make::block(V3::ZERO, v3(40.0, 20.0, 10.0)).unwrap();
    let edge = edges_where(&block, |m, _| m.y < 1e-9 && m.z > 10.0 - 1e-9);
    let r = chamfer(&block, &edge, 2.0, None).unwrap();
    assert_eq!(r.face_count(), 7);
    close(volume(&r), 8000.0 - 0.5 * 4.0 * 40.0, 1e-12);
    let r = chamfer(&block, &edge, 2.0, Some(30.0)).unwrap();
    close(
        volume(&r),
        8000.0 - 0.5 * 2.0 * 2.0 * 30f64.to_radians().tan() * 40.0,
        1e-12,
    );
    // The whole top rim.
    let top = edges_where(&block, |m, d| m.z > 10.0 - 1e-9 && d.z.abs() < 1e-9);
    let r = chamfer(&block, &top, 1.0, None).unwrap();
    assert_eq!(r.face_count(), 10);
    r.check().unwrap();
}

#[test]
fn inside_corner_is_filled() {
    // An L bracket: the inside corner runs along y.
    let p = |x: f64, z: f64| v3(x, 0.0, z);
    let pts = [
        p(0.0, 0.0),
        p(30.0, 0.0),
        p(30.0, 5.0),
        p(5.0, 5.0),
        p(5.0, 25.0),
        p(0.0, 25.0),
    ];
    let region = Region {
        loops: vec![(0..6)
            .map(|i| Seg::Line {
                a: pts[i],
                b: pts[(i + 1) % 6],
            })
            .collect()],
    };
    let bracket = make::extrude(&region, V3::Y, 0.0, 20.0).unwrap();
    let base = volume(&bracket);
    let inside = edges_where(&bracket, |m, d| {
        (m.x - 5.0).abs() < 1e-9 && (m.z - 5.0).abs() < 1e-9 && d.y.abs() > 0.5
    });
    assert_eq!(inside.len(), 1);
    let r = fillet(&bracket, &inside, 4.0).unwrap();
    assert_eq!(count(&r, SurfKind::Cylinder), 1);
    close(volume(&r), base + (16.0 - PI * 4.0) * 20.0, 1e-5);
}

#[test]
fn rims_of_a_boss_and_a_hole() {
    let plate = make::block(V3::ZERO, v3(40.0, 40.0, 10.0)).unwrap();
    let boss = make::cylinder(v3(20.0, 20.0, 10.0), V3::Z, 8.0, 12.0).unwrap();
    let body = boolean(&plate, &boss, Op::Union).unwrap();
    let rim = |body: &Body, z: f64| -> Vec<u32> {
        (0..body.edge_count() as u32)
            .filter(|&e| {
                let c = body.e_curve_ref(e);
                c.kind == CurveKind::Circle && (c.f.o.z - z).abs() < 1e-9
            })
            .collect()
    };
    let base = volume(&body);
    // Top of the boss: a convex rim becomes a torus.
    let r = fillet(&body, &rim(&body, 22.0), 2.0).unwrap();
    assert_eq!(count(&r, SurfKind::Torus), 1);
    // Pappus: the ring is the spandrel revolved about the axis.
    let ring =
        |square_centre: f64, disc_centre: f64| TAU * (4.0 * square_centre - PI * disc_centre);
    let offset = 8.0 / (3.0 * PI);
    close(volume(&r), base - ring(7.0, 6.0 + offset), 2e-4);
    // Foot of the boss: a concave rim is filled.
    let r = fillet(&body, &rim(&body, 10.0), 2.0).unwrap();
    assert_eq!(count(&r, SurfKind::Torus), 1);
    close(volume(&r), base + ring(9.0, 10.0 - offset), 2e-4);
    // A bevelled rim is a cone.
    let r = chamfer(&body, &rim(&body, 22.0), 1.5, None).unwrap();
    assert_eq!(count(&r, SurfKind::Cone), 1);
    let ring = 0.5 * 1.5 * 1.5 * TAU * (8.0 - 0.5);
    close(volume(&r), base - ring, 2e-4);
}

#[test]
fn revolved_profiles() {
    // Washer: rectangle revolved a full turn.
    let rect = |r0: f64, r1: f64, h0: f64, h1: f64| {
        vec![
            PSeg::Line {
                a: v2(r0, h0),
                b: v2(r1, h0),
            },
            PSeg::Line {
                a: v2(r1, h0),
                b: v2(r1, h1),
            },
            PSeg::Line {
                a: v2(r1, h1),
                b: v2(r0, h1),
            },
            PSeg::Line {
                a: v2(r0, h1),
                b: v2(r0, h0),
            },
        ]
    };
    let washer = make::revolve(&[rect(4.0, 10.0, 0.0, 3.0)], V3::ZERO, V3::Z, V3::X, TAU).unwrap();
    assert_eq!((washer.face_count(), washer.edge_count()), (4, 4));
    close(volume(&washer), PI * (100.0 - 16.0) * 3.0, 2e-4);
    // A quarter turn adds two flat ends.
    let quarter = make::revolve(
        &[rect(4.0, 10.0, 0.0, 3.0)],
        V3::ZERO,
        V3::Z,
        V3::X,
        0.5 * PI,
    )
    .unwrap();
    assert_eq!(quarter.face_count(), 6);
    close(volume(&quarter), 0.25 * PI * (100.0 - 16.0) * 3.0, 2e-4);
    // Cone to a point.
    let cone = make::lathe(V3::ZERO, V3::Z, &[(0.0, 0.0), (6.0, 0.0), (0.0, 9.0)]).unwrap();
    assert_eq!(cone.face_count(), 2);
    close(volume(&cone), PI * 36.0 * 9.0 / 3.0, 2e-4);
    // Half disc revolved: a sphere with no edges at all.
    let ball = make::revolve(
        &[vec![
            PSeg::Line {
                a: v2(0.0, 5.0),
                b: v2(0.0, -5.0),
            },
            PSeg::Arc {
                a: v2(0.0, -5.0),
                b: v2(0.0, 5.0),
                c: v2(0.0, 0.0),
                ccw: true,
            },
        ]],
        V3::ZERO,
        V3::Z,
        V3::X,
        TAU,
    )
    .unwrap();
    assert_eq!((ball.face_count(), ball.edge_count()), (1, 0));
    close(volume(&ball), 4.0 / 3.0 * PI * 125.0, 5e-4);
    // Ring torus from a revolved circle.
    let torus = make::revolve(
        &[vec![PSeg::Circle {
            c: v2(10.0, 0.0),
            r: 3.0,
        }]],
        V3::ZERO,
        V3::Z,
        V3::X,
        TAU,
    )
    .unwrap();
    assert_eq!((torus.face_count(), torus.edge_count()), (1, 0));
    close(volume(&torus), 2.0 * PI * PI * 10.0 * 9.0, 5e-4);
}
