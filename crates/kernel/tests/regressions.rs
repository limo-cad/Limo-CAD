//! Operations captured from real recipes that once failed, and pairs of
//! solids from the stress run that once came out wrong.
use limo_cad_kernel::demo::random_pair;
use limo_cad_kernel::props;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{boolean, Body, Op};

fn fixture(name: &str) -> Body {
    let path = format!("{}/tests/fixtures/{name}.body", env!("CARGO_MANIFEST_DIR"));
    let body = Body::from_text(&std::fs::read_to_string(path).unwrap()).unwrap();
    body.check().unwrap();
    body
}

fn volume(body: &Body) -> f64 {
    tessellate(body, &TessOptions::default()).volume()
}

/// A plane through the axis of a tube meets each wall exactly on the seam
/// vertex of its circular edges.
#[test]
fn half_tube_cut_through_the_circle_seam() {
    let (tube, half) = (fixture("turbine-cut-a"), fixture("turbine-cut-b"));
    let r = boolean(&tube, &half, Op::Cut).unwrap();
    assert_eq!((r.face_count(), r.edge_count(), r.verts.len()), (6, 12, 8));
    let ratio = volume(&r) / volume(&tube);
    assert!((ratio - 0.5).abs() < 5e-3, "{ratio}");
}

/// The tool's flat bottom touches an arched opening along one line, exactly
/// where a symmetric face would be sampled.
#[test]
fn slot_whose_floor_touches_an_arch() {
    let (jaw, slot) = (fixture("vise-slot-a"), fixture("vise-slot-b"));
    let r = boolean(&jaw, &slot, Op::Cut).unwrap();
    assert_eq!(r.face_count(), 40);
    assert!(volume(&r) < volume(&jaw));
    // Text snapshots round-trip exactly.
    assert_eq!(Body::from_text(&r.to_text()).unwrap(), r);
}

/// A pin hole that misses the axis of a ring enters each wall at a slant:
/// the section curves around the pin rise and fall by several times the
/// wall's thickness, and each bounds a band on either side.
#[test]
fn pin_hole_through_a_ring_off_its_axis() {
    let (ring, pin) = (fixture("offset-pin-a"), fixture("offset-pin-b"));
    let r = boolean(&ring, &pin, Op::Cut).unwrap();
    assert_eq!(r.face_count(), 14);
    assert!(volume(&r) < volume(&ring));
}

