#![cfg(feature = "native-occt")]
use nbcad_core::{BodyId, FeatureId};
use nbcad_occt::{OcctKernel, PlacedBodyQueryDto};
use nbcad_solid::{
    ExtrudeOperation, KernelExtrudeJobDto, KernelJobDto, KernelProfileDto, Point3Dto,
    RecomputePlanDto,
};

#[test]
fn exact_clearance_keeps_touching_partial_and_contained_solid_semantics() {
    let cube = |id, size| {
        KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(id),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![KernelProfileDto {
                profile_index: 0,
                points: [[0., 0.], [size, 0.], [size, size], [0., size]]
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
            end_offset: size,
            taper_angle_deg: 0.,
            target_body_ids: vec![],
            result_body_ids: vec![BodyId(id)],
        })
    };
    let mut kernel = OcctKernel::new().unwrap();
    let scene = kernel
        .recompute(&RecomputePlanDto {
            transaction_id: 1,
            jobs: vec![cube(1, 20.), cube(2, 2.)],
            errors: vec![],
        })
        .unwrap();
    assert!(scene.errors.is_empty());
    let pose = |body, translation| PlacedBodyQueryDto {
        body_id: BodyId(body),
        translation,
        rotation: [0., 0., 0., 1.],
    };
    for (translation, clearance, volume) in [
        ([25., 0., 0.], 5., 0.),
        ([20., 0., 0.], 0., 0.),
        ([19., 0., 0.], 0., 4.),
        ([5., 5., 5.], 0., 8.),
    ] {
        let result = kernel
            .exact_interference(pose(1, [0., 0., 0.]), pose(2, translation))
            .unwrap();
        assert!(
            (result.minimum_clearance_mm - clearance).abs() < 1e-8,
            "{result:?}"
        );
        assert!(
            (result.overlap_volume_mm3 - volume).abs() < 1e-8,
            "{result:?}"
        );
    }
}
