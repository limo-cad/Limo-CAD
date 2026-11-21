//! Offsetting the faces of a body, and hollowing bodies out.
mod common;

use common::*;
use limo_cad_kernel::make::{self, Region, Seg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::offset::{offset_faces, shell};
use limo_cad_kernel::{blend, Body, CurveKind, SurfKind};

/// The face whose outward normal is `n` at every one of its corners.
fn facing(body: &Body, n: V3) -> u32 {
    let found: Vec<u32> = (0..body.face_count() as u32)
        .filter(|&f| {
            body.f_surface(f).is_plane()
                && body.face_hes(f).all(|h| {
                    body.f_normal(f, body.verts[body.h_origin(h) as usize])
                        .dot(n)
                        > 0.999
                })
        })
        .collect();
    assert_eq!(found.len(), 1, "faces facing {n:?}: {found:?}");
    found[0]
}

fn assert_sound(body: &Body) {
    body.check().unwrap();
    assert_flat_faces_are_covered(body);
    assert_mesh_is_a_clean_skin(body);
}

#[test]
fn a_block_grows_and_shrinks_face_by_face() {
    let block = make::block(v3(0.0, 0.0, 0.0), v3(40.0, 30.0, 10.0)).unwrap();
    // Every face out by 2: still a block, 4 larger each way.
    let grown = offset_faces(&block, &[2.0; 6]).unwrap();
    assert_sound(&grown);
    assert!((volume(&grown) - 44.0 * 34.0 * 14.0).abs() < 1e-7);
    // Only the top, down by 3.
    let mut by = [0.0; 6];
    by[facing(&block, V3::Z) as usize] = -3.0;
    let lower = offset_faces(&block, &by).unwrap();
    assert!((volume(&lower) - 40.0 * 30.0 * 7.0).abs() < 1e-7);
    // Faces and edges keep their places.
    assert_eq!(lower.face_count(), 6);
    assert_eq!(lower.e_v, block.e_v);
    // Too far in, and the block is gone.
    let gone = offset_faces(&block, &[-5.0; 6]).unwrap_err();
    assert!(gone.contains("smaller"), "{gone}");
}

#[test]
fn curved_faces_keep_their_kind() {
    // A drilled, round-cornered plate: planes, outward and inward
    // cylinders, lines where they meet smoothly, circles.
    let plate = make::block(v3(0.0, 0.0, 0.0), v3(40.0, 30.0, 10.0)).unwrap();
    let upright: Vec<u32> = (0..plate.edge_count() as u32)
        .filter(|&e| plate.e_curve_ref(e).f.x.z.abs() > 0.999)
        .collect();
    let plate = blend::fillet(&plate, &upright, 6.0).unwrap();
    let drill = make::cylinder(v3(20.0, 15.0, -1.0), V3::Z, 4.0, 12.0).unwrap();
    let plate = limo_cad_kernel::boolean(&plate, &drill, limo_cad_kernel::Op::Cut).unwrap();
    let rounded = |w: f64, h: f64, r: f64| w * h - (4.0 - PI) * r * r;
    let before = (rounded(40.0, 30.0, 6.0) - PI * 16.0) * 10.0;
    assert!(
        (volume(&plate) - before).abs() < 1e-10 * before,
        "{} vs {before}",
        volume(&plate)
    );

    // Inwards by 1: corners of radius 5, a hole of radius 5, 8 thick.
    let thinner = offset_faces(&plate, &vec![-1.0; plate.face_count()]).unwrap();
    assert_sound(&thinner);
    let after = (rounded(38.0, 28.0, 5.0) - PI * 25.0) * 8.0;
    assert!(
        (volume(&thinner) - after).abs() < 1e-10 * after,
        "{} vs {after}",
        volume(&thinner)
    );
    assert_eq!(count(&thinner, SurfKind::Cylinder), 5);
    for f in 0..plate.face_count() as u32 {
        assert_eq!(thinner.f_surface(f).kind, plate.f_surface(f).kind);
    }
    for e in 0..plate.edge_count() as u32 {
        assert_eq!(thinner.e_curve_ref(e).kind, plate.e_curve_ref(e).kind);
    }
    assert!((0..plate.edge_count() as u32).any(|e| plate.e_curve_ref(e).kind == CurveKind::Circle));

    // The corners cannot shrink past nothing.
    let refused = offset_faces(&plate, &vec![-6.5; plate.face_count()]).unwrap_err();
    assert!(refused.contains("smaller"), "{refused}");
}

#[test]
fn a_tray_is_hollowed_from_its_top() {
    let blank = make::block(v3(-16.0, -12.0, 0.0), v3(16.0, 12.0, 12.0)).unwrap();
    let top = facing(&blank, V3::Z);
    for (wall, inside) in [(1.0, 30.0 * 22.0 * 11.0), (2.0, 28.0 * 20.0 * 10.0)] {
        let tray = shell(&blank, &[top], wall, true).unwrap();
        assert_sound(&tray);
        assert!((volume(&tray) - (32.0 * 24.0 * 12.0 - inside)).abs() < 1e-7);
        // Five faces outside, five inside, and the rim.
        assert_eq!(tray.face_count(), 11);
        assert_eq!(count(&tray, SurfKind::Plane), 11);
    }
    // A wall as thick as half the tray leaves nothing to hollow.
    assert!(shell(&blank, &[top], 12.0, true).is_err());
    assert!(shell(&blank, &[], 1.0, true).is_err());
}

#[test]
fn a_cup_and_a_tube() {
    let rod = make::cylinder(V3::ZERO, V3::Z, 10.0, 20.0).unwrap();
    let (top, bottom) = (facing(&rod, V3::Z), facing(&rod, -V3::Z));
    let cup = shell(&rod, &[top], 1.5, true).unwrap();
    assert_sound(&cup);
    let exact = PI * (100.0 * 20.0 - 8.5 * 8.5 * 18.5);
    assert!(
        (volume(&cup) - exact).abs() < 1e-10 * exact,
        "{}",
        volume(&cup)
    );
    // Open at both ends: a tube.
    let tube = shell(&rod, &[top, bottom], 1.5, true).unwrap();
    assert_sound(&tube);
    let exact = PI * (100.0 - 8.5 * 8.5) * 20.0;
    assert!((volume(&tube) - exact).abs() < 1e-10 * exact);
    assert_eq!(tube.face_count(), 4);
}

#[test]
fn the_wall_goes_round_an_inside_corner() {
    // An L, hollowed from the top. At the corner that points into the
    // hollow the wall is carried round in an arc, 2 thick everywhere.
    let corner = |x: f64, y: f64| v3(x, y, 0.0);
    let outline = [
        corner(0.0, 0.0),
        corner(40.0, 0.0),
        corner(40.0, 15.0),
        corner(20.0, 15.0),
        corner(20.0, 30.0),
        corner(0.0, 30.0),
    ];
    let segs: Vec<Seg> = (0..6)
        .map(|i| Seg::Line {
            a: outline[i],
            b: outline[(i + 1) % 6],
        })
        .collect();
    let blank = make::extrude(&Region { loops: vec![segs] }, V3::Z, 0.0, 10.0).unwrap();
    let hollowed = shell(&blank, &[facing(&blank, V3::Z)], 2.0, true).unwrap();
    assert_sound(&hollowed);
    let hollow = (36.0 * 11.0 + 16.0 * 15.0 + (4.0 - PI)) * 8.0;
    assert!(
        (volume(&hollowed) - (9000.0 - hollow)).abs() < 1e-7,
        "{}",
        volume(&hollowed)
    );
    assert_eq!(count(&hollowed, SurfKind::Cylinder), 1);
}

#[test]
fn a_wall_added_outside_is_rounded_where_it_turns() {
    let core = make::block(v3(0.0, 0.0, 0.0), v3(20.0, 20.0, 10.0)).unwrap();
    let skin = shell(&core, &[facing(&core, V3::Z)], 2.0, false).unwrap();
    assert_sound(&skin);
    // The block swollen by 2 below its top, less the block: slabs on five
    // faces, quarter rounds along eight edges, an eighth of a ball at each
    // of the four bottom corners.
    let wall = 2.0 * (4.0 * 200.0 + 400.0) + PI * 120.0 + 4.0 * PI * 8.0 / 6.0;
    assert!(
        (volume(&skin) - wall).abs() < 1e-10 * wall,
        "{}",
        volume(&skin)
    );
    assert_eq!(count(&skin, SurfKind::Sphere), 4);
    assert_eq!(count(&skin, SurfKind::Cylinder), 8);
}

/// What a body holds, weighed exactly: see the note in `blend.rs`.
fn holds(body: &Body, expected: f64) {
    let held = limo_cad_kernel::props::mass(body).volume;
    assert!(
        (held - expected).abs() <= 1.0e-10 * expected,
        "holds {held:.9}, not {expected:.9}: {:+.9} out",
        held - expected
    );
}

/// A boss on a plate, hollowed from underneath. Round the foot of the boss
/// the wall turns an inside corner on an arc about the foot's edge, and at
/// each corner of the boss two of those arcs run into each other. Stopped
/// short of each other instead, they leave a cube of the wall's thickness
/// standing in each corner: a wall nearly half as thick again there, in a
/// body that is sound and weighs six parts in ten thousand too much.
#[test]
fn arcs_of_a_wall_run_into_each_other_at_a_corner() {
    use limo_cad_kernel::{boolean, Op};
    let plate = make::block(V3::ZERO, v3(40.0, 25.0, 10.0)).unwrap();
    let boss = make::block(v3(10.0, 8.0, 10.0), v3(30.0, 18.0, 20.0)).unwrap();
    let body = boolean(&plate, &boss, Op::Union).unwrap();
    let hollowed = shell(&body, &[facing(&body, -V3::Z)], 2.0, true).unwrap();
    assert_sound(&hollowed);
    // The hollow is every point of the body more than 2 from its walls.
    // That is 8 high outside the boss's outline; 18 high inside the
    // outline and more than 2 from it; and in the band between, at `s`
    // from the outline, it stops sqrt(4 - s^2) short of the plate's top.
    // The outline at `s` inside a rectangle 20 by 10 is 60 - 8s long.
    let (t, tt): (f64, f64) = (2.0, 4.0);
    let band = 10.0 * (60.0 * t - 4.0 * tt) - (60.0 * PI * tt / 4.0 - 8.0 * t.powi(3) / 3.0);
    let hollow = (36.0 * 21.0 - 200.0) * 8.0 + 16.0 * 6.0 * 18.0 + band;
    holds(&hollowed, 12000.0 - hollow);
    assert_eq!(count(&hollowed, SurfKind::Cylinder), 4);
    // The four arcs meet in four mitres.
    let mitres = (0..hollowed.edge_count() as u32)
        .filter(|&e| hollowed.e_curve_ref(e).kind == CurveKind::Ellipse)
        .count();
    assert_eq!(mitres, 4);
}

/// A pocket in a plate, hollowed from underneath. Round the pocket the
/// wall is the pocket grown by 2: its floor edges and its upright edges on
/// arcs, and in each floor corner, where three of those meet, a ball.
#[test]
fn three_arcs_of_a_wall_meet_round_a_ball() {
    use limo_cad_kernel::{boolean, Op};
    let plate = make::block(V3::ZERO, v3(40.0, 25.0, 10.0)).unwrap();
    let pocket = make::block(v3(10.0, 8.0, 4.0), v3(30.0, 18.0, 11.0)).unwrap();
    let body = boolean(&plate, &pocket, Op::Cut).unwrap();
    let hollowed = shell(&body, &[facing(&body, -V3::Z)], 2.0, true).unwrap();
    assert_sound(&hollowed);
    // Below the plate's wall the hollow is 8 high, less the pocket grown
    // by 2. Level with the pocket the grown outline is the rectangle with
    // 2 all round and round corners; below its floor, at depth `s`, it is
    // grown by sqrt(4 - s^2) instead.
    let grown = |d: f64, d2: f64| 200.0 + 60.0 * d + PI * d2;
    let below = 2.0 * 200.0 + 60.0 * PI + PI * (8.0 - 8.0 / 3.0);
    let hollow = 36.0 * 21.0 * 8.0 - (4.0 * grown(2.0, 4.0) + below);
    holds(&hollowed, 8800.0 - hollow);
    assert_eq!(
        (
            count(&hollowed, SurfKind::Sphere),
            count(&hollowed, SurfKind::Cylinder)
        ),
        (4, 8)
    );
}

/// A wall added round the outside of an L-shaped block: round at its five
/// outside corners, sharp in its inside one, with the two rounds along the
/// top of the inside corner crossing in a mitre.
#[test]
fn a_wall_outside_an_l_shaped_block() {
    let outline = [
        v3(0.0, 0.0, 0.0),
        v3(40.0, 0.0, 0.0),
        v3(40.0, 10.0, 0.0),
        v3(15.0, 10.0, 0.0),
        v3(15.0, 25.0, 0.0),
        v3(0.0, 25.0, 0.0),
    ];
    let segs: Vec<Seg> = (0..6)
        .map(|i| Seg::Line {
            a: outline[i],
            b: outline[(i + 1) % 6],
        })
        .collect();
    let block = make::extrude(&Region { loops: vec![segs] }, V3::Z, 0.0, 10.0).unwrap();
    let skin = shell(&block, &[facing(&block, -V3::Z)], 2.0, false).unwrap();
    assert_sound(&skin);
    // Slice by slice the wall's outline is the L's grown by `d`: its area,
    // 130 d round its edge, a quarter disc at each of five corners, less
    // the square the two strips share in the sixth. It is grown by 2 up to
    // the top of the L and by sqrt(4 - s^2) at height `s` above it.
    let (t, tt): (f64, f64) = (2.0, 4.0);
    let grown = 625.0 + 130.0 * t + (1.25 * PI - 1.0) * tt;
    let above =
        625.0 * t + 130.0 * (PI * tt / 4.0) + (1.25 * PI - 1.0) * (tt * t - t.powi(3) / 3.0);
    holds(&skin, grown * 10.0 + above - 6250.0);
}

/// A wall round a boss one side of which leans. Under the foot of that
/// side the wall turns on an arc about the foot's edge, as it does under
/// the others, until the leaning side's own offset takes over from the
/// arc; and the arcs of the sides next to it end on that offset, not
/// square to their own edges. Ended square, they left the wall a few
/// thousandths out at those two corners.
///
/// Slice by slice the hollow is a rectangle, so what the wall holds can be
/// worked out: every side of the hollow is a flat or a round that runs
/// straight along one side of the boss.
#[test]
fn a_wall_round_a_boss_whose_side_leans() {
    use limo_cad_kernel::{boolean, Op, Xform};
    let plate = make::block(v3(-30.0, -30.0, 0.0), v3(30.0, 30.0, 10.0)).unwrap();
    let boss = make::block(v3(-10.0, -6.0, 10.0), v3(10.0, 6.0, 20.0)).unwrap();
    // One side of the boss pared off on a lean, about its own foot.
    let lean = 8.0_f64.to_radians();
    let knife = make::block(v3(10.0, -20.0, 5.0), v3(30.0, 20.0, 40.0))
        .unwrap()
        .transformed(&Xform::rotation(v3(10.0, 0.0, 10.0), V3::Y, -lean));
    let leaning = boolean(&boss, &knife, Op::Cut).unwrap();
    let body = boolean(&plate, &leaning, Op::Union).unwrap();
    let whole = 36000.0 + 12.0 * (200.0 - 50.0 * lean.tan());
    holds(&body, whole);
    let skin = shell(&body, &[facing(&body, -V3::Z)], 2.0, true).unwrap();
    assert_sound(&skin);

    // Up to 2 under the plate's top the hollow is the plate's own, 2 in
    // from its sides. From there to the plate's top it is under the boss
    // only, and short of each of the boss's sides by where the arc about
    // that side's foot is; above the plate's top by 2, up to 2 under the
    // boss's top. The leaning side's arc gives way to the side's offset
    // where the two touch, 2 sin(lean) under the plate's top.
    let arc = |z: f64| (4.0 - (10.0 - z) * (10.0 - z)).max(0.0).sqrt();
    let inside = |z: f64| if z < 10.0 { arc(z) } else { 2.0 };
    let offset_from = 10.0 - 2.0 * lean.sin();
    let leaning_side = |z: f64| {
        if z <= offset_from {
            10.0 - arc(z)
        } else {
            10.0 - 2.0 / lean.cos() - (z - 10.0) * lean.tan()
        }
    };
    let slice = |z: f64| (leaning_side(z) + 10.0 - inside(z)) * (12.0 - 2.0 * inside(z));
    // Through the arcs as 10 - 2 cos(a), which takes the root out of the
    // end where they start.
    let through =
        |from: f64, to: f64| integral(from, to, |a| slice(10.0 - 2.0 * a.cos()) * 2.0 * a.sin());
    let gives_way = lean.sin().acos();
    let hollow = 56.0 * 56.0 * 8.0
        + through(0.0, gives_way)
        + through(gives_way, 0.5 * PI)
        + integral(10.0, 18.0, slice);
    holds(&skin, whole - hollow);

    // Upright, the same boss.
    let body = boolean(&plate, &boss, Op::Union).unwrap();
    assert_sound(&shell(&body, &[facing(&body, -V3::Z)], 2.0, true).unwrap());
}
