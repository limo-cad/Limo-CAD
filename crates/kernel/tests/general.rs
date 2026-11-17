//! Booleans between surfaces that meet in curves with no closed form:
//! planes cutting tori and cones at any angle, holes through rings, cones
//! and spheres on different axes.
mod common;

use common::*;
use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{boolean, Body, CurveKind, Op};

/// A ring about the z axis: tube radius `minor` around a circle of radius
/// `major` at height `z`.
fn ring(major: f64, minor: f64, z: f64) -> Body {
    make::revolve(
        &[vec![PSeg::Circle {
            c: v2(major, z),
            r: minor,
        }]],
        V3::ZERO,
        V3::Z,
        V3::X,
        TAU,
    )
    .unwrap()
}

/// A solid cone standing on its tip at the origin.
fn cone(radius: f64, height: f64) -> Body {
    make::lathe(
        V3::ZERO,
        V3::Z,
        &[(0.0, 0.0), (radius, height), (0.0, height)],
    )
    .unwrap()
}

/// A block turned about an axis through its own centre.
fn turned_block(min: V3, max: V3, axis: V3, angle: f64) -> Body {
    make::block(min, max)
        .unwrap()
        .transformed(&Xform::rotation((min + max) * 0.5, axis, angle))
}

/// What is cut away and what is kept must add up to the whole, and the
/// union must count the shared part once.
fn assert_consistent(a: &Body, b: &Body) -> (Body, Body, Body) {
    let cut = boolean(a, b, Op::Cut).unwrap();
    let common = boolean(a, b, Op::Intersect).unwrap();
    let union = boolean(a, b, Op::Union).unwrap();
    for body in [&cut, &common, &union] {
        body.check().unwrap();
        assert_flat_faces_are_covered(body);
        assert_mesh_is_a_clean_skin(body);
    }
    let (va, vb) = (volume(a), volume(b));
    let (vcut, vcommon, vunion) = (volume(&cut), volume(&common), volume(&union));
    let scale = va.max(vb);
    assert!(
        (vcut + vcommon - va).abs() < 2.0e-5 * scale,
        "cut {vcut} + common {vcommon} != {va}"
    );
    assert!(
        (vunion + vcommon - va - vb).abs() < 2.0e-5 * scale,
        "union {vunion} + common {vcommon} != {va} + {vb}"
    );
    (cut, common, union)
}

fn meets(body: &Body) -> usize {
    (0..body.edge_count() as u32)
        .filter(|&e| body.e_curve_ref(e).kind == CurveKind::Meet)
        .count()
}

#[test]
fn ring_cut_by_a_plane_beside_its_axis() {
    // The plane x = 12 runs parallel to the axis through the tube: the two
    // ovals of a spiric section.
    let a = ring(20.0, 6.0, 0.0);
    let b = make::block(v3(12.0, -40.0, -10.0), v3(40.0, 40.0, 10.0)).unwrap();
    let (cut, common, _) = assert_consistent(&a, &b);
    assert!(meets(&cut) > 0 && meets(&common) > 0);
}

#[test]
fn ring_cut_by_a_tilted_plane() {
    let a = ring(20.0, 6.0, 0.0);
    let b = turned_block(v3(-40.0, -40.0, 0.0), v3(40.0, 40.0, 30.0), V3::X, 0.35);
    assert_consistent(&a, &b);
}

#[test]
fn cone_cut_in_a_hyperbola_and_a_parabola() {
    let a = cone(20.0, 30.0);
    // Parallel to the axis: one branch of a hyperbola.
    let beside = make::block(v3(6.0, -30.0, -5.0), v3(40.0, 30.0, 40.0)).unwrap();
    assert_consistent(&a, &beside);
    // Parallel to a ruling: a parabola.
    let slope = (20.0_f64 / 30.0).atan();
    let along = turned_block(v3(4.0, -40.0, -20.0), v3(60.0, 40.0, 60.0), V3::Y, slope);
    assert_consistent(&a, &along);
}

