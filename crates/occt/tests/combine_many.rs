#![cfg(feature = "native-occt")]

use nbcad_core::{BodyId, FeatureId};
use nbcad_occt::OcctKernel;
use nbcad_solid::{
    CombineOperation, ExtrudeOperation, KernelCombineJobDto, KernelExtrudeJobDto, KernelJobDto,
    KernelProfileDto, Point3Dto, RecomputePlanDto,
};

#[test]
fn joining_many_adjacent_tools_preserves_exact_solid_and_keep_tools() {
    let block = |id: u64, x: f64| {
        KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(id),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![KernelProfileDto {
                profile_index: 0,
                points: [[x, 0.], [x + 2., 0.], [x + 2., 10.], [x, 10.]]
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
            end_offset: 5.,
            taper_angle_deg: 0.,
            target_body_ids: vec![],
            result_body_ids: vec![BodyId(id)],
        })
    };
    for keep_tools in [false, true] {
        let mut jobs = (1..=24)
            .map(|id| block(id, (id - 1) as f64))
            .collect::<Vec<_>>();
        jobs.push(KernelJobDto::Combine(KernelCombineJobDto {
            feature_id: FeatureId(25),
            target_body_id: BodyId(1),
            tool_body_ids: (2..=24).map(BodyId).collect(),
            operation: CombineOperation::Join,
            keep_tools,
        }));
        let mut kernel = OcctKernel::new().unwrap();
        let scene = kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 1,
                jobs,
                errors: vec![],
            })
            .unwrap();
        assert!(scene.errors.is_empty(), "{:?}", scene.errors);
        assert_eq!(scene.bodies.len(), if keep_tools { 24 } else { 1 });
        let body = scene
            .bodies
            .iter()
            .find(|b| b.body_id == BodyId(1))
            .unwrap();
        assert_eq!(body.faces.len(), 6, "coplanar tool faces must be unified");
        assert_eq!(body.edges.len(), 12);
        let min_x = body
            .positions
            .chunks_exact(3)
            .map(|p| p[0])
            .fold(f32::INFINITY, f32::min);
        let max_x = body
            .positions
            .chunks_exact(3)
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!((min_x, max_x), (0., 25.));
        let request = nbcad_export::MeshExportRequest {
            body_ids: vec![BodyId(1)],
            ..Default::default()
        };
        let archive = kernel.export_3mf(&request, &[]).unwrap();
        assert!(
            archive.starts_with(b"PK"),
            "native export validates a closed oriented manifold"
        );
    }
}
