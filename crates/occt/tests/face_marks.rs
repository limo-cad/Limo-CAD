//! A face chosen on one kernel, found on the other by its mark.
//!
//! This is `edge_marks.rs` for the features that are given faces: a shell,
//! which keeps the faces it opens, and an external thread, which keeps the
//! cylinder it is cut on.
#![cfg(feature = "native-occt")]

use limo_cad_core::{BodyId, FeatureId};
use limo_cad_occt::measurement::ResolvedMeasurementSelection;
use limo_cad_occt::OcctKernel;
use limo_cad_solid::{
    is_counted_face_key, ExtrudeOperation, FaceMarkDto, HoleThreadDto, HoleThreadHand,
    HoleThreadRepresentation, HoleThreadSeries, HoleThreadStandard, KernelCurveDto,
    KernelExternalThreadJobDto, KernelExtrudeJobDto, KernelJobDto, KernelProfileDto,
    KernelSceneDto, KernelShellJobDto, Point3Dto, RecomputePlanDto,
};
use std::f64::consts::PI;

fn at(x: f64, y: f64, z: f64) -> Point3Dto {
    Point3Dto { x, y, z }
}

fn rectangle(x0: f64, x1: f64, y0: f64, y1: f64) -> KernelProfileDto {
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    KernelProfileDto {
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
    }
}

fn circle(radius: f64) -> KernelProfileDto {
    KernelProfileDto {
        profile_index: 0,
        points: (0..24)
            .map(|i| {
                let a = 2.0 * PI * i as f64 / 24.0;
                at(radius * a.cos(), radius * a.sin(), 0.0)
            })
            .collect(),
        curves: vec![KernelCurveDto::Circle {
            entity_id: 7,
            center: at(0.0, 0.0, 0.0),
            axis_point: at(radius, 0.0, 0.0),
            normal: at(0.0, 0.0, 1.0),
        }],
        holes: Vec::new(),
    }
}

fn extrude(feature: u64, profile: KernelProfileDto, height: f64, join: bool) -> KernelJobDto {
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(feature),
        operation: if join {
            ExtrudeOperation::Join
        } else {
            ExtrudeOperation::NewBody
        },
        require_single_solid: false,
        source_face: None,
        profiles: vec![profile],
        normal: at(0.0, 0.0, 1.0),
        start_offset: 0.0,
        end_offset: height,
        taper_angle_deg: 0.0,
        target_body_ids: if join { vec![BodyId(1)] } else { Vec::new() },
        result_body_ids: vec![BodyId(1)],
    })
}

/// A block 40 by 25 by 10 with a smaller block standing 20 high in the
/// middle of it, joined on: a body whose faces the two kernels do not list
/// in the same order.
fn stepped() -> Vec<KernelJobDto> {
    vec![
        extrude(1, rectangle(0.0, 40.0, 0.0, 25.0), 10.0, false),
        extrude(2, rectangle(10.0, 30.0, 8.0, 18.0), 20.0, true),
    ]
}

/// What the stepped block holds, solid.
const SOLID: f64 = 40.0 * 25.0 * 10.0 + 20.0 * 10.0 * 10.0;

fn plan(jobs: Vec<KernelJobDto>) -> RecomputePlanDto {
    RecomputePlanDto {
        transaction_id: 1,
        errors: vec![],
        jobs,
    }
}

/// The key of the flat face of the one body that looks along `normal` from
/// the height `level` along it, as a kernel's scene shows it.
fn flat(scene: &KernelSceneDto, normal: [f64; 3], level: f64) -> String {
    scene.bodies[0]
        .faces
        .iter()
        .find(|f| {
            f.plane.as_ref().is_some_and(|p| {
                let along: f64 = (0..3).map(|k| p.origin[k] * normal[k]).sum();
                (0..3).all(|k| (p.normal[k] - normal[k]).abs() < 1e-9)
                    && (along - level).abs() < 1e-9
            })
        })
        .unwrap_or_else(|| panic!("no flat face looking along {normal:?} at {level}"))
        .key
        .clone()
}

fn shell(keys: &[&str], marks: Vec<FaceMarkDto>) -> KernelJobDto {
    KernelJobDto::Shell(KernelShellJobDto {
        feature_id: FeatureId(3),
        target_body_id: BodyId(1),
        face_keys: keys.iter().map(|key| key.to_string()).collect(),
        face_marks: marks,
        thickness: 2.0,
        inward: true,
    })
}

