//! References that survive edits earlier in the history.
//!
//! A feature keeps the names of the faces and edges it was applied to. The
//! tests here change what comes before such a feature (a hole added, the
//! sketch resized, a sketch line inserted) and replay: the feature must land
//! on the same place as before, or say that its reference is gone. It must
//! never quietly move to another edge.
#![cfg(feature = "dto")]

use limo_cad_core::{BodyId, FeatureId};
use limo_cad_kernel::Kernel;
use limo_cad_solid::{
    ExtrudeOperation, HoleBottomStyle, HoleExtent, HoleStyle, KernelBodyDto, KernelChamferJobDto,
    KernelCurveDto, KernelExtrudeJobDto, KernelFilletJobDto, KernelHoleJobDto, KernelJobDto,
    KernelProfileDto, KernelSceneDto, Point3Dto, RecomputePlanDto,
};

fn at(x: f64, y: f64, z: f64) -> Point3Dto {
    Point3Dto { x, y, z }
}

/// A closed outline in the plane `z = 0` through the given corners. Each
/// side is a sketch line with the entity number given for its first corner.
fn outline(corners: &[(u64, f64, f64)]) -> KernelProfileDto {
    let n = corners.len();
    KernelProfileDto {
        profile_index: 0,
        points: corners.iter().map(|&(_, x, y)| at(x, y, 0.0)).collect(),
        curves: (0..n)
            .map(|i| {
                let (entity_id, x0, y0) = corners[i];
                let (_, x1, y1) = corners[(i + 1) % n];
                KernelCurveDto::Line {
                    entity_id,
                    start: at(x0, y0, 0.0),
                    end: at(x1, y1, 0.0),
                }
            })
            .collect(),
        holes: Vec::new(),
    }
}

/// A plate: entity 11 along the front, 12 up the right, 13 back, 14 down.
fn plate(width: f64, depth: f64) -> KernelProfileDto {
    outline(&[
        (11, 0.0, 0.0),
        (12, width, 0.0),
        (13, width, depth),
        (14, 0.0, depth),
    ])
}

fn extrude(feature: u64, profile: KernelProfileDto, height: f64) -> KernelJobDto {
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(feature),
        operation: ExtrudeOperation::NewBody,
        require_single_solid: false,
        source_face: None,
        profiles: vec![profile],
        normal: at(0.0, 0.0, 1.0),
        start_offset: 0.0,
        end_offset: height,
        taper_angle_deg: 0.0,
        target_body_ids: Vec::new(),
        result_body_ids: vec![BodyId(1)],
    })
}

/// A slot cut down through the front edge of the plate.
fn notch(feature: u64) -> KernelJobDto {
    let mut profile = outline(&[
        (21, 5.0, -2.0),
        (22, 15.0, -2.0),
        (23, 15.0, 6.0),
        (24, 5.0, 6.0),
    ]);
    profile.profile_index = 1;
    KernelJobDto::Extrude(KernelExtrudeJobDto {
        feature_id: FeatureId(feature),
        operation: ExtrudeOperation::Cut,
        require_single_solid: false,
        source_face: None,
        profiles: vec![profile],
        normal: at(0.0, 0.0, 1.0),
        start_offset: -1.0,
        end_offset: 11.0,
        taper_angle_deg: 0.0,
        target_body_ids: vec![BodyId(1)],
        result_body_ids: Vec::new(),
    })
}

fn hole(feature: u64, x: f64, y: f64, top: f64, diameter: f64) -> KernelJobDto {
    KernelJobDto::Hole(KernelHoleJobDto {
        feature_id: FeatureId(feature),
        target_body_id: BodyId(1),
        center: at(x, y, top),
        direction: at(0.0, 0.0, -1.0),
        diameter,
        extent: HoleExtent::ThroughAll,
        style: HoleStyle::Simple,
        counterbore_diameter: 0.0,
        counterbore_depth: 0.0,
        countersink_diameter: 0.0,
        countersink_angle_deg: 90.0,
        bottom_style: HoleBottomStyle::Flat,
        drill_point_angle_deg: 118.0,
        thread: None,
    })
}

