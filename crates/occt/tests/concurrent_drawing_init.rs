#![cfg(feature = "native-occt")]

//! Keep this as a separate integration-test executable with one test: no
//! earlier kernel or projection may warm OCCT's process-global drawing plane.
use limo_cad_core::{BodyId, FeatureId};
use limo_cad_occt::{DrawingProjectionRequest, OcctKernel};
use limo_cad_solid::{
    ExtrudeOperation, KernelExtrudeJobDto, KernelJobDto, KernelProfileDto, Point3Dto,
    RecomputePlanDto,
};
use std::sync::{Arc, Barrier};

fn plan() -> RecomputePlanDto {
    RecomputePlanDto {
        transaction_id: 1,
        errors: vec![],
        jobs: vec![KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(1),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![KernelProfileDto {
                profile_index: 0,
                points: [[-20., -15.], [20., -15.], [20., 15.], [-20., 15.]]
                    .into_iter()
                    .map(|[x, y]| Point3Dto { x, y, z: 0. })
                    .collect(),
                curves: vec![],
                holes: vec![],
            }],
            normal: Point3Dto {
                x: 0.,
                y: 0.,
                z: 1.,
            },
            start_offset: 0.,
            end_offset: 20.,
            taper_angle_deg: 0.,
            target_body_ids: vec![],
            result_body_ids: vec![BodyId(1)],
        })],
    }
}

#[test]
fn concurrent_first_kernels_project_independent_solids_exactly() {
    const WORKERS: usize = 12;
    let start = Arc::new(Barrier::new(WORKERS));
    let project = Arc::new(Barrier::new(WORKERS));
    let workers: Vec<_> = (0..WORKERS)
        .map(|_| {
            let start = Arc::clone(&start);
            let project = Arc::clone(&project);
            std::thread::spawn(move || {
                start.wait();
                let prepared = (|| {
                    let mut kernel = OcctKernel::new()?;
                    let scene = kernel.recompute(&plan())?;
                    Ok::<_, limo_cad_occt::OcctError>((kernel, scene))
                })();
                let request = DrawingProjectionRequest {
                    scope: Default::default(),
                    occurrence_ids: vec![],
                    resolved_occurrences: None,
                    body_ids: vec![BodyId(1)],
                    direction: [0., 0., 1.],
                    up: [0., 1., 0.],
                    include_hidden: true,
                    include_tangent_edges: false,
                    deflection: 0.05,
                    section_plane: None,
                };
                project.wait();
                let (kernel, scene) = prepared.unwrap();
                assert!(scene.errors.is_empty(), "{:?}", scene.errors);
                assert_eq!(scene.bodies.len(), 1);
                assert_eq!(scene.bodies[0].edges.len(), 12);
                let projection = kernel.drawing_projection(&request).unwrap();
                assert!(projection.visible.len() >= 4);
                for (actual, expected) in projection.bounds.into_iter().zip([-20., -15., 20., 15.])
                {
                    assert!((actual - expected).abs() < 1e-7);
                }
                let exact = serde_json::to_value(projection).unwrap();
                assert_eq!(
                    serde_json::to_value(kernel.drawing_projection(&request).unwrap()).unwrap(),
                    exact,
                    "Each kernel must retain its own exact projection cache"
                );
                exact
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    for actual in &results[1..] {
        assert_eq!(
            actual, &results[0],
            "Independent concurrent HLR output differs"
        );
    }
}
