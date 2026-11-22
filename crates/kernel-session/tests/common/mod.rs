//! Checks shared by the session's integration tests.
#![allow(dead_code)]

use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{Body, SurfKind};

/// Every flat face must be covered exactly once by its triangles: their area
/// has to equal the exact area from the boundary integrals.
pub fn assert_flat_faces_are_covered(body: &Body) {
    let mesh = tessellate(body, &TessOptions::default());
    for f in 0..body.face_count() as u32 {
        let Some(props) = limo_cad_kernel::props::planar_props(body, f) else {
            continue;
        };
        let [first, count] = mesh.faces[f as usize];
        let mut area = 0.0;
        for t in mesh.indices[first as usize..(first + count) as usize]
            .as_chunks::<3>()
            .0
        {
            let (a, b, c) = (
                mesh.positions[t[0] as usize],
                mesh.positions[t[1] as usize],
                mesh.positions[t[2] as usize],
            );
            area += 0.5 * (b - a).cross(c - a).len();
        }
        assert!(
            (area - props.area).abs() <= 0.02 * props.area + 1e-9,
            "face {f} with {} loops: meshed {area}, exact {}",
            props.loops,
            props.area
        );
    }
}

/// The display mesh must be a closed skin on the solid: every triangle
/// stays within the chord tolerance of its surface and faces outwards, and
/// every triangle side is shared with exactly one neighbour running the
/// other way.
pub fn assert_mesh_is_a_clean_skin(body: &Body) {
    let options = TessOptions::default();
    if let Err(flaw) = tessellate(body, &options).check(body, &options) {
        panic!("{flaw}");
    }
}

pub fn count(body: &Body, kind: SurfKind) -> usize {
    (0..body.face_count() as u32)
        .filter(|&f| body.f_surface(f).kind == kind)
        .count()
}

/// What a body holds, to compare with a value worked out by hand: weighed
/// as a solid.
pub fn volume(body: &Body) -> f64 {
    limo_cad_kernel::props::mass(body).volume
}