fn fillet(feature: u64, edge: &str, radius: f64) -> KernelJobDto {
    KernelJobDto::Fillet(KernelFilletJobDto {
        feature_id: FeatureId(feature),
        target_body_id: BodyId(1),
        edge_keys: vec![edge.to_string()],
        radius,
        tangent_chain: false,
    })
}

fn chamfer(feature: u64, edge: &str, distance: f64) -> KernelJobDto {
    KernelJobDto::Chamfer(KernelChamferJobDto {
        feature_id: FeatureId(feature),
        target_body_id: BodyId(1),
        edge_keys: vec![edge.to_string()],
        distance,
        angle_deg: None,
        tangent_chain: false,
    })
}

fn run(kernel: &mut Kernel, jobs: Vec<KernelJobDto>) -> KernelSceneDto {
    kernel.recompute(&RecomputePlanDto {
        transaction_id: 0,
        jobs,
        errors: Vec::new(),
    })
}

fn body(scene: &KernelSceneDto) -> &KernelBodyDto {
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    &scene.bodies[0]
}

/// Name of the upright edge at the corner `(x, y)`.
fn upright_edge(body: &KernelBodyDto, x: f64, y: f64) -> String {
    let found: Vec<&str> = body
        .edges
        .iter()
        .filter(|e| {
            e.points
                .iter()
                .all(|p| (p.x - x).abs() < 1.0e-6 && (p.y - y).abs() < 1.0e-6)
        })
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(found.len(), 1, "upright edges at ({x}, {y}): {found:?}");
    found[0].to_string()
}

/// Name of the edge along the top of the front side.
fn top_front_edge(body: &KernelBodyDto, height: f64) -> String {
    let found: Vec<&str> = body
        .edges
        .iter()
        .filter(|e| {
            e.points
                .iter()
                .all(|p| p.y.abs() < 1.0e-6 && (p.z - height).abs() < 1.0e-6)
        })
        .map(|e| e.key.as_str())
        .collect();
    assert_eq!(found.len(), 1, "edges along the top front: {found:?}");
    found[0].to_string()
}

/// Whether the body has a round of this radius about the upright axis
/// through `(x, y)`.
fn has_round(body: &KernelBodyDto, x: f64, y: f64, radius: f64) -> bool {
    body.faces.iter().any(|face| {
        face.cylinder.is_some_and(|c| {
            (c.radius - radius).abs() < 1.0e-9
                && c.axis.z.abs() > 0.999
                && (c.origin.x - x).abs() < 1.0e-6
                && (c.origin.y - y).abs() < 1.0e-6
        })
    })
}

#[test]
fn a_round_stays_on_its_edge_when_features_are_added_before_it() {
    let mut kernel = Kernel::new();
    let block = body(&run(&mut kernel, vec![extrude(1, plate(40.0, 30.0), 10.0)])).clone();
    let edge = upright_edge(&block, 40.0, 0.0);

    let rounded = run(
        &mut kernel,
        vec![extrude(1, plate(40.0, 30.0), 10.0), fillet(3, &edge, 3.0)],
    );
    assert!(has_round(body(&rounded), 37.0, 3.0, 3.0));

    // A hole through the plate and a slot in its front edge come in
    // before the round. Both cut up faces the picked edge lies on.
    let earlier = vec![
        extrude(1, plate(40.0, 30.0), 10.0),
        hole(2, 28.0, 15.0, 10.0, 6.0),
        notch(6),
    ];
    let mut jobs = earlier.clone();
    jobs.push(fillet(3, &edge, 3.0));
    let scene = run(&mut kernel, jobs);
    let scene = body(&scene);
    assert!(has_round(scene, 37.0, 3.0, 3.0), "the round moved");
    assert!(scene.faces.iter().any(|f| f
        .cylinder
        .is_some_and(|c| (c.radius - 3.0).abs() < 1.0e-9 && (c.origin.x - 28.0).abs() < 1.0e-6)));

    // The edge has the same name with and without them, though not the
    // same place in the list: its index would now point somewhere else.
    let before = body(&run(&mut kernel, earlier)).clone();
    assert_eq!(upright_edge(&before, 40.0, 0.0), edge);
    let index = |b: &KernelBodyDto| b.edges.iter().position(|e| e.key == edge).unwrap();
    assert_ne!(index(&block), index(&before), "nothing was renumbered");
}

