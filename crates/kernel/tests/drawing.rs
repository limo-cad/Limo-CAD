//! Line drawings and sections.
mod common;

use common::*;
use limo_cad_kernel::make::{self, PSeg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::project::{cut, draw, DrawOptions, Drawing, View};
use limo_cad_kernel::{boolean, Body, Op};
use std::f64::consts::TAU;

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

/// Where a set of drawn lines crosses the height `y` on paper.
fn crossings(lines: &[Vec<V2>], y: f64) -> Vec<f64> {
    let mut found = Vec::new();
    for line in lines {
        for w in line.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a.y > y) != (b.y > y) {
                found.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
    }
    found.sort_by(f64::total_cmp);
    found
}

/// A free-form skin has an outline too: seen from the side, a vase shows
/// the curve it was turned to, left and right.
#[test]
fn a_free_form_skin_shows_its_outline() {
    use limo_cad_kernel::loft;
    use limo_cad_kernel::make::Seg;
    use limo_cad_kernel::SurfKind;
    let circle = |r: f64, z: f64| {
        vec![Seg::Circle {
            c: v3(0.0, 0.0, z),
            p: v3(r, 0.0, z),
            n: V3::Z,
        }]
    };
    let vase = loft::skin(
        &[circle(8.0, 0.0), circle(12.0, 15.0), circle(6.0, 30.0)],
        false,
    )
    .unwrap();
    // Looking along x, paper is (y, z).
    let drawing = look(&[&vase], V3::X, V3::Z);
    let level = |line: &Vec<V2>| line.iter().all(|p| (p.y - line[0].y).abs() < 1.0e-9);
    let outline: Vec<Vec<V2>> = drawing
        .visible
        .iter()
        .filter(|line| !level(line))
        .cloned()
        .collect();
    assert!(!outline.is_empty(), "no outline was drawn");
    // Every point of it is a point of the skin, where the skin stands
    // edge on to the eye: in the plane x = 0.
    let skins: Vec<u32> = (0..vase.face_count() as u32)
        .filter(|&f| vase.f_surface(f).kind == SurfKind::Spline)
        .collect();
    for line in &outline {
        for p in line {
            let at = v3(0.0, p.x, p.y);
            let off = skins
                .iter()
                .map(|&f| vase.f_surface(f).sdist(at).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(off < 1.0e-6, "{p:?} is {off} off the skin");
        }
    }
    // And it is all there, on both sides, from the foot to the lip.
    for k in 1..30 {
        let z = k as f64;
        let across = crossings(&outline, z);
        assert_eq!(across.len(), 2, "at height {z}: {across:?}");
        assert!((across[0] + across[1]).abs() < 1.0e-6);
        assert!(across[1] > 5.9 && across[1] < 12.1);
    }
    let widest = outline
        .iter()
        .flatten()
        .fold(0.0_f64, |w, p| w.max(p.x.abs()));
    assert!((widest - 12.03).abs() < 0.01, "{widest}");
    // Nothing of a vase hides its own outline from the side.
    assert!(drawing.hidden.iter().all(level));
    // Tipped towards the eye it still has one, and the far rim of the
    // foot goes behind it.
    let tipped = look(&[&vase], v3(1.0, 0.0, 0.6), V3::Z);
    let curved = |lines: &[Vec<V2>]| lines.iter().filter(|line| line.len() > 2).count();
    assert!(curved(&tipped.visible) >= 3, "{}", curved(&tipped.visible));
    assert!(!tipped.hidden.is_empty());
}

/// The flanks of a thread and a rolling-ball round are drawn by their
/// outlines as well.
#[test]
fn threads_and_rounds_show_their_outlines() {
    use limo_cad_kernel::blend::fillet;
    use limo_cad_kernel::make::{Region, Seg};
    use limo_cad_kernel::{CurveKind, Frame};
    let quiet = DrawOptions {
        hidden: false,
        smooth_edges: false,
        deflection: 0.02,
    };
    // A rod with a groove screwed into it, from the side. Its edges are
    // the helices along crest and root; between them, on either side, the
    // flanks turn away from the eye, once a turn each.
    let pitch = 2.0;
    let tooth: Vec<PSeg> = {
        let p = [v2(4.0, -0.3), v2(5.2, -0.9), v2(5.2, 0.9), v2(4.0, 0.3)];
        (0..4)
            .map(|i| PSeg::Line {
                a: p[i],
                b: p[(i + 1) % 4],
            })
            .collect()
    };
    let rod = make::cylinder(V3::ZERO, V3::Z, 5.0, 9.0).unwrap();
    let groove = make::screw(
        &tooth,
        V3::ZERO,
        V3::Z,
        V3::X,
        pitch / TAU,
        -TAU,
        TAU * (9.0 / pitch + 1.0),
    )
    .unwrap();
    let threaded = boolean(&rod, &groove, Op::Cut).unwrap();
    let view = View::new(v3(0.0, -1.0, 0.0), V3::Z).unwrap();
    let with = draw(&[&threaded], &view, &quiet);
    // Lines that reach inside the rod's own outline near its side, where
    // only a flank can be: between the root and the crest.
    let flank = |line: &Vec<V2>| {
        line.iter()
            .all(|p| p.x.abs() > 3.9 && p.x.abs() < 5.0 + 1.0e-6)
            && line.iter().any(|p| p.x.abs() < 4.6)
            && line.iter().any(|p| p.x.abs() > 4.8)
    };
    let flanks = with.visible.iter().filter(|line| flank(line)).count();
    assert!(flanks >= 6, "{flanks} flank outlines");
    for line in &with.visible {
        for p in line {
            assert!(p.x.abs() <= 5.0 + 1.0e-6 && p.y >= -1.0e-6 && p.y <= 9.0 + 1.0e-6);
        }
    }
    // A rod cut off on a slant and rounded there: the round is the ball's
    // own surface, and from the side it closes the corner with an arc.
    let rod = make::cylinder(V3::ZERO, V3::Z, 5.0, 20.0).unwrap();
    let n = v3(0.3, 0.0, 1.0).unit();
    let f = Frame::from_z(v3(0.0, 0.0, 15.0), n);
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
    let above = make::extrude(&region, n, 0.0, 30.0).unwrap();
    let slanted = boolean(&rod, &above, Op::Cut).unwrap();
    let rim: Vec<u32> = (0..slanted.edge_count() as u32)
        .filter(|&e| slanted.e_curve_ref(e).kind == CurveKind::Ellipse)
        .collect();
    let rounded = fillet(&slanted, &rim, 1.0).unwrap();
    let before = draw(&[&slanted], &view, &quiet);
    let after = draw(&[&rounded], &view, &quiet);
    // Sharp, the side of the rod runs straight up into the corner. Rounded,
    // the two corners are arcs: lines of more than two points near the top
    // that the sharp one does not have.
    let arcs = |drawing: &Drawing| {
        drawing
            .visible
            .iter()
            .filter(|line| {
                line.len() > 2
                    && line.iter().all(|p| p.y > 12.0)
                    && line.iter().any(|p| p.x.abs() > 4.5)
                    && line.iter().any(|p| p.x.abs() < 4.4)
            })
            .count()
    };
    assert!(
        arcs(&after) >= arcs(&before) + 2,
        "{} then {}",
        arcs(&before),
        arcs(&after)
    );
}
