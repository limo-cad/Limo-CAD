mod common;

use common::integral;
use limo_cad_kernel::blend::{chamfer, fillet};
use limo_cad_kernel::make::{self, PSeg, Region, Seg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::props::mass;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{boolean, Body, CurveKind, Op, SurfKind};

/// What a body holds, weighed as a solid. The drawn skin of a round body
/// holds a little less or more than the body does, and a comparison loose
/// enough to pass that passes a body that is wrong as well.
fn volume(body: &Body) -> f64 {
    mass(body).volume
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
    close(volume(&r), 21600.0 - (4.0 - PI) * 60.0, 1e-10);
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
    close(volume(&r), 21600.0 - removed, 1e-10);
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
    close(volume(&r), exact, 1e-10);
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
    close(volume(&r), base + (16.0 - PI * 4.0) * 20.0, 1e-10);
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
    close(volume(&r), base - ring(7.0, 6.0 + offset), 1e-10);
    // Foot of the boss: a concave rim is filled.
    let r = fillet(&body, &rim(&body, 10.0), 2.0).unwrap();
    assert_eq!(count(&r, SurfKind::Torus), 1);
    close(volume(&r), base + ring(9.0, 10.0 - offset), 1e-10);
    // A bevelled rim is a cone.
    let r = chamfer(&body, &rim(&body, 22.0), 1.5, None).unwrap();
    assert_eq!(count(&r, SurfKind::Cone), 1);
    let ring = 0.5 * 1.5 * 1.5 * TAU * (8.0 - 0.5);
    close(volume(&r), base - ring, 1e-10);
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
    close(volume(&washer), PI * (100.0 - 16.0) * 3.0, 1e-10);
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
    close(volume(&quarter), 0.25 * PI * (100.0 - 16.0) * 3.0, 1e-10);
    // Cone to a point.
    let cone = make::lathe(V3::ZERO, V3::Z, &[(0.0, 0.0), (6.0, 0.0), (0.0, 9.0)]).unwrap();
    assert_eq!(cone.face_count(), 2);
    close(volume(&cone), PI * 36.0 * 9.0 / 3.0, 1e-10);
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
    close(volume(&ball), 4.0 / 3.0 * PI * 125.0, 1e-10);
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
    close(volume(&torus), 2.0 * PI * PI * 10.0 * 9.0, 1e-10);
}

fn assert_sound(body: &Body) {
    body.check().unwrap();
    let options = TessOptions::default();
    tessellate(body, &options).check(body, &options).unwrap();
}

/// Straight edges where a flat face meets a cylinder that runs along them.
fn seams(body: &Body) -> Vec<u32> {
    (0..body.edge_count() as u32)
        .filter(|&e| {
            let kinds = [2 * e, 2 * e + 1].map(|h| body.f_surface(body.h_face(h)).kind);
            body.e_curve_ref(e).kind == CurveKind::Line
                && body.e_sharp(e)
                && kinds.contains(&SurfKind::Plane)
                && kinds.contains(&SurfKind::Cylinder)
        })
        .collect()
}

#[test]
fn straight_edges_between_a_flat_and_a_cylinder() {
    // Half a rod, split along its axis: the flat meets the round at two
    // straight edges.
    let rod = make::cylinder(V3::ZERO, V3::Z, 20.0, 30.0).unwrap();
    let half = make::block(v3(-30.0, 0.0, -1.0), v3(30.0, 30.0, 31.0)).unwrap();
    let half = boolean(&rod, &half, Op::Intersect).unwrap();
    let edges = seams(&half);
    assert_eq!(edges.len(), 2);
    let before = 0.5 * PI * 400.0 * 30.0;

    // Rounded: a circle of radius 2 that touches the flat and, from
    // inside, the rod.
    let rounded = fillet(&half, &edges, 2.0).unwrap();
    assert_sound(&rounded);
    let x0 = (400.0_f64 - 80.0).sqrt();
    let turn = (2.0 / x0).atan();
    let corner = 0.5 * 400.0 * turn - 0.5 * x0 * 2.0 - 0.5 * 4.0 * (0.5 * PI + turn);
    // The rod's own chord error is in both volumes and drops out.
    close(volume(&half) - volume(&rounded), 2.0 * corner * 30.0, 1e-10);
    close(volume(&rounded), before - 2.0 * corner * 30.0, 1e-10);
    assert_eq!(count(&rounded, SurfKind::Cylinder), 3);
    // Nothing sharp is left along the rod.
    assert!(seams(&rounded).is_empty());

    // Bevelled 2 along each face: along the flat, and round the rod.
    let bevelled = chamfer(&half, &edges, 2.0, None).unwrap();
    assert_sound(&bevelled);
    let corner = 0.5 * 400.0 * 0.1 - 0.5 * 18.0 * 20.0 * 0.1_f64.sin();
    close(
        volume(&half) - volume(&bevelled),
        2.0 * corner * 30.0,
        1e-10,
    );

    // A round as wide as the rod does not fit.
    assert!(fillet(&half, &edges, 12.0).is_err());
}

#[test]
fn a_ridge_and_a_groove_along_a_plate() {
    let plate = make::block(v3(-30.0, -20.0, -10.0), v3(30.0, 20.0, 0.0)).unwrap();
    let bar = make::cylinder(v3(0.0, -20.0, 0.0), V3::Y, 10.0, 40.0).unwrap();

    // A half-round ridge: where it rises from the plate the edges are
    // hollow, and a round fills them.
    let ridge = boolean(&plate, &bar, Op::Union).unwrap();
    let edges = seams(&ridge);
    assert_eq!(edges.len(), 2);
    let filled = fillet(&ridge, &edges, 2.0).unwrap();
    assert_sound(&filled);
    let x0 = (144.0_f64 - 4.0).sqrt();
    let turn = (2.0 / x0).atan();
    let corner = 0.5 * x0 * 2.0 - 0.5 * 100.0 * turn - 0.5 * 4.0 * (0.5 * PI - turn);
    close(volume(&filled) - volume(&ridge), 2.0 * corner * 40.0, 1e-10);
    assert!(seams(&filled).is_empty());

    // A half-round groove: its rims stand proud, and a round takes them
    // off.
    let groove = boolean(&plate, &bar, Op::Cut).unwrap();
    let edges = seams(&groove);
    assert_eq!(edges.len(), 2);
    let eased = fillet(&groove, &edges, 2.0).unwrap();
    assert_sound(&eased);
    assert!(volume(&eased) < volume(&groove));
    assert!(seams(&eased).is_empty());
}

/// A rim between a flat face and a cone, or a cone and a cylinder: the
/// edge a chamfer leaves behind can be broken again.
#[test]
fn rims_on_a_cone() {
    use limo_cad_kernel::props::mass;
    use std::f64::consts::TAU;
    let skin = |body: &Body| {
        let options = TessOptions::default();
        if let Err(flaw) = tessellate(body, &options).check(body, &options) {
            panic!("{flaw}");
        }
    };
    let rod = make::cylinder(V3::ZERO, V3::Z, 10.0, 20.0).unwrap();
    let rim = |body: &Body, radius: f64, height: f64| -> u32 {
        (0..body.edge_count() as u32)
            .find(|&e| {
                let c = body.e_curve_ref(e);
                c.kind == CurveKind::Circle
                    && (c.r - radius).abs() < 1.0e-9
                    && (c.f.o.z - height).abs() < 1.0e-9
            })
            .unwrap()
    };
    // What a ring of triangular section takes away: its area round the
    // circle its middle runs on.
    let ring = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        let area = 0.5 * ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs();
        TAU * (a.0 + b.0 + c.0) / 3.0 * area
    };
    let whole = mass(&rod).volume;
    let broken = chamfer(&rod, &[rim(&rod, 10.0, 20.0)], 2.0, None).unwrap();
    broken.check().unwrap();
    assert_eq!(count(&broken, SurfKind::Cone), 1);
    let first = ring((10.0, 18.0), (10.0, 20.0), (8.0, 20.0));
    assert!((mass(&broken).volume - (whole - first)).abs() < 1.0e-8);

    // The upper rim the chamfer left: between the flat top and the cone.
    let d = 0.5_f64;
    let slope = std::f64::consts::FRAC_1_SQRT_2;
    let again = chamfer(&broken, &[rim(&broken, 8.0, 20.0)], d, None).unwrap();
    again.check().unwrap();
    assert_eq!(count(&again, SurfKind::Cone), 2);
    let second = ring(
        (8.0, 20.0),
        (8.0 - d, 20.0),
        (8.0 + d * slope, 20.0 - d * slope),
    );
    assert!((mass(&again).volume - (whole - first - second)).abs() < 1.0e-8);
    skin(&again);

    // The lower rim: between the cone and the cylinder, rounded.
    let r = 0.6_f64;
    let rounded = fillet(&broken, &[rim(&broken, 10.0, 18.0)], r).unwrap();
    rounded.check().unwrap();
    assert_eq!(count(&rounded, SurfKind::Torus), 1);
    // The faces meet at 135 degrees inside the solid: the round takes the
    // corner between its two tangent points less the sector under its arc.
    let phi = 0.75 * std::f64::consts::PI;
    let reach = r / (0.5 * phi).tan();
    let m = mass(&rounded);
    assert!(m.volume < whole - first && m.volume > whole - first - TAU * 10.0 * reach * reach);
    skin(&rounded);
}

