//! Corners, measured on both kernels.
//!
//! A shell, a round and a bevel are each exact along an edge on either
//! kernel: an offset plane, a cylinder about a line, a flat. Where they can
//! go wrong is at a corner, in how two or three of them are put together,
//! and a corner put together wrongly still makes a closed solid that looks
//! like the one asked for. It weighs differently, though. So every case
//! here is built on both kernels and weighed exactly, and the Limo kernel
//! has either to agree with Open CASCADE to a part in a hundred million or
//! to refuse the feature and say why. Close is not a result.
//!
//! Where the volume can be worked out by hand it is checked against that
//! as well, so that the two kernels are not merely wrong together. In a
//! few places that shows Open CASCADE to be the one that is out, on the
//! spline sides its tapered extrusions have. There this kernel is held to
//! the value worked out, and how far Open CASCADE is from it is written
//! down beside it.
//!
//! Open CASCADE is weighed by [`OcctKernel::weigh`], which integrates
//! until twelve digits stand. What its measure tool gives in one pass is
//! out in the seventh digit on a body with spline faces, which is more
//! than some of the differences looked for here.
#![cfg(feature = "native-occt")]

use limo_cad_core::{BodyId, FeatureId};
use limo_cad_occt::OcctKernel;
use limo_cad_solid::{
    ExtrudeOperation, KernelChamferJobDto, KernelCurveDto, KernelExtrudeJobDto, KernelFilletJobDto,
    KernelJobDto, KernelProfileDto, KernelSceneDto, KernelShellJobDto, Point3Dto, RecomputePlanDto,
};
use std::f64::consts::PI;

fn at(x: f64, y: f64, z: f64) -> Point3Dto {
    Point3Dto { x, y, z }
}

fn outline(corners: &[(f64, f64)]) -> KernelProfileDto {
    let n = corners.len();
    KernelProfileDto {
        profile_index: 0,
        points: corners.iter().map(|&(x, y)| at(x, y, 0.0)).collect(),
        curves: (0..n)
            .map(|i| KernelCurveDto::Line {
                entity_id: i as u64 + 1,
                start: at(corners[i].0, corners[i].1, 0.0),
                end: at(corners[(i + 1) % n].0, corners[(i + 1) % n].1, 0.0),
            })
            .collect(),
        holes: Vec::new(),
    }
}

fn rectangle(x0: f64, x1: f64, y0: f64, y1: f64) -> KernelProfileDto {
    outline(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

/// A regular outline of `n` sides about the origin, turned a little so
/// that none of its sides lies along an axis.
fn sided(n: usize, r: f64) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / n as f64 + 0.3;
            (r * a.cos(), r * a.sin())
        })
        .collect()
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

/// A D: three straight sides and half a circle, which they run into
/// smoothly.
fn dee() -> KernelProfileDto {
    let line = |id: u64, a: (f64, f64), b: (f64, f64)| KernelCurveDto::Line {
        entity_id: id,
        start: at(a.0, a.1, 0.0),
        end: at(b.0, b.1, 0.0),
    };
    KernelProfileDto {
        profile_index: 0,
        points: vec![
            at(-8.0, -6.0, 0.0),
            at(4.0, -6.0, 0.0),
            at(10.0, 0.0, 0.0),
            at(4.0, 6.0, 0.0),
            at(-8.0, 6.0, 0.0),
        ],
        curves: vec![
            line(1, (-8.0, -6.0), (4.0, -6.0)),
            KernelCurveDto::Arc {
                entity_id: 2,
                start: at(4.0, -6.0, 0.0),
                mid: at(10.0, 0.0, 0.0),
                end: at(4.0, 6.0, 0.0),
            },
            line(3, (4.0, 6.0), (-8.0, 6.0)),
            line(4, (-8.0, 6.0), (-8.0, -6.0)),
        ],
        holes: Vec::new(),
    }
}

/// A disc of radius 10 with a slice off it at `x = 4`: the end of a shaft
/// with a flat on it. Its arc and its straight side meet at two corners.
fn sliced() -> KernelProfileDto {
    let half = 84.0f64.sqrt();
    KernelProfileDto {
        profile_index: 0,
        points: vec![at(4.0, -half, 0.0), at(4.0, half, 0.0), at(-10.0, 0.0, 0.0)],
        curves: vec![
            KernelCurveDto::Line {
                entity_id: 1,
                start: at(4.0, -half, 0.0),
                end: at(4.0, half, 0.0),
            },
            KernelCurveDto::Arc {
                entity_id: 2,
                start: at(4.0, half, 0.0),
                mid: at(-10.0, 0.0, 0.0),
                end: at(4.0, -half, 0.0),
            },
        ],
        holes: Vec::new(),
    }
}

fn extruded(
    feature: u64,
    profile: KernelProfileDto,
    operation: ExtrudeOperation,
    from: f64,
    to: f64,
    taper: f64,
) -> KernelJobDto {
    let onto = !matches!(operation, ExtrudeOperation::NewBody);
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(feature),
        operation,
        require_single_solid: false,
        source_face: None,
        profiles: vec![profile],
        normal: at(0.0, 0.0, 1.0),
        start_offset: from,
        end_offset: to,
        taper_angle_deg: taper,
        target_body_ids: if onto { vec![BodyId(1)] } else { Vec::new() },
        result_body_ids: vec![BodyId(1)],
    })
}

/// The plate everything stands on: 40 by 25 by 10, or 60 square.
fn block() -> KernelJobDto {
    extruded(
        1,
        rectangle(0.0, 40.0, 0.0, 25.0),
        ExtrudeOperation::NewBody,
        0.0,
        10.0,
        0.0,
    )
}

fn wide() -> KernelJobDto {
    extruded(
        1,
        rectangle(-30.0, 30.0, -30.0, 30.0),
        ExtrudeOperation::NewBody,
        0.0,
        10.0,
        0.0,
    )
}

