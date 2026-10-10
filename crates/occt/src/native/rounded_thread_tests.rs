use super::*;
use limo_cad_core::{BodyId, FeatureId};
use limo_cad_solid::{
    HoleThreadDto, HoleThreadSeries, HoleThreadStandard, KernelExternalThreadJobDto,
    KernelExtrudeJobDto, KernelHoleJobDto, RoundedThreadProfile,
};

fn rounded_spec() -> HoleThreadDto {
    HoleThreadDto {
        standard: HoleThreadStandard::CustomTrapezoidal,
        series: HoleThreadSeries::Rounded,
        designation: "CUSTOM rounded 30 degree 24 x 4".into(),
        class: "custom".into(),
        nominal_diameter: 24.0,
        pitch: 4.0,
        threads_per_inch: None,
        hand: HoleThreadHand::Right,
        depth: None,
        representation: HoleThreadRepresentation::Modeled,
        tap_drill_designation: None,
        rounded_profile: Some(RoundedThreadProfile {
            radial_depth: 2.0,
            corner_radius: 0.3,
            radial_clearance: 0.25,
            axial_clearance: 0.2,
        }),
    }
}

fn cylinder(feature: u64, body: u64, radius: f64) -> KernelJobDto {
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(feature),
        operation: ExtrudeOperation::NewBody,
        source_face: None,
        profiles: vec![KernelProfileDto {
            profile_index: 0,
            points: (0..64)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 64.0;
                    Point3Dto {
                        x: radius * a.cos(),
                        y: radius * a.sin(),
                        z: 0.0,
                    }
                })
                .collect(),
            curves: vec![KernelCurveDto::Circle {
                entity_id: 1,
                center: [0.0, 0.0, 0.0].into(),
                axis_point: [radius, 0.0, 0.0].into(),
                normal: [0.0, 0.0, 1.0].into(),
            }],
            holes: vec![],
        }],
        normal: [0.0, 0.0, 1.0].into(),
        start_offset: 0.0,
        end_offset: 8.0,
        taper_angle_deg: 0.0,
        target_body_ids: vec![],
        result_body_ids: vec![BodyId(body)],
    })
}

#[test]
fn rounded_native_mates_have_curved_profiles_and_helical_clearance() {
    for hand in [HoleThreadHand::Right, HoleThreadHand::Left] {
        assert_rounded_mates(hand);
    }
}

#[test]
fn rounded_partial_long_shaft_and_flat_export_closed_meshes() {
    let mut shaft = cylinder(1, 1, 12.0);
    if let KernelJobDto::Extrude(job) = &mut shaft {
        job.end_offset = 175.0;
        job.normal = [1.0, 0.0, 0.0].into();
        for point in &mut job.profiles[0].points {
            *point = [-96.0, point.x, 50.0 + point.y].into();
        }
        job.profiles[0].curves = vec![KernelCurveDto::Circle {
            entity_id: 1,
            center: [-96.0, 0.0, 50.0].into(),
            axis_point: [-96.0, 12.0, 50.0].into(),
            normal: [1.0, 0.0, 0.0].into(),
        }];
    }
    let mut kernel = OcctKernel::new().unwrap();
    let mut plan = RecomputePlanDto {
        transaction_id: 1,
        errors: vec![],
        jobs: vec![shaft],
    };
    let blank = kernel.recompute(&plan).unwrap();
    let face = blank.bodies[0]
        .faces
        .iter()
        .find(|f| f.cylinder.is_some())
        .unwrap();
    let mut thread = rounded_spec();
    thread.depth = Some(165.0);
    plan.jobs
        .push(KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
            feature_id: FeatureId(2),
            target_body_id: BodyId(1),
            face_key: face.key.clone(),
            cylinder: face.cylinder.unwrap(),
            thread,
            flip: false,
        }));
    for flattened in [false, true] {
        if flattened {
            plan.jobs.push(KernelJobDto::Extrude(KernelExtrudeJobDto {
                feature_id: FeatureId(3),
                operation: ExtrudeOperation::Cut,
                source_face: None,
                profiles: vec![KernelProfileDto {
                    profile_index: 0,
                    points: vec![
                        [-120.0, -25.0, 20.0].into(),
                        [-120.0, 25.0, 20.0].into(),
                        [-120.0, 25.0, 43.0].into(),
                        [-120.0, -25.0, 43.0].into(),
                    ],
                    curves: vec![],
                    holes: vec![],
                }],
                normal: [1.0, 0.0, 0.0].into(),
                start_offset: 0.0,
                end_offset: 220.0,
                taper_angle_deg: 0.0,
                target_body_ids: vec![BodyId(1)],
                result_body_ids: vec![BodyId(2)],
            }));
        }
        let scene = kernel.recompute(&plan).unwrap();
        assert!(
            scene.errors.is_empty(),
            "flattened={flattened}: {:?}",
            scene.errors
        );
        let exported = kernel.export_3mf(&MeshExportRequest::default(), &[]);
        assert!(exported.is_ok(), "flattened={flattened}: {exported:?}");
        let fine = kernel.export_3mf(
            &MeshExportRequest {
                linear_deflection: 0.0375,
                angular_deflection: 0.175,
                ..Default::default()
            },
            &[],
        );
        assert!(fine.is_ok(), "fine flattened={flattened}: {fine:?}");
        assert_eq!(
            scene,
            kernel.recompute(&plan).unwrap(),
            "export must not change render meshes"
        );
    }
    let saved = serde_json::to_vec(&plan).unwrap();
    let mut restored = OcctKernel::new().unwrap();
    let replay = restored
        .recompute(&serde_json::from_slice(&saved).unwrap())
        .unwrap();
    assert_eq!(replay, kernel.recompute(&plan).unwrap());
    restored
        .export_3mf(&MeshExportRequest::default(), &[])
        .unwrap();
}

