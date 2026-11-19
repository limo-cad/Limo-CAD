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

    // A profile kept facing one way is the same solid along a straight
    // line. Round the bend every slice level with the profile is still a
    // copy of it: the volume is its area times the height gained.
    let facing_one_way = |path: Vec<KernelCurveDto>| {
        let mut job = sweep(rectangle(8.0, 6.0, 0.0), path);
        if let KernelJobDto::Sweep(job) = &mut job {
            job.orientation = SweepOrientation::Fixed;
        }
        job
    };
    let straight = built(vec![facing_one_way(vec![path[0].clone()])]);
    assert!((volume(&straight) - 48.0 * 15.0).abs() < 1e-9);
    // Half the bend: an eighth of a turn.
    let (sin, cos) = (PI / 4.0).sin_cos();
    let (half_sin, half_cos) = (PI / 8.0).sin_cos();
    let gentler = vec![
        path[0].clone(),
        KernelCurveDto::Arc {
            entity_id: 22,
            start: at(0.0, 0.0, 15.0),
            mid: at(30.0 - 30.0 * half_cos, 0.0, 15.0 + 30.0 * half_sin),
            end: at(30.0 - 30.0 * cos, 0.0, 15.0 + 30.0 * sin),
        },
    ];
    let carried = built(vec![facing_one_way(gentler)]);
    let weighed = limo_cad_kernel::props::mass(&carried).volume;
    let exact = 48.0 * (15.0 + 30.0 * sin);
    assert!((weighed - exact).abs() < 1e-7, "{weighed} {exact}");
    assert!(count(&carried, SurfKind::Spline) + count(&carried, SurfKind::Cylinder) >= 4);
    // By the end of the whole bend the path runs along the profile, and
    // the bar would thin out to nothing: that is said, not built.
    let refused = refusal(vec![facing_one_way(path)]);
    assert!(refused.contains("facing one way"), "{refused}");
}

#[test]
fn a_sweep_steered_by_a_rail() {
    let rail_job = |profile: KernelProfileDto, path: Vec<KernelCurveDto>, rail| {
        let mut job = sweep(profile, path);
        if let KernelJobDto::Sweep(job) = &mut job {
            job.guide_rail = rail;
        }
        job
    };
    let up = |height: f64| {
        vec![KernelCurveDto::Line {
            entity_id: 21,
            start: at(0.0, 0.0, 0.0),
            end: at(0.0, 0.0, height),
        }]
    };
    // A rail that leans away from the path makes the rod widen: a cone.
    let cone = built(vec![rail_job(
        circle(2.0, 0.0),
        up(20.0),
        vec![KernelCurveDto::Line {
            entity_id: 31,
            start: at(2.0, 0.0, 0.0),
            end: at(4.0, 0.0, 20.0),
        }],
    )]);
    let exact = PI * 20.0 / 3.0 * (4.0 + 8.0 + 16.0);
    let weighed = limo_cad_kernel::props::mass(&cone).volume;
    assert!((weighed - exact).abs() < 1e-8 * exact, "{weighed} {exact}");
    // A rail that winds a quarter turn round the path turns a bar with
    // it: every slice is still the bar's own rectangle.
    let wound = built(vec![rail_job(
        rectangle(6.0, 3.0, 0.0),
        up(30.0),
        vec![KernelCurveDto::Polyline {
            entity_id: 32,
            points: (0..=24)
                .map(|i| {
                    let t = i as f64 / 24.0;
                    let a = 0.5 * PI * t;
                    at(5.0 * a.cos(), 5.0 * a.sin(), 30.0 * t)
                })
                .collect(),
        }],
    )]);
    assert_eq!(count(&wound, SurfKind::Spline), 4);
    let weighed = limo_cad_kernel::props::mass(&wound).volume;
    // (The rail is drawn as chords of a helix, and each chord dips a
    // little towards the path in its middle; the bar, which is kept in
    // touch with the rail, shrinks with it there.)
    assert!(
        weighed < 540.0 && weighed > 540.0 * (1.0 - 1.0e-3),
        "{weighed}"
    );
    // A rail that starts on the path steers nothing.
    let refused = refusal(vec![rail_job(
        circle(2.0, 0.0),
        up(20.0),
        vec![KernelCurveDto::Line {
            entity_id: 31,
            start: at(0.0, 0.0, 0.0),
            end: at(4.0, 0.0, 20.0),
        }],
    )]);
    assert!(refused.contains("beside the path"), "{refused}");
}

/// A sweep job with the way its profile is held and its corners are made.
fn sweep_as(
    profile: KernelProfileDto,
    path: Vec<KernelCurveDto>,
    orientation: SweepOrientation,
    transition: SweepTransition,
) -> KernelJobDto {
    let mut job = sweep(profile, path);
    if let KernelJobDto::Sweep(job) = &mut job {
        job.orientation = orientation;
        job.transition = transition;
    }
    job
}

