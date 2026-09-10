use nbcad_core::{BodyId, FeatureStatus, PlaneBasis};
use nbcad_sketch::{
    BeginSketchRequest, FaceSketchOrigin, OriginPlane, PlaneRef, RectangleMode, RectangleRequest,
    SketchManager, Vec2,
};
use nbcad_solid::{
    BodyFeatureRequestDto, CommitKernelRequest, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest,
    KernelBodyDto, KernelFaceDto, KernelJobDto, KernelSceneDto, Point3Dto, SetRollbackRequest,
    SplitBodyRequest,
};

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};

fn rectangle(manager: &mut SketchManager, p1: Vec2, p2: Vec2) {
    manager
        .add_rectangle(RectangleRequest {
            mode: RectangleMode::TwoPoint,
            p1,
            p2,
            ctrl_held: false,
        })
        .unwrap();
}

fn extrusion(sketch_name: &str) -> ExtrudeRequest {
    ExtrudeRequest {
        source_face: None,
        sketch_name: sketch_name.to_string(),
        profile_indices: vec![0],
        operation: ExtrudeOperation::NewBody,
        extent: ExtrudeExtent::Distance { distance: 10.0 },
        taper_angle_deg: 0.0,
        flip: false,
        target_body_ids: Vec::new(),
    }
}

fn planar_body(body_id: BodyId, key: &str, z: f64) -> KernelBodyDto {
    KernelBodyDto {
        body_id,
        positions: vec![0.0, 0.0, z as f32, 20.0, 0.0, z as f32, 0.0, 20.0, z as f32],
        normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        indices: vec![0, 1, 2],
        faces: vec![KernelFaceDto {
            key: key.to_string(),
            first_index: 0,
            index_count: 3,
            plane: Some(PlaneBasis {
                origin: [0.0, 0.0, z],
                u: [1.0, 0.0, 0.0],
                v: [0.0, 1.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            }),
            signature: Some(nbcad_solid::PlanarFaceSignatureDto {
                centroid: Point3Dto {
                    x: 20.0 / 3.0,
                    y: 20.0 / 3.0,
                    z,
                },
                normal: Point3Dto {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
                area: 200.0,
                perimeter: 40.0 + 800.0_f64.sqrt(),
                wire_count: 1,
                edge_count: 3,
            }),
            cylinder: None,
        }],
        edges: Vec::new(),
    }
}

fn result_body_id(job: &KernelJobDto) -> BodyId {
    match job {
        KernelJobDto::Extrude(job) => job.result_body_ids[0],
        KernelJobDto::Revolve(job) => job.result_body_ids[0],
        KernelJobDto::Sweep(job) => job.result_body_ids[0],
        KernelJobDto::Loft(job) => job.result_body_ids[0],
        KernelJobDto::Rib(job) => job.result_body_ids[0],
        KernelJobDto::Fillet(job) => job.target_body_id,
        KernelJobDto::Chamfer(job) => job.target_body_id,
        KernelJobDto::Hole(job) => job.target_body_id,
        KernelJobDto::ExternalThread(job) => job.target_body_id,
        KernelJobDto::Shell(job) => job.target_body_id,
        KernelJobDto::Transform(job) => job.result_body_ids[0],
        KernelJobDto::Combine(job) => job.target_body_id,
        KernelJobDto::SplitBody(job) => job.new_body_id,
        KernelJobDto::ImportStep(job) => job.result_body_id,
    }
}

#[test]
fn rectangular_extrude_face_sketch_and_broken_reference_recompute_are_integrated() {
    let mut manager = SketchManager::new();

    manager.begin_sketch(XY).unwrap();
    rectangle(&mut manager, Vec2::new(0.0, 0.0), Vec2::new(20.0, 15.0));
    manager.end_sketch().unwrap();
    let catalog = manager.profile_catalog();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].profiles.len(), 1);

    let first_plan = manager.prepare_extrude(extrusion("Sketch1")).unwrap();
    let first_body = result_body_id(&first_plan.jobs[0]);
    let first_update = manager
        .commit_solid(CommitKernelRequest {
            transaction_id: first_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![planar_body(first_body, "support", 10.0)],
                errors: Vec::new(),
            },
        })
        .unwrap();
    assert_eq!(first_update.scene.bodies.len(), 1);
    assert_eq!(first_update.document.features.len(), 2);
    assert!(first_update
        .document
        .browser
        .iter()
        .flat_map(|node| &node.children)
        .any(|node| node.reference_id == Some(first_body.0)));

    let support_face = first_update.scene.bodies[0].faces[0].id;
    let face_sketch = manager
        .begin_sketch(PlaneRef::PlanarFace {
            face_id: support_face,
        })
        .unwrap();
    assert_eq!(face_sketch.basis.origin, [0.0, 0.0, 10.0]);
    rectangle(&mut manager, Vec2::new(2.0, 2.0), Vec2::new(8.0, 7.0));
    manager.end_sketch().unwrap();

    let second_plan = manager.prepare_extrude(extrusion("Sketch2")).unwrap();
    assert_eq!(second_plan.jobs.len(), 2, "recompute replays full history");
    let second_body = result_body_id(&second_plan.jobs[1]);
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: second_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![
                    planar_body(first_body, "support", 10.0),
                    planar_body(second_body, "top", 20.0),
                ],
                errors: Vec::new(),
            },
        })
        .unwrap();

    // The planner validates against the last good scene. The simulated
    // kernel result then changes the support topology key, exercising the
    // post-recompute broken-reference overlay on both Sketch2 and Extrude2.
    let broken_plan = manager.prepare_recompute().unwrap();
    let broken = manager
        .commit_solid(CommitKernelRequest {
            transaction_id: broken_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![
                    planar_body(first_body, "replacement", 10.0),
                    planar_body(second_body, "top", 20.0),
                ],
                errors: Vec::new(),
            },
        })
        .unwrap();

    for name in ["Sketch2", "Extrude2"] {
        let feature = broken
            .document
            .features
            .iter()
            .find(|feature| feature.name == name)
            .unwrap();
        match &feature.status {
            FeatureStatus::Error { message } => {
                assert!(message.contains("Broken reference"));
            }
            other => panic!("{name} should carry a timeline error, got {other:?}"),
        }
    }
}