/// Something standing 10 above the plate.
fn boss(profile: KernelProfileDto) -> KernelJobDto {
    extruded(2, profile, ExtrudeOperation::Join, 0.0, 20.0, 0.0)
}

/// The block with a boss 20 by 10 on it.
fn stepped() -> Vec<KernelJobDto> {
    vec![block(), boss(rectangle(10.0, 30.0, 8.0, 18.0))]
}

/// The block with a pocket 20 by 10 and 6 deep cut into it from above.
fn pocketed() -> Vec<KernelJobDto> {
    vec![
        block(),
        extruded(
            2,
            rectangle(10.0, 30.0, 8.0, 18.0),
            ExtrudeOperation::Cut,
            4.0,
            10.0,
            0.0,
        ),
    ]
}

fn plan(jobs: Vec<KernelJobDto>) -> RecomputePlanDto {
    RecomputePlanDto {
        transaction_id: 1,
        errors: vec![],
        jobs,
    }
}

/// The flat faces of the one body that look straight down from `z = 0`.
fn underside(scene: &KernelSceneDto) -> Vec<String> {
    scene.bodies[0]
        .faces
        .iter()
        .filter(|f| {
            f.plane
                .as_ref()
                .is_some_and(|p| p.normal[2] < -1.0 + 1e-9 && p.origin[2].abs() < 1e-9)
        })
        .map(|f| f.key.clone())
        .collect()
}

/// The straight edge of the one body whose middle is nearest `m`.
fn edge(scene: &KernelSceneDto, m: [f64; 3]) -> String {
    let off = |e: &limo_cad_solid::KernelEdgeDto| {
        let mid = [
            0.5 * (e.points[0].x + e.points[1].x),
            0.5 * (e.points[0].y + e.points[1].y),
            0.5 * (e.points[0].z + e.points[1].z),
        ];
        (0..3).map(|k| (mid[k] - m[k]).powi(2)).sum::<f64>()
    };
    let nearest = scene.bodies[0]
        .edges
        .iter()
        .filter(|e| e.points.len() == 2)
        .min_by(|a, b| off(a).total_cmp(&off(b)))
        .expect("a straight edge");
    assert!(
        off(nearest) < 1e-12,
        "no straight edge with its middle at {m:?}"
    );
    nearest.key.clone()
}

/// Every edge of the one body at the height `z`.
fn level(scene: &KernelSceneDto, z: f64) -> Vec<String> {
    scene.bodies[0]
        .edges
        .iter()
        .filter(|e| e.points.iter().all(|p| (p.z - z).abs() < 1e-9))
        .map(|e| e.key.clone())
        .collect()
}

fn every(scene: &KernelSceneDto) -> Vec<String> {
    scene.bodies[0]
        .edges
        .iter()
        .map(|e| e.key.clone())
        .collect()
}

fn shell(keys: Vec<String>, inward: bool) -> KernelJobDto {
    KernelJobDto::Shell(KernelShellJobDto {
        feature_id: FeatureId(9),
        target_body_id: BodyId(1),
        face_keys: keys,
        face_marks: Vec::new(),
        thickness: 2.0,
        inward,
    })
}

fn round(keys: Vec<String>) -> KernelJobDto {
    KernelJobDto::Fillet(KernelFilletJobDto {
        feature_id: FeatureId(9),
        target_body_id: BodyId(1),
        edge_keys: keys,
        edge_marks: Vec::new(),
        radius: 2.0,
        tangent_chain: false,
    })
}

fn bevel(keys: Vec<String>) -> KernelJobDto {
    KernelJobDto::Chamfer(KernelChamferJobDto {
        feature_id: FeatureId(9),
        target_body_id: BodyId(1),
        edge_keys: keys,
        edge_marks: Vec::new(),
        distance: 2.0,
        angle_deg: None,
        tangent_chain: false,
    })
}

/// What a kernel made of a case: what the body holds, or why not.
type Made = Result<f64, String>;

/// Build `base` on both kernels, then the feature each is told to make of
/// its own scene. The reference kernel first.
fn both(
    base: Vec<KernelJobDto>,
    feature: &dyn Fn(&KernelSceneDto) -> KernelJobDto,
) -> (Made, Made) {
    let mut reference = OcctKernel::new().unwrap();
    let theirs = reference.recompute(&plan(base.clone())).unwrap();
    assert!(theirs.errors.is_empty(), "{:?}", theirs.errors);
    let mut limo = limo_cad_kernel::Kernel::new();
    let ours = limo.recompute(&plan(base.clone()));
    assert!(ours.errors.is_empty(), "{:?}", ours.errors);
    let mut jobs = base.clone();
    jobs.push(feature(&theirs));
    let scene = reference.recompute(&plan(jobs)).unwrap();
    let theirs = match scene.errors.first() {
        Some(error) => Err(error.message.clone()),
        None => Ok(reference.weigh(BodyId(1)).unwrap()),
    };
    let mut jobs = base;
    jobs.push(feature(&ours));
    let scene = limo.recompute(&plan(jobs));
    let ours = match scene.errors.first() {
        Some(error) => Err(error.message.clone()),
        None => Ok(limo_cad_kernel::props::mass(limo.body(1).unwrap()).volume),
    };
    (theirs, ours)
}

