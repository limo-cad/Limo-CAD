//! A pocket cut against a face boundary keeps both of its arc edges.
//!
//! Regression for the reported "the semicircular pocket's floor arc has no
//! stroke" report: the native kernel does emit the floor arc (a circular edge
//! with a real tessellation), so the missing stroke was a display issue, not
//! missing topology. This pins the geometry side so a future kernel change
//! cannot silently drop the edge while the renderer is being fixed.
#![cfg(feature = "native-occt")]

use limo_cad_core::{OriginPlane, PlaneRef};
use limo_cad_occt::OcctKernel;
use limo_cad_sketch::{
    ArcCenterRequest, RectangleMode, RectangleRequest, SegmentRequest, SketchManager, Vec2,
};
use limo_cad_solid::{CommitKernelRequest, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest};

fn extrusion(
    sketch: &str,
    profiles: Vec<u32>,
    operation: ExtrudeOperation,
    distance: f64,
    targets: Vec<limo_cad_core::BodyId>,
) -> ExtrudeRequest {
    ExtrudeRequest {
        source_face: None,
        sketch_name: sketch.to_string(),
        profile_indices: profiles,
        operation,
        extent: ExtrudeExtent::Distance { distance },
        taper_angle_deg: 0.0,
        flip: false,
        target_body_ids: targets,
    }
}

/// Circular edges at one depth, with the tessellated point count.
fn circular_edges_at(body: &limo_cad_solid::KernelBodyDto, z: f64) -> Vec<usize> {
    body.edges
        .iter()
        .filter(|edge| edge.circle.is_some())
        .filter(|edge| {
            edge.points
                .iter()
                .all(|point| (point.z - z).abs() < 1e-6 && point.z.is_finite())
        })
        .map(|edge| edge.points.len())
        .collect()
}

#[test]
fn a_semicircular_pocket_keeps_its_opening_and_floor_arcs() {
    let mut manager = SketchManager::new();
    manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    manager
        .set_grid_snap(limo_cad_sketch::SetGridSnapRequest { enabled: false })
        .unwrap();
    manager
        .add_rectangle(RectangleRequest {
            mode: RectangleMode::TwoPoint,
            p1: Vec2::new(0.0, 0.0),
            p2: Vec2::new(25.0, 25.0),
            ctrl_held: true,
        })
        .unwrap();
    manager.end_sketch().unwrap();
    let plan = manager
        .prepare_extrude(extrusion(
            "Sketch1",
            vec![0],
            ExtrudeOperation::NewBody,
            10.0,
            vec![],
        ))
        .unwrap();
    let mut kernel = OcctKernel::new().unwrap();
    let scene = kernel.recompute(&plan).unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let update = manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene,
        })
        .unwrap();
    let body_id = update.scene.bodies[0].id;
    let face_id = update.scene.bodies[0]
        .faces
        .iter()
        .find(|face| face.plane.is_some_and(|plane| plane.normal[2] > 0.99))
        .expect("the extrusion cap")
        .id;

    manager
        .begin_sketch(PlaneRef::PlanarFace { face_id })
        .unwrap();
    let sketch = manager.active_snapshot().unwrap();
    assert_eq!(sketch.projected_edges.len(), 4, "support boundary");
    let xs = sketch
        .projected_edges
        .iter()
        .flat_map(|edge| edge.points.iter().map(|point| point.x));
    let ys = sketch
        .projected_edges
        .iter()
        .flat_map(|edge| edge.points.iter().map(|point| point.y));
    let (min_x, max_x) = (
        xs.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
    );
    let (min_y, max_y) = (
        ys.clone().fold(f64::INFINITY, f64::min),
        ys.fold(f64::NEG_INFINITY, f64::max),
    );
    assert!(min_x < max_x && min_y < max_y);
    let centre = Vec2::new((min_x + max_x) / 2.0, max_y);
    manager
        .add_line(SegmentRequest {
            from: Vec2::new(min_x, max_y),
            to_raw: centre,
            ctrl_held: true,
        })
        .unwrap();
    manager
        .add_arc_center(ArcCenterRequest {
            center: centre,
            start: Vec2::new(centre.x - 5.5, max_y),
            sweep: Vec2::new(centre.x + 5.5, max_y),
            ctrl_held: true,
            radius_mm: None,
            radius_text: None,
            angle_text: None,
            sweep_rad: None,
        })
        .unwrap();
    manager.end_sketch().unwrap();

    let entry = manager
        .profile_catalog()
        .into_iter()
        .find(|entry| entry.sketch_name == "Sketch2")
        .expect("face sketch catalog entry");
    let half_disc = std::f64::consts::PI * 5.5 * 5.5 / 2.0;
    let index = entry
        .profiles
        .iter()
        .find(|profile| (profile.area - half_disc).abs() < 1.0)
        .expect("the projected boundary must seal the half disc")
        .index;

    let mut request = extrusion(
        "Sketch2",
        vec![index],
        ExtrudeOperation::Cut,
        5.0,
        vec![body_id],
    );
    request.flip = true;
    let plan = manager.prepare_extrude(request).unwrap();
    let scene = kernel.recompute(&plan).unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let body = &scene.bodies[0];

    for (label, z) in [("opening", 10.0), ("floor", 5.0)] {
        let arcs = circular_edges_at(body, z);
        assert_eq!(
            arcs.len(),
            1,
            "{label} arc must exist exactly once at z={z}: {:?}",
            body.edges
                .iter()
                .map(|edge| (
                    edge.circle.is_some(),
                    edge.points.first().map(|point| point.z)
                ))
                .collect::<Vec<_>>()
        );
        assert!(
            arcs[0] >= 8,
            "the {label} arc must be tessellated for display, got {} points",
            arcs[0]
        );
    }
}