/// Two rounded edges that meet at the inside corner of an L: each round
/// runs on past the corner until it meets the other, and they join along
/// a mitre. The same for chamfers.
#[test]
fn rounds_meet_at_an_inside_corner() {
    use limo_cad_kernel::props::{mass, planar_props};
    let outline = [
        v3(0.0, 0.0, 0.0),
        v3(30.0, 0.0, 0.0),
        v3(30.0, 10.0, 0.0),
        v3(10.0, 10.0, 0.0),
        v3(10.0, 30.0, 0.0),
        v3(0.0, 30.0, 0.0),
    ];
    let segs: Vec<Seg> = (0..6)
        .map(|i| Seg::Line {
            a: outline[i],
            b: outline[(i + 1) % 6],
        })
        .collect();
    let block = make::extrude(&Region { loops: vec![segs] }, V3::Z, 0.0, 8.0).unwrap();
    // The top edges along the two inner walls.
    let inner = edges_where(&block, |mid, _| {
        (mid.z - 8.0).abs() < 1.0e-9
            && ((mid - v3(20.0, 10.0, 8.0)).len() < 1.0e-9
                || (mid - v3(10.0, 20.0, 8.0)).len() < 1.0e-9)
    });
    assert_eq!(inner.len(), 2);
    let top_of = |body: &Body| {
        (0..body.face_count() as u32)
            .filter_map(|f| planar_props(body, f).map(|p| (f, p)))
            .find(|(f, p)| {
                body.f_normal(*f, p.centroid).dot(V3::Z) > 0.99 && (p.centroid.z - 8.0).abs() < 1e-9
            })
            .map(|(_, p)| p.area)
            .unwrap()
    };
    let r = 2.0_f64;
    let rounded = fillet(&block, &inner, r).unwrap();
    rounded.check().unwrap();
    // The top loses a strip along each edge and the square between them.
    assert!((top_of(&rounded) - (500.0 - 2.0 * 20.0 * r - r * r)).abs() < 1.0e-9);
    // What a round takes from a square corner, along both edges; and in
    // the corner square, the shallower of the two rounds at every point.
    let dip = |a: f64| r - (r * r - (r - a) * (r - a)).sqrt();
    let n = 800;
    let mut corner = 0.0;
    for i in 0..n {
        for j in 0..n {
            let (a, b) = (
                (i as f64 + 0.5) * r / n as f64,
                (j as f64 + 0.5) * r / n as f64,
            );
            corner += dip(a).min(dip(b));
        }
    }
    corner *= (r / n as f64).powi(2);
    let taken = 40.0 * r * r * (1.0 - 0.25 * std::f64::consts::PI) + corner;
    assert!(
        (mass(&block).volume - mass(&rounded).volume - taken).abs() < 1.0e-4,
        "took {} for {taken}",
        mass(&block).volume - mass(&rounded).volume
    );
    assert_eq!(count(&rounded, SurfKind::Cylinder), 2);
    assert_eq!(count(&rounded, SurfKind::Plane), 8);

    // Chamfers: two flats that meet along a diagonal.
    let d = 1.5;
    let broken = chamfer(&block, &inner, d, None).unwrap();
    broken.check().unwrap();
    assert!((top_of(&broken) - (500.0 - 2.0 * 20.0 * d - d * d)).abs() < 1.0e-9);
    // Each chamfer takes a triangular prism; in the corner square what is
    // under both is a pyramid lying on its side.
    let taken = 40.0 * 0.5 * d * d + d * d * d / 3.0;
    assert!((mass(&block).volume - mass(&broken).volume - taken).abs() < 1.0e-9);
    assert_eq!(count(&broken, SurfKind::Plane), 10);
}

/// A rod of radius 5 with its top cut off on a slant, and the block that
/// cut it.
fn slanted_rod() -> (Body, V3, V3) {
    use limo_cad_kernel::make::{Region, Seg};
    use limo_cad_kernel::Frame;
    let rod = make::cylinder(V3::ZERO, V3::Z, 5.0, 20.0).unwrap();
    let normal = v3(0.3, 0.0, 1.0).unit();
    let f = Frame::from_z(v3(0.0, 0.0, 15.0), normal);
    let p: Vec<V3> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|&(x, y): &(f64, f64)| f.o + f.x * (x * 30.0) + f.y * (y * 30.0))
        .collect();
    let region = Region {
        loops: vec![(0..4)
            .map(|i| Seg::Line {
                a: p[i],
                b: p[(i + 1) % 4],
            })
            .collect()],
    };
    let above = make::extrude(&region, normal, 0.0, 30.0).unwrap();
    (boolean(&rod, &above, Op::Cut).unwrap(), f.o, normal)
}

