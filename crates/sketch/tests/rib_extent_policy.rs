//! Shared request policy and transaction atomicity, independent of OCCT mesh
//! correctness or live UI/MCP qualification.

use limo_cad_core::BodyId;
use limo_cad_sketch::{OriginPlane, PlaneRef, SegmentRequest, SketchManager, Vec2};
use limo_cad_solid::{
    CommitKernelRequest, EditRibRequest, ExtrudeOperation, KernelBodyDto, KernelJobDto,
    KernelSceneDto, RecomputePlanDto, RibExtent, RibRequest,
};
use std::collections::BTreeSet;

fn request(lines: &[u64], operation: ExtrudeOperation, extent: Option<RibExtent>) -> RibRequest {
    RibRequest {
        sketch_name: "Sketch1".into(),
        line_entity_ids: lines.to_vec(),
        thickness: 2.,
        depth: 5.,
        symmetric: false,
        flip: false,
        operation,
        target_body_ids: if operation == ExtrudeOperation::NewBody {
            vec![]
        } else {
            vec![BodyId(1)]
        },
        extent,
    }
}

fn commit_planning_fixture(manager: &mut SketchManager, plan: RecomputePlanDto) {
    let body_ids = plan
        .jobs
        .iter()
        .flat_map(|job| match job {
            KernelJobDto::Rib(rib) => rib.result_body_ids.iter().copied(),
            _ => panic!("This request fixture contains only Rib jobs"),
        })
        .collect::<BTreeSet<_>>();
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto {
                // A deterministic kernel acceptance stub is enough to retain
                // target body identity. Native geometry has separate tests.
                bodies: body_ids
                    .into_iter()
                    .map(|body_id| KernelBodyDto {
                        body_id,
                        topology_signature: String::new(),
                        display_warnings: vec![],
                        positions: vec![0., 0., 0., 20., 0., 5., 0., 10., 5.],
                        normals: vec![0., 0., 1., 0., 0., 1., 0., 0., 1.],
                        indices: vec![0, 1, 2],
                        faces: vec![],
                        edges: vec![],
                    })
                    .collect(),
                errors: vec![],
            },
        })
        .unwrap();
}

fn fixture() -> (SketchManager, [u64; 2]) {
    let mut manager = SketchManager::new();
    manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    let lines = [0., 10.].map(|y| {
        manager
            .add_line(SegmentRequest {
                from: Vec2::new(0., y),
                to_raw: Vec2::new(20., y),
                ctrl_held: true,
            })
            .unwrap()
            .entity_id
            .0
    });
    manager.end_sketch().unwrap();
    let plan = manager
        .prepare_rib(request(&lines[..1], ExtrudeOperation::NewBody, None))
        .unwrap();
    commit_planning_fixture(&mut manager, plan);
    (manager, lines)
}

#[test]
fn additive_through_all_add_rejects_atomically_and_finite_recovery_keeps_ids() {
    for operation in [ExtrudeOperation::NewBody, ExtrudeOperation::Join] {
        let (mut manager, lines) = fixture();
        let (mut control, control_lines) = fixture();
        let before_model = manager.export_project_model().unwrap();
        let before_document = manager.document_dto();
        assert_eq!(before_model, control.export_project_model().unwrap());

        for _ in 0..2 {
            let error = manager
                .prepare_rib(request(&lines, operation, Some(RibExtent::ThroughAll)))
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Through All requires Subtract or Common"),
                "{error}"
            );
            assert_eq!(manager.export_project_model().unwrap(), before_model);
            assert_eq!(manager.document_dto(), before_document);
        }

        let recovered = manager
            .prepare_rib(request(
                &lines,
                operation,
                Some(RibExtent::Distance { depth: 7. }),
            ))
            .unwrap();
        let expected = control
            .prepare_rib(request(
                &control_lines,
                operation,
                Some(RibExtent::Distance { depth: 7. }),
            ))
            .unwrap();
        // Includes transaction/feature/body IDs and the complete history plan.
        assert_eq!(recovered, expected);
        commit_planning_fixture(&mut manager, recovered);
        commit_planning_fixture(&mut control, expected);
        assert_eq!(
            manager.export_project_model().unwrap(),
            control.export_project_model().unwrap()
        );
    }
}

#[test]
fn additive_through_all_edit_rejects_without_allocating_more_body_ids() {
    for operation in [ExtrudeOperation::NewBody, ExtrudeOperation::Join] {
        let (mut manager, lines) = fixture();
        let (mut control, control_lines) = fixture();
        for (target, source_lines) in [(&mut manager, lines), (&mut control, control_lines)] {
            let plan = target
                .prepare_rib(request(&source_lines[..1], ExtrudeOperation::NewBody, None))
                .unwrap();
            commit_planning_fixture(target, plan);
        }
        let feature_id = manager.rib_definitions()[1].feature_id;
        assert_eq!(feature_id, control.rib_definitions()[1].feature_id);
        let before_model = manager.export_project_model().unwrap();
        let before_document = manager.document_dto();

        for _ in 0..2 {
            // Two selected lines would grow the one-line definition's body-ID
            // reservation if validation happened after allocation.
            let error = manager
                .prepare_edit_rib(EditRibRequest {
                    feature_id,
                    rib: request(&lines, operation, Some(RibExtent::ThroughAll)),
                })
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Through All requires Subtract or Common"),
                "{error}"
            );
            assert_eq!(manager.export_project_model().unwrap(), before_model);
            assert_eq!(manager.document_dto(), before_document);
        }

        let edit = |lines: &[u64]| EditRibRequest {
            feature_id,
            rib: request(lines, operation, Some(RibExtent::Distance { depth: 7. })),
        };
        let recovered = manager.prepare_edit_rib(edit(&lines)).unwrap();
        let expected = control.prepare_edit_rib(edit(&control_lines)).unwrap();
        assert_eq!(recovered, expected);
        commit_planning_fixture(&mut manager, recovered);
        commit_planning_fixture(&mut control, expected);
        assert_eq!(
            manager.export_project_model().unwrap(),
            control.export_project_model().unwrap()
        );

        // Observe the next allocation too, after the successful edit commits.
        let next = |lines: &[u64]| request(&lines[..1], ExtrudeOperation::NewBody, None);
        assert_eq!(
            manager.prepare_rib(next(&lines)).unwrap(),
            control.prepare_rib(next(&control_lines)).unwrap()
        );
    }
}

#[test]
fn subtract_and_common_through_all_remain_supported_shared_requests() {
    for operation in [ExtrudeOperation::Cut, ExtrudeOperation::Intersect] {
        let (mut manager, lines) = fixture();
        let plan = manager
            .prepare_rib(request(&lines[..1], operation, Some(RibExtent::ThroughAll)))
            .unwrap();
        let KernelJobDto::Rib(rib) = plan.jobs.last().unwrap() else {
            panic!("Expected the supported boolean Rib request");
        };
        assert_eq!(rib.operation, operation);
        assert!(rib.start_offset < 0. && rib.end_offset > 0.);
        assert!(rib.start_offset.is_finite() && rib.end_offset.is_finite());
        commit_planning_fixture(&mut manager, plan);
        assert_eq!(manager.rib_definitions().len(), 2);
    }
}
