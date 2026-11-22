//! Screw threads: helical sweeps and what they cut.
mod common;

use common::*;
use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::props::mass;
use limo_cad_kernel::{boolean, v2, v3, Body, Op, SurfKind, V3};
use std::f64::consts::{PI, TAU};

/// A tooth of trapezoid section: from `inner` to `outer`, half as wide as
/// given at each.
fn tooth(inner: f64, half_in: f64, outer: f64, half_out: f64) -> Vec<PSeg> {
    let p = [
        v2(inner, -half_in),
        v2(outer, -half_out),
        v2(outer, half_out),
        v2(inner, half_in),
    ];
    (0..4)
        .map(|i| PSeg::Line {
            a: p[i],
            b: p[(i + 1) % 4],
        })
        .collect()
}

/// What a profile sweeps in one radian: its area, each bit at its own
/// distance from the axis.
fn per_radian(inner: f64, half_in: f64, outer: f64, half_out: f64) -> f64 {
    // The width at radius ρ is linear; ∫ 2 w(ρ) ρ dρ.
    let (a, b) = (inner, outer);
    let slope = (half_out - half_in) / (b - a);
    let w = |rho: f64| half_in + slope * (rho - a);
    // Simpson is exact for a cubic.
    let f = |rho: f64| 2.0 * w(rho) * rho;
    (b - a) / 6.0 * (f(a) + 4.0 * f(0.5 * (a + b)) + f(b))
}

#[test]
fn a_tooth_swept_along_a_screw() {
    let pitch = 2.0;
    let lead = pitch / TAU;
    // A tooth that narrows outwards, one that widens, and a square one,
    // whose flanks go straight out from the axis.
    for (inner, outer, half_in, half_out, turns, hand) in [
        (4.0, 5.2, 0.8, 0.3, 0.4, 1.0),
        (4.0, 5.2, 0.8, 0.3, 1.0, 1.0),
        (4.0, 5.2, 0.8, 0.3, 6.5, 1.0),
        (4.0, 5.2, 0.8, 0.3, 3.25, -1.0),
        (4.0, 5.2, 0.3, 0.6, 2.5, 1.0),
        (4.0, 5.2, 0.5, 0.5, 0.4, 1.0),
        (4.0, 5.2, 0.5, 0.5, 6.5, 1.0),
        (4.0, 5.2, 0.5, 0.5, 3.25, -1.0),
    ] {
        let body = make::screw(
            &tooth(inner, half_in, outer, half_out),
            v3(1.0, -2.0, 0.5),
            V3::Z,
            V3::X,
            hand * lead,
            -0.7,
            -0.7 + TAU * turns,
        )
        .unwrap();
        // Two flanks, the strips inside and outside, and the two ends:
        // however many turns.
        assert_eq!(body.face_count(), 6);
        assert_eq!(count(&body, SurfKind::Helicoid), 2);
        assert_eq!(count(&body, SurfKind::Cylinder), 2);
        let m = mass(&body);
        let wanted = TAU * turns * per_radian(inner, half_in, outer, half_out);
        assert!(m.exact);
        assert!(
            (m.volume - wanted).abs() < 1.0e-9 * wanted,
            "{turns} turns: volume {} for {wanted}",
            m.volume
        );
        // Its skin: two flanks, each a line from `inner` to `outer` that
        // leans as the tooth narrows; two strips of a cylinder between
        // helices; two flat ends. A flank is no polynomial along its
        // rulings, which a rule fit for the volume once took it for.
        let lean = ((half_out - half_in) / (outer - inner)).hypot(1.0);
        let flank = |r: f64| {
            let root = (lean * r).hypot(lead);
            0.5 * r * root + 0.5 * lead * lead / lean * (lean * r / lead).asinh()
        };
        let skin = TAU
            * turns
            * (2.0 * (flank(outer) - flank(inner))
                + 2.0 * half_in * inner
                + 2.0 * half_out * outer)
            + 2.0 * (half_in + half_out) * (outer - inner);
        assert!(
            (m.area - skin).abs() < 1.0e-9 * skin,
            "{turns} turns: area {} for {skin}",
            m.area
        );
        assert_mesh_is_a_clean_skin(&body);
    }
}