/// Edges of a body that are neither straight nor circles.
fn bent_edges(body: &Body) -> Vec<u32> {
    (0..body.edge_count() as u32)
        .filter(|&e| {
            !matches!(
                body.e_curve_ref(e).kind,
                CurveKind::Line | CurveKind::Circle
            )
        })
        .collect()
}

/// A body that holds up to its own geometry and meshes to a clean skin,
/// coarse and fine.
fn sound(body: &Body) {
    use limo_cad_kernel::audit::{audit, CLOSE};
    use limo_cad_kernel::tess::{tessellate, TessOptions};
    audit(body, CLOSE).unwrap();
    for (linear, angular) in [(0.05, 0.35), (0.002, 0.04)] {
        let options = TessOptions { linear, angular };
        tessellate(body, &options)
            .check(body, &options)
            .unwrap_or_else(|flaw| panic!("meshed to {linear}: {flaw}"));
    }
}

#[test]
fn a_ball_rolled_round_a_rim_that_is_an_ellipse() {
    let (slanted, on_plane, normal) = slanted_rod();
    let rim = bent_edges(&slanted);
    assert_eq!(rim.len(), 1);
    assert_eq!(slanted.e_curve_ref(rim[0]).kind, CurveKind::Ellipse);
    let whole = mass(&slanted).volume;
    let mut before = whole;
    for radius in [0.4, 1.0, 2.0] {
        let rounded = fillet(&slanted, &rim, radius).unwrap();
        assert_eq!(rounded.face_count(), 4);
        sound(&rounded);
        // The bigger the ball, the more it takes.
        let volume = mass(&rounded).volume;
        assert!(volume < before && volume > 0.9 * whole, "{volume}");
        before = volume;
        // The round is the ball's own surface: its spine is the radius
        // from the cut and from the rod's side all the way round, and
        // where the ball touches lies in each of them.
        let face = (0..rounded.face_count() as u32)
            .find(|&f| rounded.f_surface(f).kind == SurfKind::Pipe)
            .expect("a round");
        let s = rounded.f_surface(face);
        let pipe = s.pipe_form();
        for i in 0..64 {
            let u = pipe.span[0] + TAU * i as f64 / 64.0;
            let centre = pipe.spine.point(u);
            assert!(((centre - on_plane).dot(normal) + radius).abs() < 1.0e-12);
            assert!((5.0 - centre.x.hypot(centre.y) - radius).abs() < 1.0e-12);
            assert!((s.point(u, 0.0) - on_plane).dot(normal).abs() < 1.0e-12);
            let other = s.uv(pipe.other.project(centre));
            let touch = s.point(other.x, other.y);
            assert!((touch.x.hypot(touch.y) - 5.0).abs() < 1.0e-12);
            // And halfway between the two it is the radius from the spine.
            let between = s.point(u, 0.5 * other.y);
            assert!((between.dist(centre) - radius).abs() < 1.0e-12);
        }
        // Through STEP the round goes as a spline fitted to it.
        let text = limo_cad_kernel::step::write(&[("rounded".to_string(), &rounded)], "test");
        let back = limo_cad_kernel::step::read(&text).unwrap();
        assert_eq!(back[0].face_count(), 4);
        back[0].check().unwrap();
        let again = mass(&back[0]).volume;
        assert!((again - volume).abs() < 1.0e-6 * volume, "{again} {volume}");
    }
    // Too big a ball has nowhere to roll.
    assert!(fillet(&slanted, &rim, 6.0).is_err());
}

#[test]
fn rounds_where_one_bore_runs_into_another() {
    let block = make::block(v3(-20.0, -20.0, -15.0), v3(20.0, 20.0, 15.0)).unwrap();
    let big = make::cylinder(v3(0.0, 0.0, -20.0), V3::Z, 8.0, 40.0).unwrap();
    let small = make::cylinder(v3(-30.0, 0.0, 0.0), V3::X, 4.0, 60.0).unwrap();
    let bored = boolean(&boolean(&block, &big, Op::Cut).unwrap(), &small, Op::Cut).unwrap();
    let saddles = bent_edges(&bored);
    assert_eq!(saddles.len(), 2);
    assert!(saddles.iter().all(|&e| bored.e_closed(e)));
    let eased = fillet(&bored, &saddles, 0.8).unwrap();
    assert_eq!(eased.face_count(), bored.face_count() + 2);
    assert_eq!(count(&eased, SurfKind::Pipe), 2);
    sound(&eased);
    // The ridge between the two bores is what goes.
    let (before, after) = (mass(&bored).volume, mass(&eased).volume);
    assert!(after < before && after > before - 20.0, "{before} {after}");
    // One at a time comes to the same thing.
    let one = fillet(&bored, &saddles[..1], 0.8).unwrap();
    // (The first round's own rims are bent edges too, and smooth: the
    // saddle that is left is the one that is still a crease.)
    let left: Vec<u32> = bent_edges(&one)
        .into_iter()
        .filter(|&e| one.e_curve_ref(e).kind == CurveKind::Trace)
        .collect();
    assert_eq!(left.len(), 1);
    let both = fillet(&one, &left, 0.8).unwrap();
    assert!((mass(&both).volume - after).abs() < 1.0e-7 * after);
}

#[test]
fn a_round_that_fills_where_a_rod_stands_on_a_slope() {
    use limo_cad_kernel::make::{Region, Seg};
    use limo_cad_kernel::Frame;
    // A block with a sloping top, and a rod standing up through the slope.
    let block = make::block(v3(-20.0, -20.0, 0.0), v3(20.0, 20.0, 20.0)).unwrap();
    let normal = v3(0.25, 0.1, 1.0).unit();
    let f = Frame::from_z(v3(0.0, 0.0, 10.0), normal);
    let p: Vec<V3> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|&(x, y): &(f64, f64)| f.o + f.x * (x * 60.0) + f.y * (y * 60.0))
        .collect();
    let region = Region {
        loops: vec![(0..4)
            .map(|i| Seg::Line {
                a: p[i],
                b: p[(i + 1) % 4],
            })
            .collect()],
    };
    let above = make::extrude(&region, normal, 0.0, 60.0).unwrap();
    let slope = boolean(&block, &above, Op::Cut).unwrap();
    let rod = make::cylinder(v3(2.0, -1.0, 2.0), V3::Z, 5.0, 30.0).unwrap();
    let post = boolean(&slope, &rod, Op::Union).unwrap();
    let foot = bent_edges(&post);
    assert_eq!(foot.len(), 1);
    let filled = fillet(&post, &foot, 1.5).unwrap();
    assert_eq!(count(&filled, SurfKind::Pipe), 1);
    sound(&filled);
    let (before, after) = (mass(&post).volume, mass(&filled).volume);
    assert!(after > before && after < before + 60.0, "{before} {after}");
}