/// What the Limo kernel is to make of a case.
enum Wanted {
    /// The same solid as the reference.
    Same,
    /// That, and this is what it holds.
    Exactly(f64),
    /// A refusal that says this. Open CASCADE builds these; the Limo kernel
    /// cannot yet, and does not hand back something near.
    Refused(&'static str),
    /// A solid holding this, where the reference kernel gives up.
    Alone(f64),
    /// A solid holding the first, worked out by hand, where the reference
    /// kernel makes one that holds the second more.
    Unlike(f64, f64),
}
use Wanted::*;

/// A part in a hundred million. Where an angle is not a right angle the
/// reference kernel's own figure moves in the ninth place.
const CLOSE: f64 = 1.0e-8;

fn check(
    name: &str,
    base: Vec<KernelJobDto>,
    feature: &dyn Fn(&KernelSceneDto) -> KernelJobDto,
    wanted: Wanted,
) {
    let (theirs, ours) = both(base, feature);
    let near = |a: f64, b: f64| (a - b).abs() <= CLOSE * b.abs();
    match wanted {
        Same | Exactly(_) => {
            let theirs = theirs.unwrap_or_else(|e| panic!("{name}: the reference refused: {e}"));
            let ours = ours.unwrap_or_else(|e| panic!("{name}: refused: {e}"));
            assert!(
                near(ours, theirs),
                "{name}: {ours:.6} where the reference has {theirs:.6}, {:+.6} apart",
                ours - theirs
            );
            if let Exactly(exact) = wanted {
                assert!(
                    near(ours, exact),
                    "{name}: {ours:.6} where it is {exact:.6}"
                );
            }
        }
        Refused(why) => {
            assert!(
                theirs.is_ok(),
                "{name}: the reference refused too: {theirs:?}"
            );
            match ours {
                Err(said) => assert!(said.contains(why), "{name}: refused with {said:?}"),
                Ok(ours) => panic!(
                    "{name}: built as {ours:.6}; the reference has {:.6}",
                    theirs.unwrap()
                ),
            }
        }
        Alone(exact) => {
            assert!(
                theirs.is_err(),
                "{name}: the reference builds it now: {theirs:?}"
            );
            let ours = ours.unwrap_or_else(|e| panic!("{name}: refused: {e}"));
            assert!(
                near(ours, exact),
                "{name}: {ours:.6} where it is {exact:.6}"
            );
        }
        Unlike(exact, more) => {
            let theirs = theirs.unwrap_or_else(|e| panic!("{name}: the reference refused: {e}"));
            let ours = ours.unwrap_or_else(|e| panic!("{name}: refused: {e}"));
            assert!(
                near(ours, exact),
                "{name}: {ours:.6} where it is {exact:.6}"
            );
            assert!(
                (theirs - exact - more).abs() < 2.0e-5,
                "{name}: the reference is {:+.6} from {exact:.6}, not {more:+.6}",
                theirs - exact
            );
        }
    }
}

/// How much of a square of side `r` a quarter disc of radius `r` leaves:
/// what a round takes off an outside edge, or fills an inside one with, for
/// each unit of the edge's length.
fn sliver(r: f64) -> f64 {
    (1.0 - PI / 4.0) * r * r
}

#[test]
fn a_wall_is_the_thickness_asked_for_at_every_corner() {
    let inward = |scene: &KernelSceneDto| shell(underside(scene), true);
    let outward = |scene: &KernelSceneDto| shell(underside(scene), false);
    let (t, tt) = (2.0f64, 4.0f64);

    // A plain box hollowed out: nothing to go round.
    check(
        "block, hollowed",
        vec![block()],
        &inward,
        Exactly(10000.0 - 36.0 * 21.0 * 8.0),
    );

    // A boss on a plate. Round the foot of the boss the wall has to turn
    // an inside corner, and it does so on an arc about the foot's edge:
    // every point of the new side is 2 from the old one, and directly
    // under the edge the nearest point of the old one is the edge. At each
    // corner of the boss two of those arcs run into each other.
    //
    // The hollow: 8 high under the plate outside the boss's outline; 18
    // high inside the outline and more than 2 from it; and in the band
    // between, at `s` from the outline, short of the plate's top by
    // sqrt(4 - s^2). The outline at `s` inside a rectangle 20 by 10 is
    // 60 - 8s long.
    let band = 10.0 * (60.0 * t - 4.0 * tt) - (60.0 * PI * tt / 4.0 - 8.0 * t.powi(3) / 3.0);
    let hollow = (36.0 * 21.0 - 200.0) * 8.0 + 16.0 * 6.0 * 18.0 + band;
    check(
        "boss on a plate, hollowed",
        stepped(),
        &inward,
        Exactly(12000.0 - hollow),
    );

    // The same with the boss's outline turning the other way at one
    // corner: three arcs meet there, round a ball about the corner.
    let ell = outline(&[
        (10.0, 6.0),
        (30.0, 6.0),
        (30.0, 12.0),
        (20.0, 12.0),
        (20.0, 20.0),
        (10.0, 20.0),
    ]);
    check(
        "L-shaped boss, hollowed",
        vec![block(), boss(ell)],
        &inward,
        Same,
    );
    // A pocket instead of a boss: its four floor corners are such corners.
    check("pocketed block, hollowed", pocketed(), &inward, Same);
    // Outlines whose corners are not square.
    for n in [5, 6] {
        let base = || vec![wide(), boss(outline(&sided(n, 12.0)))];
        check(&format!("{n}-sided boss, hollowed"), base(), &inward, Same);
        check(
            &format!("{n}-sided boss, wall outside"),
            base(),
            &outward,
            Same,
        );
    }
    check(
        "3-sided boss, wall outside",
        vec![wide(), boss(outline(&sided(3, 14.0)))],
        &outward,
        Same,
    );
    let cross = outline(&[
        (-4.0, -14.0),
        (4.0, -14.0),
        (4.0, -4.0),
        (14.0, -4.0),
        (14.0, 4.0),
        (4.0, 4.0),
        (4.0, 14.0),
        (-4.0, 14.0),
        (-4.0, 4.0),
        (-14.0, 4.0),
        (-14.0, -4.0),
        (-4.0, -4.0),
    ]);
    check(
        "cross-shaped boss, hollowed",
        vec![wide(), boss(cross)],
        &inward,
        Same,
    );
    check(
        "boss in a corner of the plate, hollowed",
        vec![block(), boss(rectangle(0.0, 20.0, 0.0, 12.0))],
        &inward,
        Same,
    );
    // Round things have no corners to go wrong at.
    check(
        "round boss, hollowed",
        vec![wide(), boss(circle(6.0))],
        &inward,
        Same,
    );
    check(
        "round boss, wall outside",
        vec![wide(), boss(circle(6.0))],
        &outward,
        Same,
    );
    check(
        "D-shaped boss, hollowed",
        vec![wide(), boss(dee())],
        &inward,
        Same,
    );
    check(
        "D-shaped boss, wall outside",
        vec![wide(), boss(dee())],
        &outward,
        Same,
    );
    check(
        "round pocket, hollowed",
        vec![
            wide(),
            extruded(2, circle(8.0), ExtrudeOperation::Cut, 4.0, 10.0, 0.0),
        ],
        &inward,
        Same,
    );
    // A wall added outside goes round outside corners on arcs and balls.
    check("block, wall outside", vec![block()], &outward, Same);
    check("boss on a plate, wall outside", stepped(), &outward, Same);
    check("pocketed block, wall outside", pocketed(), &outward, Same);

    // An L-shaped block with a wall added outside. Open CASCADE gives this
    // one up; it has a closed form. Slice by slice the wall's outline is
    // the L's grown by `d`, round at its five outside corners and sharp in
    // its inside one: area + 130 d + (5 pi / 4 - 1) d^2. It is grown by 2
    // up to the top of the L, and above that by sqrt(4 - s^2) at height s.
    let ell = outline(&[
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 10.0),
        (15.0, 10.0),
        (15.0, 25.0),
        (0.0, 25.0),
    ]);
    let grown = |d: f64, d2: f64| 625.0 + 130.0 * d + (5.0 * PI / 4.0 - 1.0) * d2;
    let above =
        625.0 * t + 130.0 * (PI * tt / 4.0) + (5.0 * PI / 4.0 - 1.0) * (tt * t - t.powi(3) / 3.0);
    check(
        "L-shaped block, wall outside",
        vec![extruded(1, ell, ExtrudeOperation::NewBody, 0.0, 10.0, 0.0)],
        &outward,
        Alone(grown(t, tt) * 10.0 + above - 6250.0),
    );

