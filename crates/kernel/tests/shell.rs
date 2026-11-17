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
    assert!((volume(&grown) - 44.0 * 34.0 * 14.0).abs() < 1e-6);
    // Only the top, down by 3.
    let mut by = [0.0; 6];
    by[facing(&block, V3::Z) as usize] = -3.0;
    let lower = offset_faces(&block, &by).unwrap();
    assert!((volume(&lower) - 40.0 * 30.0 * 7.0).abs() < 1e-6);
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
        (volume(&plate) - before).abs() < 1e-5 * before,
        "{} vs {before}",
        volume(&plate)
    );

    // Inwards by 1: corners of radius 5, a hole of radius 5, 8 thick.
    let thinner = offset_faces(&plate, &vec![-1.0; plate.face_count()]).unwrap();
    assert_sound(&thinner);
    let after = (rounded(38.0, 28.0, 5.0) - PI * 25.0) * 8.0;
    assert!(
        (volume(&thinner) - after).abs() < 1e-5 * after,
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
        assert!((volume(&tray) - (32.0 * 24.0 * 12.0 - inside)).abs() < 1e-6);
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
        (volume(&cup) - exact).abs() < 1e-3 * exact,
        "{}",
        volume(&cup)
    );
    // Open at both ends: a tube.
    let tube = shell(&rod, &[top, bottom], 1.5, true).unwrap();
    assert_sound(&tube);
    let exact = PI * (100.0 - 8.5 * 8.5) * 20.0;
    assert!((volume(&tube) - exact).abs() < 1e-3 * exact);
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
        (volume(&hollowed) - (9000.0 - hollow)).abs() < 1e-3,
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
        (volume(&skin) - wall).abs() < 1e-3 * wall,
        "{}",
        volume(&skin)
    );
    assert_eq!(count(&skin, SurfKind::Sphere), 4);
    assert_eq!(count(&skin, SurfKind::Cylinder), 8);
}
