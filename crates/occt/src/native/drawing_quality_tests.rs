//! Drawing curves stay analytic in OCCT; only their retained HLR samples vary.
use super::*;
use limo_cad_core::{BodyId, FeatureId};
use limo_cad_solid::{
    ExtrudeOperation, KernelCurveDto, KernelExtrudeJobDto, KernelJobDto, KernelProfileDto,
    Point3Dto, RecomputePlanDto,
};
use serde_json::json;

#[test]
fn drawing_curve_quality_bounds_chord_error_point_growth_and_reuses_cached_projection() {
    let p = |x, y| Point3Dto { x, y, z: 0. };
    let plan = RecomputePlanDto {
        transaction_id: 1,
        errors: vec![],
        jobs: vec![KernelJobDto::Extrude(KernelExtrudeJobDto {
            feature_id: FeatureId(1),
            operation: ExtrudeOperation::NewBody,
            source_face: None,
            profiles: vec![KernelProfileDto {
                profile_index: 0,
                points: vec![p(3., 0.), p(0., 3.), p(-3., 0.), p(0., -3.)],
                curves: vec![KernelCurveDto::Circle {
                    entity_id: 1,
                    center: p(0., 0.),
                    axis_point: p(3., 0.),
                    normal: Point3Dto {
                        x: 0.,
                        y: 0.,
                        z: 1.,
                    },
                }],
                holes: vec![],
            }],
            normal: Point3Dto {
                x: 0.,
                y: 0.,
                z: 1.,
            },
            start_offset: 0.,
            end_offset: 10.,
            taper_angle_deg: 0.,
            target_body_ids: vec![],
            result_body_ids: vec![BodyId(1)],
        })],
    };
    let mut kernel = OcctKernel::new().unwrap();
    let initial = kernel.recompute(&plan).unwrap();
    assert!(initial.errors.is_empty());
    let view = serde_json::from_value(json!({
        "id":1,"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],
        "position":[90.,70.],"scale":1.,"show_hidden_lines":true,"body_ids":[1]
    }))
    .unwrap();
    let request = crate::drawing_export::projection_request(
        &view,
        std::slice::from_ref(&view),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let mut previous_quality = request.clone();
    previous_quality.deflection = 0.08;
    let coarse = kernel.drawing_projection(&previous_quality).unwrap();
    let fine = kernel.drawing_projection(&request).unwrap();
    let points = |p: &DrawingProjectionDto| {
        p.visible
            .iter()
            .chain(&p.hidden)
            .chain(&p.section)
            .map(|line| line.points.len())
            .sum::<usize>()
    };
    assert!(points(&fine) > points(&coarse));
    assert!(
        points(&fine) <= points(&coarse) * 4 && points(&fine) < 128,
        "Tighter paper quality must have bounded growth on the analytic cylinder"
    );
    assert_eq!(fine.visible.len(), coarse.visible.len());
    assert_eq!(fine.hidden.len(), coarse.hidden.len());
    let mut segments = 0;
    let mut maximum_error = 0_f64;
    for line in fine.visible.iter().chain(&fine.hidden) {
        for pair in line.points.windows(2) {
            for point in pair {
                assert!(
                    (point[0].hypot(point[1]) - 3.).abs() < 1e-7,
                    "HLR samples must remain on the exact circle"
                );
            }
            let midpoint = [
                (pair[0][0] + pair[1][0]) * 0.5,
                (pair[0][1] + pair[1][1]) * 0.5,
            ];
            maximum_error = maximum_error.max((3. - midpoint[0].hypot(midpoint[1])).abs());
            segments += 1;
        }
    }
    assert!(segments >= 32);
    assert!(maximum_error <= 0.0100001);
    assert!(
        maximum_error * 3. * 5. * 2. < 0.301,
        "At 500% and 2x DPI the chord error must stay below 0.301 physical pixels"
    );
    let calculations = || {
        kernel
            .projection_calculations
            .load(std::sync::atomic::Ordering::Relaxed)
    };
    assert_eq!(calculations(), 2);
    for _ in 0..3 {
        let repeated = kernel.drawing_projection(&request).unwrap();
        assert_eq!(
            serde_json::to_value(repeated).unwrap(),
            serde_json::to_value(&fine).unwrap()
        );
    }
    assert_eq!(
        calculations(),
        2,
        "Unchanged drawing requests must reuse exact linework"
    );
    assert_eq!(
        serde_json::to_value(kernel.recompute(&plan).unwrap()).unwrap(),
        serde_json::to_value(initial).unwrap(),
        "Projection quality cannot change retained solid topology or mesh"
    );
}