#[test]
fn hole_through_a_ring() {
    let a = ring(20.0, 6.0, 0.0);
    // Along the axis direction, through the tube.
    let down = make::cylinder(v3(20.0, 0.0, -10.0), V3::Z, 3.0, 20.0).unwrap();
    assert_consistent(&a, &down);
    // Across the ring, entering and leaving through both sides of the tube.
    let across = make::cylinder(v3(-40.0, 3.0, 1.0), V3::X, 2.5, 80.0).unwrap();
    assert_consistent(&a, &across);
}

#[test]
fn cone_drilled_across() {
    let a = cone(20.0, 30.0);
    let drill = make::cylinder(v3(-30.0, 2.0, 18.0), V3::X, 4.0, 60.0).unwrap();
    assert_consistent(&a, &drill);
}

#[test]
fn two_rings_through_each_other() {
    let a = ring(20.0, 6.0, 0.0);
    let b = ring(20.0, 6.0, 0.0).transformed(&Xform::rotation(V3::ZERO, V3::X, 0.5 * PI));
    // Far enough across for the two tubes to run into each other.
    let b = b.transformed(&Xform::translation(v3(34.0, 0.0, 0.0)));
    assert_consistent(&a, &b);
}

#[test]
fn ball_on_a_cone() {
    let a = cone(20.0, 30.0);
    let ball = make::revolve(
        &[vec![
            PSeg::Arc {
                a: v2(0.0, -8.0),
                b: v2(0.0, 8.0),
                c: v2(0.0, 0.0),
                ccw: true,
            },
            PSeg::Line {
                a: v2(0.0, 8.0),
                b: v2(0.0, -8.0),
            },
        ]],
        v3(9.0, 3.0, 20.0),
        V3::Z,
        V3::X,
        TAU,
    )
    .unwrap();
    assert_consistent(&a, &ball);
}

/// Two plain solids thrown together at any angle: nothing lines up, loops
/// double back around poles and tips, holes sit on seams. Whatever comes
/// out must add up and mesh cleanly, down to fine tolerances.
#[test]
fn solids_in_any_pose() {
    let fine = |body: &Body| {
        let at = |linear: f64, angular: f64| {
            let options = TessOptions { linear, angular };
            let mesh = tessellate(body, &options);
            if let Err(flaw) = mesh.check(body, &options) {
                panic!("meshed to {linear}: {flaw}");
            }
            mesh.volume()
        };
        (4.0 * at(0.001, 0.03) - at(0.004, 0.06)) / 3.0
    };
    let mut overlapping = 0;
    for seed in 0..40 {
        let [(name_a, a), (name_b, b)] = limo_cad_kernel::demo::random_pair(seed);
        let label = format!("pair {seed} ({name_a} and {name_b})");
        let results: Vec<Result<Body, String>> = [Op::Cut, Op::Intersect, Op::Union]
            .into_iter()
            .map(|op| boolean(&a, &b, op))
            .collect();
        // Some pairs miss each other, and a small solid can vanish in a
        // large one. Anything else has to work.
        if let Some(reason) = results.iter().find_map(|r| r.as_ref().err()) {
            assert!(
                reason.contains("do not overlap") || reason.contains("removes the entire body"),
                "{label}: {reason}"
            );
            continue;
        }
        overlapping += 1;
        let bodies: Vec<&Body> = results.iter().map(|r| r.as_ref().unwrap()).collect();
        for body in &bodies {
            body.check().unwrap_or_else(|e| panic!("{label}: {e}"));
            assert_mesh_is_a_clean_skin(body);
        }
        let (va, vb) = (fine(&a), fine(&b));
        let (cut, common, union) = (fine(bodies[0]), fine(bodies[1]), fine(bodies[2]));
        let scale = va.max(vb);
        assert!(
            (cut + common - va).abs() < 5.0e-4 * scale,
            "{label}: cut {cut} + common {common} != {va}"
        );
        assert!(
            (union + common - va - vb).abs() < 5.0e-4 * scale,
            "{label}: union {union} + common {common} != {va} + {vb}"
        );
    }
    assert!(overlapping > 20, "only {overlapping} pairs overlap");
}
