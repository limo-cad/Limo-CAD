//! Volume, area and centre of mass against closed forms, and against each
//! other across Booleans.

use limo_cad_kernel::blend::fillet;
use limo_cad_kernel::demo::random_pair;
use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::props::{face_area, mass};
use limo_cad_kernel::{boolean, v2, v3, Body, CurveKind, Op, Xform, V3};
use std::f64::consts::{PI, TAU};

fn close(found: f64, wanted: f64) -> bool {
    (found - wanted).abs() <= 1.0e-10 * wanted.abs().max(1.0)
}

#[track_caller]
fn assert_mass(body: &Body, volume: f64, area: f64, centre: V3) {
    let m = mass(body);
    assert!(m.exact);
    assert!(close(m.volume, volume), "volume {} for {volume}", m.volume);
    assert!(close(m.area, area), "area {} for {area}", m.area);
    assert!(
        (m.centre - centre).len() <= 1.0e-9 * centre.len().max(1.0),
        "centre {:?} for {centre:?}",
        m.centre
    );
}

fn ball(r: f64) -> Body {
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
    make::revolve(&[half.to_vec()], V3::ZERO, V3::Z, V3::X, TAU).unwrap()
}

fn ring(major: f64, minor: f64) -> Body {
    let tube = PSeg::Circle {
        c: v2(major, 0.0),
        r: minor,
    };
    make::revolve(&[vec![tube]], V3::ZERO, V3::Z, V3::X, TAU).unwrap()
}

#[test]
fn plain_solids_weigh_what_their_formulas_say() {
    let block = make::block(v3(1.0, 2.0, 3.0), v3(11.0, 22.0, 33.0)).unwrap();
    assert_mass(&block, 6000.0, 2200.0, v3(6.0, 12.0, 18.0));

    let rod = make::cylinder(v3(5.0, -3.0, 2.0), V3::Z, 4.0, 10.0).unwrap();
    assert_mass(&rod, 160.0 * PI, 112.0 * PI, v3(5.0, -3.0, 7.0));

    // A cone standing on its tip: a third of the way down from the base.
    let (r, h) = (6.0_f64, 9.0_f64);
    let cone = make::lathe(V3::ZERO, V3::Z, &[(0.0, 0.0), (r, h), (0.0, h)]).unwrap();
    assert_mass(
        &cone,
        PI * r * r * h / 3.0,
        PI * r * r + PI * r * r.hypot(h),
        v3(0.0, 0.0, 0.75 * h),
    );

    let r = 7.5;
    assert_mass(
        &ball(r),
        4.0 / 3.0 * PI * r * r * r,
        4.0 * PI * r * r,
        V3::ZERO,
    );

    let (major, minor) = (20.0, 6.0);
    assert_mass(
        &ring(major, minor),
        2.0 * PI * PI * major * minor * minor,
        4.0 * PI * PI * major * minor,
        V3::ZERO,
    );
}

#[test]
fn a_solid_weighs_the_same_wherever_it_is_put() {
    let turn = Xform::rotation(v3(3.0, -2.0, 5.0), v3(0.3, -0.8, 0.52).unit(), 1.234);
    let shift = Xform::translation(v3(140.0, -75.0, 310.0));
    for (name, body) in [
        ("ball", ball(9.0)),
        ("ring", ring(15.0, 4.0)),
        (
            "cone",
            make::lathe(V3::ZERO, V3::Z, &[(0.0, 0.0), (5.0, 12.0), (0.0, 12.0)]).unwrap(),
        ),
        ("rod", make::cylinder(V3::ZERO, V3::Z, 3.0, 25.0).unwrap()),
    ] {
        let moved = body.transformed(&turn).transformed(&shift);
        let (here, there) = (mass(&body), mass(&moved));
        assert!(close(there.volume, here.volume), "{name}: volume");
        assert!(close(there.area, here.area), "{name}: area");
        assert!(
            (there.centre - shift.point(turn.point(here.centre))).len() < 1.0e-9,
            "{name}: centre"
        );
    }
}

