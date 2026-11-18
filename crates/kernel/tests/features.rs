//! Features as the application asks for them: one job each, checked
//! against what the shape must measure.
#![cfg(feature = "dto")]
mod common;

use common::*;
use limo_cad_core::{BodyId, FeatureId};
use limo_cad_kernel::{Body, Kernel, SurfKind};
use limo_cad_solid::{
    ExtrudeOperation, HoleBottomStyle, HoleExtent, HoleStyle, HoleThreadDto, HoleThreadHand,
    HoleThreadRepresentation, HoleThreadSeries, HoleThreadStandard, KernelCurveDto,
    KernelExternalThreadJobDto, KernelExtrudeJobDto, KernelHoleJobDto, KernelJobDto,
    KernelLoftJobDto, KernelProfileDto, KernelSceneDto, KernelShellJobDto, KernelSweepJobDto,
    LoftContinuity, Point3Dto, RecomputePlanDto, RoundedThreadProfile, SweepOrientation,
    SweepTransition,
};
use std::f64::consts::PI;

fn at(x: f64, y: f64, z: f64) -> Point3Dto {
    Point3Dto { x, y, z }
}

/// A rectangle about the origin of the plane `z`, as sketch lines 1 to 4.
fn rectangle(width: f64, depth: f64, z: f64) -> KernelProfileDto {
    let (w, d) = (0.5 * width, 0.5 * depth);
    let corners = [(-w, -d), (w, -d), (w, d), (-w, d)];
    KernelProfileDto {
        profile_index: 0,
        points: corners.iter().map(|&(x, y)| at(x, y, z)).collect(),
        curves: (0..4)
            .map(|i| KernelCurveDto::Line {
                entity_id: i as u64 + 1,
                start: at(corners[i].0, corners[i].1, z),
                end: at(corners[(i + 1) % 4].0, corners[(i + 1) % 4].1, z),
            })
            .collect(),
        holes: Vec::new(),
    }
}

/// A circle about the z axis in the plane `z`.
fn circle(radius: f64, z: f64) -> KernelProfileDto {
    KernelProfileDto {
        profile_index: 0,
        points: (0..24)
            .map(|i| {
                let a = 2.0 * PI * i as f64 / 24.0;
                at(radius * a.cos(), radius * a.sin(), z)
            })
            .collect(),
        curves: vec![KernelCurveDto::Circle {
            entity_id: 7,
            center: at(0.0, 0.0, z),
            axis_point: at(radius, 0.0, z),
            normal: at(0.0, 0.0, 1.0),
        }],
        holes: Vec::new(),
    }
}

fn extrude(profile: KernelProfileDto, height: f64, taper_angle_deg: f64) -> KernelJobDto {
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(1),
        operation: ExtrudeOperation::NewBody,
        require_single_solid: false,
        source_face: None,
        profiles: vec![profile],
        normal: at(0.0, 0.0, 1.0),
        start_offset: 0.0,
        end_offset: height,
        taper_angle_deg,
        target_body_ids: Vec::new(),
        result_body_ids: vec![BodyId(1)],
    })
}

fn loft(sections: Vec<KernelProfileDto>, ruled: bool) -> KernelJobDto {
    KernelJobDto::Loft(KernelLoftJobDto {
        feature_id: FeatureId(1),
        operation: ExtrudeOperation::NewBody,
        sections,
        ruled,
        continuity: LoftContinuity::G0,
        centerline: Vec::new(),
        guide_rail: Vec::new(),
        target_body_ids: Vec::new(),
        result_body_ids: vec![BodyId(1)],
    })
}

fn sweep(profile: KernelProfileDto, path: Vec<KernelCurveDto>) -> KernelJobDto {
    KernelJobDto::Sweep(KernelSweepJobDto {
        feature_id: FeatureId(1),
        operation: ExtrudeOperation::NewBody,
        profile,
        path,
        guide_rail: Vec::new(),
        orientation: SweepOrientation::CorrectedFrenet,
        transition: SweepTransition::Transformed,
        force_c1: false,
        target_body_ids: Vec::new(),
        result_body_ids: vec![BodyId(1)],
    })
}

fn run(kernel: &mut Kernel, jobs: Vec<KernelJobDto>) -> KernelSceneDto {
    kernel.recompute(&RecomputePlanDto {
        transaction_id: 0,
        jobs,
        errors: Vec::new(),
    })
}