/// A plate 40 by 25 by 10 with a boss 20 by 10 standing 10 high on it.
fn boss_on_a_plate() -> Body {
    let plate = make::block(V3::ZERO, v3(40.0, 25.0, 10.0)).unwrap();
    let boss = make::block(v3(10.0, 8.0, 10.0), v3(30.0, 18.0, 20.0)).unwrap();
    boolean(&plate, &boss, Op::Union).unwrap()
}

/// The plate with a pocket 20 by 10 and 6 deep in its top instead.
fn pocketed_plate() -> Body {
    let plate = make::block(V3::ZERO, v3(40.0, 25.0, 10.0)).unwrap();
    let pocket = make::block(v3(10.0, 8.0, 4.0), v3(30.0, 18.0, 11.0)).unwrap();
    boolean(&plate, &pocket, Op::Cut).unwrap()
}

/// The straight edge whose middle is at `m`.
fn edge_at(body: &Body, m: V3) -> u32 {
    let found = edges_where(body, |mid, _| (mid - m).len() < 1.0e-9);
    assert_eq!(found.len(), 1, "edges with their middle at {m:?}");
    found[0]
}

/// Whether what a body holds is `expected` to the last figure that can be
/// asked of it. These bodies are flats, cylinders and balls, which are
/// weighed exactly: a corner put together wrongly is out in the fourth or
/// fifth figure, where the drawing of a round face would hide it.
fn holds(body: &Body, expected: f64) {
    let held = mass(body).volume;
    assert!(
        (held - expected).abs() <= 1.0e-10 * expected,
        "holds {held:.9}, not {expected:.9}: {:+.9} out",
        held - expected
    );
}

/// What a round of radius 2 fills an inside edge with, or takes off an
/// outside one, for each unit of the edge's length.
const SLIVER: f64 = 4.0 - PI;

/// Two rounds along the foot of a boss, round one of its corners. Each
/// fills its own edge, and each stops where its edge does. Between their
/// two ends is a corner neither has filled, and they run on into it until
/// they cross: what is filled there is the corner cube less what lies
/// inside either of the two cylinders. Left unfilled it is a notch the
/// size of the radius, in a body that is otherwise sound.
#[test]
fn rounds_round_the_foot_of_a_boss_run_into_each_other() {
    let body = boss_on_a_plate();
    let crossing = 8.0 * (5.0 / 3.0 - 0.5 * PI);
    let (right, back) = (
        edge_at(&body, v3(30.0, 13.0, 10.0)),
        edge_at(&body, v3(20.0, 18.0, 10.0)),
    );
    let two = fillet(&body, &[right, back], 2.0).unwrap();
    two.check().unwrap();
    holds(&two, 12000.0 + SLIVER * 30.0 + crossing);
    assert_eq!(count(&two, SurfKind::Cylinder), 2);
    // They meet along a mitre, as two rounds on outside edges do.
    let mitres = |body: &Body| {
        (0..body.edge_count() as u32)
            .filter(|&e| body.e_curve_ref(e).kind == CurveKind::Ellipse)
            .count()
    };
    assert_eq!(mitres(&two), 1);

    let foot = edges_where(&body, |m, _| {
        (m.z - 10.0).abs() < 1e-9 && m.x > 5.0 && m.x < 35.0 && m.y > 5.0 && m.y < 20.0
    });
    assert_eq!(foot.len(), 4);
    let all = fillet(&body, &foot, 2.0).unwrap();
    all.check().unwrap();
    holds(&all, 12000.0 + SLIVER * 60.0 + 4.0 * crossing);
    assert_eq!((count(&all, SurfKind::Cylinder), mitres(&all)), (4, 4));

    // Bevels the same: two flats that run on and cross in a pyramid.
    let broken = chamfer(&body, &[right, back], 2.0, None).unwrap();
    broken.check().unwrap();
    holds(&broken, 12000.0 + 2.0 * 30.0 + 8.0 / 3.0);
}

/// Three rounds in the corner of a pocket. A ball of the radius rests in
/// the corner against all three walls, and what is filled there is the
/// corner cube less the ball. Three cylinders crossing each other fill
/// less: what lies inside all three of them is more than the ball.
#[test]
fn a_ball_rests_in_the_corner_of_a_pocket() {
    let body = pocketed_plate();
    let corner = [
        edge_at(&body, v3(30.0, 13.0, 4.0)),
        edge_at(&body, v3(20.0, 18.0, 4.0)),
        edge_at(&body, v3(30.0, 18.0, 7.0)),
    ];
    let rounded = fillet(&body, &corner, 2.0).unwrap();
    rounded.check().unwrap();
    // Along each edge up to the corner cube, and in the cube.
    holds(
        &rounded,
        8800.0 + SLIVER * (8.0 + 18.0 + 4.0) + 8.0 * (1.0 - PI / 6.0),
    );
    assert_eq!(
        (
            count(&rounded, SurfKind::Sphere),
            count(&rounded, SurfKind::Cylinder)
        ),
        (1, 3)
    );
    // The two on the floor alone cross as they are, in a mitre.
    let floor = fillet(&body, &corner[..2], 2.0).unwrap();
    holds(
        &floor,
        8800.0 + SLIVER * 30.0 - 8.0 * (5.0 / 3.0 - 0.5 * PI),
    );
}

/// The ball rests in a corner that is not square just the same: its centre
/// is the radius from each of the three faces, wherever that puts it.
#[test]
fn a_ball_rests_in_a_corner_that_is_not_square() {
    // A six-sided post, and one corner of its top.
    let at = |i: usize| {
        let a = TAU * i as f64 / 6.0 + 0.3;
        v3(12.0 * a.cos(), 12.0 * a.sin(), 0.0)
    };
    let segs: Vec<Seg> = (0..6)
        .map(|i| Seg::Line {
            a: at(i),
            b: at(i + 1),
        })
        .collect();
    let post = make::extrude(&Region { loops: vec![segs] }, V3::Z, 0.0, 20.0).unwrap();
    let up = v3(0.0, 0.0, 20.0);
    let corner = [
        edge_at(&post, (at(0) + at(1)) * 0.5 + up),
        edge_at(&post, (at(1) + at(2)) * 0.5 + up),
        edge_at(&post, at(1) + up * 0.5),
    ];
    let r = 2.0;
    let rounded = fillet(&post, &corner, r).unwrap();
    rounded.check().unwrap();
    assert_eq!(count(&rounded, SurfKind::Sphere), 1);
    let ball = (0..rounded.face_count() as u32)
        .map(|f| *rounded.f_surface(f))
        .find(|s| s.kind == SurfKind::Sphere)
        .unwrap();
    assert!((ball.r - r).abs() < 1e-12);
    // Its centre: the radius below the top and inside the two walls.
    let centre = ball.f.o;
    assert!((20.0 - centre.z - r).abs() < 1e-9, "{centre:?}");
    for i in [0, 1] {
        let along = (at(i + 1) - at(i)).unit();
        let inward = V3::Z.cross(along);
        assert!(
            ((centre - at(i)).dot(inward) - r).abs() < 1e-9,
            "{centre:?}"
        );
    }
    // And each round runs into it without a crease: the cylinders pass
    // through the ball's centre.
    for f in 0..rounded.face_count() as u32 {
        let s = rounded.f_surface(f);
        if s.kind == SurfKind::Cylinder {
            let off = centre - s.f.o;
            assert!((off - s.f.z * off.dot(s.f.z)).len() < 1e-9);
        }
    }
}