/// Pairs of `examples/pairs` that each went wrong for a reason of its own.
///
/// * 541: the corner of a block takes in the tip of a cone. The hyperbola
///   on one of its faces turns back within a single cell of the tracer,
///   which joined its two branches into one curve.
/// * 562: a ball pokes through the edge of a block and leaves a sliver
///   between an arc and its chord, thinner than the arc was sampled.
/// * 574: a block cuts a ring into bands that are written a whole turn of
///   the tube apart, and a hole was given to the wrong one.
/// * 361, 367: what is left of a ring is all of it but one hole, and the
///   seam it was cut open along brushed past that hole.
/// * 496: the hole winds round the tube more than once, so no circle round
///   the axis misses it.
/// * 188, 3019: a needle up a cone next to its tip faced inwards, where
///   the cone's own parameters are a skewed picture of it.
/// * 1074: two holes in a ring, and the seam between them was looked for
///   without minding that a hole reaches round the end of a turn.
/// * 1548: the tip of a cone pokes into a ring and nothing else of it
///   does. The curve round the tip is smaller than a cell of the tracer,
///   and the two were called apart.
/// * 2482: a rod leaves a cone a rim that rises steeply to just under its
///   base, and zipping the two edges together turned a triangle over.
/// * 1389: the base of one cone passes a hair from the tip of another. A
///   cone used to count with its mirror image behind the tip, and the two
///   sheets of the section all but touched there.
/// * 2731: two cones, which have no size of their own for the tracer to
///   set its first cells by; it gave up two rounds too early.
/// * 1830: the hole in a ring reaches right round it and all but meets
///   itself, leaving a neck a few hundredths of a millimetre wide. The cut
///   that opens the face out now goes through the hole.
/// * 2921: on a ball, rows of the mesh that slant were always joined
///   along the same diagonal, the long one where they slant the other way.
/// * 4117: a plane cuts a cone in a hyperbola close by its tip. Such a
///   curve is now written down exactly, ruling by ruling.
/// * 4327: a cone leaves most of a narrow ellipse in the side of a block.
///   The point taken to stand for the inside of it lay beyond its far
///   side.
/// * 218 and 6616: the rim of a cone dips into another solid through a
///   lens a fraction of a millimetre wide. The room beside one edge of it
///   was measured to the point of the other edge level with it, not the
///   nearest, and the probe stepped out through the far side.
/// * 4938: meshed finely, the cut that opens a ring's face ran alongside a
///   hole, and the pocket between the two kept the long triangles it was
///   first clipped into: too narrow for a lattice point, so nothing ever
///   looked at it again.
/// * 7168: a rod leans a few thousandths of a turn off the side of a
///   block and dips in through it, leaving a tongue whose outline is one
///   end of an ellipse eight thousand times longer than wide. The point
///   taken beside the tip stepped clean across to the other side of it:
///   the room there is now measured to whatever the step would cross.
/// * 4041, 4560: one cone only just dips through another. The closed
///   curve round the dip was found from two cells of the tracer, drawn
///   twice, and its second drawing was not seen to be the first again.
/// * 5720: the flat end of a cone slices a cap off a ring. The cap and the
///   patch of ring inside the cone are written a turn of the tube apart,
///   and the hole the cap leaves was given to the wrong one.
/// * 17920: the edge of what is left of a ball runs through its pole.
///   The few dozen points the choice of another pole was made by stepped
///   over the one that mattered, and a triangle with a corner on the pole
///   faced whichever way its other two corners lay.
/// * 15145, 18773: meshed finely, two edges of a face run alongside each
///   other closer than their chords stray from them, and the sliver
///   between is thinner than the tolerance it is meshed to. It was held to
///   half of that.
/// * 19483: a cone swallows all of a ring but a blob wider than the ring
///   is round both ways. No circle round the ring, straight or slanted,
///   misses such a hole, and the face was left undrawn. The seam now goes
///   over the hole.
#[test]
fn pairs_that_once_went_wrong() {
    let seeds = [
        188, 218, 361, 367, 496, 541, 562, 574, 1074, 1389, 1548, 1830, 2482, 2731, 2921, 3019,
        4041, 4117, 4327, 4560, 4938, 5720, 6616, 7168, 15145, 17920, 18773, 19483,
    ];
    for seed in seeds {
        let [(_, a), (_, b)] = random_pair(seed);
        let done = |op: Op| boolean(&a, &b, op).unwrap_or_else(|e| panic!("pair {seed}: {e}"));
        let (rest, shared, joined) = (done(Op::Cut), done(Op::Intersect), done(Op::Union));
        let fine = TessOptions {
            linear: if seed == 4938 { 0.001 } else { 0.004 },
            angular: if seed == 4938 { 0.03 } else { 0.06 },
        };
        for body in [&rest, &shared, &joined] {
            body.check().unwrap();
            for options in [TessOptions::default(), fine] {
                let finely = [188, 2921, 4938, 15145, 17920, 18773, 19483];
                if options.linear < 0.1 && !finely.contains(&seed) {
                    continue;
                }
                tessellate(body, &options)
                    .check(body, &options)
                    .unwrap_or_else(|flaw| panic!("pair {seed}: {flaw}"));
            }
        }
        // What is cut away and what is kept make the whole; together the two
        // weigh what they weigh apart, less what they share.
        let (va, vb) = (props::volume(&a), props::volume(&b));
        let (cut, common, union) = (
            props::volume(&rest),
            props::volume(&shared),
            props::volume(&joined),
        );
        let scale = va.max(vb);
        assert!((cut + common - va).abs() < 1.0e-8 * scale, "pair {seed}");
        assert!(
            (union + common - va - vb).abs() < 1.0e-8 * scale,
            "pair {seed}"
        );
    }
}
