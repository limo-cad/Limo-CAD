use limo_cad_kernel::blend::{chamfer, fillet};
use limo_cad_kernel::make::{self, PSeg, Region, Seg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::props::mass;
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
    close(volume(&half) - volume(&rounded), 2.0 * corner * 30.0, 1e-2);
    close(volume(&rounded), before - 2.0 * corner * 30.0, 1e-4);
    assert_eq!(count(&rounded, SurfKind::Cylinder), 3);
    // Nothing sharp is left along the rod.
    assert!(seams(&rounded).is_empty());

    // Bevelled 2 along each face: along the flat, and round the rod.
    let bevelled = chamfer(&half, &edges, 2.0, None).unwrap();
    assert_sound(&bevelled);
    let corner = 0.5 * 400.0 * 0.1 - 0.5 * 18.0 * 20.0 * 0.1_f64.sin();
    close(volume(&half) - volume(&bevelled), 2.0 * corner * 30.0, 1e-2);

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
    close(volume(&filled) - volume(&ridge), 2.0 * corner * 40.0, 1e-2);
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