/// A square thread: flanks straight out from the axis, groove and ridge
/// each half the pitch wide, so the groove takes half of the ring between
/// root and crest whatever the length.
#[test]
fn a_square_thread_and_its_nut() {
    use limo_cad_kernel::step::write;
    use limo_cad_kernel::thread::{clipped, cutter, Run};
    let (major, minor, pitch) = (12.0, 10.0, 4.0);
    for left_hand in [false, true] {
        let rod = make::cylinder(V3::ZERO, V3::Z, 0.5 * major, 30.0).unwrap();
        let run = Run {
            start: V3::ZERO,
            along: V3::Z,
            zero: V3::X,
            pitch,
            depth: 30.0,
            left_hand,
        };
        let groove = cutter(
            &tooth(0.5 * minor, 0.25 * pitch, 0.5 * major + 0.5, 0.25 * pitch),
            &run,
        )
        .unwrap();
        groove.check().unwrap();
        assert_eq!(count(&groove, SurfKind::Helicoid), 2);
        let groove = clipped(&groove, &run, 0.5 * major, true).unwrap();
        let screw = boolean(&rod, &groove, Op::Cut).unwrap();
        screw.check().unwrap();
        assert_mesh_is_a_clean_skin(&screw);
        assert_eq!(count(&screw, SurfKind::Helicoid), 2);
        let ring = PI * 0.25 * (major * major - minor * minor);
        let taken = mass(&rod).volume - mass(&screw).volume;
        assert!(
            (taken - 0.5 * ring * 30.0).abs() < 1.0e-7 * ring * 30.0,
            "the screw loses {taken} of {}",
            0.5 * ring * 30.0
        );
        // It goes out to STEP like any other thread.
        assert!(write(&[("screw".to_string(), &screw)], "test").contains("B_SPLINE_SURFACE"));

        // The nut: a block bored to the root, with the same groove cut
        // outwards from inside the bore.
        let block = make::block(v3(-12.0, -12.0, 0.0), v3(12.0, 12.0, 12.0)).unwrap();
        let bore = make::cylinder(v3(0.0, 0.0, -1.0), V3::Z, 0.5 * minor, 14.0).unwrap();
        let bored = boolean(&block, &bore, Op::Cut).unwrap();
        let run = Run { depth: 12.0, ..run };
        let groove = cutter(
            &tooth(0.5 * minor - 0.5, 0.25 * pitch, 0.5 * major, 0.25 * pitch),
            &run,
        )
        .unwrap();
        let groove = clipped(&groove, &run, 0.5 * major, true).unwrap();
        let nut = boolean(&bored, &groove, Op::Cut).unwrap();
        nut.check().unwrap();
        assert_mesh_is_a_clean_skin(&nut);
        let taken = mass(&bored).volume - mass(&nut).volume;
        assert!(
            (taken - 0.5 * ring * 12.0).abs() < 1.0e-7 * ring * 12.0,
            "the nut loses {taken} of {}",
            0.5 * ring * 12.0
        );
    }
}

#[test]
fn a_thread_cut_into_a_rod() {
    let pitch = 2.0;
    let lead = pitch / TAU;
    let (inner, outer, half_in, half_out) = (4.0, 5.2, 0.3, 0.9);
    let (radius, length) = (5.0, 9.0);
    let rod = make::cylinder(V3::ZERO, V3::Z, radius, length).unwrap();
    // From a turn below the rod to a turn above it.
    let cutter = make::screw(
        &tooth(inner, half_in, outer, half_out),
        V3::ZERO,
        V3::Z,
        V3::X,
        lead,
        -TAU,
        TAU * (length / pitch + 1.0),
    )
    .unwrap();
    let threaded = boolean(&rod, &cutter, Op::Cut).unwrap();
    threaded.check().unwrap();
    // The groove inside the rod: for every bit of the tooth within the
    // rod's radius, as many radians as the rod is long.
    let slope = (half_out - half_in) / (outer - inner);
    let at_surface = half_in + slope * (radius - inner);
    let groove = length / lead * per_radian(inner, half_in, radius, at_surface);
    let whole = PI * radius * radius * length;
    let m = mass(&threaded);
    assert!(m.exact);
    assert!(
        (m.volume - (whole - groove)).abs() < 1.0e-8 * whole,
        "volume {} for {}",
        m.volume,
        whole - groove
    );
    // Crest, root, two flanks and the two ends.
    assert_eq!(threaded.face_count(), 6);
    assert_eq!(count(&threaded, SurfKind::Helicoid), 2);
    assert_eq!(count(&threaded, SurfKind::Cylinder), 2);
    assert_eq!(count(&threaded, SurfKind::Plane), 2);
    assert_mesh_is_a_clean_skin(&threaded);
    // What was cut away and what is left make up the rod.
    let chips: Body = boolean(&rod, &cutter, Op::Intersect).unwrap();
    assert!((mass(&chips).volume - groove).abs() < 1.0e-8 * whole);
}