#[test]
fn parts_of_round_solids() {
    // A ball cut by a plane: a cap of height h.
    let (r, h) = (10.0_f64, 3.0_f64);
    let slab = make::block(v3(-20.0, -20.0, r - h), v3(20.0, 20.0, 20.0)).unwrap();
    let cap = boolean(&ball(r), &slab, Op::Intersect).unwrap();
    let a = (h * (2.0 * r - h)).sqrt();
    assert_mass(
        &cap,
        PI * h * h * (3.0 * r - h) / 3.0,
        2.0 * PI * r * h + PI * a * a,
        v3(0.0, 0.0, 0.75 * (2.0 * r - h).powi(2) / (3.0 * r - h)),
    );
    // The rest of the ball holds the lower pole and a hole round the upper.
    let rest = boolean(&ball(r), &slab, Op::Cut).unwrap();
    let whole = 4.0 / 3.0 * PI * r * r * r;
    assert!(close(mass(&rest).volume, whole - mass(&cap).volume));

    // A ring cut in half through its axis, and across it.
    let (major, minor) = (20.0, 6.0);
    let full = 2.0 * PI * PI * major * minor * minor;
    let side = make::block(v3(0.0, -40.0, -40.0), v3(40.0, 40.0, 40.0)).unwrap();
    let bend = boolean(&ring(major, minor), &side, Op::Intersect).unwrap();
    let m = mass(&bend);
    assert!(m.exact);
    assert!(close(m.volume, 0.5 * full), "half a ring: {}", m.volume);
    assert!(close(
        m.area,
        2.0 * PI * PI * major * minor + 2.0 * PI * minor * minor
    ));
    let top = make::block(v3(-40.0, -40.0, 0.0), v3(40.0, 40.0, 40.0)).unwrap();
    let upper = boolean(&ring(major, minor), &top, Op::Intersect).unwrap();
    let m = mass(&upper);
    assert!(m.exact);
    assert!(close(m.volume, 0.5 * full), "upper half: {}", m.volume);
    // Outer and inner halves of the tube are not alike, but the top half
    // is half of everything.
    assert!(close(
        m.area,
        2.0 * PI * PI * major * minor + PI * ((major + minor).powi(2) - (major - minor).powi(2))
    ));
    // A quarter: one loop that goes round nothing.
    let quarter = boolean(&bend, &top, Op::Intersect).unwrap();
    assert!(close(mass(&quarter).volume, 0.25 * full));
}

#[test]
fn a_rounded_block() {
    // Rounding the four upright edges takes four square corners for
    // quarter discs.
    let (w, d, h, r) = (30.0, 20.0, 8.0, 3.0);
    let block = make::block(V3::ZERO, v3(w, d, h)).unwrap();
    let upright: Vec<u32> = (0..block.edge_count() as u32)
        .filter(|&e| {
            let c = block.e_curve_ref(e);
            c.kind == CurveKind::Line && c.deriv(0.0).unit().cross(V3::Z).len() < 1.0e-9
        })
        .collect();
    assert_eq!(upright.len(), 4);
    let rounded = fillet(&block, &upright, r).unwrap();
    let corner = (4.0 - PI) * r * r;
    let m = mass(&rounded);
    assert!(close(m.volume, (w * d - corner) * h));
    assert!(close(
        m.area,
        2.0 * (w * d - corner) + (2.0 * (w + d) - 8.0 * r + TAU * r) * h
    ));
    assert!((m.centre - v3(0.5 * w, 0.5 * d, 0.5 * h)).len() < 1.0e-9);
    // Each rounded corner is a quarter of a cylinder.
    let quarter = (0..rounded.face_count() as u32)
        .filter(|&f| !rounded.f_surface(f).is_plane())
        .map(|f| face_area(&rounded, f).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(quarter.len(), 4);
    assert!(quarter.iter().all(|a| close(*a, 0.25 * TAU * r * h)));
}

/// What two solids share and what they make together weigh as much as the
/// two of them, and what is left of one after the other is what it had
/// that they did not share.
#[test]
fn booleans_keep_the_books() {
    let mut checked = 0;
    for seed in 0..120 {
        let [(name_a, a), (name_b, b)] = random_pair(seed);
        let (Ok(both), Ok(either), Ok(left)) = (
            boolean(&a, &b, Op::Intersect),
            boolean(&a, &b, Op::Union),
            boolean(&a, &b, Op::Cut),
        ) else {
            continue;
        };
        let (va, vb) = (mass(&a).volume, mass(&b).volume);
        let (shared, joined, rest) = (mass(&both), mass(&either), mass(&left));
        if !(shared.exact && joined.exact && rest.exact) {
            continue;
        }
        let scale = va + vb;
        assert!(
            (shared.volume + joined.volume - va - vb).abs() < 1.0e-8 * scale,
            "pair {seed} ({name_a}, {name_b}): {} shared and {} joined of {va} and {vb}",
            shared.volume,
            joined.volume
        );
        assert!(
            (rest.volume + shared.volume - va).abs() < 1.0e-8 * scale,
            "pair {seed} ({name_a}, {name_b}): {} left and {} shared of {va}",
            rest.volume,
            shared.volume
        );
        checked += 1;
    }
    assert!(checked > 60, "only {checked} pairs could be compared");
}

/// The box a body fits in comes from where its edges and faces turn back,
/// not from the corners of its facets.
#[test]
fn round_solids_are_as_wide_as_they_are() {
    use limo_cad_kernel::props::bounds;
    let size = |body: &Body| {
        let b = bounds(body);
        b.hi - b.lo
    };
    let near = |found: V3, wanted: V3| (found - wanted).len() < 1.0e-9;
    let rod = make::cylinder(v3(5.0, -3.0, 2.0), V3::Z, 5.0, 20.0).unwrap();
    assert!(near(size(&rod), v3(10.0, 10.0, 20.0)));
    assert!(near(size(&ball(7.5)), v3(15.0, 15.0, 15.0)));
    assert!(near(size(&ring(20.0, 6.0)), v3(52.0, 52.0, 12.0)));
    // Turned an eighth of a turn about its axis a rod is no wider; laid
    // over at 45 degrees it is as long as it is wide.
    let turned = rod.transformed(&Xform::rotation(v3(5.0, -3.0, 2.0), V3::Z, 0.25 * PI));
    assert!(near(size(&turned), v3(10.0, 10.0, 20.0)));
    let laid = make::cylinder(V3::ZERO, V3::Z, 5.0, 20.0)
        .unwrap()
        .transformed(&Xform::rotation(V3::ZERO, V3::X, 0.25 * PI));
    let across = (20.0 + 10.0) / 2.0_f64.sqrt();
    assert!(near(size(&laid), v3(10.0, across, across)));
    // A ring on its side.
    let upright = ring(20.0, 6.0).transformed(&Xform::rotation(V3::ZERO, V3::X, 0.5 * PI));
    assert!(near(size(&upright), v3(52.0, 12.0, 52.0)));
    // Half a ball: flat on one side.
    let slab = make::block(v3(-20.0, -20.0, 0.0), v3(20.0, 20.0, 20.0)).unwrap();
    let dome = boolean(&ball(7.5), &slab, Op::Intersect).unwrap();
    assert!(near(size(&dome), v3(15.0, 15.0, 7.5)));
}

/// The inertia tensor as three moments and three products, for comparing.
fn spins(body: &Body) -> ([f64; 3], [f64; 3], f64) {
    let m = mass(body);
    assert!(m.exact);
    let i = m.inertia;
    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
        assert_eq!(i[a][b], i[b][a]);
    }
    (
        [i[0][0], i[1][1], i[2][2]],
        [i[0][1], i[1][2], i[2][0]],
        m.volume,
    )
}

