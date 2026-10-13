//! Real-kernel regressions: dependent profiles must exist before replay, and
//! introducing implicit boundaries must not reinterpret legacy profile indices.
#![cfg(feature = "native-occt")]
use limo_cad_core::{BodyId, OriginPlane, PlaneRef};
use limo_cad_occt::OcctKernel;
use limo_cad_sketch::{
    ArcCenterRequest, RectangleMode, RectangleRequest, SegmentRequest, SetGridSnapRequest,
    SketchManager, Vec2,
};
use limo_cad_solid::{
    CommitKernelRequest, DatumPlaneRequest, DatumPlaneSourceDto, EditExtrudeRequest, ExtrudeExtent,
    ExtrudeOperation, ExtrudeRequest, LoftRequest, Point2Dto, ProfileRefDto, RecomputePlanDto,
    RevolveRequest, RibRequest, SetRollbackRequest, SweepRequest,
};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};
fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
fn extrusion(name: &str, index: u32, target: Option<BodyId>, distance: f64) -> ExtrudeRequest {
    ExtrudeRequest {
        source_face: None,
        sketch_name: name.into(),
        profile_indices: vec![index],
        operation: if target.is_some() {
            ExtrudeOperation::Cut
        } else {
            ExtrudeOperation::NewBody
        },
        extent: ExtrudeExtent::Distance { distance },
        taper_angle_deg: 0.0,
        flip: target.is_some(),
        target_body_ids: target.into_iter().collect(),
    }
}
fn apply(m: &mut SketchManager, k: &mut OcctKernel, plan: RecomputePlanDto) {
    assert!(plan.errors.is_empty(), "{:?}", plan.errors);
    let scene = k.recompute(&plan).unwrap();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    m.commit_solid(CommitKernelRequest {
        transaction_id: plan.transaction_id,
        scene,
    })
    .unwrap();
}
fn face_sketch(bottom: bool) -> (SketchManager, OcctKernel, BodyId) {
    let mut m = SketchManager::new();
    m.begin_sketch(XY).unwrap();
    m.set_grid_snap(SetGridSnapRequest { enabled: false })
        .unwrap();
    m.add_rectangle(RectangleRequest {
        mode: RectangleMode::TwoPoint,
        p1: v(0.0, 0.0),
        p2: v(25.0, 25.0),
        ctrl_held: true,
    })
    .unwrap();
    m.end_sketch().unwrap();
    let plan = m
        .prepare_extrude(extrusion("Sketch1", 0, None, 10.0))
        .unwrap();
    let mut k = OcctKernel::new().unwrap();
    apply(&mut m, &mut k, plan);
    let body = &m.solid_scene().bodies[0];
    let body_id = body.id;
    let face_id = body
        .faces
        .iter()
        .find(|f| {
            f.plane.is_some_and(|p| {
                if bottom {
                    p.normal[2] < -0.99
                } else {
                    p.normal[2] > 0.99
                }
            })
        })
        .unwrap()
        .id;
    m.begin_sketch(PlaneRef::PlanarFace { face_id }).unwrap();
    (m, k, body_id)
}
fn bounds(m: &SketchManager) -> (f64, f64, f64) {
    let dto = m.active_snapshot().unwrap();
    let points: Vec<_> = dto.projected_edges.iter().flat_map(|e| &e.points).collect();
    (
        points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
        points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
    )
}
fn profile_areas(m: &SketchManager) -> Vec<(u32, f64)> {
    m.profile_catalog()
        .iter()
        .find(|c| c.sketch_name == "Sketch2")
        .unwrap()
        .profiles
        .iter()
        .map(|p| (p.index, p.area))
        .collect()
}

fn mesh_volume(body: &limo_cad_solid::BodyDto) -> f64 {
    let point = |index: u32| {
        let p = &body.mesh.positions[index as usize * 3..index as usize * 3 + 3];
        [p[0] as f64, p[1] as f64, p[2] as f64]
    };
    body.mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let [a, b, c] = [point(triangle[0]), point(triangle[1]), point(triangle[2])];
            a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0])
        })
        .sum::<f64>()
        .abs()
        / 6.0
}

fn exact_volume(kernel: &OcctKernel, id: BodyId) -> f64 {
    let pose = || limo_cad_occt::PlacedBodyQueryDto {
        body_id: id,
        translation: [0.; 3],
        rotation: [0., 0., 0., 1.],
    };
    kernel
        .exact_interference(pose(), pose())
        .unwrap()
        .overlap_volume_mm3
}