fn with(mut jobs: Vec<KernelJobDto>, last: KernelJobDto) -> RecomputePlanDto {
    jobs.push(last);
    plan(jobs)
}

/// What the reference kernel's one body holds.
fn weighed(kernel: &OcctKernel) -> f64 {
    kernel
        .detached_measurement_snapshot(&[BodyId(1)])
        .unwrap()
        .inspect(&[ResolvedMeasurementSelection {
            body_id: BodyId(1),
            kind: 0,
            index: 0,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        }])
        .unwrap()
        .selections[0]
        .volume
        .unwrap()
}

fn held(kernel: &limo_cad_kernel::Kernel) -> f64 {
    limo_cad_kernel::props::mass(kernel.body(1).unwrap()).volume
}

/// One test, so that nothing else in this file is making kernels while the
/// last part of it has the variable set.
#[test]
fn a_face_chosen_on_one_kernel_is_found_on_the_other() {
    the_cylinder_of_a_thread();
    the_faces_a_shell_opens();
}

fn the_faces_a_shell_opens() {
    let mut reference = OcctKernel::new().unwrap();
    let theirs = reference.recompute(&plan(stepped())).unwrap();
    let mut limo = limo_cad_kernel::Kernel::new();
    let ours = limo.recompute(&plan(stepped()));
    assert!(theirs.errors.is_empty() && ours.errors.is_empty());

    // The face the body stands on, as each kernel knows it.
    let down = [0.0, 0.0, -1.0];
    let (counted, named) = (flat(&theirs, down, 0.0), flat(&ours, down, 0.0));
    assert!(is_counted_face_key(&counted) && !is_counted_face_key(&named));

    // Each kernel opens it under its own key, and says where it is: five
    // points of the face, on the plane z = 0 and inside the outline.
    let scene = reference
        .recompute(&with(stepped(), shell(&[&counted], Vec::new())))
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let hollow = weighed(&reference);
    assert!(hollow < 0.6 * SOLID, "{hollow}");
    let found = reference.feature_keys();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].face_keys, vec![counted.clone()]);
    assert!(found[0].edge_keys.is_empty());
    let counted_mark = found[0].face_marks[0].clone();

    let scene = limo.recompute(&with(stepped(), shell(&[&named], Vec::new())));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    // The two kernels round the inside of the step a little differently
    // where its corners turn, so they agree on the shell only nearly. Each
    // has to agree with itself exactly, whichever way it found the face.
    let ours_hollow = held(&limo);
    let ours_faces = scene.bodies[0].faces.len();
    assert!(
        (ours_hollow - hollow).abs() < 2.0e-3 * hollow,
        "{hollow} and {ours_hollow}"
    );
    let found = limo.feature_keys();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].face_keys, vec![named.clone()]);
    let named_mark = found[0].face_marks[0].clone();
    for mark in [&counted_mark, &named_mark] {
        assert_eq!(mark.points.len(), 5);
        for p in &mark.points {
            assert!(p.z.abs() < 1e-9, "{p:?}");
            assert!(p.x > 0.0 && p.x < 40.0 && p.y > 0.0 && p.y < 25.0, "{p:?}");
        }
    }

    // The reference kernel's count, on the Limo kernel: found by the mark,
    // and from then on called by its name here. The face that stands at
    // the same count in this kernel's own list is another face.
    let scene = limo.recompute(&with(
        stepped(),
        shell(&[&counted], vec![counted_mark.clone()]),
    ));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    assert!((held(&limo) - ours_hollow).abs() < 1e-9 * hollow);
    assert_eq!(limo.feature_keys()[0].face_keys, vec![named.clone()]);

    // The Limo kernel's name, on the reference kernel.
    let scene = reference
        .recompute(&with(stepped(), shell(&[&named], vec![named_mark.clone()])))
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    assert!((weighed(&reference) - hollow).abs() < 1e-9 * hollow);
    assert_eq!(reference.feature_keys()[0].face_keys, vec![counted.clone()]);

    // A key in the other's language with no mark beside it is refused by
    // both, and the body is left solid.
    let scene = reference
        .recompute(&with(stepped(), shell(&[&named], Vec::new())))
        .unwrap();
    assert!(
        scene.errors[0].message.contains("carries no mark"),
        "{:?}",
        scene.errors
    );
    assert!((weighed(&reference) - SOLID).abs() < 1e-6);
    let scene = limo.recompute(&with(stepped(), shell(&[&counted], Vec::new())));
    assert!(
        scene.errors[0].message.contains("carries no mark"),
        "{:?}",
        scene.errors
    );
    assert!((held(&limo) - SOLID).abs() < 1e-6);

    // A mark that points at nothing finds nothing, on either.
    let astray = FaceMarkDto {
        points: named_mark
            .points
            .iter()
            .map(|p| at(p.x, p.y, p.z - 1.0))
            .collect(),
    };
    let scene = limo.recompute(&with(stepped(), shell(&[&counted], vec![astray.clone()])));
    assert!(
        scene.errors[0].message.contains("where it was marked"),
        "{:?}",
        scene.errors
    );
    let scene = reference
        .recompute(&with(stepped(), shell(&[&named], vec![astray])))
        .unwrap();
    assert!(
        scene.errors[0].message.contains("where it was marked"),
        "{:?}",
        scene.errors
    );

    // A key in a kernel's own language is followed as it always was, and a
    // broken one is still called broken.
    let scene = reference
        .recompute(&with(stepped(), shell(&["face:broken"], Vec::new())))
        .unwrap();
    assert!(
        scene.errors[0]
            .message
            .contains("Invalid face topology reference"),
        "{:?}",
        scene.errors
    );
    let scene = limo.recompute(&with(stepped(), shell(&["f0"], Vec::new())));
    assert!(
        scene.errors[0]
            .message
            .contains("referenced Shell face no longer exists"),
        "{:?}",
        scene.errors
    );

    // Two faces at once, the second of them a side wall: one mark for
    // each, and both found.
    let side = [1.0, 0.0, 0.0];
    let (counted_side, named_side) = (flat(&theirs, side, 40.0), flat(&ours, side, 40.0));
    let scene = reference
        .recompute(&with(
            stepped(),
            shell(&[&counted, &counted_side], Vec::new()),
        ))
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let open_twice = weighed(&reference);
    let marks = reference.feature_keys()[0].face_marks.clone();
    assert_eq!(marks.len(), 2);
    let scene = limo.recompute(&with(stepped(), shell(&[&named, &named_side], Vec::new())));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let ours_twice = held(&limo);
    assert!((ours_twice - open_twice).abs() < 2.0e-3 * open_twice);
    assert!(ours_twice < ours_hollow - 100.0, "a second wall is gone");
    let scene = limo.recompute(&with(stepped(), shell(&[&counted, &counted_side], marks)));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    assert!((held(&limo) - ours_twice).abs() < 1e-9 * ours_twice);
    assert_eq!(
        limo.feature_keys()[0].face_keys,
        vec![named.clone(), named_side]
    );

    // A project saved before there were marks has only the count. Inside
    // Limo CAD the Limo kernel does not have to refuse it: the adapter has
    // Open CASCADE read its own count and say where the face is.
    std::env::set_var("LIMO_CAD_KERNEL", "limo");
    let carried = OcctKernel::new();
    std::env::remove_var("LIMO_CAD_KERNEL");
    let mut carried = carried.unwrap();
    assert!(carried.uses_limo_kernel());
    let scene = carried
        .recompute(&with(stepped(), shell(&[&counted], Vec::new())))
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let found = carried.feature_keys();
    assert_eq!(found[0].face_keys, vec![named]);
    assert_eq!(found[0].face_marks[0].points.len(), 5);
    assert_eq!(
        scene.bodies[0].faces.len(),
        ours_faces,
        "the same shell the Limo kernel makes under its own key"
    );
}