fn assert_rounded_mates(hand: HoleThreadHand) {
    let handedness = if hand == HoleThreadHand::Right {
        1.0
    } else {
        -1.0
    };
    let spec = || {
        let mut thread = rounded_spec();
        thread.hand = hand;
        thread
    };
    let mut kernel = OcctKernel::new().unwrap();
    let male_blank = cylinder(1, 1, 12.0);
    let female_blank = cylinder(3, 2, 16.0);
    let blank = kernel
        .recompute(&RecomputePlanDto {
            transaction_id: 1,
            errors: vec![],
            jobs: vec![male_blank.clone()],
        })
        .unwrap();
    let face = blank.bodies[0]
        .faces
        .iter()
        .find(|f| f.cylinder.is_some())
        .unwrap();
    let male = KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
        feature_id: FeatureId(2),
        target_body_id: BodyId(1),
        face_key: face.key.clone(),
        cylinder: face.cylinder.unwrap(),
        thread: spec(),
        flip: false,
    });
    let female = KernelJobDto::Hole(KernelHoleJobDto {
        feature_id: FeatureId(4),
        target_body_id: BodyId(2),
        center: [0.0, 0.0, 0.0].into(),
        direction: [0.0, 0.0, 1.0].into(),
        diameter: 20.0,
        extent: HoleExtent::ThroughAll,
        style: HoleStyle::Simple,
        counterbore_diameter: 0.0,
        counterbore_depth: 0.0,
        countersink_diameter: 0.0,
        countersink_angle_deg: 90.0,
        bottom_style: HoleBottomStyle::Flat,
        drill_point_angle_deg: 118.0,
        thread: Some(spec()),
    });
    let plan = RecomputePlanDto {
        transaction_id: 2,
        errors: vec![],
        jobs: vec![male_blank, male, female_blank, female],
    };

    let saved = serde_json::to_vec(&plan).unwrap();
    let scene = kernel
        .recompute(&serde_json::from_slice(&saved).unwrap())
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    assert_eq!(scene.bodies.len(), 2);
    let step =
        String::from_utf8(kernel.export_step(&StepExportRequest::default()).unwrap()).unwrap();
    assert_eq!(step.matches("MANIFOLD_SOLID_BREP").count(), 2);
    assert!(
        step.contains("RATIONAL_B_SPLINE_SURFACE"),
        "circular-arc section must survive as rational curved faces"
    );
    for (body_id, lo, hi) in [(1, 10.0, 12.0), (2, 10.25, 12.25)] {
        let body = scene
            .bodies
            .iter()
            .find(|b| b.body_id == BodyId(body_id))
            .unwrap();
        let radii: Vec<_> = body
            .indices
            .iter()
            .map(|index| &body.positions[*index as usize * 3..*index as usize * 3 + 3])
            .filter_map(|p| {
                let radius = f64::from(p[0]).hypot(f64::from(p[1]));
                (p[2] > 0.1 && p[2] < 7.9 && radius < 13.0).then_some(radius)
            })
            .collect();
        let min = radii.iter().copied().fold(f64::INFINITY, f64::min);
        let max = radii.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (min - lo).abs() < 0.03 && (max - hi).abs() < 0.03,
            "body{body_id} radii {min}..{max}, expected {lo}..{hi}"
        );
    }

    let z0 = 1.0
        - 15_f64.to_radians().tan()
        - 0.3 * (1.0 / 15_f64.to_radians().cos() - 15_f64.to_radians().tan());
    let body = scene
        .bodies
        .iter()
        .find(|b| b.body_id == BodyId(1))
        .unwrap();
    let mut curved_samples = 0;
    for point in body.positions.as_chunks::<3>().0 {
        let x = f64::from(point[0]);
        let y = f64::from(point[1]);
        let z = f64::from(point[2]);
        let radius = x.hypot(y);
        if !(0.1..7.9).contains(&z) || !(10.025..10.19).contains(&radius) {
            continue;
        }
        let phase =
            (z - handedness * 4.0 * y.atan2(x) / std::f64::consts::TAU + 2.0).rem_euclid(4.0) - 2.0;
        let circle_radius = (radius - 10.3).hypot(phase.abs() - z0);
        assert!(
            (circle_radius - 0.3).abs() < 0.001,
            "root is not circular: r={radius}, phase={phase}, circle={circle_radius}"
        );
        curved_samples += 1;
    }
    assert!(
        curved_samples >= 4,
        "native mesh must sample the rounded root: {curved_samples}"
    );
    let fixed = PlacedBodyQueryDto {
        body_id: BodyId(2),
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
    };

    for quarter in 0..4 {
        let angle = handedness * std::f64::consts::FRAC_PI_2 * quarter as f64;
        let moving = PlacedBodyQueryDto {
            body_id: BodyId(1),
            translation: [0.0, 0.0, 2.0 + quarter as f64],
            rotation: [0.0, 0.0, (angle * 0.5).sin(), (angle * 0.5).cos()],
        };
        let fit = kernel.exact_interference(moving, fixed).unwrap();
        eprintln!("rounded {hand:?} mating quarter {quarter}: {fit:?}");
        assert!(
            fit.overlap_volume_mm3.abs() < 1e-6,
            "quarter{quarter}: {fit:?}"
        );
        assert!(fit.minimum_clearance_mm > 0.05, "quarter{quarter}: {fit:?}");
    }
    let wrong_phase = kernel
        .exact_interference(
            PlacedBodyQueryDto {
                body_id: BodyId(1),
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
            },
            fixed,
        )
        .unwrap();
    assert!(
        wrong_phase.overlap_volume_mm3 > 10.0,
        "incorrect assembly phase must interfere: {wrong_phase:?}"
    );
    if hand == HoleThreadHand::Right {
        let mut partial = plan;
        if let KernelJobDto::ExternalThread(job) = &mut partial.jobs[1] {
            job.thread.depth = Some(2.0);
            job.flip = job.cylinder.axis.z < 0.0;
        }
        if let KernelJobDto::Hole(job) = &mut partial.jobs[3] {
            job.thread.as_mut().unwrap().depth = Some(2.0);
        }
        let scene = kernel.recompute(&partial).unwrap();
        assert!(
            scene.errors.is_empty(),
            "partial thread: {:?}",
            scene.errors
        );
        for (body_id, expected_radius) in [(1, 12.0), (2, 10.25)] {
            let body = scene
                .bodies
                .iter()
                .find(|b| b.body_id == BodyId(body_id))
                .unwrap();
            let radii: Vec<_> = body
                .indices
                .iter()
                .filter_map(|index| {
                    let p = &body.positions[*index as usize * 3..*index as usize * 3 + 3];
                    let r = f64::from(p[0]).hypot(f64::from(p[1]));
                    (p[2] > 2.01 && p[2] < 8.01 && r < 13.0).then_some(r)
                })
                .collect();
            assert!(!radii.is_empty(), "unthreaded region must remain");
            assert!(
                radii.iter().all(|r| (r - expected_radius).abs() < 0.03),
                "partial thread cuts beyond its requested depth on body {body_id}: {radii:?}"
            );
        }
    }
}