/// The one body a plan makes, checked for soundness.
fn built(jobs: Vec<KernelJobDto>) -> Body {
    let mut kernel = Kernel::new();
    let scene = run(&mut kernel, jobs);
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let body = kernel.body(1).unwrap().clone();
    body.check().unwrap();
    assert_flat_faces_are_covered(&body);
    assert_mesh_is_a_clean_skin(&body);
    body
}

fn refusal(jobs: Vec<KernelJobDto>) -> String {
    let scene = run(&mut Kernel::new(), jobs);
    assert_eq!(scene.errors.len(), 1, "{:?}", scene.errors);
    scene.errors[0].message.clone()
}

#[test]
fn tapered_extrusions() {
    // The corners are 10√2 from the middle; ten degrees there.
    let rate = 10.0_f64.to_radians().tan() / (10.0 * 2.0_f64.sqrt());
    let top = 400.0 * (1.0 + rate * 10.0).powi(2);
    let body = built(vec![extrude(rectangle(20.0, 20.0, 0.0), 10.0, 10.0)]);
    let exact = 10.0 / 3.0 * (400.0 + top + (400.0 * top).sqrt());
    assert!((volume(&body) - exact).abs() < 1e-6, "{}", volume(&body));
    assert_eq!(count(&body, SurfKind::Plane), 6);

    // A disc narrows to a cone.
    let body = built(vec![extrude(circle(10.0, 0.0), 10.0, -20.0)]);
    let r1 = 10.0 - 10.0 * 20.0_f64.to_radians().tan();
    let exact = PI * 10.0 / 3.0 * (100.0 + 10.0 * r1 + r1 * r1);
    assert!(
        (volume(&body) - exact).abs() < 1e-4 * exact,
        "{}",
        volume(&body)
    );
    assert_eq!(count(&body, SurfKind::Cone), 1);
}

#[test]
fn a_loft_through_three_rectangles() {
    let sections = vec![
        rectangle(20.0, 16.0, 0.0),
        rectangle(16.0, 12.0, 12.0),
        rectangle(10.0, 8.0, 24.0),
    ];
    let body = built(vec![loft(sections.clone(), true)]);
    // Two slices of pyramids: by the prismatoid rule.
    let slice = |a: (f64, f64), b: (f64, f64)| {
        12.0 / 6.0 * (a.0 * a.1 + (a.0 + b.0) * (a.1 + b.1) + b.0 * b.1)
    };
    let exact = slice((20.0, 16.0), (16.0, 12.0)) + slice((16.0, 12.0), (10.0, 8.0));
    assert!((volume(&body) - exact).abs() < 1e-6, "{}", volume(&body));
    assert_eq!(body.face_count(), 10);
    assert_eq!(count(&body, SurfKind::Plane), 10);

    // A smooth loft through the three bulges between them: one skin up
    // each side, no edge where the middle section was.
    let smooth = built(vec![loft(sections, false)]);
    assert_eq!(smooth.face_count(), 6);
    assert_eq!(count(&smooth, SurfKind::Spline), 4);
    // Every slice is still a rectangle. Its sides and its height are the
    // parabolas through the three sections, each at its place along the
    // skin: by how far its corners and the middles of its sides are from
    // those of the section before.
    let moved = |dx: f64, dy: f64| {
        4.0 * (dx * dx + dy * dy + 144.0).sqrt()
            + 2.0 * (dx * dx + 144.0).sqrt()
            + 2.0 * (dy * dy + 144.0).sqrt()
    };
    let (first, second) = (moved(2.0, 2.0), moved(3.0, 2.0));
    let mid = first / (first + second);
    let through = |a: f64, b: f64, c: f64, t: f64| {
        a * (t - mid) * (t - 1.0) / mid
            + b * t * (t - 1.0) / (mid * (mid - 1.0))
            + c * t * (t - mid) / (1.0 - mid)
    };
    let n = 2000;
    let bulged: f64 = (0..n)
        .map(|i| {
            let (a, b) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            let f = |t: f64| {
                let rise = (through(0.0, 12.0, 24.0, t + 1.0e-6)
                    - through(0.0, 12.0, 24.0, t - 1.0e-6))
                    / 2.0e-6;
                through(20.0, 16.0, 10.0, t) * through(16.0, 12.0, 8.0, t) * rise
            };
            (b - a) / 6.0 * (f(a) + 4.0 * f(0.5 * (a + b)) + f(b))
        })
        .sum();
    let weighed = limo_cad_kernel::props::mass(&smooth).volume;
    assert!(
        (weighed - bulged).abs() < 1.0e-7 * bulged,
        "{weighed} {bulged}"
    );
    assert!(bulged > exact);
}