#[test]
fn planar_face_sketch_origin_can_use_face_center_or_global_projection() {
    let mut manager = SketchManager::new();
    manager.begin_sketch(XY).unwrap();
    rectangle(&mut manager, Vec2::new(0.0, 0.0), Vec2::new(20.0, 15.0));
    manager.end_sketch().unwrap();
    let plan = manager.prepare_extrude(extrusion("Sketch1")).unwrap();
    let body_id = result_body_id(&plan.jobs[0]);
    let update = manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![planar_body(body_id, "support", 10.0)],
                errors: Vec::new(),
            },
        })
        .unwrap();
    let face_id = update.scene.bodies[0].faces[0].id;

    let centered = manager
        .begin_sketch_with_options(BeginSketchRequest {
            plane: PlaneRef::PlanarFace { face_id },
            name: None,
            face_origin: FaceSketchOrigin::FaceCenter,
        })
        .unwrap();
    for (actual, expected) in centered
        .basis
        .origin
        .into_iter()
        .zip([20.0 / 3.0, 20.0 / 3.0, 10.0])
    {
        assert!((actual - expected).abs() < 1e-12);
    }
    manager.end_sketch().unwrap();

    let projected = manager
        .begin_sketch_with_options(BeginSketchRequest {
            plane: PlaneRef::PlanarFace { face_id },
            name: None,
            face_origin: FaceSketchOrigin::GlobalOriginProjection,
        })
        .unwrap();
    assert_eq!(projected.basis.origin, [0.0, 0.0, 10.0]);
}

#[test]
fn split_body_is_inserted_at_the_build_cursor() {
    let mut manager = SketchManager::new();
    manager.begin_sketch(XY).unwrap();
    rectangle(&mut manager, Vec2::new(-10.0, -10.0), Vec2::new(10.0, 10.0));
    manager.end_sketch().unwrap();

    let extrude_plan = manager.prepare_extrude(extrusion("Sketch1")).unwrap();
    let target_body = result_body_id(&extrude_plan.jobs[0]);
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: extrude_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![planar_body(target_body, "target", 10.0)],
                errors: Vec::new(),
            },
        })
        .unwrap();

    // This later sketch represents the External Thread/Hole features that
    // were present after the user's marker in the reported project.
    manager.begin_sketch(XY).unwrap();
    manager.end_sketch().unwrap();
    let rollback_plan = manager
        .prepare_set_rollback(SetRollbackRequest { rollback_index: 2 })
        .unwrap();
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: rollback_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![planar_body(target_body, "target", 10.0)],
                errors: Vec::new(),
            },
        })
        .unwrap();

    let split_plan = manager
        .prepare_body_feature(BodyFeatureRequestDto::SplitBody(SplitBodyRequest {
            body_id: target_body,
            plane: PlaneRef::OriginPlane {
                plane: OriginPlane::Yz,
            },
            plane_basis: None,
        }))
        .unwrap();
    assert_eq!(
        split_plan.jobs.len(),
        2,
        "future history must remain rolled back"
    );
    let split_body = result_body_id(split_plan.jobs.last().unwrap());
    let update = manager
        .commit_solid(CommitKernelRequest {
            transaction_id: split_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![
                    planar_body(target_body, "positive", 10.0),
                    planar_body(split_body, "negative", 10.0),
                ],
                errors: Vec::new(),
            },
        })
        .unwrap();

    assert_eq!(
        update
            .document
            .features
            .iter()
            .map(|feature| feature.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Sketch1", "Extrude1", "SplitBody1", "Sketch2"]
    );
    assert_eq!(update.document.rollback_index, 3);
}

#[test]
fn new_sketch_is_inserted_at_the_build_cursor() {
    let mut manager = SketchManager::new();
    manager.begin_sketch(XY).unwrap();
    rectangle(&mut manager, Vec2::new(-10.0, -10.0), Vec2::new(10.0, 10.0));
    manager.end_sketch().unwrap();

    let extrude_plan = manager.prepare_extrude(extrusion("Sketch1")).unwrap();
    let target_body = result_body_id(&extrude_plan.jobs[0]);
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: extrude_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![planar_body(target_body, "target", 10.0)],
                errors: Vec::new(),
            },
        })
        .unwrap();

    manager.begin_sketch(XY).unwrap();
    manager.end_sketch().unwrap();
    let rollback_plan = manager
        .prepare_set_rollback(SetRollbackRequest { rollback_index: 2 })
        .unwrap();
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: rollback_plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![planar_body(target_body, "target", 10.0)],
                errors: Vec::new(),
            },
        })
        .unwrap();

    manager.begin_sketch(XY).unwrap();
    manager.end_sketch().unwrap();

    let names = manager
        .document_dto()
        .features
        .iter()
        .map(|feature| feature.name.clone())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["Sketch1", "Extrude1", "Sketch3", "Sketch2"]);
    assert_eq!(
        manager
            .finished_sketches()
            .iter()
            .map(|sketch| sketch.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Sketch1", "Sketch3", "Sketch2"],
        "finished list must follow feature-tree order, not append order"
    );
    assert_eq!(
        manager.document_dto().rollback_index,
        3,
        "later history must stay rolled back after a mid-timeline sketch"
    );
}