#[test]
fn all_face_sketch_consumers_survive_reopen_rollback_and_upstream_edits() {
    for consumer in ["revolve", "sweep", "loft", "rib"] {
        let (mut m, mut k, base_body) = face_sketch(true);
        let (x, _, y) = bounds(&m);
        let basis = m.active_snapshot().unwrap().basis;
        let points = [
            v(x + 5., y),
            v(x + 5., y - 4.),
            v(x + 10., y - 4.),
            v(x + 10., y),
        ];
        let mut lines = Vec::new();
        for pair in points.windows(2) {
            lines.push(
                m.add_line(SegmentRequest {
                    from: pair[0],
                    to_raw: pair[1],
                    ctrl_held: true,
                })
                .unwrap()
                .entity_id
                .0,
            );
        }
        m.end_sketch().unwrap();
        let areas = profile_areas(&m);
        let index = areas
            .iter()
            .find(|(_, area)| (area - 20.).abs() < 1e-6)
            .unwrap()
            .0;
        let profile = ProfileRefDto {
            sketch_name: "Sketch2".into(),
            profile_index: index,
        };
        let plan = match consumer {
            "revolve" => m
                .prepare_revolve(RevolveRequest {
                    sketch_name: "Sketch2".into(),
                    profile_indices: vec![index],
                    axis_origin: Point2Dto::new(x - 10., 0.),
                    axis_direction: Point2Dto::new(0., 1.),
                    axis_line_sketch_name: None,
                    axis_line_entity_id: None,
                    angle_deg: 360.,
                    flip: false,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: vec![],
                })
                .unwrap(),
            "sweep" => {
                m.begin_sketch(PlaneRef::OriginPlane {
                    plane: OriginPlane::Xz,
                })
                .unwrap();
                let path = m
                    .add_line(SegmentRequest {
                        from: v(0., 0.),
                        to_raw: v(0., -10.),
                        ctrl_held: true,
                    })
                    .unwrap()
                    .entity_id;
                m.end_sketch().unwrap();
                m.prepare_sweep(SweepRequest {
                    profile,
                    path_sketch_name: "Sketch3".into(),
                    path_entity_ids: vec![path.0],
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: vec![],
                    guide_rail: None,
                    orientation: Default::default(),
                    transition: Default::default(),
                    force_c1: false,
                })
                .unwrap()
            }
            "loft" => {
                let datum = m
                    .create_datum_plane(DatumPlaneRequest {
                        name: None,
                        source: DatumPlaneSourceDto::Offset {
                            reference: XY,
                            distance: -10.,
                        },
                    })
                    .unwrap();
                m.begin_sketch(PlaneRef::DatumPlane {
                    datum_id: datum.planes.last().unwrap().datum_id,
                })
                .unwrap();
                let a = basis.to_3d([points[0].x, points[0].y]);
                let b = basis.to_3d([points[2].x, points[2].y]);
                m.add_rectangle(RectangleRequest {
                    mode: RectangleMode::TwoPoint,
                    p1: v(a[0], a[1]),
                    p2: v(b[0], b[1]),
                    ctrl_held: true,
                })
                .unwrap();
                m.end_sketch().unwrap();
                m.prepare_loft(LoftRequest {
                    sections: vec![
                        profile,
                        ProfileRefDto {
                            sketch_name: "Sketch3".into(),
                            profile_index: 0,
                        },
                    ],
                    ruled: true,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: vec![],
                    continuity: Default::default(),
                    centerline: None,
                    guide_rail: None,
                })
                .unwrap()
            }
            _ => m
                .prepare_rib(RibRequest {
                    sketch_name: "Sketch2".into(),
                    line_entity_ids: vec![lines[1]],
                    thickness: 2.,
                    depth: 3.,
                    symmetric: true,
                    flip: false,
                    operation: ExtrudeOperation::NewBody,
                    target_body_ids: vec![],
                    extent: None,
                })
                .unwrap(),
        };
        apply(&mut m, &mut k, plan);
        let expected = match consumer {
            "revolve" => 20. * TAU * 17.5,
            "rib" => 30.,
            _ => 200.,
        };
        let check = |m: &SketchManager| {
            let scene = m.solid_scene();
            assert_eq!(scene.bodies.len(), 2, "{consumer}");
            let generated = scene.bodies.iter().find(|b| b.id != base_body).unwrap();
            let volume = mesh_volume(generated);
            assert!(
                (volume - expected).abs() < expected * 0.015,
                "{consumer}: wrong region/solid volume {volume}, expected {expected}"
            );
            assert_eq!(profile_areas(m), areas);
        };
        check(&m);
        let mut loaded = SketchManager::new();
        let mut fresh_kernel = OcctKernel::new().unwrap();
        let plan = loaded
            .prepare_load_project(m.export_project_model().unwrap())
            .unwrap();
        apply(&mut loaded, &mut fresh_kernel, plan);
        check(&loaded);
        let count = loaded.document().features().features.len();
        let plan = loaded
            .prepare_set_rollback(SetRollbackRequest {
                rollback_index: count - 1,
            })
            .unwrap();
        apply(&mut loaded, &mut fresh_kernel, plan);
        assert_eq!(loaded.solid_scene().bodies.len(), 1);
        let plan = loaded
            .prepare_set_rollback(SetRollbackRequest {
                rollback_index: count,
            })
            .unwrap();
        apply(&mut loaded, &mut fresh_kernel, plan);
        check(&loaded);
        loaded.edit_sketch("Sketch2").unwrap();
        loaded.end_sketch().unwrap();
        let plan = loaded.prepare_recompute().unwrap();
        apply(&mut loaded, &mut fresh_kernel, plan);
        check(&loaded);
        let base = loaded.extrude_definitions()[0].feature_id;
        let plan = loaded
            .prepare_edit_extrude(EditExtrudeRequest {
                feature_id: base,
                extrude: extrusion("Sketch1", 0, None, 12.),
            })
            .unwrap();
        apply(&mut loaded, &mut fresh_kernel, plan);
        check(&loaded);
    }
}