#[test]
fn a_loft_through_circles_and_one_with_a_twist() {
    let body = built(vec![loft(
        vec![circle(10.0, 0.0), circle(6.0, 10.0), circle(6.0, 20.0)],
        true,
    )]);
    let exact = PI * 10.0 / 3.0 * (100.0 + 60.0 + 36.0) + PI * 36.0 * 10.0;
    assert!(
        (volume(&body) - exact).abs() < 1e-4 * exact,
        "{}",
        volume(&body)
    );
    assert_eq!(
        (
            count(&body, SurfKind::Cone),
            count(&body, SurfKind::Cylinder)
        ),
        (1, 1)
    );

    // A square turned an eighth of a turn on the way up: its sides would
    // have to twist.
    let mut turned = rectangle(20.0, 20.0, 10.0);
    let spin = |p: &mut Point3Dto| {
        let (x, y) = (p.x, p.y);
        let k = 0.5_f64.sqrt();
        (p.x, p.y) = (k * (x - y), k * (x + y));
    };
    turned.points.iter_mut().for_each(spin);
    for curve in &mut turned.curves {
        if let KernelCurveDto::Line { start, end, .. } = curve {
            spin(start);
            spin(end);
        }
    }
    let twisted = built(vec![loft(vec![rectangle(20.0, 20.0, 0.0), turned], true)]);
    assert_eq!(
        (
            count(&twisted, SurfKind::Spline),
            count(&twisted, SurfKind::Plane)
        ),
        (4, 2)
    );
    // A slice at height `t` is the square a fraction `t` of the way from
    // each corner to the corner it is joined to, an eighth of a turn on.
    // Its area is a parabola in `t`: 400 at both ends, and in the middle
    // what the mixed products of the two squares' corners make it.
    let k = 0.5_f64.sqrt();
    let low = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)];
    let high = low.map(|(x, y): (f64, f64)| (k * (x - y), k * (x + y)));
    let cross = |p: &[(f64, f64); 4], q: &[(f64, f64); 4]| -> f64 {
        (0..4)
            .map(|i| p[i].0 * q[(i + 1) % 4].1 - p[i].1 * q[(i + 1) % 4].0)
            .sum::<f64>()
            * 0.5
    };
    let mixed = cross(&low, &high) + cross(&high, &low);
    let exact = 10.0 * (400.0 + 400.0 + 0.5 * mixed) / 3.0;
    let weighed = limo_cad_kernel::props::mass(&twisted).volume;
    assert!(
        (weighed - exact).abs() < 1.0e-9 * exact,
        "{weighed} {exact}"
    );
}

#[test]
fn a_rod_swept_along_two_arcs() {
    // Up from the origin, round a quarter turn to the right, then a
    // quarter turn back up: an S of two bends of radius 20.
    let k = 0.5_f64.sqrt();
    let path = vec![
        KernelCurveDto::Arc {
            entity_id: 21,
            start: at(0.0, 0.0, 0.0),
            mid: at(20.0 - 20.0 * k, 0.0, 20.0 * k),
            end: at(20.0, 0.0, 20.0),
        },
        KernelCurveDto::Arc {
            entity_id: 22,
            start: at(20.0, 0.0, 20.0),
            mid: at(20.0 + 20.0 * k, 0.0, 40.0 - 20.0 * k),
            end: at(40.0, 0.0, 40.0),
        },
    ];
    let body = built(vec![sweep(circle(2.0, 0.0), path.clone())]);
    // Pappus: the disc's area times the length its middle travels.
    let exact = PI * 4.0 * (PI * 20.0);
    assert!(
        (volume(&body) - exact).abs() < 1e-4 * exact,
        "{}",
        volume(&body)
    );
    assert_eq!(count(&body, SurfKind::Torus), 2);
    assert_eq!(body.face_count(), 4);

    // The same path drawn from its far end is the same sweep.
    let mut backwards = path;
    backwards.reverse();
    for curve in &mut backwards {
        if let KernelCurveDto::Arc { start, end, .. } = curve {
            std::mem::swap(start, end);
        }
    }
    let again = built(vec![sweep(circle(2.0, 0.0), backwards)]);
    assert!((volume(&again) - exact).abs() < 1e-4 * exact);
}

