//! Projected support-face boundary edges.
//!
//! A sketch hosted on a planar body face receives that face's boundary edges as
//! history-stage projected geometry. The boundary seals regions the user draws
//! against it, while a face bounded only by projections never becomes a
//! selectable profile (and never turns drawn geometry nested inside it into a
//! hole).
use limo_cad_core::{BodyId, EdgeId, PlaneBasis};
use limo_cad_sketch::{
    ArcCenterRequest, OriginPlane, PlaneRef, ProjectedEdgeDto, RectangleMode, RectangleRequest,
    SegmentRequest, SketchManager, SketchSession, SnapTarget, Vec2,
};
use limo_cad_solid::{
    CommitKernelRequest, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest, KernelBodyDto,
    KernelEdgeDto, KernelFaceDto, KernelJobDto, KernelSceneDto, Point3Dto,
};

/// Matches `PROJECTED_EDGE_ID_BASE` in the sketch manager: projected boundary
/// ids must stay above every authored entity id.
const RESERVED_ID_FLOOR: u64 = 1 << 40;

const FACE_WIDTH: f64 = 20.0;
const FACE_HEIGHT: f64 = 15.0;
const FACE_Z: f64 = 10.0;

fn point(x: f64, y: f64, z: f64) -> Point3Dto {
    Point3Dto { x, y, z }
}

/// A rectangular support face at `FACE_Z` with its real boundary edges, so the
/// manager can project them.
fn face_body(body_id: BodyId) -> KernelBodyDto {
    let corners = [
        (0.0, 0.0),
        (FACE_WIDTH, 0.0),
        (FACE_WIDTH, FACE_HEIGHT),
        (0.0, FACE_HEIGHT),
    ];
    let edges = (0..4)
        .map(|index| KernelEdgeDto {
            key: format!("boundary{index}"),
            points: vec![
                point(corners[index].0, corners[index].1, FACE_Z),
                point(
                    corners[(index + 1) % 4].0,
                    corners[(index + 1) % 4].1,
                    FACE_Z,
                ),
            ],
            circle: None,
            refinable: true,
        })
        .collect();
    KernelBodyDto {
        topology_signature: String::new(),
        display_warnings: Vec::new(),
        body_id,
        positions: vec![
            0.0,
            0.0,
            FACE_Z as f32,
            FACE_WIDTH as f32,
            0.0,
            FACE_Z as f32,
            0.0,
            FACE_HEIGHT as f32,
            FACE_Z as f32,
        ],
        normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        indices: vec![0, 1, 2],
        faces: vec![KernelFaceDto {
            linear_seam_edge_keys: Vec::new(),
            planar_inner_ring_edge_keys: Vec::new(),
            outer_shell: None,
            key: "support".to_string(),
            first_index: 0,
            index_count: 3,
            plane: Some(PlaneBasis {
                origin: [0.0, 0.0, FACE_Z],
                u: [1.0, 0.0, 0.0],
                v: [0.0, 1.0, 0.0],
                normal: [0.0, 0.0, 1.0],
            }),
            signature: None,
            cylinder: None,
            edge_keys: (0..4).map(|index| format!("boundary{index}")).collect(),
            cone: None,
        }],
        edges,
    }
}

fn result_body_id(job: &KernelJobDto) -> BodyId {
    match job {
        KernelJobDto::Extrude(job) => job.result_body_ids[0],
        other => panic!("expected an extrude job, got {other:?}"),
    }
}

/// Extrude a base rectangle into the synthetic support body, then host a sketch
/// on its top face — the fixture the whole feature exists for.
fn manager_with_face_sketch() -> (SketchManager, limo_cad_core::FaceId) {
    let mut manager = SketchManager::new();
    manager
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    manager
        .add_rectangle(RectangleRequest {
            mode: RectangleMode::TwoPoint,
            p1: Vec2::new(0.0, 0.0),
            p2: Vec2::new(FACE_WIDTH, FACE_HEIGHT),
            ctrl_held: true,
        })
        .unwrap();
    manager.end_sketch().unwrap();
    let plan = manager
        .prepare_extrude(ExtrudeRequest {
            source_face: None,
            sketch_name: "Sketch1".to_string(),
            profile_indices: vec![0],
            operation: ExtrudeOperation::NewBody,
            extent: ExtrudeExtent::Distance { distance: FACE_Z },
            taper_angle_deg: 0.0,
            flip: false,
            target_body_ids: Vec::new(),
        })
        .unwrap();
    let body_id = result_body_id(&plan.jobs[0]);
    let update = manager
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![face_body(body_id)],
                errors: Vec::new(),
            },
        })
        .unwrap();
    let face_id = update.scene.bodies[0].faces[0].id;
    manager
        .begin_sketch(PlaneRef::PlanarFace { face_id })
        .unwrap();
    (manager, face_id)
}