fn metric(nominal: f64, pitch: f64) -> HoleThreadDto {
    HoleThreadDto {
        standard: HoleThreadStandard::IsoMetric,
        series: HoleThreadSeries::MetricCoarse,
        designation: format!("M{nominal} x {pitch} - 6g"),
        class: "6g".to_string(),
        nominal_diameter: nominal,
        pitch,
        threads_per_inch: None,
        hand: HoleThreadHand::Right,
        depth: Some(6.0),
        representation: HoleThreadRepresentation::Modeled,
        tap_drill_designation: None,
        rounded_profile: None,
    }
}

/// A pin of 8 across standing on a wider foot: two cylinders, so that the
/// one to thread is not simply the only one there is.
fn pin() -> Vec<KernelJobDto> {
    vec![
        extrude(1, circle(7.0), 4.0, false),
        extrude(2, circle(4.0), 16.0, true),
    ]
}

/// The key of the cylinder of that radius, and the cylinder itself.
fn round(scene: &KernelSceneDto, radius: f64) -> (String, limo_cad_solid::CylindricalSurfaceDto) {
    let face = scene.bodies[0]
        .faces
        .iter()
        .find(|f| {
            f.cylinder
                .as_ref()
                .is_some_and(|c| (c.radius - radius).abs() < 1e-9)
        })
        .unwrap_or_else(|| panic!("no cylinder of radius {radius}"));
    (face.key.clone(), face.cylinder.unwrap())
}