#[test]
fn a_bar_swept_along_a_line_and_a_bend() {
    let path = vec![
        KernelCurveDto::Line {
            entity_id: 21,
            start: at(0.0, 0.0, 0.0),
            end: at(0.0, 0.0, 15.0),
        },
        KernelCurveDto::Arc {
            entity_id: 22,
            start: at(0.0, 0.0, 15.0),
            mid: at(
                30.0 - 30.0 * 0.5_f64.sqrt(),
                0.0,
                15.0 + 30.0 * 0.5_f64.sqrt(),
            ),
            end: at(30.0, 0.0, 45.0),
        },
    ];
    let body = built(vec![sweep(rectangle(8.0, 6.0, 0.0), path.clone())]);
    let exact = 48.0 * (15.0 + 0.5 * PI * 30.0);
    assert!(
        (volume(&body) - exact).abs() < 1e-4 * exact,
        "{}",
        volume(&body)
    );

    // A path that turns a corner is not followed.
    let corner = vec![
        KernelCurveDto::Line {
            entity_id: 21,
            start: at(0.0, 0.0, 0.0),
            end: at(0.0, 0.0, 15.0),
        },
        KernelCurveDto::Line {
            entity_id: 22,
            start: at(0.0, 0.0, 15.0),
            end: at(10.0, 0.0, 15.0),
        },
    ];
    let refused = refusal(vec![sweep(rectangle(8.0, 6.0, 0.0), corner)]);
    assert!(refused.contains("smoothly"), "{refused}");

    // A profile kept facing one way is the same solid along a straight
    // line. Round a bend it is another one, and that is said, not guessed.
    let facing_one_way = |path: Vec<KernelCurveDto>| {
        let mut job = sweep(rectangle(8.0, 6.0, 0.0), path);
        if let KernelJobDto::Sweep(job) = &mut job {
            job.orientation = SweepOrientation::Fixed;
        }
        job
    };
    let straight = built(vec![facing_one_way(vec![path[0].clone()])]);
    assert!((volume(&straight) - 48.0 * 15.0).abs() < 1e-9);
    let refused = refusal(vec![facing_one_way(path)]);
    assert!(refused.contains("facing one way"), "{refused}");
}

#[test]
fn a_tray_hollowed_through_the_session() {
    let mut kernel = Kernel::new();
    let blank = extrude(rectangle(32.0, 24.0, 0.0), 12.0, 0.0);
    let scene = run(&mut kernel, vec![blank.clone()]);
    let top = scene.bodies[0]
        .faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| p.normal[2] > 0.999))
        .unwrap()
        .key
        .clone();
    let shell = KernelJobDto::Shell(KernelShellJobDto {
        feature_id: FeatureId(2),
        target_body_id: BodyId(1),
        face_keys: vec![top],
        thickness: 1.0,
        inward: true,
    });
    let scene = run(&mut kernel, vec![blank, shell]);
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let tray = kernel.body(1).unwrap();
    assert!((volume(tray) - (32.0 * 24.0 * 12.0 - 30.0 * 22.0 * 11.0)).abs() < 1e-6);
    // Inside and outside of each wall have different names.
    let mut names: Vec<&str> = scene.bodies[0]
        .faces
        .iter()
        .map(|f| f.key.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 11);
}

fn metric(nominal: f64, pitch: f64, class: &str, depth: Option<f64>) -> HoleThreadDto {
    HoleThreadDto {
        standard: HoleThreadStandard::IsoMetric,
        series: HoleThreadSeries::MetricCoarse,
        designation: format!("M{nominal} x {pitch} - {class}"),
        class: class.to_string(),
        nominal_diameter: nominal,
        pitch,
        threads_per_inch: None,
        hand: HoleThreadHand::Right,
        depth,
        representation: HoleThreadRepresentation::Modeled,
        tap_drill_designation: None,
        rounded_profile: None,
    }
}