/// Three even bevels at a square corner run together in a point, and the
/// corner is not left as that point: it is cut flat, through the three
/// places where two of the bevels meet on a face.
#[test]
fn three_bevels_at_a_square_corner_are_cut_flat() {
    let block = make::block(V3::ZERO, v3(40.0, 25.0, 10.0)).unwrap();
    let corner = [
        edge_at(&block, v3(40.0, 12.5, 10.0)),
        edge_at(&block, v3(20.0, 25.0, 10.0)),
        edge_at(&block, v3(40.0, 25.0, 5.0)),
    ];
    let d: f64 = 2.0;
    let broken = chamfer(&block, &corner, d, None).unwrap();
    broken.check().unwrap();
    // Three prisms, less what any two share, plus what all three share,
    // and the pyramid under the flat.
    let taken = 0.5 * d * d * (25.0 + 40.0 + 10.0) - d.powi(3) + d.powi(3) / 4.0 + d.powi(3) / 12.0;
    holds(&broken, 10000.0 - taken);
    // Six faces of the block, three bevels and the flat between them.
    assert_eq!(count(&broken, SurfKind::Plane), 10);
    // In the corner of a pocket the same flat is filled in.
    let pocket = pocketed_plate();
    let inside = [
        edge_at(&pocket, v3(30.0, 13.0, 4.0)),
        edge_at(&pocket, v3(20.0, 18.0, 4.0)),
        edge_at(&pocket, v3(30.0, 18.0, 7.0)),
    ];
    let filled = chamfer(&pocket, &inside, d, None).unwrap();
    filled.check().unwrap();
    let added = 0.5 * d * d * (10.0 + 20.0 + 6.0) - d.powi(3) + d.powi(3) / 4.0 + d.powi(3) / 12.0;
    holds(&filled, 8800.0 + added);
}

/// A corner where blended edges meet is finished exactly or not at all.
/// The tools of the edges are right each on its own; left to meet as they
/// are at a corner nobody has worked out, they make a sound solid that is
/// a tenth of the radius cubed away from the blend that was asked for.
#[test]
fn a_corner_that_cannot_be_finished_is_refused() {
    let body = boss_on_a_plate();
    // An outside edge and an inside edge: an upright of the boss and the
    // foot it stands on.
    let mixed = [
        edge_at(&body, v3(30.0, 18.0, 15.0)),
        edge_at(&body, v3(30.0, 13.0, 10.0)),
    ];
    for refused in [
        fillet(&body, &mixed, 2.0),
        chamfer(&body, &mixed, 2.0, None),
    ] {
        let said = refused.unwrap_err();
        assert!(
            said.contains("an inside edge and an outside edge"),
            "{said}"
        );
    }
    // Each of them alone is no trouble.
    for one in mixed {
        fillet(&body, &[one], 2.0).unwrap().check().unwrap();
    }
    // Three bevels at a corner that is not square.
    let at = |i: usize| {
        let a = TAU * i as f64 / 6.0 + 0.3;
        v3(12.0 * a.cos(), 12.0 * a.sin(), 0.0)
    };
    let segs: Vec<Seg> = (0..6)
        .map(|i| Seg::Line {
            a: at(i),
            b: at(i + 1),
        })
        .collect();
    let post = make::extrude(&Region { loops: vec![segs] }, V3::Z, 0.0, 20.0).unwrap();
    let up = v3(0.0, 0.0, 20.0);
    let corner = [
        edge_at(&post, (at(0) + at(1)) * 0.5 + up),
        edge_at(&post, (at(1) + at(2)) * 0.5 + up),
        edge_at(&post, at(1) + up * 0.5),
    ];
    let said = chamfer(&post, &corner, 2.0, None).unwrap_err();
    assert!(said.contains("cannot be finished"), "{said}");
    // Two of them cross as they are, and that is right.
    chamfer(&post, &corner[..2], 2.0, None)
        .unwrap()
        .check()
        .unwrap();
}
/// A boss on a plate 60 square and 10 thick. Its outline is 20 by 12 where
/// it starts, on the underside of the plate, and smaller by `lean` of
/// itself for every unit it rises: its sides lean in when `lean` is more
/// than nothing and hang over when it is less. It ends 20 up.
///
/// Its two pairs of sides lean differently, the long sides less than the
/// short ones, so that at a corner nothing is the mirror of anything.
struct LeaningBoss {
    lean: f64,
}

/// A blend's section beside a face that leans: how far out from the face
/// it is, at a distance from the flat face it starts on.
#[derive(Clone, Copy)]
struct Beside {
    /// How much further in the leaning face is for each unit of distance
    /// from the flat one.
    slope: f64,
    /// The radius of a round, or the distance of a bevel.
    size: f64,
    round: bool,
}

impl Beside {
    /// The distance from the flat face at which the blend runs out.
    fn ends(&self) -> f64 {
        let lean = self.slope.atan();
        if self.round {
            self.size / (0.25 * PI + 0.5 * lean).tan() * lean.cos()
        } else {
            self.size * lean.cos()
        }
    }

    fn width(&self, h: f64) -> f64 {
        if h >= self.ends() {
            return 0.0;
        }
        let lean = self.slope.atan();
        if self.round {
            let r = self.size;
            r / (0.25 * PI + 0.5 * lean).tan() + self.slope * h - (2.0 * r * h - h * h).sqrt()
        } else {
            self.size - h * ((1.0 + lean.sin()) / lean.cos() - self.slope)
        }
    }