#[test]
fn completed_boundary_pocket_reopens_and_recomputes_without_retargeting() {
    for bottom in [false, true] {
        let (mut m, mut k, body) = face_sketch(bottom);
        let (min_x, max_x, max_y) = bounds(&m);
        let center = v((min_x + max_x) / 2.0, max_y);
        let boundary = m.active_snapshot().unwrap().projected_edges;
        m.add_arc_center(ArcCenterRequest {
            center,
            start: v(center.x - 5.0, max_y),
            sweep: v(center.x + 5.0, max_y),
            ctrl_held: true,
            radius_mm: None,
            radius_text: None,
            angle_text: None,
            sweep_rad: Some(PI),
        })
        .unwrap();
        m.end_sketch().unwrap();
        let areas = profile_areas(&m);
        let index = areas
            .iter()
            .find(|(_, area)| (area - PI * 25.0 / 2.0).abs() < 0.2)
            .unwrap()
            .0;
        let plan = m
            .prepare_extrude(extrusion("Sketch2", index, Some(body), 5.0))
            .unwrap();
        apply(&mut m, &mut k, plan);
        let saved = m.export_project_model().unwrap();
        let mut loaded = SketchManager::new();
        let mut fresh_kernel = OcctKernel::new().unwrap();
        let plan = loaded.prepare_load_project(saved).unwrap();
        assert_eq!(
            plan.jobs.len(),
            2,
            "both base and consuming pocket must replay"
        );
        apply(&mut loaded, &mut fresh_kernel, plan);
        assert_eq!(profile_areas(&loaded), areas);

        let edited = loaded.edit_sketch("Sketch2").unwrap();
        assert_eq!(edited.projected_edges, boundary);
        assert_eq!(edited.reference_midpoints.len(), boundary.len());
        loaded.end_sketch().unwrap();
        let plan = loaded.prepare_recompute().unwrap();
        apply(&mut loaded, &mut fresh_kernel, plan);
        assert_eq!(profile_areas(&loaded), areas);
        if bottom {
            let base = loaded.extrude_definitions()[0].feature_id;
            let plan = loaded
                .prepare_edit_extrude(EditExtrudeRequest {
                    feature_id: base,
                    extrude: extrusion("Sketch1", 0, None, 12.0),
                })
                .unwrap();
            apply(&mut loaded, &mut fresh_kernel, plan);
            assert_eq!(profile_areas(&loaded), areas);
        }
        assert!(loaded.solid_scene().bodies[0]
            .edges
            .iter()
            .any(|e| e.circle.is_some() && e.points.iter().all(|p| (p.z - 5.0).abs() < 1e-6)));
    }
}