/// An M8 x 1.25 thread of class 6g on part of a pin: the face is turned
/// down to the class's major diameter, and the groove stops where the
/// thread does.
#[test]
fn a_metric_thread_on_part_of_a_pin() {
    use limo_cad_kernel::thread::{clipped, cutter, iso_external, Run};
    // The class at its largest: 28 thousandths under the basic profile's
    // 8, 7.188 and 6.466.
    let (major, middle, minor) = (7.972, 7.160, 6.438);
    let pin = make::cylinder(V3::ZERO, V3::Z, 4.0, 14.0).unwrap();
    for depth in [5.0, 10.0, 14.0] {
        let run = Run {
            start: V3::ZERO,
            along: V3::Z,
            zero: V3::X,
            pitch: 1.25,
            depth,
            left_hand: false,
        };
        let over = 1.25e-4;
        let sleeve = make::lathe(
            v3(0.0, 0.0, -over),
            V3::Z,
            &[
                (0.5 * major, 0.0),
                (4.0 + over, 0.0),
                (4.0 + over, depth + 2.0 * over),
                (0.5 * major, depth + 2.0 * over),
            ],
        )
        .unwrap();
        let turned = boolean(&pin, &sleeve, Op::Cut).unwrap();
        turned.check().unwrap();
        let profile = iso_external(major, middle, minor, 1.25).unwrap();
        let groove = cutter(&profile, &run).unwrap();
        groove.check().unwrap();
        let groove = clipped(&groove, &run, 0.5 * major, true)
            .unwrap_or_else(|e| panic!("depth {depth}: {e}"));
        groove.check().unwrap();
        assert_mesh_is_a_clean_skin(&groove);
        let threaded =
            boolean(&turned, &groove, Op::Cut).unwrap_or_else(|e| panic!("depth {depth}: {e}"));
        threaded.check().unwrap();
        // What the groove takes is the part of the tooth within the
        // turned face, for as many radians as the thread is long.
        let slope = (PI / 6.0).tan();
        let at_inner = 0.25 * 1.25 - 0.5 * (middle - minor) * slope;
        let at_outer = 0.25 * 1.25 + 0.5 * (major - middle) * slope;
        let taken = depth / (1.25 / TAU) * per_radian(0.5 * minor, at_inner, 0.5 * major, at_outer);
        let (before, after) = (mass(&turned).volume, mass(&threaded).volume);
        assert!(
            (before - after - taken).abs() < 1.0e-7 * before,
            "depth {depth}: {before} less {taken} is not {after}"
        );
        // Crest, root and two flanks; the ends of the pin and its plain
        // part; and where the thread stops short, the step it was turned
        // down from and the wall the groove ends at.
        let short = depth < 14.0;
        assert_eq!(count(&threaded, SurfKind::Helicoid), 2, "depth {depth}");
        assert_eq!(
            count(&threaded, SurfKind::Plane),
            if short { 4 } else { 2 },
            "depth {depth}"
        );
        assert_mesh_is_a_clean_skin(&threaded);
    }
}