    /// The integral of `f(h) * width(h)` over the blend's height. For a
    /// round the height is run through as `r (1 - cos a)`, which takes the
    /// square root out of the end where the round starts.
    fn over(&self, upto: f64, f: impl Fn(f64) -> f64) -> f64 {
        let upto = upto.min(self.ends());
        if self.round {
            let r = self.size;
            integral(0.0, (1.0 - upto / r).acos(), |a| {
                let h = r * (1.0 - a.cos());
                f(h) * self.width(h) * r * a.sin()
            })
        } else {
            integral(0.0, upto, |h| f(h) * self.width(h))
        }
    }
}

impl LeaningBoss {
    fn body(&self) -> Body {
        let plate = make::block(v3(-30.0, -30.0, 0.0), v3(30.0, 30.0, 10.0)).unwrap();
        let c = [
            v3(-10.0, -6.0, 0.0),
            v3(10.0, -6.0, 0.0),
            v3(10.0, 6.0, 0.0),
            v3(-10.0, 6.0, 0.0),
        ];
        let segs = (0..4)
            .map(|i| Seg::Line {
                a: c[i],
                b: c[(i + 1) % 4],
            })
            .collect();
        let region = Region { loops: vec![segs] };
        let boss = make::taper(&region, V3::Z, 0.0, 20.0, V3::ZERO, -self.lean).unwrap();
        boolean(&plate, &boss, Op::Union).unwrap()
    }

    /// Half the boss's length and half its width at height `z`.
    fn half(&self, z: f64) -> (f64, f64) {
        (10.0 * (1.0 - self.lean * z), 6.0 * (1.0 - self.lean * z))
    }

    /// The straight edge nearest a point: the dimensions of a leaning boss
    /// are not round numbers.
    fn edge(&self, body: &Body, near: V3) -> u32 {
        (0..body.edge_count() as u32)
            .filter(|&e| body.e_curve_ref(e).kind == CurveKind::Line)
            .min_by(|&a, &b| {
                (body.e_mid(a) - near)
                    .len()
                    .total_cmp(&(body.e_mid(b) - near).len())
            })
            .unwrap()
    }

    /// The edges of one corner: at the foot of the short side and of the
    /// long side (`z` 10), or on top of them (`z` 20), and the upright
    /// between the two.
    fn corner(&self, body: &Body, z: f64) -> (u32, u32) {
        let (a, b) = self.half(z);
        (
            self.edge(body, v3(a, 0.0, z)),
            self.edge(body, v3(0.0, b, z)),
        )
    }

    fn upright(&self, body: &Body) -> u32 {
        let (a, b) = self.half(15.0);
        self.edge(body, v3(a, b, 15.0))
    }

    /// What blends on the short side, the long side and both change, at
    /// the foot (`top` false: they fill in) or on top (they take away).
    /// Slice by slice a blend is a strip along its side, as wide as its
    /// section is there, and two of them at a corner are two strips that
    /// cross: at the foot the corner between them is filled as far out as
    /// both reach, on top what both take is taken once.
    fn changes(&self, top: bool, size: f64, round: bool) -> (f64, f64, f64) {
        let (kx, ky) = (10.0 * self.lean, 6.0 * self.lean);
        let (short, long) = (
            Beside {
                slope: kx,
                size,
                round,
            },
            Beside {
                slope: ky,
                size,
                round,
            },
        );
        // Half the boss's size at a distance from the plate or the top.
        let at = |h: f64| {
            if top {
                self.half(20.0 - h)
            } else {
                self.half(10.0 + h)
            }
        };
        let one = short.over(f64::MAX, |h| 2.0 * at(h).1);
        let other = long.over(f64::MAX, |h| 2.0 * at(h).0);
        let both = short.over(long.ends(), |h| long.width(h));
        if top {
            (-one, -other, -one - other + both)
        } else {
            (one, other, one + other + both)
        }
    }

    /// What a blend on one upright edge takes: its section, for as long as
    /// the edge is.
    fn upright_takes(&self, size: f64, round: bool) -> f64 {
        let (kx, ky) = (10.0 * self.lean, 6.0 * self.lean);
        let between = PI - (kx * ky / ((1.0 + kx * kx) * (1.0 + ky * ky)).sqrt()).acos();
        let section = if round {
            size * size / (0.5 * between).tan() - 0.5 * (PI - between) * size * size
        } else {
            0.5 * size * size * between.sin()
        };
        section * 10.0 * (1.0 + kx * kx + ky * ky).sqrt()
    }
}

/// A blend ends on the face that ends its edge, and that face may lean. At
/// the foot of a boss whose sides lean, a round stops on the side next to
/// it, further along its edge the higher up it is; where two meet at a
/// corner, each stops on the other's side and they cross in between. All
/// of it has a value that can be had slice by slice, for sides that lean
/// in and for sides that hang over.
#[test]
fn blends_end_on_faces_that_lean() {
    for lean in [1.0, -1.0] {
        let boss = LeaningBoss {
            lean: lean * 8.0f64.to_radians().tan() / 136.0f64.sqrt(),
        };
        let body = boss.body();
        let whole = mass(&body).volume;
        for (round, size) in [(true, 2.0), (false, 2.0)] {
            let blend = |edges: &[u32]| {
                let made = if round {
                    fillet(&body, edges, size)
                } else {
                    chamfer(&body, edges, size, None)
                }
                .unwrap();
                made.check().unwrap();
                made
            };
            for top in [false, true] {
                let (short, long) = boss.corner(&body, if top { 20.0 } else { 10.0 });
                let (one, other, both) = boss.changes(top, size, round);
                holds(&blend(&[short]), whole + one);
                holds(&blend(&[long]), whole + other);
                holds(&blend(&[short, long]), whole + both);
            }
            holds(
                &blend(&[boss.upright(&body)]),
                whole - boss.upright_takes(size, round),
            );
        }
    }
}

/// A blend stops where its edge does. It is never carried on past the end
/// of the edge with nothing to stop it: a second boss a millimetre from
/// the first, in line with the edge, is not touched, and neither is a step
/// that goes on below the edge's end.
#[test]
fn a_blend_stops_where_its_edge_does() {
    let plate = make::block(V3::ZERO, v3(60.0, 40.0, 10.0)).unwrap();
    let first = make::block(v3(5.0, 10.0, 10.0), v3(25.0, 30.0, 20.0)).unwrap();
    for (gap, height) in [(1.0, 20.0), (1.0, 19.0), (0.0, 19.0)] {
        let second = make::block(v3(25.0 + gap, 10.0, 10.0), v3(46.0, 30.0, height)).unwrap();
        let body = boolean(
            &boolean(&plate, &first, Op::Union).unwrap(),
            &second,
            Op::Union,
        )
        .unwrap();
        let whole = mass(&body).volume;
        let edge = edge_at(&body, v3(15.0, 10.0, 20.0));
        let rounded = fillet(&body, &[edge], 2.0).unwrap();
        rounded.check().unwrap();
        holds(&rounded, whole - SLIVER * 20.0);
        holds(
            &chamfer(&body, &[edge], 2.0, None).unwrap(),
            whole - 2.0 * 20.0,
        );
    }
}

