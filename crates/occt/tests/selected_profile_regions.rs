//! Real sketch discovery, history replay, and native OCCT profile selection.
#![cfg(feature = "native-occt")]

use limo_cad_core::{BodyId, OriginPlane, PlaneRef};
use limo_cad_occt::{OcctKernel, PlacedBodyQueryDto};
use limo_cad_sketch::{
    RectangleMode, RectangleRequest, SegmentRequest, SetGridSnapRequest, SketchManager, Vec2,
};
use limo_cad_solid::{
    CommitKernelRequest, DatumPlaneRequest, DatumPlaneSourceDto, EditExtrudeRequest,
    EditRevolveRequest, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest, KernelJobDto, Point2Dto,
    RecomputePlanDto, RevolveRequest,
};
use serde_json::{json, Value};
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
            let saved = manager.export_project_model().unwrap();
            let mut reopened = SketchManager::new();
            let plan = reopened.prepare_load_project(saved).unwrap();
            let mut reopened_kernel = OcctKernel::new().unwrap();
            apply(&mut reopened, &mut reopened_kernel, plan);
            assert!((volume(&reopened_kernel, body.id) - expected).abs() < expected * 1e-8);
        }
    }
}

#[test]
fn pre_region_selection_fixture_reopens_as_the_original_ring() {
    let mut saved =
        include_str!("../../sketch/tests/fixtures/legacy_nested_profile_v14.json").to_string();
    for _ in 0..2 {
        let mut manager = SketchManager::new();
        let mut kernel = OcctKernel::new().unwrap();
        let plan = manager.prepare_load_project(saved).unwrap();
        apply(&mut manager, &mut kernel, plan);
        assert_eq!(manager.solid_scene().bodies.len(), 1);
        assert_eq!(manager.solid_scene().bodies[0].id, BodyId(1));
        assert!((volume(&kernel, BodyId(1)) - 12_000.).abs() < 1e-6);
        assert_eq!(manager.extrude_definitions()[0].profile_indices, vec![0]);
        saved = manager.export_project_model().unwrap();
        let model: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            model["schema_version"],
            limo_cad_sketch::PROJECT_SCHEMA_VERSION
        );
    }
}

fn other_consumer(manager: &mut SketchManager, tool: &str, profile: u32) -> RecomputePlanDto {
    let xy = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    };
    if tool == "sweep" {
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xz,
            })
            .unwrap();
        let line = manager
            .add_line(SegmentRequest {
                from: Vec2::new(0., 0.),
                to_raw: Vec2::new(0., 10.),
                ctrl_held: true,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        manager
            .prepare_sweep(
                serde_json::from_value(json!({
                    "profile":{"sketch_name":"Sketch1","profile_index":profile},
                    "path_sketch_name":"Sketch2","path_entity_ids":[line.entity_id]
                }))
                .unwrap(),
            )
            .unwrap()
    } else {
        let plane = manager
            .create_datum_plane(DatumPlaneRequest {
                name: Some("Loft section".into()),
                source: DatumPlaneSourceDto::Offset {
                    reference: xy,
                    distance: 10.,
                },
            })
            .unwrap();
        manager
            .begin_sketch(PlaneRef::DatumPlane {
                datum_id: plane.planes.last().unwrap().datum_id,
            })
            .unwrap();
        for (x1, y1, x2, y2) in [(10., 0., 50., 40.), (20., 10., 40., 30.)] {
            manager
                .add_rectangle(RectangleRequest {
                    mode: RectangleMode::TwoPoint,
                    p1: Vec2::new(x1, y1),
                    p2: Vec2::new(x2, y2),
                    ctrl_held: true,
                })
                .unwrap();
        }
        manager.end_sketch().unwrap();
        manager
            .prepare_loft(
                serde_json::from_value(json!({
                    "sections":[
                        {"sketch_name":"Sketch1","profile_index":profile},
                        {"sketch_name":"Sketch2","profile_index":profile}
                    ], "ruled":true
                }))
                .unwrap(),
            )
            .unwrap()
    }
}