/// The cylinder face of body 1 as the scene shows it, to thread.
fn thread_on(kernel: &mut Kernel, radius: f64, thread: HoleThreadDto, flip: bool) -> KernelJobDto {
    let scene = kernel.scene(Vec::new());
    let face = scene.bodies[0]
        .faces
        .iter()
        .find(|f| {
            f.cylinder
                .as_ref()
                .is_some_and(|c| (c.radius - radius).abs() < 1.0e-9)
        })
        .expect("a cylinder to thread");
    KernelJobDto::ExternalThread(KernelExternalThreadJobDto {
        feature_id: FeatureId(2),
        target_body_id: BodyId(1),
        face_key: face.key.clone(),
        cylinder: face.cylinder.unwrap(),
        thread,
        flip,
    })
}

/// The one body a plan makes: a sound solid under a clean skin. (A face
/// turned down by a tolerance class leaves a ring too thin for the area
/// of its display triangles to say much.)
fn threaded(jobs: Vec<KernelJobDto>) -> Body {
    let mut kernel = Kernel::new();
    let scene = run(&mut kernel, jobs);
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let body = kernel.body(1).unwrap().clone();
    body.check().unwrap();
    assert_mesh_is_a_clean_skin(&body);
    body
}

#[test]
fn threads_on_a_pin_and_in_a_plate() {
    // M8 on a pin, part of the way from either end, then all of it.
    for (depth, flip) in [(Some(5.0), false), (Some(10.0), true), (None, false)] {
        let pin = extrude(circle(4.0, 0.0), 14.0, 0.0);
        let mut kernel = Kernel::new();
        assert!(run(&mut kernel, vec![pin.clone()]).errors.is_empty());
        let thread = thread_on(&mut kernel, 4.0, metric(8.0, 1.25, "6g", depth), flip);
        let body = threaded(vec![pin, thread]);
        assert_eq!(count(&body, SurfKind::Helicoid), 2);
        let plain = PI * 16.0 * 14.0;
        let volume = limo_cad_kernel::props::volume(&body);
        assert!(volume < plain && volume > 0.8 * plain, "{volume}");
    }
    // The wrong face for the thread, and a thread longer than its face.
    let pin = extrude(circle(4.0, 0.0), 14.0, 0.0);
    let mut kernel = Kernel::new();
    run(&mut kernel, vec![pin.clone()]);
    let thread = thread_on(&mut kernel, 4.0, metric(10.0, 1.5, "6g", None), false);
    assert!(refusal(vec![pin.clone(), thread]).contains("major diameter"));
    let thread = thread_on(&mut kernel, 4.0, metric(8.0, 1.25, "6g", Some(20.0)), false);
    assert!(refusal(vec![pin, thread]).contains("exceeds"));

    // M6 tapped through a plate, and to a depth in a blind hole.
    for (extent, depth) in [
        (HoleExtent::ThroughAll, None),
        (HoleExtent::Distance { depth: 9.0 }, Some(6.0)),
    ] {
        let plate = extrude(rectangle(30.0, 20.0, 0.0), 12.0, 0.0);
        let hole = KernelJobDto::Hole(KernelHoleJobDto {
            feature_id: FeatureId(2),
            target_body_id: BodyId(1),
            center: at(3.0, -2.0, 12.0),
            direction: at(0.0, 0.0, -1.0),
            diameter: 5.0,
            extent,
            style: HoleStyle::Simple,
            counterbore_diameter: 0.0,
            counterbore_depth: 0.0,
            countersink_diameter: 0.0,
            countersink_angle_deg: 90.0,
            bottom_style: HoleBottomStyle::Flat,
            drill_point_angle_deg: 118.0,
            thread: Some(metric(6.0, 1.0, "6H", depth)),
        });
        let body = threaded(vec![plate, hole]);
        assert_eq!(count(&body, SurfKind::Helicoid), 2);
    }
}