/// A blend wider than a face beside its edge is refused. Carried out, it
/// would run on past the far side of that face: off a plate thinner than
/// the round and into the open, over the top of a step lower than it, or
/// below the foot of one and into what the step stands on. Each of those
/// is a closed solid, and none of them is a blend between the two faces.
#[test]
fn a_blend_wider_than_its_face_is_refused() {
    let said = "too large for one of the faces beside its edge";
    let thin = make::block(V3::ZERO, v3(20.0, 10.0, 1.0)).unwrap();
    let edge = edge_at(&thin, v3(10.0, 0.0, 1.0));
    assert!(fillet(&thin, &[edge], 2.0).unwrap_err().contains(said));
    assert!(chamfer(&thin, &[edge], 2.0, None)
        .unwrap_err()
        .contains(said));
    // As wide as the face is no wider than it.
    holds(
        &fillet(&thin, &[edge], 1.0).unwrap(),
        200.0 - 0.25 * SLIVER * 20.0,
    );

    let block = make::block(V3::ZERO, v3(20.0, 10.0, 10.0)).unwrap();
    let step = make::block(v3(0.0, 0.0, 10.0), v3(10.0, 10.0, 11.0)).unwrap();
    let stepped = boolean(&block, &step, Op::Union).unwrap();
    for z in [10.0, 11.0] {
        let edge = edge_at(&stepped, v3(10.0, 5.0, z));
        assert!(fillet(&stepped, &[edge], 2.0).unwrap_err().contains(said));
        fillet(&stepped, &[edge], 0.5).unwrap().check().unwrap();
    }
    // Two rounds that would need the same strip of a face between them.
    let bar = make::block(V3::ZERO, v3(20.0, 3.0, 10.0)).unwrap();
    let (near, far) = (
        edge_at(&bar, v3(10.0, 0.0, 10.0)),
        edge_at(&bar, v3(10.0, 3.0, 10.0)),
    );
    assert!(fillet(&bar, &[near, far], 2.0).unwrap_err().contains(said));
    holds(
        &fillet(&bar, &[near, far], 1.5).unwrap(),
        600.0 - 2.0 * 0.5625 * SLIVER * 20.0,
    );
}

/// What a round takes away may run over a hole in one of its faces: there
/// is nothing in the hole to take, and the round is cut through by it. A
/// round that fills an inside edge may not: it would be built over the
/// hole.
#[test]
fn a_round_runs_over_a_hole_but_is_not_built_over_one() {
    let block = make::block(V3::ZERO, v3(20.0, 10.0, 10.0)).unwrap();
    // A hole of radius 1 whose middle is 2 from the edge.
    let drill = make::cylinder(v3(10.0, 2.0, -1.0), V3::Z, 1.0, 12.0).unwrap();
    let holed = boolean(&block, &drill, Op::Cut).unwrap();
    let whole = mass(&holed).volume;
    let rounded = fillet(&holed, &[edge_at(&holed, v3(10.0, 0.0, 10.0))], 2.0).unwrap();
    rounded.check().unwrap();
    // The sliver all along the edge, less what of it was in the hole
    // already: over half the hole's disc, the sliver's height.
    let in_the_hole = integral(0.0, 0.5 * PI, |a| {
        let u = a.sin();
        2.0 * a.cos() * (2.0 - (4.0 - u * u).sqrt()) * a.cos()
    });
    let held = mass(&rounded).volume;
    let expected = whole - SLIVER * 20.0 + in_the_hole;
    assert!(
        (held - expected).abs() < 1.0e-8 * expected,
        "holds {held:.9}, not {expected:.9}"
    );

    // A hole as near the foot of a wall that stands on the block.
    let wall = make::block(v3(0.0, 0.0, 10.0), v3(20.0, 3.0, 20.0)).unwrap();
    let drill = make::cylinder(v3(10.0, 5.0, -1.0), V3::Z, 1.0, 12.0).unwrap();
    let holed = boolean(&boolean(&block, &wall, Op::Union).unwrap(), &drill, Op::Cut).unwrap();
    let foot = edge_at(&holed, v3(10.0, 3.0, 10.0));
    let said = fillet(&holed, &[foot], 2.0).unwrap_err();
    assert!(
        said.contains("too large for one of the faces beside its edge"),
        "{said}"
    );
}

/// A boss the shape of a disc with a slice off it, as the end of a shaft
/// with a flat on it is. Round its top there are two edges, an arc and a
/// straight one, and neither runs on into the other. The arc ends on the
/// flat, which does not pass through the arc's axis, so its round is not
/// ended where the arc ends but cut off by the flat, as the round of a
/// straight edge is by a face that leans; and the straight edge's round
/// is cut off by the round side.
///
/// Slice by slice each round is a strip along its edge inside the outline,
/// and both together are the outline less the outline moved in.
#[test]
fn an_arc_ends_on_a_flat_that_misses_its_axis() {
    let (radius, flat, r) = (10.0f64, 4.0f64, 2.0f64);
    let half = (radius * radius - flat * flat).sqrt();
    let plate = make::block(v3(-30.0, -30.0, 0.0), v3(30.0, 30.0, 10.0)).unwrap();
    let outline = vec![
        Seg::Line {
            a: v3(flat, -half, 0.0),
            b: v3(flat, half, 0.0),
        },
        Seg::Arc {
            a: v3(flat, half, 0.0),
            m: v3(-radius, 0.0, 0.0),
            b: v3(flat, -half, 0.0),
        },
    ];
    let boss = make::extrude(
        &Region {
            loops: vec![outline],
        },
        V3::Z,
        10.0,
        20.0,
    )
    .unwrap();
    let body = boolean(&plate, &boss, Op::Union).unwrap();
    let whole = mass(&body).volume;
    let on_top = |kind: CurveKind| -> u32 {
        let found: Vec<u32> = (0..body.edge_count() as u32)
            .filter(|&e| {
                body.e_curve_ref(e).kind == kind && (body.e_mid(e).z - 20.0).abs() < 1.0e-9
            })
            .collect();
        assert_eq!(found.len(), 1);
        found[0]
    };
    let (arc, straight) = (on_top(CurveKind::Circle), on_top(CurveKind::Line));
    // What of a disc of radius `of` lies beyond the line `x = at`.
    let beyond = |of: f64, at: f64| of * of * (at / of).acos() - at * (of * of - at * at).sqrt();
    // The outline moved in by `w`, all round or on one side only.
    let within = |round: f64, straight: f64| {
        let of = radius - round;
        PI * of * of - beyond(of, flat - straight)
    };
    // A round's strip is as wide, at depth `h` under the top, as its
    // section is there.
    let taken = |strip: &dyn Fn(f64) -> f64| {
        integral(0.0, 0.5 * PI, |a| strip(r * (1.0 - a.sin())) * r * a.sin())
    };
    for (edges, strip) in [
        (
            vec![arc],
            &(|w: f64| within(0.0, 0.0) - within(w, 0.0)) as &dyn Fn(f64) -> f64,
        ),
        (vec![straight], &|w: f64| within(0.0, 0.0) - within(0.0, w)),
        (vec![arc, straight], &|w: f64| {
            within(0.0, 0.0) - within(w, w)
        }),
    ] {
        let rounded = fillet(&body, &edges, r).unwrap();
        rounded.check().unwrap();
        holds(&rounded, whole - taken(strip));
    }
}

