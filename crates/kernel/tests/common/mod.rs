//! Checks shared by the kernel's integration tests.
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

/// Volume to compare with closed forms. The chord error of a mesh falls with
/// the square of its step, so two meshes extrapolate it away.
pub fn volume(body: &Body) -> f64 {
    let at =
        |linear: f64, angular: f64| tessellate(body, &TessOptions { linear, angular }).volume();
    (4.0 * at(0.0005, 0.02) - at(0.002, 0.04)) / 3.0
}

/// The integral of `f` from `a` to `b`: five-point Gauss rules on two
/// hundred pieces, which is past the last figure of a double for the
/// smooth things the tests integrate.
pub fn integral(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    const X: [f64; 5] = [
        0.0,
        0.538_469_310_105_683_1,
        -0.538_469_310_105_683_1,
        0.906_179_845_938_664,
        -0.906_179_845_938_664,
    ];
    const W: [f64; 5] = [
        0.568_888_888_888_888_9,
        0.478_628_670_499_366_47,
        0.478_628_670_499_366_47,
        0.236_926_885_056_189_08,
        0.236_926_885_056_189_08,
    ];
    let pieces = 200;
    let step = (b - a) / pieces as f64;
    (0..pieces)
        .map(|i| {
            let mid = a + (i as f64 + 0.5) * step;
            (0..5)
                .map(|k| W[k] * f(mid + 0.5 * step * X[k]))
                .sum::<f64>()
                * 0.5
                * step
        })
        .sum()
}
