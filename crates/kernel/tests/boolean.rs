use limo_cad_kernel::make;
use limo_cad_kernel::math::*;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{boolean, Body, Op, SurfKind};

fn fine() -> TessOptions {
    TessOptions {
        linear: 0.0005,
        angular: 0.02,
    }
}

fn volume(body: &Body) -> f64 {
    tessellate(body, &fine()).volume()
}

fn bx(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) -> Body {
    make::block(v3(x0, y0, z0), v3(x1, y1, z1)).unwrap()
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

#[test]
fn pocket_from_a_flush_top_face() {
    let block = bx(0.0, 0.0, 0.0, 30.0, 30.0, 8.0);
    let tool = bx(7.0, 7.0, 4.0, 23.0, 23.0, 8.0);
    let r = boolean(&block, &tool, Op::Cut).unwrap();
    // Six block faces, four walls and a floor.
    assert_eq!(r.face_count(), 11);
    close(volume(&r), 30.0 * 30.0 * 8.0 - 16.0 * 16.0 * 4.0, 1e-12);
}

#[test]
fn overlapping_blocks_union_cut_intersect() {
    let a = bx(0.0, 0.0, 0.0, 10.0, 10.0, 10.0);
    let b = bx(5.0, 5.0, 5.0, 15.0, 15.0, 15.0);
    let u = boolean(&a, &b, Op::Union).unwrap();
    close(volume(&u), 2000.0 - 125.0, 1e-12);
    assert_eq!(u.face_count(), 12);
    let c = boolean(&a, &b, Op::Cut).unwrap();
    close(volume(&c), 1000.0 - 125.0, 1e-12);
    assert_eq!(c.face_count(), 9);
    let i = boolean(&a, &b, Op::Intersect).unwrap();
    close(volume(&i), 125.0, 1e-12);
    assert_eq!(i.face_count(), 6);
}

#[test]
fn stacked_blocks_merge_into_one_box() {
    let a = bx(0.0, 0.0, 0.0, 10.0, 10.0, 5.0);
    let b = bx(0.0, 0.0, 5.0, 10.0, 10.0, 12.0);
    let u = boolean(&a, &b, Op::Union).unwrap();
    close(volume(&u), 1200.0, 1e-12);
    // Coplanar side faces unify; the shared face disappears.
    assert_eq!((u.face_count(), u.edge_count(), u.verts.len()), (6, 12, 8));
}

#[test]
fn boss_on_a_larger_plate() {
    let plate = bx(0.0, 0.0, 0.0, 40.0, 30.0, 5.0);
    let boss = bx(10.0, 10.0, 5.0, 20.0, 20.0, 15.0);
    let u = boolean(&plate, &boss, Op::Union).unwrap();
    close(volume(&u), 40.0 * 30.0 * 5.0 + 1000.0, 1e-12);
    assert_eq!(u.face_count(), 6 + 5);
}

#[test]
fn through_hole_and_blind_hole() {
    let block = bx(0.0, 0.0, 0.0, 40.0, 30.0, 10.0);
    let drill = make::cylinder(v3(20.0, 15.0, -5.0), V3::Z, 4.0, 20.0).unwrap();
    let r = boolean(&block, &drill, Op::Cut).unwrap();
    assert_eq!(r.face_count(), 7);
    assert_eq!(count(&r, SurfKind::Cylinder), 1);
    close(volume(&r), 12000.0 - PI * 16.0 * 10.0, 1e-5);

    let blind = make::cylinder(v3(20.0, 15.0, 4.0), V3::Z, 4.0, 6.0).unwrap();
    let r = boolean(&block, &blind, Op::Cut).unwrap();
    assert_eq!(r.face_count(), 8);
    close(volume(&r), 12000.0 - PI * 16.0 * 6.0, 1e-5);
}

#[test]
fn cylinder_boss_on_a_block() {
    let block = bx(0.0, 0.0, 0.0, 40.0, 30.0, 10.0);
    let boss = make::cylinder(v3(20.0, 15.0, 10.0), V3::Z, 6.0, 8.0).unwrap();
    let r = boolean(&block, &boss, Op::Union).unwrap();
    assert_eq!(r.face_count(), 8);
    close(volume(&r), 12000.0 + PI * 36.0 * 8.0, 1e-5);
}

#[test]
fn tube_from_coaxial_cylinders() {
    let outer = make::cylinder(V3::ZERO, V3::Z, 10.0, 20.0).unwrap();
    let inner = make::cylinder(V3::ZERO, V3::Z, 6.0, 20.0).unwrap();
    let tube = boolean(&outer, &inner, Op::Cut).unwrap();
    assert_eq!(tube.face_count(), 4);
    close(volume(&tube), PI * (100.0 - 36.0) * 20.0, 2e-4);
}

#[test]
fn cut_that_separates_a_block_in_two() {
    let block = bx(0.0, 0.0, 0.0, 30.0, 10.0, 10.0);
    let slot = bx(10.0, -1.0, -1.0, 20.0, 11.0, 11.0);
    let r = boolean(&block, &slot, Op::Cut).unwrap();
    assert_eq!(r.face_count(), 12);
    close(volume(&r), 2000.0, 1e-12);
}

#[test]
fn tool_flush_with_a_side_face() {
    let block = bx(0.0, 0.0, 0.0, 30.0, 20.0, 10.0);
    // A notch flush with the top, the front and the right side.
    let tool = bx(20.0, 0.0, 5.0, 30.0, 8.0, 10.0);
    let r = boolean(&block, &tool, Op::Cut).unwrap();
    close(volume(&r), 6000.0 - 10.0 * 8.0 * 5.0, 1e-12);
    assert_eq!(r.face_count(), 9);
}

#[test]
fn nested_bodies() {
    let big = bx(0.0, 0.0, 0.0, 20.0, 20.0, 20.0);
    let small = bx(5.0, 5.0, 5.0, 10.0, 10.0, 10.0);
    let u = boolean(&big, &small, Op::Union).unwrap();
    assert_eq!(u.face_count(), 6);
    close(volume(&u), 8000.0, 1e-12);
    let i = boolean(&big, &small, Op::Intersect).unwrap();
    assert_eq!(i.face_count(), 6);
    close(volume(&i), 125.0, 1e-12);
    // A void: the inner shell is kept inside-out.
    let c = boolean(&big, &small, Op::Cut).unwrap();
    assert_eq!(c.face_count(), 12);
    close(volume(&c), 8000.0 - 125.0, 1e-12);
}

#[test]
fn hole_through_an_edge_of_the_block() {
    let block = bx(0.0, 0.0, 0.0, 20.0, 20.0, 10.0);
    // The drill axis sits on the block's vertical edge at x = y = 0.
    let drill = make::cylinder(v3(0.0, 0.0, -1.0), V3::Z, 5.0, 12.0).unwrap();
    let r = boolean(&block, &drill, Op::Cut).unwrap();
    close(volume(&r), 4000.0 - 0.25 * PI * 25.0 * 10.0, 1e-5);
    assert_eq!(count(&r, SurfKind::Cylinder), 1);
    assert_eq!(r.face_count(), 7);
}

#[test]
fn chained_operations_stay_valid() {
    let mut body = bx(0.0, 0.0, 0.0, 60.0, 40.0, 10.0);
    let mut expected = 24000.0;
    for (i, j) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let (x, y) = (15.0 + 30.0 * i as f64, 10.0 + 20.0 * j as f64);
        let drill = make::cylinder(v3(x, y, -1.0), V3::Z, 3.0, 12.0).unwrap();
        body = boolean(&body, &drill, Op::Cut).unwrap();
        expected -= PI * 9.0 * 10.0;
    }
    let rib = bx(28.0, 0.0, 10.0, 32.0, 40.0, 25.0);
    body = boolean(&body, &rib, Op::Union).unwrap();
    expected += 4.0 * 40.0 * 15.0;
    body.check().unwrap();
    assert_eq!(count(&body, SurfKind::Cylinder), 4);
    close(volume(&body), expected, 1e-5);
}

