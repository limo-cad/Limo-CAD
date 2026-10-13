//! Print-in-place demo meshes for slicer smoke fixtures.
//!
//! Three-body captive **drawer clip**: housing rail + sliding drawer + side
//! latch bar. Every inter-body **box pair** keeps ≥ [`CLEAR_MM`] AABB
//! separation (no fused solids, no intentional interference). Printed flat.

use limo_cad_core::{BodyAppearance, BodyId, Rgba8};

use crate::{find_preset, TriangleMesh};

/// Minimum FDM clearance between distinct print-in-place bodies (mm).
pub const CLEAR_MM: f32 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Aabb {
    min: [f32; 3],
    max: [f32; 3],
}

impl Aabb {
    fn from_box(b: [f32; 6]) -> Self {
        Self {
            min: [b[0], b[2], b[4]],
            max: [b[1], b[3], b[5]],
        }
    }

    /// Euclidean gap when AABBs are separated; `0.0` when overlapping.
    fn separation(self, other: Self) -> f32 {
        let mut gap = [0.0f32; 3];
        for (i, axis_gap) in gap.iter_mut().enumerate() {
            if self.max[i] < other.min[i] {
                *axis_gap = other.min[i] - self.max[i];
            } else if other.max[i] < self.min[i] {
                *axis_gap = self.min[i] - other.max[i];
            }
        }
        if gap[0] > 0.0 || gap[1] > 0.0 || gap[2] > 0.0 {
            (gap[0] * gap[0] + gap[1] * gap[1] + gap[2] * gap[2]).sqrt()
        } else {
            0.0
        }
    }
}

/// Axis-aligned box as a watertight triangle mesh fragment (local coords).
fn box_solid(
    xmin: f32,
    xmax: f32,
    ymin: f32,
    ymax: f32,
    zmin: f32,
    zmax: f32,
) -> (Vec<f32>, Vec<u32>) {
    debug_assert!(xmax > xmin && ymax > ymin && zmax > zmin);
    let positions = vec![
        xmin, ymin, zmin, xmax, ymin, zmin, xmax, ymax, zmin, xmin, ymax, zmin, xmin, ymin, zmax,
        xmax, ymin, zmax, xmax, ymax, zmax, xmin, ymax, zmax,
    ];
    let indices = vec![
        0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3,
        1, 2, 6, 1, 6, 5,
    ];
    (positions, indices)
}

fn mesh_from_boxes(body_id: BodyId, name: &str, boxes: &[[f32; 6]]) -> TriangleMesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for &[xmin, xmax, ymin, ymax, zmin, zmax] in boxes {
        let (p, i) = box_solid(xmin, xmax, ymin, ymax, zmin, zmax);
        let base = (positions.len() / 3) as u32;
        positions.extend(p.into_iter().map(f64::from));
        indices.extend(i.into_iter().map(|v| v + base));
    }
    TriangleMesh {
        body_id,
        name: name.to_string(),
        positions,
        indices,
    }
}

/// Pairwise box clearance across bodies (nested PIP parts need this, not hull AABBs).
pub fn assert_box_clearances(bodies: &[(&str, &[[f32; 6]])], min_clear_mm: f32) {
    assert!(bodies.len() >= 2);
    for (name, boxes) in bodies {
        for b in *boxes {
            assert!(
                b[4] >= -1e-4,
                "body {name} box penetrates bed (z_min={})",
                b[4]
            );
            assert!(
                b[1] > b[0] && b[3] > b[2] && b[5] > b[4],
                "degenerate box on {name}"
            );
        }
    }
    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            for a in bodies[i].1 {
                for b in bodies[j].1 {
                    let gap = Aabb::from_box(*a).separation(Aabb::from_box(*b));
                    assert!(
                        gap + 1e-3 >= min_clear_mm,
                        "bodies '{}' and '{}' violate {} mm clearance (gap={gap})\n  a={a:?}\n  b={b:?}",
                        bodies[i].0,
                        bodies[j].0,
                        min_clear_mm
                    );
                }
            }
        }
    }
}