/// The rim of a round boss with a narrow slot cut into it is an arc that
/// ends on both walls of the slot. Neither wall passes through the boss's
/// axis, and the arc comes round to each wall's own flat again on the far
/// side of the boss, so a wall cannot cut through the whole of the round:
/// it pares it at the end that is on it. Slice by slice the round is a
/// ring inside the boss's outline, less what of that ring the slot has
/// taken already.
///
/// Wherever the slot is cut the round is the same. Opposite the seam of
/// the boss's side the rim is two arcs, each of which reaches the other
/// wall's flat at its far end; at the seam it is one arc of nearly a full
/// turn, whose middle is on the far side of both walls.
#[test]
fn a_rim_that_ends_on_both_walls_of_a_slot() {
    let (radius, gap, r) = (18.5f64, 0.6f64, 0.5f64);
    let plate = make::block(v3(-30.0, -30.0, 0.0), v3(30.0, 30.0, 10.0)).unwrap();
    let boss = make::cylinder(v3(0.0, 0.0, 10.0), V3::Z, radius, 14.0).unwrap();
    let whole = boolean(&plate, &boss, Op::Union).unwrap();
    // What of a disc of radius `of` lies in the slot's width, on one side
    // of the middle.
    let in_the_slot = |of: f64| gap * (of * of - gap * gap).sqrt() + of * of * (gap / of).asin();
    let ring = |w: f64| {
        PI * (radius * radius - (radius - w) * (radius - w))
            - (in_the_slot(radius) - in_the_slot(radius - w))
    };
    let round_takes = integral(0.0, 0.5 * PI, |a| ring(r * (1.0 - a.sin())) * r * a.sin());
    // A bevel's section is as wide as it is deep.
    let bevel_takes = integral(0.0, r, |h| ring(r - h));
    for (lo, hi) in [
        (v3(-gap, -25.0, 10.0), v3(gap, -10.0, 30.0)),
        (v3(-gap, 10.0, 10.0), v3(gap, 25.0, 30.0)),
        (v3(-25.0, -gap, 10.0), v3(-10.0, gap, 30.0)),
        (v3(10.0, -gap, 10.0), v3(25.0, gap, 30.0)),
    ] {
        let slot = make::block(lo, hi).unwrap();
        let body = boolean(&whole, &slot, Op::Cut).unwrap();
        let held = mass(&body).volume;
        let rim: Vec<u32> = (0..body.edge_count() as u32)
            .filter(|&e| {
                let c = body.e_curve_ref(e);
                c.kind == CurveKind::Circle
                    && (c.r - radius).abs() < 1.0e-9
                    && (c.f.o.z - 24.0).abs() < 1.0e-9
            })
            .collect();
        assert!(!rim.is_empty());
        let rounded = fillet(&body, &rim, r).unwrap();
        rounded.check().unwrap();
        holds(&rounded, held - round_takes);
        let cut = chamfer(&body, &rim, r, None).unwrap();
        cut.check().unwrap();
        holds(&cut, held - bevel_takes);
    }
}

/// A bracket bent from a strip: an L 32 by 44 and 8 thick, 40 wide. Its
/// bend is rounded 4 inside, the corners of its two ends 3, and then a
/// round of 0.6 is run along the rim of its two flat sides. The picked
/// edges run on through the rounded corners of the ends and across them,
/// so the round goes all the way round the inside of the bracket in one
/// chain and all the way round the outside in another, which turns two
/// square corners where the back meets the base.
///
/// Along a straight edge a round takes a sliver for each unit of length.
/// Inside a quarter turn of radius `R` it takes a quarter of a ring from
/// `R - w` to `R` at each depth, and outside one a quarter of a ring from
/// `R` to `R + w`. At a square corner two of them take the corner once.
#[test]
fn a_round_all_the_way_round_a_bracket() {
    let corners = [
        v3(0.0, 0.0, 0.0),
        v3(32.0, 0.0, 0.0),
        v3(32.0, 8.0, 0.0),
        v3(8.0, 8.0, 0.0),
        v3(8.0, 44.0, 0.0),
        v3(0.0, 44.0, 0.0),
    ];
    let strip = (0..6)
        .map(|i| Seg::Line {
            a: corners[i],
            b: corners[(i + 1) % 6],
        })
        .collect();
    let bent = make::extrude(&Region { loops: vec![strip] }, V3::Z, 0.0, 40.0).unwrap();
    let bend = fillet(&bent, &[edge_at(&bent, v3(8.0, 8.0, 20.0))], 4.0).unwrap();
    let ends = [
        v3(32.0, 4.0, 0.0),
        v3(32.0, 4.0, 40.0),
        v3(4.0, 44.0, 0.0),
        v3(4.0, 44.0, 40.0),
    ]
    .map(|m| edge_at(&bend, m));
    let body = fillet(&bend, &ends, 3.0).unwrap();
    let sliver_of = |r: f64| (1.0 - 0.25 * PI) * r * r;
    holds(
        &body,
        40.0 * (32.0 * 8.0 + 36.0 * 8.0) + sliver_of(4.0) * 40.0 - 4.0 * sliver_of(3.0) * 8.0,
    );
    let whole = mass(&body).volume;

    // One edge of each run is enough to pick, and the rest follow: the
    // base and the back of the outside, which meet at the square corners,
    // and the inside.
    let rim =
        [v3(14.5, 0.0, 0.0), v3(0.0, 20.5, 0.0), v3(20.5, 8.0, 0.0)].map(|m| edge_at(&body, m));
    let r = 0.6;
    let rounded = fillet(&body, &rim, r).unwrap();
    rounded.check().unwrap();
    // What the width of a round, squared, comes to over its depth.
    let squared = r.powi(3) * (5.0 / 3.0 - 0.5 * PI);
    let straight = 2.0 * (41.0 + 29.0 + 17.0 + 29.0) + 4.0 * 34.0;
    let inside = 8.0 * 0.25 * PI * (6.0 * sliver_of(r) - squared);
    let outside = 2.0 * 0.25 * PI * (8.0 * sliver_of(r) + squared);
    holds(
        &rounded,
        whole - (straight * sliver_of(r) + inside + outside - 2.0 * squared),
    );
}
