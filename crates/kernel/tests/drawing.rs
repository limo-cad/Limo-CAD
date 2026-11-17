//! Line drawings and sections.
mod common;

use common::*;
use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::project::{cut, draw, DrawOptions, Drawing, View};
use limo_cad_kernel::{boolean, Body, Op};

fn length(lines: &[Vec<V2>]) -> f64 {
    lines
        .iter()
        .map(|line| line.windows(2).map(|w| (w[1] - w[0]).len()).sum::<f64>())
        .sum()
}

/// Runs that lie along the segment from `a` to `b`, as stretches of it
/// (0 at `a`, 1 at `b`), sorted.
fn along(lines: &[Vec<V2>], a: V2, b: V2) -> Vec<(f64, f64)> {
    let d = b - a;
    let at = |p: V2| (p - a).dot(d) / d.dot(d);
    let on = |p: V2| ((p - a).cross(d) / d.len()).abs() < 1.0e-6;
    let mut spans: Vec<(f64, f64)> = lines
        .iter()
        .filter(|line| line.iter().all(|p| on(*p)))
        .map(|line| {
            let (lo, hi) = line
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(at(*p)), hi.max(at(*p)))
                });
            (lo, hi)
        })
        .collect();
    spans.sort_by(|x, y| x.0.total_cmp(&y.0));
    spans
}

fn look(bodies: &[&Body], toward: V3, up: V3) -> Drawing {
    let view = View::new(toward, up).unwrap();
    draw(bodies, &view, &DrawOptions::default())
}

#[test]
fn a_block_from_a_corner() {
    let block = make::block(v3(0.0, 0.0, 0.0), v3(30.0, 20.0, 10.0)).unwrap();
    let drawing = look(&[&block], v3(1.0, 1.0, 1.0), V3::Z);
    // Three faces show, with nine edges between them; the three edges that
    // meet in the far corner are behind.
    assert_eq!(drawing.visible.len(), 9);
    assert_eq!(drawing.hidden.len(), 3);
    // On paper every edge keeps its length times the sine of its angle to
    // the line of sight.
    let seen = (2.0_f64 / 3.0).sqrt();
    let all = 4.0 * (30.0 + 20.0 + 10.0) * seen;
    let behind = (30.0 + 20.0 + 10.0) * seen;
    assert!((length(&drawing.hidden) - behind).abs() < 1e-6);
    assert!((length(&drawing.visible) - (all - behind)).abs() < 1e-6);
}

#[test]
fn a_rod_and_a_ball_show_their_outlines() {
    let rod = make::cylinder(V3::ZERO, V3::Z, 10.0, 20.0).unwrap();
    let drawing = look(&[&rod], V3::X, V3::Z);
    // From the side: two upright lines where the rod turns away, and its
    // two rims seen edge on.
    for x in [-10.0, 10.0] {
        let spans = along(&drawing.visible, v2(x, 0.0), v2(x, 20.0));
        assert!(
            spans.iter().any(|s| s.0 < 1.0e-6 && s.1 > 1.0 - 1.0e-6),
            "no outline at {x}: {spans:?}"
        );
    }
    for z in [0.0, 20.0] {
        let spans = along(&drawing.visible, v2(-10.0, z), v2(10.0, z));
        assert!(!spans.is_empty(), "no rim at {z}");
    }
    // From above: the rim, and nothing of the outline.
    let above = look(&[&rod], V3::Z, V3::Y);
    assert!(
        (length(&above.visible) - TAU * 10.0).abs() < 0.05,
        "seen {} in {} runs, hidden {} in {}",
        length(&above.visible),
        above.visible.len(),
        length(&above.hidden),
        above.hidden.len()
    );

    let half = [
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
    ];
    let ball = make::revolve(&[half.to_vec()], V3::ZERO, V3::Z, V3::X, TAU).unwrap();
    let drawing = look(&[&ball], v3(2.0, -1.0, 0.5), V3::Z);
    assert!((length(&drawing.visible) - TAU * 8.0).abs() < 0.05);
    assert!(drawing.hidden.is_empty());
}