    // A boss whose sides lean. Each arc of the wall ends on the leaning
    // side next to it, not square to its own edge, and the two cross
    // between those.
    for taper in [-8.0, 8.0] {
        check(
            &format!("boss with sides leaning {taper}, hollowed"),
            vec![
                wide(),
                extruded(
                    2,
                    rectangle(-10.0, 10.0, -6.0, 6.0),
                    ExtrudeOperation::Join,
                    0.0,
                    20.0,
                    taper,
                ),
            ],
            &inward,
            Same,
        );
    }
    // And a sharp corner on the boss, less than a right angle: each arc
    // ends on the side beyond the corner, which leans back over it.
    check(
        "3-sided boss, hollowed",
        vec![wide(), boss(outline(&sided(3, 14.0)))],
        &inward,
        Same,
    );
}

#[test]
fn rounds_and_bevels_meet_at_a_corner_as_the_reference_makes_them() {
    let (r, r3) = (2.0f64, 8.0f64);
    // One edge at a time there is nothing to meet.
    check(
        "one top edge",
        vec![block()],
        &|s: &KernelSceneDto| round(vec![edge(s, [40.0, 12.5, 10.0])]),
        Exactly(10000.0 - sliver(r) * 25.0),
    );
    check(
        "one foot edge of the boss",
        stepped(),
        &|s: &KernelSceneDto| round(vec![edge(s, [30.0, 13.0, 10.0])]),
        Exactly(12000.0 + sliver(r) * 10.0),
    );
    check(
        "one upright edge of the boss",
        stepped(),
        &|s: &KernelSceneDto| round(vec![edge(s, [30.0, 18.0, 15.0])]),
        Exactly(12000.0 - sliver(r) * 10.0),
    );

    // Two rounds along the foot of the boss, round one of its corners.
    // Each fills its own edge in, and where they would stop short of each
    // other they run on and cross: what is filled in the corner is the
    // corner cube less what lies inside either of the two cylinders.
    let crossing = (5.0 / 3.0 - PI / 2.0) * r3;
    let foot = |s: &KernelSceneDto| {
        round(vec![
            edge(s, [30.0, 13.0, 10.0]),
            edge(s, [20.0, 18.0, 10.0]),
        ])
    };
    check(
        "two foot edges of the boss",
        stepped(),
        &foot,
        Exactly(12000.0 + sliver(r) * 30.0 + crossing),
    );
    check(
        "all four foot edges of the boss",
        stepped(),
        &|s: &KernelSceneDto| {
            round(
                level(s, 10.0)
                    .into_iter()
                    .filter(|k| {
                        // The foot of the boss, not the rim of the plate.
                        let e = s.bodies[0].edges.iter().find(|e| &e.key == k).unwrap();
                        e.points
                            .iter()
                            .all(|p| p.x > 5.0 && p.x < 35.0 && p.y > 5.0 && p.y < 20.0)
                    })
                    .collect(),
            )
        },
        Exactly(12000.0 + sliver(r) * 60.0 + 4.0 * crossing),
    );

    // Three rounds in the corner of a pocket: a ball rests in the corner,
    // and what is filled there is the corner cube less the ball.
    let corner = |s: &KernelSceneDto| {
        vec![
            edge(s, [30.0, 13.0, 4.0]),
            edge(s, [20.0, 18.0, 4.0]),
            edge(s, [30.0, 18.0, 7.0]),
        ]
    };
    check(
        "three edges of a pocket corner",
        pocketed(),
        &|s: &KernelSceneDto| round(corner(s)),
        Exactly(8800.0 + sliver(r) * (8.0 + 18.0 + 4.0) + (1.0 - PI / 6.0) * r3),
    );
    check(
        "two floor edges of a pocket",
        pocketed(),
        &|s: &KernelSceneDto| round(corner(s)[..2].to_vec()),
        Same,
    );
    check(
        "the whole floor of a pocket",
        pocketed(),
        &|s: &KernelSceneDto| round(level(s, 4.0)),
        Same,
    );

    // Outside corners.
    let top = |s: &KernelSceneDto| {
        vec![
            edge(s, [40.0, 12.5, 10.0]),
            edge(s, [20.0, 25.0, 10.0]),
            edge(s, [40.0, 25.0, 5.0]),
        ]
    };
    check(
        "two top edges of a block",
        vec![block()],
        &|s: &KernelSceneDto| round(top(s)[..2].to_vec()),
        Same,
    );
    check(
        "three edges of a block corner",
        vec![block()],
        &|s: &KernelSceneDto| round(top(s)),
        Same,
    );
    check(
        "every edge of a block",
        vec![block()],
        &|s: &KernelSceneDto| round(every(s)),
        Same,
    );

    // Corners that are not square: a ball rests in those just the same,
    // and a round that stops at one is cut off by the face it stops at.
    for n in [3usize, 5, 6] {
        let radius = if n == 3 { 14.0 } else { 12.0 };
        let pts = sided(n, radius);
        let base = || vec![wide(), boss(outline(&sided(n, radius)))];
        let mid = move |i: usize, z: f64| {
            let (a, b) = (pts[i % n], pts[(i + 1) % n]);
            [0.5 * (a.0 + b.0), 0.5 * (a.1 + b.1), z]
        };
        let pts = sided(n, radius);
        let up = move |i: usize| [pts[i % n].0, pts[i % n].1, 15.0];
        check(
            &format!("{n} sides: one top edge"),
            base(),
            &|s: &KernelSceneDto| round(vec![edge(s, mid(0, 20.0))]),
            Same,
        );
        check(
            &format!("{n} sides: two top edges"),
            base(),
            &|s: &KernelSceneDto| round(vec![edge(s, mid(0, 20.0)), edge(s, mid(1, 20.0))]),
            Same,
        );
        check(
            &format!("{n} sides: three edges of a top corner"),
            base(),
            &|s: &KernelSceneDto| {
                round(vec![
                    edge(s, mid(0, 20.0)),
                    edge(s, mid(1, 20.0)),
                    edge(s, up(1)),
                ])
            },
            Same,
        );
        check(
            &format!("{n} sides: the top and the uprights"),
            base(),
            &|s: &KernelSceneDto| {
                round(
                    (0..n)
                        .flat_map(|i| [edge(s, mid(i, 20.0)), edge(s, up(i))])
                        .collect(),
                )
            },
            Same,
        );
        if n > 3 {
            check(
                &format!("{n} sides: two foot edges"),
                base(),
                &|s: &KernelSceneDto| round(vec![edge(s, mid(0, 10.0)), edge(s, mid(1, 10.0))]),
                Same,
            );
            check(
                &format!("{n} sides: the whole foot"),
                base(),
                &|s: &KernelSceneDto| round((0..n).map(|i| edge(s, mid(i, 10.0))).collect()),
                Same,
            );
        }
    }
    // Round the foot and the top of a D, its straight sides running into
    // its arc.
    let dee_base = || vec![wide(), boss(dee())];
    check(
        "the top of a D",
        dee_base(),
        &|s: &KernelSceneDto| round(level(s, 20.0)),
        Same,
    );
    check(
        "the foot of a D",
        dee_base(),
        &|s: &KernelSceneDto| {
            round(
                level(s, 10.0)
                    .into_iter()
                    .filter(|k| {
                        let e = s.bodies[0].edges.iter().find(|e| &e.key == k).unwrap();
                        e.points
                            .iter()
                            .all(|p| p.x.abs() < 20.0 && p.y.abs() < 20.0)
                    })
                    .collect(),
            )
        },
        Same,
    );

    // A disc with a slice off it. Round its top the arc and the straight
    // edge do not run on into each other: the arc's round is cut off by
    // the flat, which misses the arc's axis, and the straight edge's by
    // the round side. Slice by slice the two rounds together are the
    // outline less the outline moved in by as much as a round's section
    // is wide there.
    let slice_base = || vec![wide(), boss(sliced())];
    let beyond = |of: f64, at: f64| of * of * (at / of).acos() - at * (of * of - at * at).sqrt();
    let within = |w: f64| PI * (10.0 - w).powi(2) - beyond(10.0 - w, 4.0 - w);
    let taken = integral(0.0, 0.5 * PI, |a| {
        (within(0.0) - within(r * (1.0 - a.sin()))) * r * a.sin()
    });
    check(
        "slice of a disc: round the top",
        slice_base(),
        &|s: &KernelSceneDto| round(level(s, 20.0)),
        Exactly(36000.0 + 10.0 * within(0.0) - taken),
    );
    check(
        "slice of a disc: bevel the top",
        slice_base(),
        &|s: &KernelSceneDto| bevel(level(s, 20.0)),
        Same,
    );

    // Bevels. Two cross as two rounds do; three at a square corner are cut
    // flat, through the three places where two of them meet on a face.
    check(
        "bevel: two foot edges of the boss",
        stepped(),
        &|s: &KernelSceneDto| {
            bevel(vec![
                edge(s, [30.0, 13.0, 10.0]),
                edge(s, [20.0, 18.0, 10.0]),
            ])
        },
        Same,
    );
    check(
        "bevel: two top edges of a block",
        vec![block()],
        &|s: &KernelSceneDto| bevel(top(s)[..2].to_vec()),
        Same,
    );
    check(
        "bevel: three edges of a block corner",
        vec![block()],
        &|s: &KernelSceneDto| bevel(top(s)),
        Same,
    );
    check(
        "bevel: every edge of a block",
        vec![block()],
        &|s: &KernelSceneDto| bevel(every(s)),
        Same,
    );
    check(
        "bevel: three edges of a pocket corner",
        pocketed(),
        &|s: &KernelSceneDto| bevel(corner(s)),
        Same,
    );
    check(
        "bevel: two floor edges of a pocket",
        pocketed(),
        &|s: &KernelSceneDto| bevel(corner(s)[..2].to_vec()),
        Same,
    );

    // What cannot be finished exactly is refused. An inside edge and an
    // outside edge that meet: the reference kernel blends the corner
    // between them with a patch of its own, and the two tools left as they
    // are make a solid a tenth of the radius cubed away from that.
    let said = "an inside edge and an outside edge that meet at a corner";
    check(
        "the rim and the wall of a pocket",
        pocketed(),
        &|s: &KernelSceneDto| {
            round(vec![
                edge(s, [30.0, 13.0, 10.0]),
                edge(s, [30.0, 18.0, 7.0]),
            ])
        },
        Refused(said),
    );
    check(
        "an upright edge of the boss and its foot",
        stepped(),
        &|s: &KernelSceneDto| {
            round(vec![
                edge(s, [30.0, 18.0, 15.0]),
                edge(s, [30.0, 13.0, 10.0]),
            ])
        },
        Refused(said),
    );
    check(
        "every edge of a boss on a plate",
        stepped(),
        &|s: &KernelSceneDto| round(every(s)),
        Refused(said),
    );
    // Round the foot of the slice of a disc. There the plate goes round
    // the outside of the two corners, and the piece that belongs between
    // an arc's round and a straight edge's is not made yet.
    check(
        "slice of a disc: round the foot",
        slice_base(),
        &|s: &KernelSceneDto| {
            round(
                level(s, 10.0)
                    .into_iter()
                    .filter(|k| {
                        let e = s.bodies[0].edges.iter().find(|e| &e.key == k).unwrap();
                        e.points
                            .iter()
                            .all(|p| p.x.abs() < 20.0 && p.y.abs() < 20.0)
                    })
                    .collect(),
            )
        },
        Refused("corner of a kind that cannot be finished"),
    );
    // Three bevels at a corner that is not square.
    let pts = sided(6, 12.0);
    let mid = |i: usize, z: f64| {
        [
            0.5 * (pts[i].0 + pts[i + 1].0),
            0.5 * (pts[i].1 + pts[i + 1].1),
            z,
        ]
    };
    check(
        "bevel: three edges of a six-sided boss's corner",
        vec![wide(), boss(outline(&sided(6, 12.0)))],
        &|s: &KernelSceneDto| {
            bevel(vec![
                edge(s, mid(0, 20.0)),
                edge(s, mid(1, 20.0)),
                edge(s, [pts[1].0, pts[1].1, 15.0]),
            ])
        },
        Refused("corner of a kind that cannot be finished"),
    );
}

