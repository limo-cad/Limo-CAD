//! Operations captured from real recipes that once failed.
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