/// The rounded 30-degree form, cut as a screw and as the nut for it.
#[test]
fn a_rounded_thread_and_its_nut() {
    use limo_cad_kernel::thread::{clipped, cutter, rounded, Run};
    let (major, minor, pitch, radius) = (24.0, 20.0, 4.0, 0.3);
    // The screw: a rod of the major diameter, threaded along all of it.
    let rod = make::cylinder(V3::ZERO, V3::Z, 0.5 * major, 30.0).unwrap();
    let run = Run {
        start: V3::ZERO,
        along: V3::Z,
        zero: V3::X,
        pitch,
        depth: 30.0,
        left_hand: false,
    };
    let profile = rounded(major, minor, pitch, radius, 0.0, false).unwrap();
    let groove = cutter(&profile, &run).unwrap();
    groove.check().unwrap();
    assert_eq!(count(&groove, SurfKind::Coil), 4);
    assert_eq!(count(&groove, SurfKind::Helicoid), 4);
    assert_mesh_is_a_clean_skin(&groove);
    let groove = clipped(&groove, &run, 0.5 * major, true).unwrap();
    groove.check().unwrap();
    let screw = boolean(&rod, &groove, Op::Cut).unwrap();
    screw.check().unwrap();
    assert_mesh_is_a_clean_skin(&screw);
    let taken = mass(&rod).volume - mass(&screw).volume;
    assert!(taken > 0.0);
    assert!((taken - mass(&boolean(&rod, &groove, Op::Intersect).unwrap()).volume).abs() < 1.0e-6);
    // The groove is cut off square at both ends of the rod, and its tooth
    // is the same above its own middle as below. So it takes what the
    // tooth takes turned once round the axis, for every turn and part of
    // a turn the rod is long. The tooth is worked out here from the form's
    // own numbers: flanks at fifteen degrees through a quarter of the
    // pitch at mid height, a round at the root and one at the crest.
    let (lo, hi, lean) = (0.5 * minor, 0.5 * major, PI / 12.0);
    let reach = radius * (1.0 / lean.cos() - lean.tan());
    let narrow = 0.25 * pitch - 0.5 * (hi - lo) * lean.tan() - reach;
    let wide = 0.25 * pitch + 0.5 * (hi - lo) * lean.tan() + reach;
    // Half the tooth's width at each distance from the axis, taken along
    // the round at the root, the flank and the round at the crest in turn.
    let from = lean.sin().acos();
    let root = integral(0.0, from, |a| {
        let rho = lo + radius - radius * a.cos();
        2.0 * (narrow + radius * a.sin()) * rho * radius * a.sin()
    });
    let (start, end) = (
        lo + radius * (1.0 - lean.sin()),
        hi - radius * (1.0 - lean.sin()),
    );
    let flank = integral(start, end, |rho| {
        2.0 * (narrow + radius * lean.cos() + (rho - start) * lean.tan()) * rho
    });
    let crest = integral(0.0, from, |a| {
        let rho = hi - radius + radius * a.cos();
        2.0 * (wide - radius * a.sin()) * rho * radius * a.sin()
    });
    let once = TAU * (root + flank + crest);
    let turns = 30.0 / pitch;
    assert!(
        (taken - turns * once).abs() < 1.0e-9 * taken,
        "the screw's groove takes {taken:.9}, not {:.9}",
        turns * once
    );

    // The nut: a block bored to the minor diameter, with a little room.
    let (clearance, axial) = (0.15, 0.2);
    let block = make::block(v3(-20.0, -20.0, 0.0), v3(20.0, 20.0, 18.0)).unwrap();
    let bore = make::cylinder(v3(0.0, 0.0, -1.0), V3::Z, 0.5 * minor + clearance, 20.0).unwrap();
    let bored = boolean(&block, &bore, Op::Cut).unwrap();
    let run = Run {
        start: v3(0.0, 0.0, 18.0),
        along: -V3::Z,
        zero: V3::X,
        pitch,
        depth: 18.0,
        left_hand: false,
    };
    let profile = rounded(
        major + 2.0 * clearance,
        minor + 2.0 * clearance,
        pitch,
        radius,
        axial,
        true,
    )
    .unwrap();
    let groove = cutter(&profile, &run).unwrap();
    groove.check().unwrap();
    let nut = boolean(&bored, &groove, Op::Cut).unwrap_or_else(|e| panic!("the nut: {e}"));
    nut.check().unwrap();
    assert_mesh_is_a_clean_skin(&nut);
    assert!(mass(&nut).volume < mass(&bored).volume);
}

/// A threaded part can be drawn and written out like any other.
#[test]
fn a_threaded_rod_is_drawn_and_exported() {
    use limo_cad_kernel::project::{draw, DrawOptions, View};
    use limo_cad_kernel::step::{read, write};
    let pitch = 2.0;
    let rod = make::cylinder(V3::ZERO, V3::Z, 5.0, 9.0).unwrap();
    let groove = make::screw(
        &tooth(4.0, 0.3, 5.2, 0.9),
        V3::ZERO,
        V3::Z,
        V3::X,
        pitch / TAU,
        -TAU,
        TAU * (9.0 / pitch + 1.0),
    )
    .unwrap();
    let threaded = boolean(&rod, &groove, Op::Cut).unwrap();
    // From the side: the helices of crest and root show as lines.
    let view = View::new(v3(0.0, -1.0, 0.0), V3::Z).unwrap();
    let drawing = draw(
        &[&threaded],
        &view,
        &DrawOptions {
            hidden: true,
            smooth_edges: false,
            deflection: 0.02,
        },
    );
    assert!(
        drawing.visible.len() >= 8,
        "{} lines",
        drawing.visible.len()
    );
    for line in drawing.visible.iter().chain(&drawing.hidden) {
        for p in line {
            assert!(p.x.abs() <= 5.0 + 1.0e-6 && p.y >= -1.0e-6 && p.y <= 9.0 + 1.0e-6);
        }
    }
    // Written out, its flanks are spline surfaces, and are read back as
    // such: the same faces round the same solid.
    let text = write(&[("threaded".to_string(), &threaded)], "test");
    assert!(text.contains("B_SPLINE_SURFACE_WITH_KNOTS"));
    assert!(text.contains("CYLINDRICAL_SURFACE"));
    let back = read(&text).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].face_count(), threaded.face_count());
    assert!(count(&back[0], SurfKind::Spline) >= 2);
    let (before, after) = (mass(&threaded).volume, mass(&back[0]).volume);
    assert!((after - before).abs() < 1.0e-8 * before, "{after} {before}");
    // The body's own text keeps everything.
    assert_eq!(Body::from_text(&threaded.to_text()).unwrap(), threaded);
}
