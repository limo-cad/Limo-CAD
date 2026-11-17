//! Clearance between bodies and what they share.

use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::near::{clash, gap, Part};
use limo_cad_kernel::{boolean, v2, v3, Body, Op, Xform, V3};
use std::f64::consts::{PI, TAU};

fn ball(centre: V3, r: f64) -> Body {
    let half = [
        PSeg::Arc {
            a: v2(0.0, -r),
            b: v2(0.0, r),
            c: v2(0.0, 0.0),
            ccw: true,
        },
        PSeg::Line {
            a: v2(0.0, r),
            b: v2(0.0, -r),
        },
    ];
    make::revolve(&[half.to_vec()], centre, V3::Z, V3::X, TAU).unwrap()
}

fn ring(major: f64, minor: f64) -> Body {
    let tube = PSeg::Circle {
        c: v2(major, 0.0),
        r: minor,
    };
    make::revolve(&[vec![tube]], V3::ZERO, V3::Z, V3::X, TAU).unwrap()
}

#[track_caller]
fn assert_apart(a: &Body, b: &Body, wanted: f64) {
    let found = clash(a, b).unwrap();
    assert!(
        (found.clearance - wanted).abs() < 1.0e-9 * wanted.max(1.0),
        "clearance {} for {wanted}",
        found.clearance
    );
    assert_eq!(found.overlap, 0.0);
    // The two points are that far apart, and the same from the other side.
    assert!((found.a.dist(found.b) - wanted).abs() < 1.0e-9 * wanted.max(1.0));
    let back = clash(b, a).unwrap();
    assert!((back.clearance - wanted).abs() < 1.0e-9 * wanted.max(1.0));
}

#[test]
fn blocks_apart() {
    let a = make::block(V3::ZERO, v3(10.0, 10.0, 10.0)).unwrap();
    // Face to face, edge to edge, corner to corner.
    let beside = make::block(v3(13.0, 2.0, 2.0), v3(20.0, 8.0, 8.0)).unwrap();
    assert_apart(&a, &beside, 3.0);
    let across = make::block(v3(13.0, 14.0, 0.0), v3(20.0, 20.0, 10.0)).unwrap();
    assert_apart(&a, &across, 5.0);
    let beyond = make::block(v3(12.0, 13.0, 16.0), v3(20.0, 20.0, 20.0)).unwrap();
    assert_apart(&a, &beyond, 7.0);
    // Turned, so that an edge comes nearest to the middle of a face.
    let turned = make::block(v3(-5.0, -5.0, 0.0), v3(5.0, 5.0, 10.0))
        .unwrap()
        .transformed(&Xform::rotation(V3::ZERO, V3::Z, 0.25 * PI))
        .transformed(&Xform::translation(v3(20.0, 5.0, 0.0)));
    assert_apart(&a, &turned, 10.0 - 50.0_f64.sqrt());
}

#[test]
fn round_things_apart() {
    let plate = make::block(v3(-30.0, -30.0, -4.0), v3(30.0, 30.0, 0.0)).unwrap();
    // A ball has no edge to walk along: only its face faces the plate.
    assert_apart(&plate, &ball(v3(3.0, -2.0, 9.0), 6.0), 3.0);
    assert_apart(
        &ball(v3(0.0, 0.0, 0.0), 5.0),
        &ball(v3(12.0, 9.0, 20.0), 4.0),
        25.0 - 9.0,
    );
    // A rod lying over the plate.
    let rod = make::cylinder(v3(-10.0, 0.0, 7.0), V3::X, 2.0, 20.0).unwrap();
    assert_apart(&plate, &rod, 5.0);
    // Two rods that cross without meeting: nearest along the line square
    // to both.
    let upright = make::cylinder(v3(0.0, 9.0, -20.0), V3::Z, 3.0, 60.0).unwrap();
    assert_apart(&rod, &upright, 9.0 - 2.0 - 3.0);
    // Tilted, the same line is still the nearest.
    let tilted = make::cylinder(v3(0.0, 9.0, -20.0), V3::Z, 3.0, 60.0)
        .unwrap()
        .transformed(&Xform::rotation(v3(0.0, 9.0, 7.0), V3::Y, 0.4));
    assert_apart(&rod, &tilted, 4.0);
    // A ring round a rod touches nothing, and holds none of it.
    let through = make::cylinder(v3(0.0, 0.0, -20.0), V3::Z, 3.0, 40.0).unwrap();
    assert_apart(&ring(15.0, 4.0), &through, 15.0 - 4.0 - 3.0);
    // A ball over the hole of a ring comes nearest all the way round.
    assert_apart(
        &ring(15.0, 4.0),
        &ball(v3(0.0, 0.0, 12.0), 2.0),
        15.0_f64.hypot(12.0) - 4.0 - 2.0,
    );
}