/// The reported case: a line ending at the arc centre plus a semicircle whose
/// far endpoint rests on the support face boundary.
fn draw_semicircle_against_boundary(manager: &mut SketchManager) {
    manager
        .add_line(SegmentRequest {
            from: Vec2::new(0.0, FACE_HEIGHT),
            to_raw: Vec2::new(10.0, FACE_HEIGHT),
            ctrl_held: true,
        })
        .unwrap();
    manager
        .add_arc_center(ArcCenterRequest {
            center: Vec2::new(10.0, FACE_HEIGHT),
            start: Vec2::new(5.0, FACE_HEIGHT),
            sweep: Vec2::new(15.0, FACE_HEIGHT),
            ctrl_held: true,
            radius_mm: None,
            radius_text: None,
            angle_text: None,
            sweep_rad: None,
        })
        .unwrap();
}

#[test]
fn a_face_sketch_projects_the_support_boundary() {
    let (manager, _) = manager_with_face_sketch();
    let sketch = manager.active_snapshot().unwrap();
    assert_eq!(sketch.projected_edges.len(), 4);
    for edge in &sketch.projected_edges {
        assert!(
            edge.id >= RESERVED_ID_FLOOR,
            "projected ids must use the reserved range"
        );
        assert_eq!(edge.points.len(), 2, "a straight boundary edge stays exact");
        assert!(matches!(
            edge.points[0],
            point if point.x >= 0.0 && point.x <= FACE_WIDTH
        ));
    }

    assert!(sketch.projected_edges.iter().any(|edge| {
        edge.points
            .iter()
            .all(|point| (point.y - FACE_HEIGHT).abs() < 1e-9)
    }));
}

#[test]
fn a_semicircle_drawn_against_the_boundary_becomes_a_profile() {
    let (mut manager, _) = manager_with_face_sketch();
    draw_semicircle_against_boundary(&mut manager);
    manager.end_sketch().unwrap();

    let catalog = manager.profile_catalog();
    let entry = catalog
        .iter()
        .find(|entry| entry.sketch_name == "Sketch2")
        .expect("the face sketch is in the catalog");
    assert!(entry.profile_error.is_none(), "{:?}", entry.profile_error);

    let half_disc = std::f64::consts::PI * 25.0 / 2.0;
    assert!(
        entry
            .profiles
            .iter()
            .any(|profile| (profile.area - half_disc).abs() < 0.2 && profile.nesting_depth == 0),
        "the semicircle must be a selectable profile: {:?}",
        entry
            .profiles
            .iter()
            .map(|profile| (profile.area, profile.nesting_depth))
            .collect::<Vec<_>>()
    );

    assert!(
        entry
            .profiles
            .iter()
            .all(|profile| profile.area < FACE_WIDTH * FACE_HEIGHT - 1.0),
        "the support face outline leaked into the profile catalog"
    );

    assert!(entry
        .profiles
        .iter()
        .all(|profile| profile.parent_index.is_none()));
}

#[test]
fn a_shape_drawn_inside_a_face_stays_the_only_profile() {
    let (mut manager, _) = manager_with_face_sketch();
    manager
        .add_rectangle(RectangleRequest {
            mode: RectangleMode::TwoPoint,
            p1: Vec2::new(2.0, 2.0),
            p2: Vec2::new(8.0, 7.0),
            ctrl_held: true,
        })
        .unwrap();
    manager.end_sketch().unwrap();

    let catalog = manager.profile_catalog();
    let entry = catalog
        .iter()
        .find(|entry| entry.sketch_name == "Sketch2")
        .expect("the face sketch is in the catalog");
    assert_eq!(entry.profiles.len(), 1);
    assert!((entry.profiles[0].area - 30.0).abs() < 1e-6);
    assert_eq!(entry.profiles[0].nesting_depth, 0);
    assert!(entry.profiles[0].parent_index.is_none());
}