fn legacy_json(m: &SketchManager) -> String {
    let mut model: serde_json::Value =
        serde_json::from_str(&m.export_project_model().unwrap()).unwrap();
    model["schema_version"] = 7.into();
    model.as_object_mut().unwrap().remove("print_intent");
    if let Some(views) = model["views"].as_array_mut() {
        for view in views {
            view.as_object_mut().unwrap().remove("id");
        }
    }
    for sketch in model["sketches"].as_array_mut().unwrap() {
        sketch.as_object_mut().unwrap().remove("support_boundary");
        sketch.as_object_mut().unwrap().remove("profile_identities");
        sketch
            .as_object_mut()
            .unwrap()
            .remove("entity_id_high_water");
        sketch["snapshot"]
            .as_object_mut()
            .unwrap()
            .remove("reference_edges");
        sketch["snapshot"]
            .as_object_mut()
            .unwrap()
            .remove("generated_points");
    }
    model.to_string()
}

#[test]
fn analytic_circle_contacts_between_render_samples_close_minor_regions_on_both_normals() {
    for bottom in [false, true] {
        for sweep in [PI / 3., 0.3] {
            let mut m = SketchManager::new();
            m.begin_sketch(XY).unwrap();
            m.set_grid_snap(SetGridSnapRequest { enabled: false })
                .unwrap();
            m.add_circle(limo_cad_sketch::CircleRequest {
                mode: limo_cad_sketch::CircleMode::CenterDiameter,
                p1: Vec2::ZERO,
                p2: v(10., 0.),
                ctrl_held: true,
            })
            .unwrap();
            m.end_sketch().unwrap();
            let mut k = OcctKernel::new().unwrap();
            let plan = m
                .prepare_extrude(extrusion("Sketch1", 0, None, 10.))
                .unwrap();
            apply(&mut m, &mut k, plan);
            let face = m.solid_scene().bodies[0]
                .faces
                .iter()
                .find(|f| {
                    f.plane.is_some_and(|p| {
                        if bottom {
                            p.normal[2] < -0.9
                        } else {
                            p.normal[2] > 0.9
                        }
                    })
                })
                .unwrap()
                .id;
            let dto = m
                .begin_sketch(PlaneRef::PlanarFace { face_id: face })
                .unwrap();
            let circle = dto.projected_edges.iter().find_map(|e| e.circle).unwrap();
            let at = |angle: f64| {
                circle.center + v(circle.radius * angle.cos(), circle.radius * angle.sin())
            };

            m.add_line(SegmentRequest {
                from: at(0.373),
                to_raw: at(0.373 + sweep),
                ctrl_held: true,
            })
            .unwrap();
            m.end_sketch().unwrap();
            let catalog = m.profile_catalog();
            let profiles = &catalog
                .iter()
                .find(|c| c.sketch_name == "Sketch2")
                .unwrap()
                .profiles;
            assert_eq!(profiles.len(), 2, "both circle segments must close");
            let index = profiles
                .iter()
                .min_by(|a, b| a.area.total_cmp(&b.area))
                .unwrap()
                .index;
            let plan = m
                .prepare_extrude(extrusion("Sketch2", index, None, 2.))
                .unwrap();
            apply(&mut m, &mut k, plan);
            let expected = circle.radius.powi(2) * (sweep - sweep.sin());
            let actual = exact_volume(&k, m.solid_scene().bodies[1].id);
            assert!(
                (actual - expected).abs() < 1e-7,
                "bottom={bottom}, sweep={sweep}, volume={actual}, expected={expected}"
            );
            let mut loaded = SketchManager::new();
            let mut fresh = OcctKernel::new().unwrap();
            let plan = loaded
                .prepare_load_project(m.export_project_model().unwrap())
                .unwrap();
            apply(&mut loaded, &mut fresh, plan);
            assert!(
                (exact_volume(&fresh, loaded.solid_scene().bodies[1].id) - expected).abs() < 1e-7
            );
        }
    }
}