#[test]
fn one_block_hides_part_of_another() {
    // Looking along +y: the first block stands in front of the left end of
    // the second.
    let front = make::block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0)).unwrap();
    let back = make::block(v3(5.0, 20.0, 2.0), v3(25.0, 30.0, 8.0)).unwrap();
    let drawing = look(&[&front, &back], -V3::Y, V3::Z);
    // The lower front edge of the back block runs from x = 5 to x = 25 on
    // paper at height 2: hidden up to x = 10, seen from there on.
    let (a, b) = (v2(5.0, 2.0), v2(25.0, 2.0));
    let seen = along(&drawing.visible, a, b);
    let unseen = along(&drawing.hidden, a, b);
    assert!(
        seen.iter()
            .any(|s| (s.0 - 0.25).abs() < 1e-4 && (s.1 - 1.0).abs() < 1e-6),
        "{seen:?}"
    );
    assert!(
        unseen
            .iter()
            .any(|s| s.0.abs() < 1e-6 && (s.1 - 0.25).abs() < 1e-4),
        "{unseen:?}"
    );
    // Without hidden lines nothing of it is reported.
    let view = View::new(-V3::Y, V3::Z).unwrap();
    let plain = draw(
        &[&front, &back],
        &view,
        &DrawOptions {
            hidden: false,
            ..Default::default()
        },
    );
    assert!(plain.hidden.is_empty());
    assert_eq!(plain.visible.len(), drawing.visible.len());
}

#[test]
fn a_plane_cuts_a_drilled_plate() {
    let plate = make::block(v3(0.0, 0.0, 0.0), v3(40.0, 30.0, 10.0)).unwrap();
    let drill = make::cylinder(v3(20.0, 15.0, -1.0), V3::Z, 5.0, 12.0).unwrap();
    let plate = boolean(&plate, &drill, Op::Cut).unwrap();
    // Through the middle of the hole: two rectangles of material.
    let through = cut(&plate, v3(20.0, 0.0, 0.0), V3::X, None, 0.01).unwrap();
    let kept = through.kept.unwrap();
    kept.check().unwrap();
    assert!((volume(&kept) - 0.5 * volume(&plate)).abs() < 1e-4 * volume(&plate));
    assert_eq!(through.outlines.len(), 2);
    for line in &through.outlines {
        let around: f64 = line.windows(2).map(|w| w[0].dist(w[1])).sum();
        assert!((around - 40.0).abs() < 1e-6, "{around}");
        assert!(line.iter().all(|p| (p.x - 20.0).abs() < 1e-9));
    }
    // Only a slice 4 deep behind the plane.
    let slice = cut(&plate, v3(20.0, 0.0, 0.0), V3::X, Some(4.0), 0.01).unwrap();
    let kept = slice.kept.unwrap();
    assert!(kept.bbox().lo.x > 16.0 - 1e-6 && kept.bbox().hi.x < 20.0 + 1e-6);
    assert_eq!(slice.outlines.len(), 2);

    // Beside the plate, on its face, and in front of all of it.
    let beside = cut(&plate, v3(50.0, 0.0, 0.0), V3::X, None, 0.01).unwrap();
    assert!(beside.outlines.is_empty() && !beside.touches && beside.kept.is_some());
    let on_face = cut(&plate, v3(40.0, 0.0, 0.0), V3::X, None, 0.01).unwrap();
    assert!(on_face.outlines.is_empty() && on_face.touches);
    let before = cut(&plate, v3(-5.0, 0.0, 0.0), V3::X, None, 0.01).unwrap();
    assert!(before.kept.is_none() && before.outlines.is_empty());
}

/// A plane laid on a face of the body cuts nothing, and says where it
/// touches: the outline of that face, holes and all.
#[test]
fn a_plane_on_a_face_reports_the_face() {
    use limo_cad_kernel::project::cut;
    let plate = make::block(V3::ZERO, v3(18.0, 12.0, 4.0)).unwrap();
    let bore = make::cylinder(v3(9.0, 6.0, -1.0), V3::Z, 2.0, 6.0).unwrap();
    let plate = boolean(&plate, &bore, Op::Cut).unwrap();
    for (height, facing) in [(4.0, V3::Z), (4.0, -V3::Z), (0.0, V3::Z), (0.0, -V3::Z)] {
        let found = cut(&plate, v3(0.0, 0.0, height), facing, None, 0.01).unwrap();
        assert!(found.outlines.is_empty() && found.touches);
        assert_eq!(found.contact.len(), 2, "the face and its hole");
        let length = |line: &Vec<V3>| line.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>();
        let mut lengths: Vec<f64> = found.contact.iter().map(length).collect();
        lengths.sort_by(f64::total_cmp);
        assert!((lengths[1] - 60.0).abs() < 1.0e-9);
        // The hole is drawn in chords, a little shorter than the circle.
        assert!((lengths[0] - 4.0 * std::f64::consts::PI).abs() < 0.05);
    }
    // Clear of the body there is nothing to report.
    let found = cut(&plate, v3(0.0, 0.0, 9.0), V3::Z, None, 0.01).unwrap();
    assert!(found.outlines.is_empty() && !found.touches && found.contact.is_empty());
}