/// The integral of `f` from `a` to `b`, by five-point Gauss rules on two
/// hundred pieces.
fn integral(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    const X: [f64; 5] = [
        0.0,
        0.538_469_310_105_683_1,
        -0.538_469_310_105_683_1,
        0.906_179_845_938_664,
        -0.906_179_845_938_664,
    ];
    const W: [f64; 5] = [
        0.568_888_888_888_888_9,
        0.478_628_670_499_366_47,
        0.478_628_670_499_366_47,
        0.236_926_885_056_189_08,
        0.236_926_885_056_189_08,
    ];
    let pieces = 200;
    let step = (b - a) / pieces as f64;
    (0..pieces)
        .map(|i| {
            let mid = a + (i as f64 + 0.5) * step;
            (0..5)
                .map(|k| W[k] * f(mid + 0.5 * step * X[k]))
                .sum::<f64>()
                * 0.5
                * step
        })
        .sum()
}

/// A blend's section beside a face that leans: how far out from the face
/// it is, at a distance from the flat face it starts on.
#[derive(Clone, Copy)]
struct Beside {
    /// How much further in the leaning face is for each unit of distance
    /// from the flat one.
    slope: f64,
    /// The radius of a round, or the distance of a bevel.
    size: f64,
    round: bool,
}

impl Beside {
    /// The distance from the flat face at which the blend runs out.
    fn ends(&self) -> f64 {
        let lean = self.slope.atan();
        if self.round {
            self.size / (0.25 * PI + 0.5 * lean).tan() * lean.cos()
        } else {
            self.size * lean.cos()
        }
    }