#[test]
fn every_legacy_profile_consumer_keeps_geometry_and_body_ids_after_migration() {
    for tool in ["extrude", "revolve", "sweep", "loft"] {
        let mut manager = fixture("nested");
        let regions = &manager.profile_catalog()[0].profiles;
        let outer = regions.iter().find(|p| p.nesting_depth == 0).unwrap().index;
        let inner = regions.iter().find(|p| p.nesting_depth == 1).unwrap().index;
        let plan = if matches!(tool, "extrude" | "revolve") {
            prepare(&mut manager, tool, &[outer], false)
        } else {
            other_consumer(&mut manager, tool, outer)
        };
        let mut kernel = OcctKernel::new().unwrap();
        apply(&mut manager, &mut kernel, plan);
        let body = manager.solid_scene().bodies[0].id;
        let expected = volume(&kernel, body);
        let mut model: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        model["schema_version"] = json!(14);
        let key = format!("{tool}s");
        let definition = &mut model[&key][0];
        if matches!(tool, "extrude" | "revolve") {
            definition["profile_indices"] = json!([inner]);
            definition.as_object_mut().unwrap().remove("merge_profiles");
        } else if tool == "sweep" {
            definition["profile"]["profile_index"] = json!(inner);
        } else {
            for section in definition["sections"].as_array_mut().unwrap() {
                section["profile_index"] = json!(inner);
            }
        }
        let mut saved = model.to_string();
        for _ in 0..2 {
            let mut reopened = SketchManager::new();
            let mut reopened_kernel = OcctKernel::new().unwrap();
            let plan = reopened.prepare_load_project(saved).unwrap();
            apply(&mut reopened, &mut reopened_kernel, plan);
            assert_eq!(reopened.solid_scene().bodies.len(), 1, "{tool}");
            assert_eq!(reopened.solid_scene().bodies[0].id, body, "{tool}");
            assert!(
                (volume(&reopened_kernel, body) - expected).abs() < expected * 1e-8,
                "{tool}"
            );
            saved = reopened.export_project_model().unwrap();
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

#[test]
fn legacy_targetless_join_preserves_disjoint_bodies_and_downstream_targets_on_reopen() {
    for tool in ["extrude", "revolve"] {
        let mut manager = fixture("separate");
        let indices: Vec<_> = manager.profile_catalog()[0]
            .profiles
            .iter()
            .map(|profile| profile.index)
            .collect();
        let plan = if tool == "extrude" {
            let mut request = extrude(&indices, false);
            request.operation = ExtrudeOperation::Join;
            manager.prepare_extrude(request).unwrap()
        } else {
            let mut request = revolve(&indices, false);
            request.operation = ExtrudeOperation::Join;
            manager.prepare_revolve(request).unwrap()
        };
        let mut kernel = OcctKernel::new().unwrap();
        apply(&mut manager, &mut kernel, plan);
        assert_eq!(manager.solid_scene().bodies.len(), 1, "{tool}");
        let body = manager.solid_scene().bodies[0].id;
        let joined_volume = volume(&kernel, body);
        let expected_joined = if tool == "extrude" {
            2000.
        } else {
            // Two annular cylinders, radii 10..20 and 30..40, height 10.
            std::f64::consts::PI * 10_000.
        };
        assert!(
            (joined_volume - expected_joined).abs() < expected_joined * 1e-8,
            "{tool}"
        );

        // The single historic BodyId must remain usable by later features,
        // even though the old targetless Join contains disjoint solids.
        let mut cut = extrude(&indices[..1], false);
        cut.operation = ExtrudeOperation::Cut;
        cut.target_body_ids = vec![body];
        cut.extent = ExtrudeExtent::Distance { distance: 5. };
        let plan = manager.prepare_extrude(cut).unwrap();
        apply(&mut manager, &mut kernel, plan);
        let expected = volume(&kernel, body);
        assert!(expected > 0. && expected < joined_volume, "{tool}");
        let current: Value =
            serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        for version in [14, limo_cad_sketch::PROJECT_SCHEMA_VERSION] {
            let mut model = current.clone();
            model["schema_version"] = json!(version);
            if version == 14 {
                for key in ["extrudes", "revolves"] {
                    for definition in model[key].as_array_mut().unwrap() {
                        definition.as_object_mut().unwrap().remove("merge_profiles");
                    }
                }
            }
            let mut saved = model.to_string();
            for _ in 0..2 {
                let mut reopened = SketchManager::new();
                let mut reopened_kernel = OcctKernel::new().unwrap();
                let plan = reopened.prepare_load_project(saved).unwrap();
                apply(&mut reopened, &mut reopened_kernel, plan);
                assert_eq!(reopened.solid_scene().bodies.len(), 1, "{tool}, v{version}");
                assert_eq!(reopened.solid_scene().bodies[0].id, body);
                assert!(
                    (volume(&reopened_kernel, body) - expected).abs() < expected * 1e-8,
                    "{tool}, v{version}"
                );
                let cut = reopened.extrude_definitions().last().unwrap().clone();
                assert_eq!(cut.operation, ExtrudeOperation::Cut);
                assert_eq!(cut.target_body_ids, vec![body]);
                saved = reopened.export_project_model().unwrap();
            }
        }
    }
}