#[test]
fn legacy_thread_partial_depth_does_not_cut_the_unthreaded_shank_or_bore() {
    let mut kernel = OcctKernel::new().unwrap();
    let shaft = cylinder(1, 1, 3.0);
    let blank = kernel
        .recompute(&RecomputePlanDto {
            transaction_id: 1,
            errors: vec![],
            jobs: vec![shaft.clone()],
        })
        .unwrap();
    let face = blank.bodies[0]
        .faces
        .iter()
        .find(|f| f.cylinder.is_some())
        .unwrap();
    let spec = |class: &str| HoleThreadDto {
        standard: HoleThreadStandard::IsoMetric,
        series: HoleThreadSeries::MetricCoarse,
        class: class.into(),
        designation: format!("M6 x 1 - {class}"),
        nominal_diameter: 6.0,
        pitch: 1.0,
        threads_per_inch: None,
        hand: HoleThreadHand::Right,
        depth: Some(2.0),
        representation: HoleThreadRepresentation::Modeled,
        tap_drill_designation: None,
        rounded_profile: None,
    };
    let scene = kernel
        .recompute(&RecomputePlanDto {
            transaction_id: 2,
            errors: vec![],
            jobs: vec![
                shaft,
                KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
                    feature_id: FeatureId(2),
                    target_body_id: BodyId(1),
                    face_key: face.key.clone(),
                    cylinder: face.cylinder.unwrap(),
                    thread: spec("6g"),
                    flip: face.cylinder.as_ref().unwrap().axis.z < 0.0,
                }),
                cylinder(3, 2, 8.0),
                KernelJobDto::Hole(KernelHoleJobDto {
                    feature_id: FeatureId(4),
                    target_body_id: BodyId(2),
                    center: [0.0; 3].into(),
                    direction: [0.0, 0.0, 1.0].into(),
                    diameter: 5.0,
                    extent: HoleExtent::ThroughAll,
                    style: HoleStyle::Simple,
                    counterbore_diameter: 0.0,
                    counterbore_depth: 0.0,
                    countersink_diameter: 0.0,
                    countersink_angle_deg: 90.0,
                    bottom_style: HoleBottomStyle::Flat,
                    drill_point_angle_deg: 118.0,
                    thread: Some(spec("6H")),
                }),
            ],
        })
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let minor = limo_cad_solid::iso_metric_grade6_envelope(6.0, 1.0, ThreadFit::Internal)
        .unwrap()
        .modeled_minor
        / 2.0;
    for (body_id, expected_radius) in [(1, 3.0), (2, minor)] {
        let body = scene
            .bodies
            .iter()
            .find(|b| b.body_id == BodyId(body_id))
            .unwrap();
        let radii: Vec<_> = body
            .indices
            .iter()
            .filter_map(|index| {
                let p = &body.positions[*index as usize * 3..*index as usize * 3 + 3];
                let r = f64::from(p[0]).hypot(f64::from(p[1]));
                (p[2] > 2.01 && p[2] < 8.01 && r < 3.1).then_some(r)
            })
            .collect();
        assert!(!radii.is_empty());
        let min = radii.iter().copied().fold(f64::INFINITY, f64::min);
        let max = radii.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            (min - expected_radius).abs() < 0.01 && (max - expected_radius).abs() < 0.01,
            "body{body_id}: requested2mmthread altered unthreaded radius {min}..{max}, expected{expected_radius}"
        );
    }
}