#[test]
fn a_shaft_in_a_bore() {
    let plate = make::block(v3(-20.0, -20.0, 0.0), v3(20.0, 20.0, 10.0)).unwrap();
    let bore = make::cylinder(v3(0.0, 0.0, -1.0), V3::Z, 5.1, 12.0).unwrap();
    let drilled = boolean(&plate, &bore, Op::Cut).unwrap();
    let shaft = make::cylinder(v3(0.0, 0.0, -10.0), V3::Z, 5.0, 30.0).unwrap();
    assert_apart(&drilled, &shaft, 0.1);
    // Off centre the gap closes on one side.
    let off = make::cylinder(v3(0.04, -0.03, -10.0), V3::Z, 5.0, 30.0).unwrap();
    assert_apart(&drilled, &off, 0.05);
}

#[test]
fn touching_and_sharing() {
    let a = make::block(V3::ZERO, v3(10.0, 10.0, 10.0)).unwrap();
    // Face on face: no gap, nothing shared.
    let on = make::block(v3(10.0, 2.0, 2.0), v3(20.0, 8.0, 8.0)).unwrap();
    let found = clash(&a, &on).unwrap();
    assert_eq!((found.clearance, found.overlap), (0.0, 0.0));
    // A press fit: the same cylinder from both sides.
    let bore = make::cylinder(v3(5.0, 5.0, -1.0), V3::Z, 3.0, 12.0).unwrap();
    let drilled = boolean(&a, &bore, Op::Cut).unwrap();
    let pin = make::cylinder(v3(5.0, 5.0, 0.0), V3::Z, 3.0, 10.0).unwrap();
    let found = clash(&drilled, &pin).unwrap();
    assert_eq!((found.clearance, found.overlap), (0.0, 0.0));
    // Running into each other.
    let into = make::block(v3(8.0, 8.0, 8.0), v3(20.0, 20.0, 20.0)).unwrap();
    let found = clash(&a, &into).unwrap();
    assert_eq!(found.clearance, 0.0);
    assert!((found.overlap - 8.0).abs() < 1.0e-9);
    // Wholly inside: all of the smaller one is shared.
    let within = ball(v3(5.0, 5.0, 5.0), 3.0);
    let found = clash(&a, &within).unwrap();
    assert_eq!(found.clearance, 0.0);
    assert!((found.overlap - 36.0 * PI).abs() < 1.0e-8);
    // A ball that pokes through one face.
    let poking = ball(v3(5.0, 5.0, 11.0), 3.0);
    let found = clash(&a, &poking).unwrap();
    let h = 2.0_f64;
    assert!((found.overlap - PI * h * h * (9.0 - h) / 3.0).abs() < 1.0e-8);
}

#[test]
fn parts_of_bodies() {
    let a = make::block(V3::ZERO, v3(10.0, 10.0, 10.0)).unwrap();
    let rod = make::cylinder(v3(20.0, 5.0, 0.0), V3::Z, 2.0, 10.0).unwrap();
    // The far face of the block is further from the rod than the block.
    let far = (0..a.face_count() as u32)
        .find(|&f| a.f_normal(f, a.f_surface(f).f.o).dot(V3::X) < -0.5)
        .unwrap();
    let found = gap(&Part::face(&a, far), &Part::body(&rod)).unwrap();
    assert!((found.distance - 18.0).abs() < 1.0e-9);
    // Rim to rim, and a corner to the rod.
    let rim = (0..rod.edge_count() as u32)
        .find(|&e| rod.e_mid(e).z > 5.0)
        .unwrap();
    let corner = v3(10.0, 10.0, 10.0);
    let found = gap(&Part::point(&a, corner), &Part::edge(&rod, rim)).unwrap();
    assert!((found.distance - (10.0_f64.hypot(5.0) - 2.0)).abs() < 1.0e-9);
}