    fn width(&self, h: f64) -> f64 {
        if h >= self.ends() {
            return 0.0;
        }
        let lean = self.slope.atan();
        if self.round {
            let r = self.size;
            r / (0.25 * PI + 0.5 * lean).tan() + self.slope * h - (2.0 * r * h - h * h).sqrt()
        } else {
            self.size - h * ((1.0 + lean.sin()) / lean.cos() - self.slope)
        }
    }

    /// The integral of `f(h) * width(h)` over the blend's height. For a
    /// round the height is run through as `r (1 - cos a)`, which takes the
    /// square root out of the end where the round starts.
    fn over(&self, upto: f64, f: impl Fn(f64) -> f64) -> f64 {
        let upto = upto.min(self.ends());
        if self.round {
            let r = self.size;
            integral(0.0, (1.0 - upto / r).acos(), |a| {
                let h = r * (1.0 - a.cos());
                f(h) * self.width(h) * r * a.sin()
            })
        } else {
            integral(0.0, upto, |h| f(h) * self.width(h))
        }
    }
}

/// The boss a tapered extrusion makes of an outline 20 by 12 drawn on the
/// underside of the plate and taken 20 up: the outline is smaller by
/// `lean` of itself for every unit it rises, so its two pairs of sides
/// lean differently and nothing at a corner is the mirror of anything.
/// Leaning in when the taper is less than nothing, hanging over when more.
struct Leaning {
    taper: f64,
    lean: f64,
}