#[test]
fn splitting_consumed_profile_reports_missing_reference_and_undo_restores_it() {
    let mut m = SketchManager::new();
    m.begin_sketch(XY).unwrap();
    m.set_grid_snap(SetGridSnapRequest { enabled: false })
        .unwrap();
    m.add_rectangle(RectangleRequest {
        mode: RectangleMode::TwoPoint,
        p1: v(0., 0.),
        p2: v(20., 20.),
        ctrl_held: true,
    })
    .unwrap();
    m.end_sketch().unwrap();
    let mut k = OcctKernel::new().unwrap();
    let plan = m
        .prepare_extrude(extrusion("Sketch1", 0, None, 10.))
        .unwrap();
    apply(&mut m, &mut k, plan);
    m.edit_sketch("Sketch1").unwrap();
    m.add_line(SegmentRequest {
        from: v(0., 10.),
        to_raw: v(20., 10.),
        ctrl_held: true,
    })
    .unwrap();
    m.end_sketch().unwrap();
    let plan = m.prepare_recompute().unwrap();
    assert!(
        plan.jobs.is_empty(),
        "must not extrude an arbitrary half instead"
    );
    assert!(
        plan.errors
            .iter()
            .any(|e| e.message.contains("profile 0 was not found")),
        "{:?}",
        plan.errors
    );

    let scene = k.recompute(&plan).unwrap();
    m.commit_solid(CommitKernelRequest {
        transaction_id: plan.transaction_id,
        scene,
    })
    .unwrap();
    m.edit_sketch("Sketch1").unwrap();
    m.undo().unwrap();
    m.end_sketch().unwrap();
    let plan = m.prepare_recompute().unwrap();
    apply(&mut m, &mut k, plan);
    assert!((mesh_volume(&m.solid_scene().bodies[0]) - 4000.).abs() < 1e-5);
}

#[test]
fn holes_and_concave_support_faces_close_regions_on_both_normals_and_reopen() {
    for hole in [false, true] {
        for bottom in [false, true] {
            let mut m = SketchManager::new();
            m.begin_sketch(XY).unwrap();
            m.set_grid_snap(SetGridSnapRequest { enabled: false })
                .unwrap();
            if hole {
                for (a, b) in [(v(0., 0.), v(20., 20.)), (v(5., 5.), v(15., 15.))] {
                    m.add_rectangle(RectangleRequest {
                        mode: RectangleMode::TwoPoint,
                        p1: a,
                        p2: b,
                        ctrl_held: true,
                    })
                    .unwrap();
                }
            } else {
                let p = [
                    v(0., 0.),
                    v(30., 0.),
                    v(30., 10.),
                    v(10., 10.),
                    v(10., 30.),
                    v(0., 30.),
                ];
                for i in 0..p.len() {
                    m.add_line(SegmentRequest {
                        from: p[i],
                        to_raw: p[(i + 1) % p.len()],
                        ctrl_held: true,
                    })
                    .unwrap();
                }
            }
            m.end_sketch().unwrap();
            let profile = m.profile_catalog()[0]
                .profiles
                .iter()
                .max_by(|a, b| a.area.total_cmp(&b.area))
                .unwrap()
                .index;
            let plan = m
                .prepare_extrude(extrusion("Sketch1", profile, None, 10.))
                .unwrap();
            let mut k = OcctKernel::new().unwrap();
            apply(&mut m, &mut k, plan);
            let scene = m.solid_scene();
            let face = scene.bodies[0]
                .faces
                .iter()
                .find(|f| {
                    f.plane.is_some_and(|p| {
                        if bottom {
                            p.normal[2] < -0.9
                        } else {
                            p.normal[2] > 0.9
                        }
                    })
                })
                .unwrap();
            let sketch = m
                .begin_sketch(PlaneRef::PlanarFace { face_id: face.id })
                .unwrap();
            let basis = sketch.basis;
            let to_local = |p: Vec2| {
                let d = [
                    p.x - basis.origin[0],
                    p.y - basis.origin[1],
                    if bottom { 0. } else { 10. } - basis.origin[2],
                ];
                v(
                    d.iter().zip(basis.u).map(|(a, b)| a * b).sum(),
                    d.iter().zip(basis.v).map(|(a, b)| a * b).sum(),
                )
            };
            let (path, area) = if hole {
                (vec![v(5., 7.), v(2., 7.), v(2., 13.), v(5., 13.)], 18.)
            } else {
                (
                    vec![v(10., 16.), v(4., 16.), v(4., 6.), v(16., 6.), v(16., 10.)],
                    84.,
                )
            };
            for p in path.windows(2) {
                m.add_line(SegmentRequest {
                    from: to_local(p[0]),
                    to_raw: to_local(p[1]),
                    ctrl_held: true,
                })
                .unwrap();
            }
            m.end_sketch().unwrap();
            let catalog = m.profile_catalog();
            let profile = catalog
                .iter()
                .find(|c| c.sketch_name == "Sketch2")
                .unwrap()
                .profiles
                .iter()
                .find(|p| (p.area - area).abs() < 1e-5)
                .unwrap_or_else(|| {
                    panic!(
                        "missing region {area} on hole={hole}, bottom={bottom}: {:?}",
                        catalog
                    )
                })
                .index;
            let plan = m
                .prepare_extrude(extrusion("Sketch2", profile, None, 2.))
                .unwrap();
            apply(&mut m, &mut k, plan);
            assert!((mesh_volume(&m.solid_scene().bodies[1]) - 2. * area).abs() < 1e-4);
            let json = m.export_project_model().unwrap();
            let mut reopened = SketchManager::new();
            let mut fresh = OcctKernel::new().unwrap();
            let plan = reopened.prepare_load_project(json).unwrap();
            apply(&mut reopened, &mut fresh, plan);
            assert!((mesh_volume(&reopened.solid_scene().bodies[1]) - 2. * area).abs() < 1e-4);
        }
    }
}