/// Three-body captive drawer clip (housing + drawer + side latch).
///
/// Layout (mm), printed as separate objects on one plate:
/// - **Housing** — T-rail with rear stop and latch pocket on +X
/// - **Drawer** — notched T-section slider (~14 mm Y travel, mid-stroke pose)
/// - **Latch** — captive side bar that can slide −X into the drawer notch
///
/// Clearance rule: every inter-body box pair is ≥ [`CLEAR_MM`] apart.
pub fn print_in_place_clip() -> (Vec<TriangleMesh>, Vec<BodyAppearance>) {
    let housing_boxes: &[[f32; 6]] = &[
        [0.0, 50.0, 0.0, 42.0, 0.0, 2.5],
        [0.0, 4.0, 0.0, 42.0, 2.5, 7.5],
        [28.0, 32.0, 0.0, 42.0, 2.5, 7.5],
        [0.0, 11.0, 0.0, 42.0, 7.5, 10.0],
        [21.0, 32.0, 0.0, 42.0, 7.5, 10.0],
        [0.0, 32.0, 38.5, 42.0, 2.5, 10.0],
        [0.0, 11.0, 0.0, 3.5, 2.5, 10.0],
        [21.0, 32.0, 0.0, 3.5, 2.5, 10.0],
        [11.0, 21.0, 0.0, 3.5, 7.5, 10.0],
        [32.0, 50.0, 10.0, 32.0, 0.0, 2.5],
        [32.0, 50.0, 30.4, 32.0, 2.5, 8.5],
        [32.0, 50.0, 10.0, 11.6, 2.5, 8.5],
        [48.6, 50.0, 11.6, 30.4, 2.5, 8.5],
        [32.0, 50.0, 10.0, 32.0, 8.5, 10.0],
    ];

    let x0 = 4.0 + CLEAR_MM;
    let x1 = 28.0 - CLEAR_MM;
    let neck0 = 11.0 + CLEAR_MM;
    let neck1 = 21.0 - CLEAR_MM;
    let z0 = 2.5 + CLEAR_MM;
    let z1 = 7.5 - CLEAR_MM;
    let z_stem = 10.0 + CLEAR_MM;
    let y0 = 12.0;
    let y1 = 34.0;

    let drawer_boxes: &[[f32; 6]] = &[
        [x0, x1, y0, 18.0, z0, z1],
        [x0, x1, 22.0, y1, z0, z1],
        [x0, x1 - 3.0, 18.0, 22.0, z0, z1],
        [neck0, neck1, y0, y1, z1, z_stem],
        [8.0, 24.0, y0, y1, z_stem, 13.5],
        [neck0, neck1, 5.5, y0, z0, z1],
    ];

    let latch_boxes: &[[f32; 6]] = &[[
        32.0 + CLEAR_MM,
        48.6 - CLEAR_MM,
        12.0 + CLEAR_MM,
        30.0 - CLEAR_MM,
        2.5 + CLEAR_MM,
        8.5 - CLEAR_MM,
    ]];

    assert_box_clearances(
        &[
            ("PIP Clip Housing", housing_boxes),
            ("PIP Clip Drawer", drawer_boxes),
            ("PIP Clip Latch", latch_boxes),
        ],
        CLEAR_MM,
    );

    let housing = mesh_from_boxes(BodyId(1), "PIP Clip Housing", housing_boxes);
    let drawer = mesh_from_boxes(BodyId(2), "PIP Clip Drawer", drawer_boxes);
    let latch = mesh_from_boxes(BodyId(3), "PIP Clip Latch", latch_boxes);

    let mut housing_app = find_preset("bambu.pla.basic.black")
        .unwrap()
        .to_appearance(BodyId(1));
    housing_app.color = Rgba8::opaque(20, 20, 20);
    housing_app.color_name = "Black".into();

    let mut drawer_app = find_preset("bambu.pla.basic.red")
        .unwrap()
        .to_appearance(BodyId(2));
    drawer_app.color = Rgba8::opaque(200, 40, 40);
    drawer_app.color_name = "Red".into();

    let mut latch_app = find_preset("bambu.pla.basic.blue")
        .unwrap()
        .to_appearance(BodyId(3));
    latch_app.color = Rgba8::opaque(40, 90, 200);
    latch_app.color_name = "Blue".into();

    (
        vec![housing, drawer, latch],
        vec![housing_app, drawer_app, latch_app],
    )
}

/// Backward-compatible alias used by older docs/fixture names.
pub fn print_in_place_latch() -> (Vec<TriangleMesh>, Vec<BodyAppearance>) {
    print_in_place_clip()
}