#[test]
fn moments_of_inertia_are_the_textbook_ones() {
    let near = |a: f64, b: f64, scale: f64| (a - b).abs() <= 1.0e-10 * scale.max(1.0);
    // A block, wherever it stands: a twelfth of its mass times the squares
    // of the two sides across each axis.
    let block = make::block(v3(3.0, -7.0, 2.0), v3(13.0, -1.0, 6.0)).unwrap();
    let (moments, products, v) = spins(&block);
    let (a, b, c) = (10.0, 6.0, 4.0);
    let exact = [
        v * (b * b + c * c) / 12.0,
        v * (c * c + a * a) / 12.0,
        v * (a * a + b * b) / 12.0,
    ];
    for k in 0..3 {
        assert!(near(moments[k], exact[k], exact[k]), "{moments:?}");
        assert!(near(products[k], 0.0, exact[0]), "{products:?}");
    }
    // A rod along z: half its mass times its radius squared about its
    // axis; a quarter of that and a twelfth of its length squared across.
    let (r, h) = (4.0, 15.0);
    let rod = make::cylinder(v3(1.0, 2.0, 3.0), V3::Z, r, h).unwrap();
    let (moments, products, v) = spins(&rod);
    assert!(near(v, PI * r * r * h, v));
    let across = v * (3.0 * r * r + h * h) / 12.0;
    assert!(near(moments[2], 0.5 * v * r * r, across), "{moments:?}");
    assert!(near(moments[0], across, across) && near(moments[1], across, across));
    assert!(products.iter().all(|p| near(*p, 0.0, across)));
    // The same rod lying askew: the same three moments about its own
    // axes, read off the tensor along them.
    let axis = v3(1.0, 2.0, 2.0).unit();
    let askew = make::cylinder(v3(-5.0, 0.5, 2.0), axis, r, h).unwrap();
    let m = mass(&askew);
    let along = |d: V3| {
        let i = m.inertia;
        let row = |k: usize| i[k][0] * d.x + i[k][1] * d.y + i[k][2] * d.z;
        d.x * row(0) + d.y * row(1) + d.z * row(2)
    };
    assert!(
        near(along(axis), 0.5 * v * r * r, across),
        "{}",
        along(axis)
    );
    assert!(near(along(axis.any_perp().unit()), across, across));
    // A ball: two fifths of its mass times its radius squared, every way.
    let ball = ball(6.0).transformed(&Xform::translation(v3(2.0, 1.0, -3.0)));
    let (moments, _, v) = spins(&ball);
    for k in 0..3 {
        assert!(near(moments[k], 0.4 * v * 36.0, v * 36.0), "{moments:?}");
    }
    // A ring about z: its mass times (R² + ¾ r²) about its axis, and
    // times (R²/2 + ⅝ r²) across.
    let (big, small) = (12.0, 3.0);
    let ring = make::revolve(
        &[vec![PSeg::Circle {
            c: v2(big, 0.0),
            r: small,
        }]],
        V3::ZERO,
        V3::Z,
        V3::X,
        TAU,
    )
    .unwrap();
    let (moments, _, v) = spins(&ring);
    let about = v * (big * big + 0.75 * small * small);
    assert!(near(moments[2], about, about), "{moments:?}");
    let across = v * (0.5 * big * big + 0.625 * small * small);
    assert!(near(moments[0], across, about) && near(moments[1], across, about));
}