#[test]
fn projections_survive_a_save_and_reload_through_the_recompute() {
    let (mut manager, _) = manager_with_face_sketch();
    draw_semicircle_against_boundary(&mut manager);
    manager.end_sketch().unwrap();
    let json = manager.export_project_model().unwrap();

    assert!(json.contains("support_boundary"));

    let body_id = manager.solid_scene().bodies[0].id;
    let mut loaded = SketchManager::new();
    let replay = loaded.prepare_load_project(json).unwrap();
    assert!(replay.errors.is_empty());
    loaded
        .commit_solid(CommitKernelRequest {
            transaction_id: replay.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![face_body(body_id)],
                errors: Vec::new(),
            },
        })
        .unwrap();

    let reloaded = loaded
        .finished_sketches()
        .into_iter()
        .find(|sketch| sketch.name == "Sketch2")
        .expect("the face sketch survives the round trip");
    assert_eq!(reloaded.projected_edges.len(), 4);

    let half_disc = std::f64::consts::PI * 25.0 / 2.0;
    let entry = loaded
        .profile_catalog()
        .into_iter()
        .find(|entry| entry.sketch_name == "Sketch2")
        .unwrap();
    assert!(
        entry
            .profiles
            .iter()
            .any(|profile| (profile.area - half_disc).abs() < 0.2),
        "the reloaded sketch must recover the sealed region"
    );
}

#[test]
fn geometry_snaps_exactly_onto_the_projected_boundary() {
    let basis = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    }
    .basis()
    .unwrap();
    let mut session = SketchSession::new(
        "Sketch1",
        PlaneRef::PlanarFace {
            face_id: limo_cad_core::FaceId(1),
        },
        basis,
        false,
    );
    session.set_projected_edges(vec![ProjectedEdgeDto {
        id: RESERVED_ID_FLOOR,
        edge_id: EdgeId(7),
        points: vec![Vec2::new(0.0, 10.0), Vec2::new(20.0, 10.0)],
        circle: None,
    }]);

    let preview = session.preview_segment(Vec2::new(0.0, 0.0), Vec2::new(8.0, 10.2), false);
    match preview.snap {
        SnapTarget::ProjectedEdge { edge, position } => {
            assert_eq!(edge, EdgeId(7));
            assert!((position.x - 8.0).abs() < 1e-9);
            assert!((position.y - 10.0).abs() < 1e-9);
        }
        other => panic!("expected a projected-edge acquisition, got {other:?}"),
    }

    session
        .add_line(Vec2::new(0.0, 0.0), Vec2::new(8.0, 10.2), false)
        .unwrap();
    let dto = session.dto();
    let end = dto
        .entities
        .iter()
        .find_map(|entity| match entity {
            limo_cad_sketch::EntityDto::Line { end, .. } => Some(*end),
            _ => None,
        })
        .expect("the committed line is in the snapshot");
    assert_eq!(end, Vec2::new(8.0, 10.0));
}

#[test]
fn invalid_saved_boundaries_and_point_ownership_are_rejected_atomically() {
    let (mut manager, _) = manager_with_face_sketch();
    draw_semicircle_against_boundary(&mut manager);
    manager.end_sketch().unwrap();
    let original = manager.export_project_model().unwrap();
    for invalid in [
        "entity_id",
        "duplicate",
        "radius",
        "origin_plane",
        "generated_point",
    ] {
        let mut model: serde_json::Value = serde_json::from_str(&original).unwrap();
        let face = model["sketches"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|s| s["name"] == "Sketch2")
            .unwrap();
        match invalid {
            "entity_id" => face["support_boundary"][0]["id"] = 1.into(),
            "duplicate" => {
                let first = face["support_boundary"][0].clone();
                face["support_boundary"].as_array_mut().unwrap().push(first);
            }
            "radius" => {
                face["support_boundary"][0]["circle"] =
                    serde_json::json!({"center":{"x":0,"y":0},"radius":-1,"closed":false})
            }
            "origin_plane" => {
                face["plane"] = serde_json::json!({"type":"origin_plane","plane":"xy"})
            }
            "generated_point" => face["snapshot"]["generated_points"] = serde_json::json!([999999]),
            _ => unreachable!(),
        }
        assert!(
            manager.prepare_load_project(model.to_string()).is_err(),
            "{invalid}"
        );
        assert_eq!(
            manager.export_project_model().unwrap(),
            original,
            "rejected load must be atomic"
        );
    }
}