/// Four-body print-in-place **cam bolt** (no supports, flat on bed).
///
/// Mechanical story after printing:
/// 1. Slide the **bolt** along Y (T-channel on the left).
/// 2. Stepped **wedge** on the bolt drives the **follower** out along +X.
/// 3. Twist the **dial** so its lobe drops into the follower notch to lock travel.
///
/// Print rules baked in:
/// - Every inter-body box pair ≥ [`CLEAR_MM`]
/// - Max height ~12.5 mm; stepped wedge (≈45°) — no supports
/// - No intentional interference / fused solids
pub fn print_in_place_cam_bolt() -> (Vec<TriangleMesh>, Vec<BodyAppearance>) {
    let housing_boxes: &[[f32; 6]] = &[
        [0.0, 70.0, 0.0, 58.0, 0.0, 2.5],
        [0.0, 3.5, 0.0, 42.0, 2.5, 8.0],
        [0.0, 8.5, 0.0, 42.0, 8.0, 10.0],
        [13.5, 18.0, 0.0, 42.0, 8.0, 10.0],
        [0.0, 18.0, 0.0, 3.5, 2.5, 10.0],
        [8.5, 13.5, 0.0, 3.5, 8.0, 10.0],
        [0.0, 20.0, 40.0, 42.0, 2.5, 10.0],
        [18.0, 20.0, 0.0, 16.0, 2.5, 8.0],
        [18.0, 20.0, 28.0, 42.0, 2.5, 8.0],
        [20.0, 44.0, 12.0, 14.0, 2.5, 8.0],
        [20.0, 26.0, 30.0, 32.0, 2.5, 8.0],
        [36.0, 44.0, 30.0, 32.0, 2.5, 8.0],
        [42.0, 44.0, 14.0, 30.0, 2.5, 8.0],
        [20.0, 44.0, 12.0, 32.0, 8.0, 10.0],
        [44.0, 46.0, 36.0, 56.0, 2.5, 8.5],
        [66.0, 68.0, 36.0, 56.0, 2.5, 8.5],
        [46.0, 66.0, 36.0, 38.0, 2.5, 8.5],
        [46.0, 66.0, 54.0, 56.0, 2.5, 8.5],
        [46.0, 52.0, 38.0, 54.0, 8.5, 10.0],
        [60.0, 66.0, 38.0, 54.0, 8.5, 10.0],
        [52.0, 60.0, 38.0, 44.0, 8.5, 10.0],
        [52.0, 60.0, 50.0, 54.0, 8.5, 10.0],
    ];

    let bx0 = 3.5 + CLEAR_MM;
    let bx1 = 18.0 - CLEAR_MM;
    let neck0 = 8.5 + CLEAR_MM;
    let neck1 = 13.5 - CLEAR_MM;
    let bz0 = 2.5 + CLEAR_MM;
    let bz1 = 8.0 - CLEAR_MM;
    let by0 = 6.0;
    let by1 = 34.0;

    let w1 = (bz0 + 1.0).min(bz1);
    let w2 = (bz0 + 2.0).min(bz1);
    let w3 = (bz0 + 3.0).min(bz1);
    let w4 = (bz0 + 4.0).min(bz1);

    let bolt_boxes: &[[f32; 6]] = &[
        [bx0, bx1, by0, by1, bz0, bz1],
        [neck0, neck1, by0, by1, bz1, 10.0 + CLEAR_MM],
        [6.5, 15.5, by0, by1, 10.0 + CLEAR_MM, 12.5],
        [neck0, neck1, 3.5 + CLEAR_MM, by0, bz0, bz1],
        [bx1, 22.0, 16.0 + CLEAR_MM, 18.0, bz0, w1],
        [bx1, 24.5, 17.0, 19.0, bz0, w2],
        [bx1, 27.0, 18.0, 20.0, bz0, w3],
        [bx1, 28.0 - CLEAR_MM, 19.0, 21.0, bz0, w4],
    ];

    let follower_boxes: &[[f32; 6]] = &[
        [
            30.0 + CLEAR_MM,
            42.0 - CLEAR_MM,
            14.0 + CLEAR_MM,
            30.0 - CLEAR_MM,
            2.5 + CLEAR_MM,
            8.0 - CLEAR_MM,
        ],
        [
            28.0 + CLEAR_MM,
            30.0 + CLEAR_MM,
            18.5,
            22.5,
            2.5 + CLEAR_MM,
            7.0,
        ],
        [27.0, 35.0, 32.0 + CLEAR_MM, 35.0, 2.5 + CLEAR_MM, 7.5],
    ];

    let dial_boxes: &[[f32; 6]] = &[
        [
            46.0 + CLEAR_MM,
            66.0 - CLEAR_MM,
            38.0 + CLEAR_MM,
            54.0 - CLEAR_MM,
            2.5 + CLEAR_MM,
            8.5 - CLEAR_MM,
        ],
        [
            50.0,
            62.0,
            38.0 + CLEAR_MM,
            40.0,
            2.5 + CLEAR_MM,
            8.5 - CLEAR_MM,
        ],
        [
            50.0,
            62.0,
            52.0,
            54.0 - CLEAR_MM,
            2.5 + CLEAR_MM,
            8.5 - CLEAR_MM,
        ],
        [
            30.0,
            44.0 - CLEAR_MM,
            35.0 + CLEAR_MM,
            38.0 - CLEAR_MM,
            3.0,
            7.2,
        ],
        [
            53.0,
            59.0,
            44.0 + CLEAR_MM,
            50.0 - CLEAR_MM,
            8.5 + CLEAR_MM,
            12.5,
        ],
    ];

    assert_box_clearances(
        &[
            ("PIP Cam Housing", housing_boxes),
            ("PIP Cam Bolt", bolt_boxes),
            ("PIP Cam Follower", follower_boxes),
            ("PIP Cam Dial", dial_boxes),
        ],
        CLEAR_MM,
    );

    let housing = mesh_from_boxes(BodyId(1), "PIP Cam Housing", housing_boxes);
    let bolt = mesh_from_boxes(BodyId(2), "PIP Cam Bolt", bolt_boxes);
    let follower = mesh_from_boxes(BodyId(3), "PIP Cam Follower", follower_boxes);
    let dial = mesh_from_boxes(BodyId(4), "PIP Cam Dial", dial_boxes);

    let mut housing_app = find_preset("bambu.pla.basic.black")
        .unwrap()
        .to_appearance(BodyId(1));
    housing_app.color = Rgba8::opaque(20, 20, 20);
    housing_app.color_name = "Black".into();

    let mut bolt_app = find_preset("bambu.pla.basic.red")
        .unwrap()
        .to_appearance(BodyId(2));
    bolt_app.color = Rgba8::opaque(200, 40, 40);
    bolt_app.color_name = "Red".into();

    let mut follower_app = find_preset("bambu.pla.basic.blue")
        .unwrap()
        .to_appearance(BodyId(3));
    follower_app.color = Rgba8::opaque(40, 90, 200);
    follower_app.color_name = "Blue".into();

    let mut dial_app = find_preset("bambu.pla.basic.orange")
        .unwrap()
        .to_appearance(BodyId(4));
    dial_app.color = Rgba8::opaque(240, 120, 40);
    dial_app.color_name = "Orange".into();

    (
        vec![housing, bolt, follower, dial],
        vec![housing_app, bolt_app, follower_app, dial_app],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_has_three_bodies_and_dfm_clearance() {
        let (meshes, apps) = print_in_place_clip();
        assert_eq!(meshes.len(), 3);
        assert_eq!(apps.len(), 3);
        for mesh in &meshes {
            assert!(mesh.positions.len() >= 8 * 3);
            assert_eq!(mesh.indices.len() % 3, 0);
            assert!(mesh.indices.len() / 3 >= 12);
        }
        let drawer = &meshes[1];
        let min_z = drawer
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[2])
            .fold(f64::MAX, f64::min);
        assert!(min_z >= 2.5 + f64::from(CLEAR_MM) - 1e-3);
    }

    #[test]
    fn cam_bolt_has_four_bodies_and_dfm_clearance() {
        let (meshes, apps) = print_in_place_cam_bolt();
        assert_eq!(meshes.len(), 4);
        assert_eq!(apps.len(), 4);
        for mesh in &meshes {
            assert!(mesh.positions.len() >= 8 * 3);
            assert!(mesh.triangle_count() >= 12);
        }
        assert!(meshes.iter().all(|m| {
            m.positions
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| p[2])
                .fold(f64::MAX, f64::min)
                >= -1e-3
        }));
    }

    #[test]
    fn overlapping_aabbs_report_zero_gap() {
        let a = Aabb {
            min: [0.0, 0.0, 0.0],
            max: [10.0, 10.0, 10.0],
        };
        let b = Aabb {
            min: [5.0, 5.0, 5.0],
            max: [15.0, 15.0, 15.0],
        };
        assert_eq!(a.separation(b), 0.0);
        let c = Aabb {
            min: [10.4, 0.0, 0.0],
            max: [20.0, 10.0, 10.0],
        };
        assert!((a.separation(c) - 0.4).abs() < 1e-4);
    }
}