#[test]
fn a_round_follows_its_edge_when_the_sketch_is_resized() {
    let mut kernel = Kernel::new();
    let block = body(&run(&mut kernel, vec![extrude(1, plate(40.0, 30.0), 10.0)])).clone();
    let edge = upright_edge(&block, 40.0, 0.0);
    let wider = run(
        &mut kernel,
        vec![extrude(1, plate(55.0, 30.0), 10.0), fillet(3, &edge, 3.0)],
    );
    assert!(has_round(body(&wider), 52.0, 3.0, 3.0));
}

#[test]
fn a_round_stays_on_its_edge_when_a_sketch_line_is_inserted() {
    let mut kernel = Kernel::new();
    let block = body(&run(&mut kernel, vec![extrude(1, plate(40.0, 30.0), 10.0)])).clone();
    let edge = upright_edge(&block, 40.0, 0.0);

    // The back left corner is cut off by a new line, entity 15, which
    // comes before the line that closes the outline.
    let clipped = outline(&[
        (11, 0.0, 0.0),
        (12, 40.0, 0.0),
        (13, 40.0, 30.0),
        (15, 10.0, 30.0),
        (14, 0.0, 20.0),
    ]);
    let scene = run(
        &mut kernel,
        vec![extrude(1, clipped.clone(), 10.0), fillet(3, &edge, 3.0)],
    );
    assert!(has_round(body(&scene), 37.0, 3.0, 3.0));

    // The other way round: an edge picked on the clipped plate is found on
    // the plain one, as long as both its sides are still there.
    let cut = body(&run(&mut kernel, vec![extrude(1, clipped, 10.0)])).clone();
    let back = upright_edge(&cut, 40.0, 30.0);
    let scene = run(
        &mut kernel,
        vec![extrude(1, plate(40.0, 30.0), 10.0), fillet(3, &back, 3.0)],
    );
    assert!(has_round(body(&scene), 37.0, 27.0, 3.0));
}

#[test]
fn a_reference_to_something_that_is_gone_asks_to_be_reselected() {
    let mut kernel = Kernel::new();
    let block = body(&run(&mut kernel, vec![extrude(1, plate(40.0, 30.0), 10.0)])).clone();
    let edge = upright_edge(&block, 40.0, 0.0);
    // The right side is now a different sketch line: the corner is where it
    // was, but it is not the edge that was picked.
    let redrawn = outline(&[
        (11, 0.0, 0.0),
        (99, 40.0, 0.0),
        (13, 40.0, 30.0),
        (14, 0.0, 30.0),
    ]);
    let scene = run(
        &mut kernel,
        vec![extrude(1, redrawn, 10.0), fillet(3, &edge, 3.0)],
    );
    assert_eq!(scene.errors.len(), 1);
    assert_eq!(scene.errors[0].feature_id, FeatureId(3));
    assert!(
        scene.errors[0].message.contains("reselect"),
        "{}",
        scene.errors[0].message
    );
}

