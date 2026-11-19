//! The demonstration parts, built with the kernel's own API.
mod common;

use common::*;
use limo_cad_kernel::math::*;
use limo_cad_kernel::tess::{tessellate, TessOptions};
use limo_cad_kernel::{demo, make, SurfKind};

#[test]
fn mounting_block() {
    let started = std::time::Instant::now();
    let block = demo::MountingBlock::default().build().unwrap();
    let modelling = started.elapsed();
    block.check().unwrap();
    assert_flat_faces_are_covered(&block);
    assert_mesh_is_a_clean_skin(&block);
    // One torus at the foot of the boss and four around the rim corners.
    assert_eq!(count(&block, SurfKind::Torus), 5);
    assert_eq!(count(&block, SurfKind::Cone), 1);
    let mesh = tessellate(&block, &TessOptions::default());
    assert!(mesh.volume() > 50_000.0 && mesh.volume() < 80.0 * 50.0 * 14.0 + 8_000.0);
    eprintln!(
        "mounting block: {} faces, {} edges, modelled in {modelling:?}, {} triangles",
        block.face_count(),
        block.edge_count(),
        mesh.triangle_count()
    );
}

/// The edge break of the bracket is one fillet whose chains run through
/// straight edges and around rounded corners. Its volume has a closed form.
#[test]
fn bracket_edge_break_follows_its_tangent_chains() {
    let started = std::time::Instant::now();
    let bracket = demo::Bracket::default().build().unwrap();
    let modelling = started.elapsed();
    bracket.check().unwrap();
    assert_flat_faces_are_covered(&bracket);
    assert_mesh_is_a_clean_skin(&bracket);
    // The same faces Open CASCADE builds for this part.
    assert_eq!(bracket.face_count(), 38);
    assert_eq!(count(&bracket, SurfKind::Plane), 8);
    // Eight rounded plate corners and two ends of the bend.
    assert_eq!(count(&bracket, SurfKind::Torus), 10);

    let quarter = 1.0 - 0.25 * PI;
    let r = 0.6;
    // Folded plate, bend, plate corners, holes.
    let plate = (32.0 * 8.0 + 8.0 * 36.0) * 40.0 + quarter * 16.0 * 40.0
        - 4.0 * quarter * 9.0 * 8.0
        - 2.0 * PI * 3.5 * 3.5 * 8.0
        - PI * 13.0 * 13.0 * 8.0;
    // The break removes its cross-section along every chain. Around a corner
    // the path is that of the section's centroid (Pappus).
    let centroid = r * (5.0 / 6.0 - 0.25 * PI) / quarter;
    let straight = 2.0 * (29.0 + 17.0 + 29.0 + 41.0) + 4.0 * 34.0;
    let around = 8.0 * 0.5 * PI * (3.0 - centroid) + 2.0 * 0.5 * PI * (4.0 + centroid);
    // Where two chains meet square on, their sections overlap once.
    let overlap = 2.0 * (5.0 / 3.0 - 0.5 * PI) * r * r * r;
    let expected = plate - quarter * r * r * (straight + around) + overlap;
    let actual = volume(&bracket);
    assert!(
        (actual - expected).abs() < 0.05,
        "volume {actual} instead of {expected}"
    );
    eprintln!(
        "bracket: {} faces, {} edges, modelled in {modelling:?}, volume {actual:.4} (exact {expected:.4})",
        bracket.face_count(),
        bracket.edge_count()
    );
}

#[test]
fn cross_drilled_shaft() {
    let drilled = demo::DrilledShaft::default().build().unwrap();
    drilled.check().unwrap();
    assert_mesh_is_a_clean_skin(&drilled);
    // Wall, cross hole, axial bore above and below the cross hole.
    assert_eq!(count(&drilled, SurfKind::Cylinder), 4);
    assert_eq!(count(&drilled, SurfKind::Cone), 1);
}

#[test]
fn rounded_box() {
    let pebble = demo::RoundedBox::default().build().unwrap();
    pebble.check().unwrap();
    assert_mesh_is_a_clean_skin(&pebble);
    assert_eq!(count(&pebble, SurfKind::Sphere), 8);
    assert_eq!(count(&pebble, SurfKind::Cylinder), 12);
    let (a, b, c, r) = (50.0, 34.0, 22.0, 6.0);
    // Minkowski sum of the inner box and a ball.
    let (x, y, z) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
    let expected = x * y * z
        + 2.0 * r * (x * y + y * z + z * x)
        + PI * r * r * (x + y + z)
        + 4.0 / 3.0 * PI * r * r * r;
    let actual = volume(&pebble);
    assert!(
        (actual - expected).abs() < 1.0e-4 * expected,
        "{actual} vs {expected}"
    );
}

