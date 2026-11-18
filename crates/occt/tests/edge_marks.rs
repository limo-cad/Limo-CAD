//! An edge chosen on one kernel, found on the other by its mark.
#![cfg(feature = "native-occt")]

use limo_cad_core::{BodyId, FeatureId};
use limo_cad_occt::OcctKernel;
use limo_cad_solid::{
    is_counted_edge_key, EdgeMarkDto, ExtrudeOperation, KernelCurveDto, KernelExtrudeJobDto,
    KernelFilletJobDto, KernelJobDto, KernelProfileDto, KernelSceneDto, Point3Dto,
    RecomputePlanDto,
};
use std::f64::consts::PI;

fn at(x: f64, y: f64, z: f64) -> Point3Dto {
    Point3Dto { x, y, z }
}

/// A block 20 by 10 by 8 about the z axis, standing on the plane z = 0.
fn block() -> KernelJobDto {
    let corners = [(-10.0, -5.0), (10.0, -5.0), (10.0, 5.0), (-10.0, 5.0)];
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(1),
        operation: ExtrudeOperation::NewBody,
        require_single_solid: false,
        source_face: None,
        profiles: vec![KernelProfileDto {
            profile_index: 0,
            points: corners.iter().map(|&(x, y)| at(x, y, 0.0)).collect(),
            curves: (0..4)
                .map(|i| KernelCurveDto::Line {
                    entity_id: i as u64 + 1,
                    start: at(corners[i].0, corners[i].1, 0.0),
                    end: at(corners[(i + 1) % 4].0, corners[(i + 1) % 4].1, 0.0),
                })
                .collect(),
            holes: Vec::new(),
        }],
        normal: at(0.0, 0.0, 1.0),
        start_offset: 0.0,
        end_offset: 8.0,
        taper_angle_deg: 0.0,
        target_body_ids: Vec::new(),
        result_body_ids: vec![BodyId(1)],
    })
}

fn plan(jobs: Vec<KernelJobDto>) -> RecomputePlanDto {
    RecomputePlanDto {
        transaction_id: 1,
        errors: vec![],
        jobs,
    }
}

/// The key and the mark of the upright edge of the block at `(x, y)`, as
/// a kernel's scene shows it.
fn upright(scene: &KernelSceneDto, x: f64, y: f64) -> (String, EdgeMarkDto) {
    let edge = scene.bodies[0]
        .edges
        .iter()
        .find(|e| {
            e.points.len() == 2
                && e.points
                    .iter()
                    .all(|p| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9)
        })
        .expect("the upright edge");
    (edge.key.clone(), EdgeMarkDto::from_polyline(&edge.points))
}

fn round(key: &str, marks: Vec<EdgeMarkDto>) -> KernelJobDto {
    KernelJobDto::Fillet(KernelFilletJobDto {
        feature_id: FeatureId(2),
        target_body_id: BodyId(1),
        edge_keys: vec![key.to_string()],
        edge_marks: marks,
        radius: 2.0,
        tangent_chain: false,
    })
}

/// What the block holds with one upright edge rounded to a radius of 2.
fn rounded() -> f64 {
    20.0 * 10.0 * 8.0 - (1.0 - PI / 4.0) * 4.0 * 8.0
}

#[test]
fn an_edge_chosen_on_one_kernel_is_found_on_the_other() {
    use limo_cad_occt::measurement::ResolvedMeasurementSelection;
    let mut reference = OcctKernel::new().unwrap();
    let theirs = reference.recompute(&plan(vec![block()])).unwrap();
    let mut limo = limo_cad_kernel::Kernel::new();
    let ours = limo.recompute(&plan(vec![block()]));
    let (counted, counted_mark) = upright(&theirs, 10.0, 5.0);
    let (named, named_mark) = upright(&ours, 10.0, 5.0);
    assert!(is_counted_edge_key(&counted) && !is_counted_edge_key(&named));
    assert_eq!(counted_mark.points.len(), 5);

    // The reference kernel's key, on the Limo kernel.
    let scene = limo.recompute(&plan(vec![
        block(),
        round(&counted, vec![counted_mark.clone()]),
    ]));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let weighed = limo_cad_kernel::props::mass(limo.body(1).unwrap()).volume;
    assert!((weighed - rounded()).abs() < 1e-9, "{weighed}");
    // It says what the edge is called here, and where it is.
    let found = limo.finish_edges();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].edge_keys, vec![named.clone()]);
    assert_eq!(found[0].edge_marks[0].points.len(), 5);

    // The Limo kernel's name, on the reference kernel.
    let scene = reference
        .recompute(&plan(vec![
            block(),
            round(&named, vec![named_mark.clone()]),
        ]))
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let measured = reference
        .detached_measurement_snapshot(&[BodyId(1)])
        .unwrap()
        .inspect(&[ResolvedMeasurementSelection {
            body_id: BodyId(1),
            kind: 0,
            index: 0,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }])
        .unwrap();
    let weighed = measured.selections[0].volume.unwrap();
    assert!((weighed - rounded()).abs() < 1e-6, "{weighed}");
    let found = reference.finish_edges();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].edge_keys, vec![counted.clone()]);

    // A name with no mark beside it gives the reference kernel nothing to
    // find the edge by, and it says so. (A count with no mark is from
    // before there were marks; the Limo kernel takes it as it always did,
    // as a count of its own edges.)
    let scene = reference
        .recompute(&plan(vec![block(), round(&named, Vec::new())]))
        .unwrap();
    assert!(
        scene.errors[0].message.contains("carries no mark"),
        "{:?}",
        scene.errors
    );

    // A mark that points at nothing finds nothing: the edge it was made
    // for is not there, and no other is taken in its place.
    let astray = EdgeMarkDto {
        points: named_mark
            .points
            .iter()
            .map(|p| at(p.x - 1.0, p.y - 1.0, p.z))
            .collect(),
    };
    let scene = limo.recompute(&plan(vec![block(), round(&counted, vec![astray.clone()])]));
    assert!(
        scene.errors[0].message.contains("where it was marked"),
        "{:?}",
        scene.errors
    );
    let scene = reference
        .recompute(&plan(vec![block(), round(&named, vec![astray])]))
        .unwrap();
    assert!(
        scene.errors[0].message.contains("where it was marked"),
        "{:?}",
        scene.errors
    );

    // A key in a kernel's own language is followed as it always was,
    // mark or no mark, and a broken one is still called broken.
    let scene = limo.recompute(&plan(vec![block(), round(&named, Vec::new())]));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let scene = reference
        .recompute(&plan(vec![block(), round("edge:broken", Vec::new())]))
        .unwrap();
    assert!(
        scene.errors[0]
            .message
            .contains("Invalid edge topology reference"),
        "{:?}",
        scene.errors
    );
}