#[test]
fn rounded_blind_thread_respects_the_hole_floor_and_partial_depth() {
    assert_blind_thread_depth(rounded_spec(), 20.0);
}

#[test]
fn legacy_blind_thread_respects_the_hole_floor_and_partial_depth() {
    let mut thread = rounded_spec();
    thread.standard = HoleThreadStandard::IsoMetric;
    thread.series = HoleThreadSeries::MetricCoarse;
    thread.class = "6H".into();
    thread.designation = "M6 x 1 - 6H".into();
    thread.nominal_diameter = 6.0;
    thread.pitch = 1.0;
    thread.rounded_profile = None;
    assert_blind_thread_depth(thread, 5.0);
}

fn assert_blind_thread_depth(mut thread: HoleThreadDto, predrill: f64) {
    let hole_depth = 4.0;
    let stock_depth = 12.0;
    let stock_radius = thread.nominal_diameter * 0.5 + 4.0;
    let minor_radius = rounded_thread_diameters(&thread, ThreadFit::Internal)
        .unwrap()
        .map(|diameters| diameters[2])
        .unwrap_or_else(|| {
            iso_metric_thread_envelope(&thread, ThreadFit::Internal)
                .unwrap()
                .unwrap()
                .modeled_minor
        })
        * 0.5;

    for depth in [None, Some(hole_depth), Some(2.0)] {
        thread.depth = depth;
        let mut stock = cylinder(1, 1, stock_radius);
        if let KernelJobDto::Extrude(job) = &mut stock {
            job.end_offset = stock_depth;
        }
        let plan = RecomputePlanDto {
            transaction_id: 1,
            errors: vec![],
            jobs: vec![
                stock,
                KernelJobDto::Hole(KernelHoleJobDto {
                    feature_id: FeatureId(2),
                    target_body_id: BodyId(1),
                    center: [0.0; 3].into(),
                    direction: [0.0, 0.0, 1.0].into(),
                    diameter: predrill,
                    extent: HoleExtent::Distance { depth: hole_depth },
                    style: HoleStyle::Simple,
                    counterbore_diameter: 0.0,
                    counterbore_depth: 0.0,
                    countersink_diameter: 0.0,
                    countersink_angle_deg: 90.0,
                    bottom_style: HoleBottomStyle::Flat,
                    drill_point_angle_deg: 118.0,
                    thread: Some(thread.clone()),
                }),
            ],
        };
        let mut kernel = OcctKernel::new().unwrap();
        let saved = serde_json::to_vec(&plan).unwrap();
        let scene = kernel
            .recompute(&serde_json::from_slice(&saved).unwrap())
            .unwrap();
        assert!(
            scene.errors.is_empty(),
            "depth={depth:?}: {:?}",
            scene.errors
        );
        assert_eq!(scene.bodies.len(), 1);
        let cavity: Vec<_> = scene.bodies[0]
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| (f64::from(p[0]).hypot(f64::from(p[1])), f64::from(p[2])))
            .filter(|(radius, z)| *radius < stock_radius - 0.01 && *z < stock_depth - 0.01)
            .collect();
        let deepest = cavity.iter().map(|(_, z)| *z).fold(0.0, f64::max);
        assert!(
            (deepest - hole_depth).abs() < 0.001,
            "depth={depth:?}: blind hole floor is {hole_depth} mm, but cavity reaches {deepest} mm"
        );
        if let Some(thread_depth) = depth.filter(|value| *value < hole_depth) {
            let plain_bore: Vec<_> = cavity
                .iter()
                .filter(|(radius, z)| *z > thread_depth + 0.001 && *radius > minor_radius * 0.99)
                .collect();
            assert!(!plain_bore.is_empty());
            assert!(
                plain_bore
                    .iter()
                    .all(|(radius, _)| (*radius - minor_radius).abs() < 0.001),
                "partial blind thread changed its remaining plain bore: {plain_bore:?}"
            );
        }
        kernel
            .export_3mf(&MeshExportRequest::default(), &[])
            .unwrap();
    }
}

