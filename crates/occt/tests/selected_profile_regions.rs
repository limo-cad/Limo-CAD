//! Real sketch discovery, history replay, and native OCCT profile selection.
#![cfg(feature = "native-occt")]

use limo_cad_core::{BodyId, OriginPlane, PlaneRef};
use limo_cad_occt::{OcctKernel, PlacedBodyQueryDto};
use limo_cad_sketch::{
    RectangleMode, RectangleRequest, SegmentRequest, SetGridSnapRequest, SketchManager, Vec2,
};
use limo_cad_solid::{
    CommitKernelRequest, EditExtrudeRequest, EditRevolveRequest, ExtrudeExtent, ExtrudeOperation,
    ExtrudeRequest, KernelJobDto, Point2Dto, RecomputePlanDto, RevolveRequest,
};
use std::f64::consts::TAU;

fn fixture(kind: &str) -> SketchManager {
    let mut manager = SketchManager::new();
    manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    manager
        .set_grid_snap(SetGridSnapRequest { enabled: false })
        .unwrap();
    let rectangles = match kind {
        "nested" | "shared" => vec![(10., 0., 50., 40.)],
        "overlap" => vec![(10., 0., 40., 20.), (20., 0., 50., 20.)],
        "separate" => vec![(10., 0., 20., 10.), (30., 0., 40., 10.)],
        "corner" => vec![(10., 0., 20., 10.), (20., 10., 30., 20.)],
        _ => panic!("unknown fixture"),
    };
    for (x1, y1, x2, y2) in rectangles {
        manager
            .add_rectangle(RectangleRequest {
                mode: RectangleMode::TwoPoint,
                p1: Vec2::new(x1, y1),
                p2: Vec2::new(x2, y2),
                ctrl_held: true,
            })
            .unwrap();
    }
    if kind == "nested" {
        manager
            .add_rectangle(RectangleRequest {
                mode: RectangleMode::TwoPoint,
                p1: Vec2::new(20., 10.),
                p2: Vec2::new(40., 30.),
                ctrl_held: true,
            })
            .unwrap();
    } else if kind == "shared" {
        manager
            .add_line(SegmentRequest {
                from: Vec2::new(30., 0.),
                to_raw: Vec2::new(30., 40.),
                ctrl_held: true,
            })
            .unwrap();
    }
    manager.end_sketch().unwrap();
    manager
}

fn extrude(indices: &[u32], merge: bool) -> ExtrudeRequest {
    ExtrudeRequest {
        source_face: None,
        sketch_name: "Sketch1".into(),
        profile_indices: indices.to_vec(),
        merge_profiles: merge,
        operation: ExtrudeOperation::NewBody,
        extent: ExtrudeExtent::Distance { distance: 10. },
        taper_angle_deg: 0.,
        flip: false,
        target_body_ids: vec![],
    }
}

fn revolve(indices: &[u32], merge: bool) -> RevolveRequest {
    RevolveRequest {
        sketch_name: "Sketch1".into(),
        profile_indices: indices.to_vec(),
        merge_profiles: merge,
        axis_origin: Point2Dto::new(0., 0.),
        axis_direction: Point2Dto::new(0., 1.),
        axis_line_sketch_name: None,
        axis_line_entity_id: None,
        angle_deg: 360.,
        flip: false,
        operation: ExtrudeOperation::NewBody,
        target_body_ids: vec![],
    }
}

fn prepare(
    manager: &mut SketchManager,
    tool: &str,
    indices: &[u32],
    merge: bool,
) -> RecomputePlanDto {
    match tool {
        "extrude" => manager.prepare_extrude(extrude(indices, merge)).unwrap(),
        "revolve" => manager.prepare_revolve(revolve(indices, merge)).unwrap(),
        _ => unreachable!(),
    }
}

fn apply(manager: &mut SketchManager, kernel: &mut OcctKernel, plan: RecomputePlanDto) {
    assert!(plan.errors.is_empty(), "{:?}", plan.errors);
    let scene = kernel.recompute(&plan).unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene,
        })
        .unwrap();
}

fn volume(kernel: &OcctKernel, id: BodyId) -> f64 {
    let placed = || PlacedBodyQueryDto {
        body_id: id,
        translation: [0.; 3],
        rotation: [0., 0., 0., 1.],
    };
    kernel
        .exact_interference(placed(), placed())
        .unwrap()
        .overlap_volume_mm3
}