#[test]
fn features_chain_on_the_names_of_earlier_ones() {
    let mut kernel = Kernel::new();
    let base = vec![extrude(1, plate(40.0, 30.0), 10.0)];
    let block = body(&run(&mut kernel, base.clone())).clone();
    let corner = upright_edge(&block, 40.0, 0.0);
    let rim = top_front_edge(&block, 10.0);

    // Round the corner, then chamfer the top front edge, which the round
    // has just shortened.
    let jobs = |extra: Option<KernelJobDto>| {
        let mut jobs = base.clone();
        jobs.extend(extra);
        jobs.push(fillet(3, &corner, 3.0));
        jobs.push(chamfer(4, &rim, 1.0));
        jobs
    };
    let plain = body(&run(&mut kernel, jobs(None))).clone();
    let drilled = body(&run(
        &mut kernel,
        jobs(Some(hole(2, 12.0, 15.0, 10.0, 6.0))),
    ))
    .clone();
    for scene in [&plain, &drilled] {
        assert!(has_round(scene, 37.0, 3.0, 3.0));
        // The chamfer is a flat face leaning 45 degrees over the front.
        assert!(scene.faces.iter().any(|f| f.plane.is_some_and(|p| {
            (p.normal[1] + p.normal[2].abs()).abs() < 1.0e-9 && p.normal[0].abs() < 1.0e-9
        })));
    }
    // Every face the two results share by name is the same kind of face:
    // the hole added its own and renamed none.
    for face in &plain.faces {
        let twin = drilled.faces.iter().find(|f| f.key == face.key);
        let twin = twin.unwrap_or_else(|| panic!("{} is gone", face.key));
        assert_eq!(face.plane.is_some(), twin.plane.is_some());
        assert_eq!(
            face.cylinder.map(|c| c.radius),
            twin.cylinder.map(|c| c.radius)
        );
    }
    assert_eq!(drilled.faces.len(), plain.faces.len() + 1);
}

#[test]
fn an_edge_cut_in_two_is_still_that_edge() {
    let mut kernel = Kernel::new();
    let base = extrude(1, plate(40.0, 30.0), 10.0);
    let block = body(&run(&mut kernel, vec![base.clone()])).clone();
    let rim = top_front_edge(&block, 10.0);
    // A slot through the front now cuts the picked edge in two. The
    // chamfer bevels both pieces.
    let scene = run(&mut kernel, vec![base, notch(6), chamfer(4, &rim, 1.0)]);
    let bevels = body(&scene)
        .faces
        .iter()
        .filter(|f| {
            f.plane.is_some_and(|p| {
                (p.normal[1] + p.normal[2].abs()).abs() < 1.0e-9 && p.normal[0].abs() < 1.0e-9
            })
        })
        .count();
    assert_eq!(bevels, 2);
}

#[test]
fn names_are_unique_and_indices_from_older_documents_still_work() {
    let mut kernel = Kernel::new();
    let jobs = vec![
        extrude(1, plate(40.0, 30.0), 10.0),
        hole(2, 12.0, 15.0, 10.0, 6.0),
        hole(5, 28.0, 15.0, 10.0, 6.0),
    ];
    let scene = body(&run(&mut kernel, jobs.clone())).clone();
    let mut faces: Vec<&str> = scene.faces.iter().map(|f| f.key.as_str()).collect();
    let mut edges: Vec<&str> = scene.edges.iter().map(|e| e.key.as_str()).collect();
    let (nf, ne) = (faces.len(), edges.len());
    faces.sort_unstable();
    faces.dedup();
    edges.sort_unstable();
    edges.dedup();
    assert_eq!((faces.len(), edges.len()), (nf, ne), "a name is used twice");

    // Before names there were indices; a document that kept one is read as
    // it always was.
    let position = scene
        .edges
        .iter()
        .position(|e| e.key == upright_edge(&scene, 40.0, 0.0))
        .unwrap();
    let mut old = jobs;
    old.push(fillet(3, &format!("edge:{position}"), 3.0));
    assert!(has_round(body(&run(&mut kernel, old)), 37.0, 3.0, 3.0));
}