/// Simpson integration of `f` over `[a, b]`.
fn integrate(f: impl Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    let n = 20_000;
    let h = (b - a) / n as f64;
    let mut sum = f(a) + f(b);
    for i in 1..n {
        sum += f(a + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

#[test]
fn cross_drilled_shaft() {
    // A hole of radius 3 straight through a shaft of radius 10.
    let shaft = make::cylinder(v3(0.0, 0.0, -20.0), V3::Z, 10.0, 40.0).unwrap();
    let drill = make::cylinder(v3(-15.0, 0.0, 0.0), V3::X, 3.0, 30.0).unwrap();
    let r = boolean(&shaft, &drill, Op::Cut).unwrap();
    // Wall, two caps and the bore; the wall now has two holes in it.
    assert_eq!((r.face_count(), count(&r, SurfKind::Cylinder)), (4, 2));
    let removed = integrate(
        |y| 4.0 * (100.0 - y * y).sqrt() * (9.0 - y * y).max(0.0).sqrt(),
        -3.0,
        3.0,
    );
    close(volume(&r), PI * 100.0 * 40.0 - removed, 2e-4);
    r.check().unwrap();
}

#[test]
fn drill_breaking_out_of_the_side() {
    // The drill axis misses the shaft axis by 8, so the hole is open along
    // the shaft's flank: the intersection curve has turning points.
    let shaft = make::cylinder(v3(0.0, 0.0, -20.0), V3::Z, 10.0, 40.0).unwrap();
    let drill = make::cylinder(v3(-15.0, 8.0, 0.0), V3::X, 4.0, 30.0).unwrap();
    let r = boolean(&shaft, &drill, Op::Cut).unwrap();
    // Chord of the shaft at offset y, weighted by the drill's width there.
    let removed = integrate(
        |y| {
            let chord = 2.0 * (100.0 - y * y).max(0.0).sqrt();
            let width = 2.0 * (16.0 - (y - 8.0) * (y - 8.0)).max(0.0).sqrt();
            chord * width
        },
        4.0,
        10.0,
    );
    close(volume(&r), PI * 100.0 * 40.0 - removed, 5e-4);
    r.check().unwrap();
}

#[test]
fn hole_into_a_ball() {
    let ball = make::revolve(
        &[vec![
            make::PSeg::Line {
                a: v2(0.0, 12.0),
                b: v2(0.0, -12.0),
            },
            make::PSeg::Arc {
                a: v2(0.0, -12.0),
                b: v2(0.0, 12.0),
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
    // Off-centre, so the rim is not a circle.
    let drill = make::cylinder(v3(5.0, 0.0, -20.0), V3::Z, 3.0, 40.0).unwrap();
    let r = boolean(&ball, &drill, Op::Cut).unwrap();
    assert_eq!(count(&r, SurfKind::Sphere), 1);
    assert_eq!(count(&r, SurfKind::Cylinder), 1);
    let coarse = tessellate(&r, &TessOptions::default()).volume();
    assert!(coarse < 4.0 / 3.0 * PI * 1728.0 && coarse > 0.9 * 4.0 / 3.0 * PI * 1728.0);
    r.check().unwrap();
}

/// A rod half as thick as a ball, through it and touching it from the
/// inside: the curve the two meet in is a figure of eight, crossing itself
/// where they touch. The part of the ball inside the rod has been known
/// since 1692 to hold `2/3 (π − 4/3) R³`.
#[test]
fn a_rod_that_touches_the_ball_it_goes_through() {
    use limo_cad_kernel::make::PSeg;
    use limo_cad_kernel::props::mass;
    let r = 10.0;
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
    let ball = make::revolve(&[half.to_vec()], V3::ZERO, V3::Z, V3::X, TAU).unwrap();
    let rod = make::cylinder(v3(0.5 * r, 0.0, -2.0 * r), V3::Z, 0.5 * r, 4.0 * r).unwrap();
    let window = 2.0 / 3.0 * (PI - 4.0 / 3.0) * r * r * r;
    let (whole, bar) = (mass(&ball).volume, mass(&rod).volume);
    for (op, expected) in [
        (Op::Intersect, window),
        (Op::Cut, whole - window),
        (Op::Union, whole + bar - window),
    ] {
        let body = boolean(&ball, &rod, op).unwrap_or_else(|e| panic!("{op:?}: {e}"));
        limo_cad_kernel::audit::audit(&body, limo_cad_kernel::audit::CLOSE).unwrap();
        let options = TessOptions::default();
        tessellate(&body, &options).check(&body, &options).unwrap();
        close(mass(&body).volume, expected, 1e-10);
    }
}