impl Leaning {
    fn of(taper: f64) -> Leaning {
        Leaning {
            taper,
            lean: -taper.to_radians().tan() / 136.0f64.sqrt(),
        }
    }

    fn jobs(&self) -> Vec<KernelJobDto> {
        vec![
            wide(),
            extruded(
                2,
                rectangle(-10.0, 10.0, -6.0, 6.0),
                ExtrudeOperation::Join,
                0.0,
                20.0,
                self.taper,
            ),
        ]
    }

    /// Half the boss's length and half its width at height `z`.
    fn half(&self, z: f64) -> (f64, f64) {
        (10.0 * (1.0 - self.lean * z), 6.0 * (1.0 - self.lean * z))
    }

    /// The plate and what of the boss stands above it.
    fn whole(&self) -> f64 {
        let cube = |z: f64| (1.0 - self.lean * z).powi(3);
        36000.0 + 240.0 * (cube(10.0) - cube(20.0)) / (3.0 * self.lean)
    }

    /// The middles of the edges of one corner: at the foot of the short
    /// side and of the long side (`z` 10) or on top of them (`z` 20).
    fn corner(&self, z: f64) -> ([f64; 3], [f64; 3]) {
        let (a, b) = self.half(z);
        ([a, 0.0, z], [0.0, b, z])
    }

    fn upright(&self) -> [f64; 3] {
        let (a, b) = self.half(15.0);
        [a, b, 15.0]
    }

    /// What blends on the short side, the long side and both change, at
    /// the foot (they fill in) or on top (they take away). Slice by slice
    /// a blend is a strip along its side, as wide as its section is there,
    /// and two at a corner are two strips that cross: at the foot the
    /// corner between them is filled as far out as both reach, on top what
    /// both take is taken once.
    fn changes(&self, top: bool, size: f64, round: bool) -> (f64, f64, f64) {
        let beside = |slope: f64| Beside { slope, size, round };
        let (short, long) = (beside(10.0 * self.lean), beside(6.0 * self.lean));
        // Half the boss's size at a distance from the plate or the top.
        let at = |h: f64| {
            if top {
                self.half(20.0 - h)
            } else {
                self.half(10.0 + h)
            }
        };
        let one = short.over(f64::MAX, |h| 2.0 * at(h).1);
        let other = long.over(f64::MAX, |h| 2.0 * at(h).0);
        let both = short.over(long.ends(), |h| long.width(h));
        if top {
            (-one, -other, -one - other + both)
        } else {
            (one, other, one + other + both)
        }
    }

    /// What a blend on one upright edge takes: its section, for as long as
    /// the edge is.
    fn upright_takes(&self, size: f64, round: bool) -> f64 {
        let (kx, ky) = (10.0 * self.lean, 6.0 * self.lean);
        let between = PI - (kx * ky / ((1.0 + kx * kx) * (1.0 + ky * ky)).sqrt()).acos();
        let section = if round {
            size * size / (0.5 * between).tan() - 0.5 * (PI - between) * size * size
        } else {
            0.5 * size * size * between.sin()
        };
        section * 10.0 * (1.0 + kx * kx + ky * ky).sqrt()
    }
}

/// A bar 20 by 12 pushed up through the plate on a lean of `lean` degrees
/// towards +x. Its sides are flat on both kernels: an extrusion along a
/// leaning line, not a tapered one.
fn leaning_bar(lean: f64) -> KernelJobDto {
    let (sin, cos) = lean.to_radians().sin_cos();
    let corner = |u: f64, v: f64| at(u * cos, v, -u * sin);
    let points = [
        corner(-10.0, -6.0),
        corner(10.0, -6.0),
        corner(10.0, 6.0),
        corner(-10.0, 6.0),
    ];
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(2),
        operation: ExtrudeOperation::Join,
        require_single_solid: false,
        source_face: None,
        profiles: vec![KernelProfileDto {
            profile_index: 0,
            points: points.to_vec(),
            curves: (0..4)
                .map(|i| KernelCurveDto::Line {
                    entity_id: i as u64 + 1,
                    start: points[i],
                    end: points[(i + 1) % 4],
                })
                .collect(),
            holes: Vec::new(),
        }],
        normal: at(sin, 0.0, cos),
        start_offset: 3.0,
        end_offset: 25.0,
        taper_angle_deg: 0.0,
        target_body_ids: vec![BodyId(1)],
        result_body_ids: vec![BodyId(1)],
    })
}