#[test]
fn crossing_rectangle_notch_cuts_only_its_in_stock_region_and_replays() {
    for bottom in [false, true] {
        for upper_edge in [false, true] {
            let (mut m, mut k, body) = face_sketch(bottom);
            let (min_x, _, max_y) = bounds(&m);
            let min_y = max_y - 25.0;
            let (y1, y2) = if upper_edge {
                (max_y - 7.0, max_y + 1.0)
            } else {
                (min_y - 1.0, min_y + 7.0)
            };
            m.add_rectangle(RectangleRequest {
                mode: RectangleMode::TwoPoint,
                p1: v(min_x + 5.0, y1),
                p2: v(min_x + 10.0, y2),
                ctrl_held: true,
            })
            .unwrap();
            m.end_sketch().unwrap();
            let areas = profile_areas(&m);
            assert_eq!(areas.len(), 3, "notch, outside strip and stock remainder");
            for expected in [35.0, 5.0, 590.0] {
                assert_eq!(
                    areas
                        .iter()
                        .filter(|(_, area)| (area - expected).abs() < 1e-6)
                        .count(),
                    1
                );
            }

            let index = areas
                .iter()
                .find(|(_, area)| (area - 35.0).abs() < 1e-6)
                .unwrap()
                .0;
            let mut cut = extrusion("Sketch2", index, Some(body), 10.0);
            cut.extent = ExtrudeExtent::ThroughAll;
            let plan = m.prepare_extrude(cut).unwrap();
            apply(&mut m, &mut k, plan);
            assert!((exact_volume(&k, body) - 5900.0).abs() < 1e-5);

            let mut reopened = SketchManager::new();
            let mut fresh = OcctKernel::new().unwrap();
            let plan = reopened
                .prepare_load_project(m.export_project_model().unwrap())
                .unwrap();
            apply(&mut reopened, &mut fresh, plan);
            assert_eq!(profile_areas(&reopened), areas);
            assert!((exact_volume(&fresh, body) - 5900.0).abs() < 1e-5);
            reopened.edit_sketch("Sketch2").unwrap();
            reopened.end_sketch().unwrap();
            let plan = reopened.prepare_recompute().unwrap();
            apply(&mut reopened, &mut fresh, plan);
            assert_eq!(profile_areas(&reopened), areas);
            assert!((exact_volume(&fresh, body) - 5900.0).abs() < 1e-5);
            for (rollback_index, volume) in [(2, 6250.0), (4, 5900.0)] {
                let plan = reopened
                    .prepare_set_rollback(SetRollbackRequest { rollback_index })
                    .unwrap();
                apply(&mut reopened, &mut fresh, plan);
                assert!((exact_volume(&fresh, body) - volume).abs() < 1e-5);
            }
        }
    }
}