/// Every part must stay valid over the ranges the interactive demo offers.
#[test]
fn parts_stay_valid_across_their_dimensions() {
    let mut built = 0;
    let steps = 7;
    let lerp = |lo: f64, hi: f64, i: usize| lo + (hi - lo) * i as f64 / (steps - 1) as f64;
    for i in 0..steps {
        for j in 0..steps {
            let corner = lerp(0.0, 10.0, i);
            // A rim round larger than the corner it runs around would have to
            // swallow that corner; the kernel reports this instead.
            let rim = if i == 0 {
                lerp(0.0, 4.0, j)
            } else {
                lerp(0.0, 4.0, j).min(0.85 * corner)
            };
            let block = demo::MountingBlock {
                corner,
                rim,
                boss_radius: lerp(10.0, 16.0, (i + j) % steps),
                // High enough for the round at its foot and the bevel on its top.
                boss_height: lerp(5.0, 20.0, j),
                bore_radius: lerp(3.0, 8.0, i),
                counterbore_radius: lerp(3.5, 6.0, (2 * i + j) % steps),
                ..Default::default()
            };
            let body = block.build().unwrap_or_else(|e| panic!("{block:?}: {e}"));
            body.check().unwrap();
            assert_flat_faces_are_covered(&body);
            assert_mesh_is_a_clean_skin(&body);
            built += 1;

            let shaft = demo::DrilledShaft {
                cross_radius: lerp(1.0, 9.0, i),
                cross_height: lerp(14.0, 46.0, j),
                bore_radius: lerp(1.0, 9.0, (i + 3 * j) % steps),
                chamfer: lerp(0.0, 2.5, j),
                ..Default::default()
            };
            let body = shaft.build().unwrap_or_else(|e| panic!("{shaft:?}: {e}"));
            body.check().unwrap();
            assert_mesh_is_a_clean_skin(&body);
            built += 1;

            let bracket = demo::Bracket {
                bend: lerp(0.0, 8.0, i),
                corner: lerp(1.5, 6.0, j),
                edge_break: lerp(0.0, 1.0, (i + j) % steps),
                upright_hole_radius: lerp(4.0, 13.0, i),
                ..Default::default()
            };
            let body = bracket
                .build()
                .unwrap_or_else(|e| panic!("{bracket:?}: {e}"));
            body.check().unwrap();
            assert_flat_faces_are_covered(&body);
            assert_mesh_is_a_clean_skin(&body);
            built += 1;
        }
        let pebble = demo::RoundedBox {
            radius: lerp(0.0, 10.0, i),
            ..Default::default()
        };
        let body = pebble.build().unwrap();
        body.check().unwrap();
        assert_mesh_is_a_clean_skin(&body);
        built += 1;
    }
    eprintln!("{built} variants built");
}

#[test]
fn plate_with_five_holes_is_meshed_exactly() {
    use limo_cad_kernel::make::{Region, Seg};
    let rect = [
        v3(0.0, 0.0, 0.0),
        v3(80.0, 0.0, 0.0),
        v3(80.0, 50.0, 0.0),
        v3(0.0, 50.0, 0.0),
    ];
    let mut loops: Vec<Vec<Seg>> = vec![(0..4)
        .map(|i| Seg::Line {
            a: rect[i],
            b: rect[(i + 1) % 4],
        })
        .collect()];
    for (x, y, r) in [
        (40.0, 25.0, 17.0),
        (10.0, 10.0, 5.0),
        (70.0, 10.0, 5.0),
        (10.0, 40.0, 5.0),
        (70.0, 40.0, 5.0),
    ] {
        loops.push(vec![Seg::Circle {
            c: v3(x, y, 0.0),
            p: v3(x + r, y, 0.0),
            n: V3::Z,
        }]);
    }
    let plate = make::extrude(&Region { loops }, V3::Z, 0.0, 5.0).unwrap();
    assert_flat_faces_are_covered(&plate);
}

/// The interactive demo lets every dimension of every part move freely
/// within its range; whatever it lands on has to model.
#[test]
fn parts_model_anywhere_in_their_ranges() {
    // A fixed sequence, so a failure can be found again.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut unit = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    for part in &demo::PARTS {
        let whole = part.default_body().unwrap();
        whole.check().unwrap();
        for _ in 0..40 {
            let values: Vec<f64> = part
                .dimensions
                .iter()
                .map(|d| ((d.min + (d.max - d.min) * unit()) * 10.0).round() / 10.0)
                .collect();
            let body = (part.build)(&values)
                .unwrap_or_else(|e| panic!("{} at {values:?}: {e}", part.name));
            body.check().unwrap();
            assert_flat_faces_are_covered(&body);
            assert_mesh_is_a_clean_skin(&body);
        }
    }
}