/// Blends beside faces that lean. A blend ends on the face that ends its
/// edge, and where that face leans the end is not square to the edge;
/// where two blends meet at a corner each ends on the other's side, and
/// they cross between those.
///
/// On a bar pushed through the plate on a lean, whose sides are flat on
/// both kernels, the two agree throughout. On a tapered boss they do not
/// quite, and the difference is the reference kernel's: it keeps the sides
/// of its tapered extrusion as splines, and on those it squares two rounds
/// off at a corner and rolls a patch of its own between them where on flat
/// sides it runs them into each other. Every case here has a value that
/// can be had slice by slice, and this kernel is held to that.
#[test]
fn blends_end_on_faces_that_lean() {
    for lean in [8.0f64, 20.0] {
        let base = || vec![wide(), leaning_bar(lean)];
        let (sin, cos) = lean.to_radians().sin_cos();
        // Where the bar's two leaning sides stand on the plate.
        let (over, away) = ((10.0 + 10.0 * sin) / cos, (-10.0 + 10.0 * sin) / cos);
        let foot = |side: usize| {
            [
                [over, 0.0, 10.0],
                [away, 0.0, 10.0],
                [0.5 * (over + away), 6.0, 10.0],
                [0.5 * (over + away), -6.0, 10.0],
            ][side]
        };
        for (name, sides) in [
            ("the foot under the overhang", vec![0]),
            ("the foot of the side that leans away", vec![1]),
            ("the foot of an upright side", vec![2]),
            ("the feet of the overhang and an upright side", vec![0, 2]),
            (
                "the feet of the side leaning away and an upright side",
                vec![1, 2],
            ),
            ("all four feet", vec![0, 1, 2, 3]),
        ] {
            for rounded in [true, false] {
                let kind = if rounded { "round" } else { "bevel" };
                check(
                    &format!("bar leaning {lean}, {kind}: {name}"),
                    base(),
                    &|s: &KernelSceneDto| {
                        let keys = sides.iter().map(|&side| edge(s, foot(side))).collect();
                        if rounded {
                            round(keys)
                        } else {
                            bevel(keys)
                        }
                    },
                    Same,
                );
            }
        }
        // The bar's own corner edges lean with it. A round on one takes a
        // sliver as long as the line through the sliver's middle is, from
        // the plate's top to the bar's end.
        let mut limo = limo_cad_kernel::Kernel::new();
        assert!(limo.recompute(&plan(base())).errors.is_empty());
        let whole = limo_cad_kernel::props::mass(limo.body(1).unwrap()).volume;
        let middle = 2.0 * (5.0 / 6.0 - PI / 4.0) / (1.0 - PI / 4.0);
        let long = |u: f64| 25.0 - (10.0 + u * sin) / cos;
        for u in [10.0f64, -10.0] {
            // The edge runs from the plate's top to the bar's end.
            let halfway = 0.5 * ((10.0 + u * sin) / cos + 25.0);
            let at = [u * cos + halfway * sin, 6.0, halfway * cos - u * sin];
            check(
                &format!("bar leaning {lean}: a corner edge"),
                base(),
                &|s: &KernelSceneDto| round(vec![edge(s, at)]),
                Exactly(whole - sliver(2.0) * long(u - u.signum() * middle)),
            );
        }
    }
    check(
        "bar leaning 8, hollowed",
        vec![wide(), leaning_bar(8.0)],
        &|s: &KernelSceneDto| shell(underside(s), true),
        Same,
    );

    for taper in [-8.0f64, 8.0] {
        let boss = Leaning::of(taper);
        let whole = boss.whole();
        let leans_in = taper < 0.0;
        for rounded in [true, false] {
            let kind = if rounded { "round" } else { "bevel" };
            let blend = |keys: Vec<String>| {
                if rounded {
                    round(keys)
                } else {
                    bevel(keys)
                }
            };
            for top in [false, true] {
                let place = if top { "on top" } else { "at the foot" };
                let (short, long) = boss.corner(if top { 20.0 } else { 10.0 });
                let (one, other, both) = boss.changes(top, 2.0, rounded);
                check(
                    &format!("taper {taper}, {kind} {place}: the short side"),
                    boss.jobs(),
                    &|s: &KernelSceneDto| blend(vec![edge(s, short)]),
                    Exactly(whole + one),
                );
                check(
                    &format!("taper {taper}, {kind} {place}: the long side"),
                    boss.jobs(),
                    &|s: &KernelSceneDto| blend(vec![edge(s, long)]),
                    Exactly(whole + other),
                );
                // Two at a corner. At the foot the reference kernel rolls
                // its patch between two rounds and gives two bevels up;
                // on top it crosses them as this kernel does, but for two
                // bevels on a boss that hangs over.
                let exact = whole + both;
                let wanted = match (top, rounded, leans_in) {
                    (false, true, true) => Unlike(exact, -0.121_869),
                    (false, true, false) => Unlike(exact, -0.100_542),
                    (false, false, _) => Alone(exact),
                    (true, false, false) => Unlike(exact, 0.002_074),
                    (true, ..) => Exactly(exact),
                };
                check(
                    &format!("taper {taper}, {kind} {place}: both"),
                    boss.jobs(),
                    &|s: &KernelSceneDto| blend(vec![edge(s, short), edge(s, long)]),
                    wanted,
                );
            }
            // An upright edge. Under a boss that hangs over, the
            // reference kernel stops the blend a little short.
            let exact = whole - boss.upright_takes(2.0, rounded);
            check(
                &format!("taper {taper}, {kind}: an upright edge"),
                boss.jobs(),
                &|s: &KernelSceneDto| blend(vec![edge(s, boss.upright())]),
                match (leans_in, rounded) {
                    (true, _) => Exactly(exact),
                    (false, true) => Unlike(exact, 0.000_415),
                    (false, false) => Unlike(exact, 0.000_419),
                },
            );
        }
    }
}

/// A blend stops where its edge does. It is not carried on past the end
/// with nothing to stop it: a second boss a millimetre on, in line with
/// the edge, is left as it is.
#[test]
fn a_blend_stops_where_its_edge_does() {
    let second = |height: f64| {
        extruded(
            3,
            rectangle(11.0, 25.0, -6.0, 6.0),
            ExtrudeOperation::Join,
            0.0,
            10.0 + height,
            0.0,
        )
    };
    for height in [10.0, 9.0] {
        let base = || {
            vec![
                wide(),
                boss(rectangle(-10.0, 10.0, -6.0, 6.0)),
                second(height),
            ]
        };
        let whole = 36000.0 + 2400.0 + 14.0 * 12.0 * height;
        check(
            &format!("a round in line with a boss {height} high"),
            base(),
            &|s: &KernelSceneDto| round(vec![edge(s, [0.0, -6.0, 20.0])]),
            Exactly(whole - sliver(2.0) * 20.0),
        );
        check(
            &format!("a bevel in line with a boss {height} high"),
            base(),
            &|s: &KernelSceneDto| bevel(vec![edge(s, [0.0, -6.0, 20.0])]),
            Exactly(whole - 2.0 * 20.0),
        );
    }
}
