//! STEP out and back in.

use limo_cad_kernel::demo::showcase;
use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::props::mass;
use limo_cad_kernel::step::{read, write};
use limo_cad_kernel::{boolean, v2, v3, Body, CurveKind, Op, V3};
use std::f64::consts::TAU;

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

/// Out and back in: the same solid, face for face.
#[track_caller]
fn assert_survives(name: &str, body: &Body) {
    let text = write(&[(name.to_string(), body)], "test");
    let back = read(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut whole = Body::new();
    for solid in &back {
        whole.append(solid);
    }
    whole.check().unwrap_or_else(|e| panic!("{name}: {e}"));
    let (before, after) = (mass(body), mass(&whole));
    let close = |a: f64, b: f64| (a - b).abs() <= 1.0e-9 * a.abs().max(1.0);
    assert!(
        close(before.volume, after.volume) && close(before.area, after.area),
        "{name}: volume {} and area {} came back as {} and {}",
        before.volume,
        before.area,
        after.volume,
        after.area
    );
    assert!(
        (before.centre - after.centre).len() < 1.0e-8,
        "{name}: centre"
    );
    assert_eq!(body.face_count(), whole.face_count(), "{name}: faces");
    assert_eq!(body.edge_count(), whole.edge_count(), "{name}: edges");
}

#[test]
fn plain_solids_come_back_as_they_went() {
    assert_survives(
        "block",
        &make::block(v3(1.0, 2.0, 3.0), v3(11.0, 22.0, 33.0)).unwrap(),
    );
    assert_survives(
        "rod",
        &make::cylinder(v3(5.0, -3.0, 2.0), v3(1.0, 2.0, 2.0), 4.0, 10.0).unwrap(),
    );
    assert_survives(
        "cone",
        &make::lathe(V3::ZERO, V3::Z, &[(0.0, 0.0), (6.0, 9.0), (0.0, 9.0)]).unwrap(),
    );
    // Standing on its base, the tip up: the other way along its axis.
    assert_survives(
        "spire",
        &make::lathe(V3::ZERO, V3::Z, &[(0.0, 0.0), (6.0, 0.0), (0.0, 9.0)]).unwrap(),
    );
    assert_survives(
        "cup",
        &make::lathe(
            v3(3.0, 4.0, 5.0),
            v3(0.0, 1.0, 1.0),
            &[
                (2.0, 0.0),
                (8.0, 0.0),
                (11.0, 9.0),
                (9.0, 9.0),
                (6.5, 1.5),
                (2.0, 1.5),
            ],
        )
        .unwrap(),
    );
    assert_survives("ball", &ball(7.5));
    let tube = PSeg::Circle {
        c: v2(20.0, 0.0),
        r: 6.0,
    };
    assert_survives(
        "ring",
        &make::revolve(&[vec![tube]], V3::ZERO, V3::Z, V3::X, TAU).unwrap(),
    );
}

#[test]
fn parts_come_back_as_they_went() {
    let (mut exact, mut approximate) = (0, 0);
    for (name, body) in showcase().unwrap() {
        // Curves where two round surfaces meet go out as cubics that
        // follow them to a ten-millionth of a millimetre, and come back
        // as those cubics: the same faces, and the same weight to within
        // what that leaves.
        let free = (0..body.edge_count() as u32).any(|e| {
            !matches!(
                body.e_curve_ref(e).kind,
                CurveKind::Line | CurveKind::Circle | CurveKind::Ellipse
            )
        });
        if free {
            let text = write(&[(name.to_string(), &body)], "test");
            assert!(text.contains("B_SPLINE_CURVE_WITH_KNOTS"), "{name}");
            let back = read(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(back.len(), 1, "{name}");
            assert_eq!(back[0].face_count(), body.face_count(), "{name}");
            back[0].check().unwrap();
            let (before, after) = (mass(&body), mass(&back[0]));
            assert!(
                (after.volume - before.volume).abs() < 1.0e-8 * before.volume,
                "{name}: {} for {}",
                after.volume,
                before.volume
            );
            approximate += 1;
        } else {
            assert_survives(name, &body);
            exact += 1;
        }
    }
    assert!(approximate >= 1, "no part with a free-form edge was tried");
    assert!(exact >= 3, "only {exact} parts went round");
}

#[test]
fn several_bodies_and_a_hollow_one() {
    let a = make::block(V3::ZERO, v3(10.0, 10.0, 10.0)).unwrap();
    let b = make::cylinder(v3(30.0, 0.0, 0.0), V3::Z, 4.0, 10.0).unwrap();
    let text = write(&[("a".into(), &a), ("b".into(), &b)], "two parts");
    let back = read(&text).unwrap();
    assert_eq!(back.len(), 2);
    assert!((mass(&back[0]).volume - 1000.0).abs() < 1.0e-9);
    // A block with a ball-shaped hollow inside: two shells, one solid's
    // worth of material.
    let hollow = boolean(
        &a,
        &ball(3.0).transformed(&limo_cad_kernel::Xform::translation(v3(5.0, 5.0, 5.0))),
        Op::Cut,
    )
    .unwrap();
    let text = write(&[("hollow".into(), &hollow)], "");
    let back = read(&text).unwrap();
    let total: f64 = back.iter().map(|b| mass(b).volume).sum();
    assert!((total - mass(&hollow).volume).abs() < 1.0e-8, "{total}");
}

#[test]
fn what_cannot_be_read_is_refused_by_name() {
    assert!(read("not a STEP file")
        .unwrap_err()
        .contains("not a STEP file"));
    let empty = "ISO-10303-21;\nHEADER;ENDSEC;\nDATA;ENDSEC;\nEND-ISO-10303-21;";
    assert!(read(empty).unwrap_err().contains("no solid"));
    // Inches come in as millimetres.
    let block = make::block(V3::ZERO, v3(1.0, 2.0, 3.0)).unwrap();
    let text = write(&[("b".into(), &block)], "").replace(
        "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
        "(CONVERSION_BASED_UNIT('INCH',#9999)LENGTH_UNIT()NAMED_UNIT(#9998))",
    );
    let back = read(&text).unwrap();
    assert!((mass(&back[0]).volume - 6.0 * 25.4_f64.powi(3)).abs() < 1.0e-6);
}