#[test]
fn a_bar_swept_round_a_corner() {
    // Up ten, then eight across: a right-angled corner, turning towards
    // +x. The bar is 4 across the turn and 2 along the line it turns about.
    let corner = |across: f64, rise: f64| {
        vec![
            KernelCurveDto::Line {
                entity_id: 21,
                start: at(0.0, 0.0, 0.0),
                end: at(0.0, 0.0, 10.0),
            },
            KernelCurveDto::Line {
                entity_id: 22,
                start: at(0.0, 0.0, 10.0),
                end: at(across, 0.0, 10.0 + rise),
            },
        ]
    };
    let weigh = |body: &Body| limo_cad_kernel::props::mass(body).volume;
    // Sharp: both legs run on until they meet. Each is the bar cut on the
    // slant through the middle of its end, which weighs what it would cut
    // square.
    for transition in [SweepTransition::RightCorner, SweepTransition::Transformed] {
        let sharp = built(vec![sweep_as(
            rectangle(4.0, 2.0, 0.0),
            corner(8.0, 0.0),
            SweepOrientation::CorrectedFrenet,
            transition,
        )]);
        assert!(
            (weigh(&sharp) - 8.0 * 18.0).abs() < 1e-9,
            "{}",
            weigh(&sharp)
        );
        assert_eq!(count(&sharp, SurfKind::Plane), sharp.face_count());
    }
    // Round: the outer half of the bar swung a quarter turn about the
    // corner, in place of the square the two legs share on the inside.
    let round = built(vec![sweep_as(
        rectangle(4.0, 2.0, 0.0),
        corner(8.0, 0.0),
        SweepOrientation::CorrectedFrenet,
        SweepTransition::RoundCorner,
    )]);
    let exact = 8.0 * 18.0 - 2.0 * 16.0 / 4.0 + 0.5 * PI * 2.0 * 16.0 / 8.0;
    assert!((weigh(&round) - exact).abs() < 1e-9, "{}", weigh(&round));
    assert_eq!(count(&round, SurfKind::Cylinder), 1);
    // A corner of another angle, either way.
    for (across, rise) in [(8.0, 5.0), (8.0, -4.0), (-6.0, 3.0)] {
        for transition in [SweepTransition::RightCorner, SweepTransition::RoundCorner] {
            let body = built(vec![sweep_as(
                rectangle(4.0, 2.0, 0.0),
                corner(across, rise),
                SweepOrientation::CorrectedFrenet,
                transition,
            )]);
            let legs = 8.0 * (10.0 + (across * across + rise * rise).sqrt());
            if transition == SweepTransition::RightCorner {
                assert!((weigh(&body) - legs).abs() < 1e-9, "{}", weigh(&body));
            } else {
                assert!(weigh(&body) < legs && weigh(&body) > 0.9 * legs);
            }
        }
    }
    // A tube round a sharp corner keeps its bore.
    let mut tube = rectangle(4.0, 2.0, 0.0);
    tube.holes.push(circle(0.5, 0.0));
    let piped = built(vec![sweep_as(
        tube,
        corner(8.0, 0.0),
        SweepOrientation::CorrectedFrenet,
        SweepTransition::RightCorner,
    )]);
    let exact = (8.0 - PI * 0.25) * 18.0;
    assert!((weigh(&piped) - exact).abs() < 1e-9, "{}", weigh(&piped));
    // Kept facing one way, the bar just goes where the path goes.
    let level = built(vec![sweep_as(
        rectangle(4.0, 2.0, 0.0),
        corner(8.0, 6.0),
        SweepOrientation::Fixed,
        SweepTransition::Transformed,
    )]);
    assert!(
        (weigh(&level) - 8.0 * 16.0).abs() < 1e-9,
        "{}",
        weigh(&level)
    );
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
        face_marks: Vec::new(),
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
        face_mark: None,
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

/// A circle of `radius` about `centre`, flat against `normal`.
fn circle_at(radius: f64, centre: [f64; 3], normal: [f64; 3]) -> KernelProfileDto {
    use limo_cad_kernel::{v3, Frame};
    let frame = Frame::from_z(
        v3(centre[0], centre[1], centre[2]),
        v3(normal[0], normal[1], normal[2]),
    );
    let on = |a: f64| {
        let p = frame.o + (frame.x * a.cos() + frame.y * a.sin()) * radius;
        at(p.x, p.y, p.z)
    };
    KernelProfileDto {
        profile_index: 0,
        points: (0..24).map(|i| on(2.0 * PI * i as f64 / 24.0)).collect(),
        curves: vec![KernelCurveDto::Circle {
            entity_id: 7,
            center: at(centre[0], centre[1], centre[2]),
            axis_point: on(0.0),
            normal: at(frame.z.x, frame.z.y, frame.z.z),
        }],
        holes: Vec::new(),
    }
}

#[test]
fn a_loft_along_a_centerline() {
    let steered = |sections, centerline, rail| {
        let mut job = loft(sections, false);
        if let KernelJobDto::Loft(job) = &mut job {
            job.centerline = centerline;
            job.guide_rail = rail;
        }
        job
    };
    // Two equal rings at the ends of a quarter bend: a quarter of a ring
    // doughnut, which holds the ring's area times the way its middle goes.
    let (sin, cos) = (PI / 4.0).sin_cos();
    let bend = vec![KernelCurveDto::Arc {
        entity_id: 21,
        start: at(0.0, 0.0, 0.0),
        mid: at(30.0 - 30.0 * cos, 0.0, 30.0 * sin),
        end: at(30.0, 0.0, 30.0),
    }];
    let elbow = built(vec![steered(
        vec![
            circle_at(5.0, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            circle_at(5.0, [30.0, 0.0, 30.0], [1.0, 0.0, 0.0]),
        ],
        bend.clone(),
        Vec::new(),
    )]);
    let weighed = limo_cad_kernel::props::mass(&elbow);
    let exact = PI * 25.0 * 30.0 * PI / 2.0;
    assert!(
        (weighed.volume - exact).abs() < 2.0e-5 * exact,
        "{} {exact}",
        weighed.volume
    );
    // Without the centerline the same two rings are joined straight across
    // the corner, which is a good deal less.
    let straight = built(vec![loft(
        vec![
            circle_at(5.0, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            circle_at(5.0, [30.0, 0.0, 30.0], [1.0, 0.0, 0.0]),
        ],
        false,
    )]);
    assert!(limo_cad_kernel::props::mass(&straight).volume < 0.95 * exact);
    // A ring that grows on the way round: every station between is a ring
    // of the size in between, so the elbow holds what a cone bent round
    // holds, to the second order in how thick it is.
    let flared = built(vec![steered(
        vec![
            circle_at(4.0, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            circle_at(6.0, [30.0, 0.0, 30.0], [1.0, 0.0, 0.0]),
        ],
        bend.clone(),
        Vec::new(),
    )]);
    let exact = PI * (16.0 + 24.0 + 36.0) / 3.0 * 30.0 * PI / 2.0;
    let weighed = limo_cad_kernel::props::mass(&flared).volume;
    assert!(
        (weighed - exact).abs() < 2.0e-3 * exact,
        "{weighed} {exact}"
    );
    // Three sections along a line and a bend, the middle one where the
    // two join: the skin passes through all three.
    let reach = vec![
        KernelCurveDto::Line {
            entity_id: 20,
            start: at(0.0, 0.0, -20.0),
            end: at(0.0, 0.0, 0.0),
        },
        bend[0].clone(),
    ];
    let necked = built(vec![steered(
        vec![
            circle_at(5.0, [0.0, 0.0, -20.0], [0.0, 0.0, 1.0]),
            circle_at(3.0, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            circle_at(5.0, [30.0, 0.0, 30.0], [1.0, 0.0, 0.0]),
        ],
        reach,
        Vec::new(),
    )]);
    let bounds = limo_cad_kernel::props::bounds(&necked);
    assert!((bounds.lo.z + 20.0).abs() < 1e-9 && (bounds.hi.x - 30.0).abs() < 1e-9);
    let waist = limo_cad_kernel::boolean(
        &necked,
        &limo_cad_kernel::make::block(
            limo_cad_kernel::v3(-10.0, -10.0, -0.01),
            limo_cad_kernel::v3(10.0, 10.0, 0.01),
        )
        .unwrap(),
        limo_cad_kernel::Op::Intersect,
    )
    .unwrap();
    let slice = limo_cad_kernel::props::mass(&waist).volume / 0.02;
    assert!((slice - PI * 9.0).abs() < 1e-3 * PI * 9.0, "{slice}");
}

#[test]
fn a_loft_steered_by_a_rail() {
    // Two equal rings one above the other, and a rail beside them that
    // swells out halfway up: the loft swells with it.
    let (r, height, swell) = (4.0, 40.0, 1.5);
    let rail = vec![KernelCurveDto::Arc {
        entity_id: 31,
        start: at(r, 0.0, 0.0),
        mid: at(r * swell, 0.0, 0.5 * height),
        end: at(r, 0.0, height),
    }];
    let mut job = loft(vec![circle(r, 0.0), circle(r, height)], false);
    if let KernelJobDto::Loft(job) = &mut job {
        job.guide_rail = rail;
    }
    let barrel = built(vec![job]);
    let bounds = limo_cad_kernel::props::bounds(&barrel);
    assert!(
        (bounds.hi.x - r * swell).abs() < 1e-3 && (bounds.lo.x + r * swell).abs() < 1e-3,
        "{bounds:?}"
    );
    // It holds more than the rod between the rings and less than a rod as
    // wide as the swell.
    let weighed = limo_cad_kernel::props::mass(&barrel).volume;
    let rod = PI * r * r * height;
    assert!(
        weighed > 1.3 * rod && weighed < swell * swell * rod,
        "{weighed}"
    );
}