fn thread(
    key: &str,
    mark: Option<FaceMarkDto>,
    cylinder: limo_cad_solid::CylindricalSurfaceDto,
) -> KernelJobDto {
    KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
        feature_id: FeatureId(3),
        target_body_id: BodyId(1),
        face_key: key.to_string(),
        face_mark: mark,
        cylinder,
        thread: metric(8.0, 1.25),
        flip: false,
    })
}

fn the_cylinder_of_a_thread() {
    let mut reference = OcctKernel::new().unwrap();
    let theirs = reference.recompute(&plan(pin())).unwrap();
    let mut limo = limo_cad_kernel::Kernel::new();
    let ours = limo.recompute(&plan(pin()));
    assert!(theirs.errors.is_empty(), "{:?}", theirs.errors);
    assert!(ours.errors.is_empty(), "{:?}", ours.errors);
    let (counted, cylinder) = round(&theirs, 4.0);
    let (named, _) = round(&ours, 4.0);
    assert!(is_counted_face_key(&counted) && !is_counted_face_key(&named));

    // Cut under each kernel's own key, with what each says of the face.
    let scene = reference
        .recompute(&with(pin(), thread(&counted, None, cylinder)))
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let cut = weighed(&reference);
    let found = reference.feature_keys();
    assert_eq!(found[0].face_keys, vec![counted.clone()]);
    let counted_mark = found[0].face_marks[0].clone();
    let scene = limo.recompute(&with(pin(), thread(&named, None, cylinder)));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let ours_cut = held(&limo);
    assert!(
        (ours_cut - cut).abs() < 2.0e-3 * cut,
        "{cut} and {ours_cut}"
    );
    let named_mark = limo.feature_keys()[0].face_marks[0].clone();
    // On the pin, not on the foot: 4 from the axis and above the foot.
    for mark in [&counted_mark, &named_mark] {
        assert_eq!(mark.points.len(), 5);
        for p in &mark.points {
            assert!((p.x.hypot(p.y) - 4.0).abs() < 1e-9 && p.z > 4.0, "{p:?}");
        }
    }

    // Across: the count on the Limo kernel, the name on the reference.
    let scene = limo.recompute(&with(pin(), thread(&counted, Some(counted_mark), cylinder)));
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    assert!((held(&limo) - ours_cut).abs() < 1e-9 * ours_cut);
    assert_eq!(limo.feature_keys()[0].face_keys, vec![named.clone()]);
    let scene = reference
        .recompute(&with(pin(), thread(&named, Some(named_mark), cylinder)))
        .unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    assert!((weighed(&reference) - cut).abs() < 1e-9 * cut);
    assert_eq!(reference.feature_keys()[0].face_keys, vec![counted.clone()]);

    // With no mark, refused by both.
    let scene = limo.recompute(&with(pin(), thread(&counted, None, cylinder)));
    assert!(
        scene.errors[0].message.contains("carries no mark"),
        "{:?}",
        scene.errors
    );
    let scene = reference
        .recompute(&with(pin(), thread(&named, None, cylinder)))
        .unwrap();
    assert!(
        scene.errors[0].message.contains("carries no mark"),
        "{:?}",
        scene.errors
    );
}