#[test]
fn rounded_external_thread_preserves_the_adjacent_shoulder() {
    assert_threaded_shoulder(rounded_spec());
}

#[test]
fn legacy_external_thread_preserves_the_adjacent_shoulder() {
    let mut thread = rounded_spec();
    thread.standard = HoleThreadStandard::IsoMetric;
    thread.series = HoleThreadSeries::MetricCoarse;
    thread.class = "6g".into();
    thread.designation = "M6 x 1 - 6g".into();
    thread.nominal_diameter = 6.0;
    thread.pitch = 1.0;
    thread.rounded_profile = None;
    assert_threaded_shoulder(thread);
}

fn assert_threaded_shoulder(mut thread: HoleThreadDto) {
    let shaft_radius = thread.nominal_diameter * 0.5;
    for toward_shoulder in [true, false] {
        for depth in [None, Some(4.0), Some(2.0)] {
            thread.depth = depth;
            let mut shaft = cylinder(1, 1, shaft_radius);
            if let KernelJobDto::Extrude(job) = &mut shaft {
                job.end_offset = 4.0;
            }
            let mut shoulder = cylinder(2, 1, shaft_radius + 2.0);
            if let KernelJobDto::Extrude(job) = &mut shoulder {
                job.operation = ExtrudeOperation::Join;
                job.start_offset = 4.0;
                job.end_offset = 8.0;
                job.target_body_ids = vec![BodyId(1)];
            }
            let mut kernel = OcctKernel::new().unwrap();
            let mut plan = RecomputePlanDto {
                transaction_id: 1,
                errors: vec![],
                jobs: vec![shaft, shoulder],
            };
            let blank = kernel.recompute(&plan).unwrap();
            assert!(blank.errors.is_empty(), "joined stock: {:?}", blank.errors);
            assert_eq!(blank.bodies.len(), 1);
            let face = blank.bodies[0]
                .faces
                .iter()
                .find(|f| {
                    f.cylinder
                        .as_ref()
                        .is_some_and(|c| (c.radius - shaft_radius).abs() < 1e-6)
                })
                .unwrap();
            let cylinder = face.cylinder.unwrap();
            plan.jobs
                .push(KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
                    feature_id: FeatureId(3),
                    target_body_id: BodyId(1),
                    face_key: face.key.clone(),
                    flip: (cylinder.axis.z < 0.0) == toward_shoulder,
                    cylinder,
                    thread: thread.clone(),
                }));
            let scene = kernel.recompute(&plan).unwrap();
            assert!(
                scene.errors.is_empty(),
                "toward_shoulder={toward_shoulder}, depth={depth:?}: {:?}",
                scene.errors
            );
            let body = &scene.bodies[0];
            let points: Vec<_> = body
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| (f64::from(p[0]).hypot(f64::from(p[1])), f64::from(p[2])))
                .collect();
            let shoulder_cuts: Vec<_> = points
                .iter()
                .filter(|(r, z)| *z > 4.001 && *z < 7.999 && *r < shaft_radius + 1.9)
                .collect();
            assert!(
                shoulder_cuts.is_empty(),
                "toward_shoulder={toward_shoulder}, depth={depth:?}: thread cut into adjacent shoulder at {:?}",
                &shoulder_cuts[..shoulder_cuts.len().min(4)]
            );
            let end = depth.unwrap_or(4.0);
            assert!(
                points.iter().any(|(r, z)| *r < shaft_radius - 0.05
                    && if toward_shoulder {
                        *z > 0.001 && *z < end - 0.001
                    } else {
                        *z > 4.0 - end + 0.001 && *z < 3.999
                    }),
                "requested shaft thread must actually remove material"
            );
            if end < 4.0 {
                let plain: Vec<_> = body
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|triangle| {
                        triangle
                            .iter()
                            .map(|index| points[*index as usize])
                            .collect::<Vec<_>>()
                    })
                    .filter(|triangle| {
                        let z = triangle.iter().map(|(_, z)| z).sum::<f64>() / 3.0;
                        if toward_shoulder {
                            z > end + 0.001 && z < 3.999
                        } else {
                            z > 0.001 && z < 4.0 - end - 0.001
                        }
                    })
                    .collect();
                assert!(!plain.is_empty());
                assert!(
                    plain
                        .iter()
                        .flatten()
                        .all(|(r, _)| (*r - shaft_radius).abs() < 0.001),
                    "partial thread must retain the remaining plain shank"
                );
            }
            kernel
                .export_3mf(&MeshExportRequest::default(), &[])
                .unwrap();
        }
    }
}