/// A hole tapped right on the edge of a plate, and right on its corner:
/// the faces of the plate pass through the axis of the thread and cut its
/// flanks along their rulings.
#[test]
fn tapped_holes_on_the_edge_and_the_corner_of_a_plate() {
    // The same with the rounded thread, whose roots the plate cuts in the
    // circles they are swept from.
    let round = HoleThreadDto {
        standard: HoleThreadStandard::CustomTrapezoidal,
        series: HoleThreadSeries::Rounded,
        designation: "Tr8 x 2 rounded".to_string(),
        class: "custom".to_string(),
        nominal_diameter: 8.0,
        pitch: 2.0,
        threads_per_inch: None,
        hand: HoleThreadHand::Right,
        depth: None,
        representation: HoleThreadRepresentation::Modeled,
        tap_drill_designation: None,
        rounded_profile: Some(RoundedThreadProfile {
            radial_depth: 1.0,
            corner_radius: 0.2,
            radial_clearance: 0.1,
            axial_clearance: 0.1,
        }),
    };
    for (x, y, part, tap, drill) in [
        (15.0, 0.0, 0.5, metric(6.0, 1.0, "6H", None), 5.0),
        (15.0, 10.0, 0.25, metric(6.0, 1.0, "6H", None), 5.0),
        (15.0, 0.0, 0.5, round.clone(), 6.0),
        (15.0, 10.0, 0.25, round.clone(), 6.0),
    ] {
        let plate = extrude(rectangle(30.0, 20.0, 0.0), 12.0, 0.0);
        let hole = |thread: Option<HoleThreadDto>| {
            KernelJobDto::Hole(KernelHoleJobDto {
                feature_id: FeatureId(2),
                target_body_id: BodyId(1),
                center: at(x, y, 12.0),
                direction: at(0.0, 0.0, -1.0),
                diameter: drill,
                extent: HoleExtent::ThroughAll,
                style: HoleStyle::Simple,
                counterbore_diameter: 0.0,
                counterbore_depth: 0.0,
                countersink_diameter: 0.0,
                countersink_angle_deg: 90.0,
                bottom_style: HoleBottomStyle::Flat,
                drill_point_angle_deg: 118.0,
                thread,
            })
        };
        let plain = threaded(vec![plate.clone(), hole(None)]);
        let tapped = threaded(vec![plate.clone(), hole(Some(tap.clone()))]);
        // Each flank comes out in as many pieces as it has turns.
        assert!(count(&tapped, SurfKind::Helicoid) >= 2 * 6, "at {x}, {y}");
        // What the thread takes on top of the drill is that part of what it
        // takes from a whole hole.
        let whole = {
            let hole = |thread: Option<HoleThreadDto>| {
                let mut job = hole(thread);
                if let KernelJobDto::Hole(job) = &mut job {
                    job.center = at(3.0, -2.0, 12.0);
                }
                job
            };
            let weigh = |jobs| limo_cad_kernel::props::volume(&threaded(jobs));
            weigh(vec![plate.clone(), hole(None)])
                - weigh(vec![plate.clone(), hole(Some(tap.clone()))])
        };
        let taken =
            limo_cad_kernel::props::volume(&plain) - limo_cad_kernel::props::volume(&tapped);
        assert!(
            (taken - part * whole).abs() < 1.0e-6 * whole,
            "at {x}, {y}: the thread takes {taken}, {part} of {whole}"
        );
    }
}

#[test]
fn a_rounded_thread_through_the_session() {
    let thread = HoleThreadDto {
        standard: HoleThreadStandard::CustomTrapezoidal,
        series: HoleThreadSeries::Rounded,
        designation: "Tr24 x 4 rounded".to_string(),
        class: "custom".to_string(),
        nominal_diameter: 24.0,
        pitch: 4.0,
        threads_per_inch: None,
        hand: HoleThreadHand::Left,
        depth: None,
        representation: HoleThreadRepresentation::Modeled,
        tap_drill_designation: None,
        rounded_profile: Some(RoundedThreadProfile {
            radial_depth: 2.0,
            corner_radius: 0.3,
            radial_clearance: 0.15,
            axial_clearance: 0.2,
        }),
    };
    let rod = extrude(circle(12.0, 0.0), 30.0, 0.0);
    let mut kernel = Kernel::new();
    run(&mut kernel, vec![rod.clone()]);
    let job = thread_on(&mut kernel, 12.0, thread, false);
    let body = threaded(vec![rod, job]);
    // Two flanks, and a round at each end of each.
    assert_eq!(count(&body, SurfKind::Helicoid), 2);
    assert_eq!(count(&body, SurfKind::Coil), 4);
}