#[test]
fn inner_square_and_surrounding_ring_create_the_selected_material_in_both_tools() {
    for tool in ["extrude", "revolve"] {
        for (depth, area) in [(0, 1200.), (1, 400.)] {
            let mut manager = fixture("nested");
            let profile = manager.profile_catalog()[0]
                .profiles
                .iter()
                .find(|p| p.nesting_depth == depth)
                .unwrap()
                .index;
            let plan = prepare(&mut manager, tool, &[profile], false);
            let profiles = match &plan.jobs[0] {
                KernelJobDto::Extrude(j) => &j.profiles,
                KernelJobDto::Revolve(j) => &j.profiles,
                _ => unreachable!(),
            };
            assert_eq!(profiles[0].profile_index, profile);
            assert_eq!(profiles[0].holes.len(), if depth == 0 { 1 } else { 0 });
            let mut kernel = OcctKernel::new().unwrap();
            apply(&mut manager, &mut kernel, plan);
            let body = &manager.solid_scene().bodies[0];
            let expected = area * if tool == "extrude" { 10. } else { TAU * 30. };
            assert!(
                (volume(&kernel, body.id) - expected).abs() < expected * 1e-8,
                "{tool}, depth {depth}"
            );
        }
    }
}

#[test]
fn contacting_regions_merge_persist_and_can_be_edited_back_to_separate_bodies() {
    for tool in ["extrude", "revolve"] {
        for kind in ["nested", "shared", "overlap"] {
            let mut manager = fixture(kind);
            let indices: Vec<_> = manager.profile_catalog()[0]
                .profiles
                .iter()
                .map(|p| p.index)
                .collect();
            assert_eq!(indices.len(), if kind == "overlap" { 3 } else { 2 });
            let plan = prepare(&mut manager, tool, &indices, true);
            let mut kernel = OcctKernel::new().unwrap();
            apply(&mut manager, &mut kernel, plan);
            assert_eq!(manager.solid_scene().bodies.len(), 1, "{tool} {kind}");
            let body = &manager.solid_scene().bodies[0];
            let merged_id = body.id;
            let area = if kind == "overlap" { 800. } else { 1600. };
            let expected = area * if tool == "extrude" { 10. } else { TAU * 30. };
            assert!(
                (volume(&kernel, merged_id) - expected).abs() < expected * 1e-8,
                "{tool} {kind}"
            );
            if tool == "extrude" {
                assert_eq!(body.faces.len(), 6, "shared boundaries must disappear");
                assert_eq!(body.edges.len(), 12);
            }
            let saved = manager.export_project_model().unwrap();
            let mut loaded = SketchManager::new();
            let mut reopened_kernel = OcctKernel::new().unwrap();
            let plan = loaded.prepare_load_project(saved).unwrap();
            apply(&mut loaded, &mut reopened_kernel, plan);
            assert_eq!(loaded.solid_scene().bodies.len(), 1);
            assert_eq!(loaded.solid_scene().bodies[0].id, merged_id);
            for merge in [false, true] {
                let plan = if tool == "extrude" {
                    let definition = loaded.extrude_definitions()[0].clone();
                    assert_eq!(definition.operation, ExtrudeOperation::NewBody);
                    loaded
                        .prepare_edit_extrude(EditExtrudeRequest {
                            feature_id: definition.feature_id,
                            extrude: extrude(&indices, merge),
                        })
                        .unwrap()
                } else {
                    let definition = loaded.revolve_definitions()[0].clone();
                    assert_eq!(definition.operation, ExtrudeOperation::NewBody);
                    loaded
                        .prepare_edit_revolve(EditRevolveRequest {
                            feature_id: definition.feature_id,
                            revolve: revolve(&indices, merge),
                        })
                        .unwrap()
                };
                apply(&mut loaded, &mut reopened_kernel, plan);
                assert_eq!(
                    loaded.solid_scene().bodies.len(),
                    if merge { 1 } else { indices.len() }
                );
                assert_eq!(loaded.solid_scene().bodies[0].id, merged_id);
            }
        }
    }
}

#[test]
fn separate_and_corner_only_regions_cannot_be_disguised_as_one_body() {
    for tool in ["extrude", "revolve"] {
        for kind in ["separate", "corner"] {
            for merge in [false, true] {
                let mut manager = fixture(kind);
                let indices: Vec<_> = manager.profile_catalog()[0]
                    .profiles
                    .iter()
                    .map(|p| p.index)
                    .collect();
                assert_eq!(indices.len(), 2);
                let plan = prepare(&mut manager, tool, &indices, merge);
                let mut kernel = OcctKernel::new().unwrap();
                let scene = kernel.recompute(&plan).unwrap();
                if merge {
                    assert_eq!(scene.errors.len(), 1, "{tool} {kind}");
                    assert!(
                        scene.errors[0].message.contains("contacting regions"),
                        "{:?}",
                        scene.errors
                    );
                    assert!(
                        scene.bodies.is_empty(),
                        "failed merges cannot publish a compound as a body"
                    );
                } else {
                    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
                    assert_eq!(scene.bodies.len(), 2);
                }
            }
        }
    }
}