#[test]
fn legacy_corner_rectangle_consumer_keeps_profile_zero_during_edit_and_rollback() {
    let (mut m, mut k, body) = face_sketch(false);
    let (x, _, y) = bounds(&m);
    m.add_rectangle(RectangleRequest {
        mode: RectangleMode::TwoPoint,
        p1: v(x, y),
        p2: v(x + 5.0, y - 5.0),
        ctrl_held: true,
    })
    .unwrap();
    m.end_sketch().unwrap();
    assert_eq!(
        profile_areas(&m).len(),
        2,
        "new sketches allow the remainder region"
    );
    let plan = m.prepare_load_project(legacy_json(&m)).unwrap();
    apply(&mut m, &mut k, plan);
    assert_eq!(profile_areas(&m), vec![(0, 25.0)]);
    let plan = m
        .prepare_extrude(extrusion("Sketch2", 0, Some(body), 5.0))
        .unwrap();
    apply(&mut m, &mut k, plan);
    let plan = m.prepare_load_project(legacy_json(&m)).unwrap();
    assert_eq!(plan.jobs.len(), 2);
    apply(&mut m, &mut k, plan);
    for rollback_index in [4, 3, 4] {
        let plan = m
            .prepare_set_rollback(SetRollbackRequest { rollback_index })
            .unwrap();
        apply(&mut m, &mut k, plan);
        assert!(m.edit_sketch("Sketch2").unwrap().projected_edges.is_empty());
        m.end_sketch().unwrap();
        let plan = m.prepare_recompute().unwrap();
        apply(&mut m, &mut k, plan);
        assert_eq!(profile_areas(&m), vec![(0, 25.0)]);
    }
}

#[test]
fn partial_circular_boundary_samples_keep_direction_on_both_face_normals() {
    for sweep in [FRAC_PI_2, PI, PI * 1.5] {
        let mut m = SketchManager::new();
        m.begin_sketch(XY).unwrap();
        m.set_grid_snap(SetGridSnapRequest { enabled: false })
            .unwrap();
        let end = v(5.0 * sweep.cos(), 5.0 * sweep.sin());
        m.add_arc_center(ArcCenterRequest {
            center: Vec2::ZERO,
            start: v(5.0, 0.0),
            sweep: end,
            ctrl_held: true,
            radius_mm: None,
            radius_text: None,
            angle_text: None,
            sweep_rad: Some(sweep),
        })
        .unwrap();
        m.add_line(limo_cad_sketch::SegmentRequest {
            from: end,
            to_raw: v(5.0, 0.0),
            ctrl_held: true,
        })
        .unwrap();
        m.end_sketch().unwrap();
        let mut k = OcctKernel::new().unwrap();
        let plan = m
            .prepare_extrude(extrusion("Sketch1", 0, None, 10.0))
            .unwrap();
        apply(&mut m, &mut k, plan);
        let caps: Vec<_> = m.solid_scene().bodies[0]
            .faces
            .iter()
            .filter(|f| f.plane.is_some())
            .map(|f| f.id)
            .collect();
        let mut directions = Vec::new();
        for face_id in caps {
            let dto = m.begin_sketch(PlaneRef::PlanarFace { face_id }).unwrap();
            for edge in &dto.projected_edges {
                let Some(circle) = edge.circle else { continue };
                let angles: Vec<_> = edge
                    .points
                    .iter()
                    .map(|p| (p.y - circle.center.y).atan2(p.x - circle.center.x))
                    .collect();
                let travel: f64 = angles
                    .windows(2)
                    .map(|p| (p[1] - p[0] + PI).rem_euclid(2.0 * PI) - PI)
                    .sum();
                assert!((travel.abs() - sweep).abs() < 1e-6);
                directions.push(travel.signum());

                let body_edge = m.solid_scene().bodies[0]
                    .edges
                    .iter()
                    .find(|e| e.id == edge.edge_id)
                    .unwrap()
                    .clone();
                for (p, q) in edge.points.iter().zip(&body_edge.points) {
                    let world = dto.basis.to_3d([p.x, p.y]);
                    assert!(
                        (world[0] - q.x).abs() < 1e-6
                            && (world[1] - q.y).abs() < 1e-6
                            && (world[2] - q.z).abs() < 1e-6
                    );
                }
            }
            m.end_sketch().unwrap();
        }
        assert!(directions.contains(&-1.0) && directions.contains(&1.0));
    }
}
