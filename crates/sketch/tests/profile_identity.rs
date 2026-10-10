use limo_cad_core::{OriginPlane, PlaneRef};
use limo_cad_sketch::{
    CircleMode, CircleRequest, MoveCopyRequest, RectangleMode, RectangleRequest, SegmentRequest,
    SetGridSnapRequest, SketchManager, Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
fn manager() -> SketchManager {
    let mut m = SketchManager::new();
    m.begin_sketch(PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    })
    .unwrap();
    m.set_grid_snap(SetGridSnapRequest { enabled: false })
        .unwrap();
    m
}
fn rectangle(
    m: &mut SketchManager,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Vec<limo_cad_sketch::EntityId> {
    m.add_rectangle(RectangleRequest {
        mode: RectangleMode::TwoPoint,
        p1: v(x, y),
        p2: v(x + w, y + h),
        ctrl_held: true,
    })
    .unwrap()
    .entities
}
fn catalog(m: &SketchManager) -> Vec<limo_cad_solid::ProfileLoopDto> {
    m.profile_catalog()[0].profiles.clone()
}
fn load(m: &mut SketchManager, json: String) {
    let plan = m.prepare_load_project(json).unwrap();
    assert!(plan.jobs.is_empty());
    m.commit_solid(limo_cad_solid::CommitKernelRequest {
        transaction_id: plan.transaction_id,
        scene: limo_cad_solid::KernelSceneDto {
            bodies: vec![],
            errors: vec![],
        },
    })
    .unwrap();
}

#[test]
fn adding_and_moving_regions_preserves_identity_and_hole_parentage_across_reload() {
    let mut m = manager();
    let outer = rectangle(&mut m, 20., 20., 20., 20.);
    rectangle(&mut m, 25., 25., 5., 5.);
    m.end_sketch().unwrap();
    let before = catalog(&m);
    let outer_id = before
        .iter()
        .find(|p| (p.area - 400.).abs() < 1e-6)
        .unwrap()
        .index;
    let hole_id = before
        .iter()
        .find(|p| (p.area - 25.).abs() < 1e-6)
        .unwrap()
        .index;
    m.edit_sketch("Sketch1").unwrap();
    rectangle(&mut m, -100., -100., 10., 10.);
    m.end_sketch().unwrap();
    let after = catalog(&m);
    assert_eq!(
        after
            .iter()
            .find(|p| (p.area - 400.).abs() < 1e-6)
            .unwrap()
            .index,
        outer_id
    );
    assert_eq!(
        after
            .iter()
            .find(|p| p.index == hole_id)
            .unwrap()
            .parent_index,
        Some(outer_id)
    );
    m.edit_sketch("Sketch1").unwrap();
    m.move_copy_entities(MoveCopyRequest {
        entity_ids: outer,
        dx: 50.,
        dy: 0.,
        copy: false,
    })
    .unwrap();
    m.end_sketch().unwrap();
    assert_eq!(
        catalog(&m)
            .iter()
            .find(|p| (p.area - 400.).abs() < 1e-6)
            .unwrap()
            .index,
        outer_id
    );
    assert_eq!(
        catalog(&m)
            .iter()
            .find(|p| p.index == hole_id)
            .unwrap()
            .parent_index,
        None
    );
    let mut loaded = SketchManager::new();
    load(&mut loaded, m.export_project_model().unwrap());
    assert_eq!(catalog(&loaded), catalog(&m));
}

#[test]
fn splitting_a_region_fails_closed_and_undo_does_not_recycle_source_identities() {
    let mut m = manager();
    let original_entities = rectangle(&mut m, 20., 20., 20., 20.);
    m.end_sketch().unwrap();
    let original = catalog(&m)[0].index;
    m.edit_sketch("Sketch1").unwrap();
    m.add_line(SegmentRequest {
        from: v(20., 30.),
        to_raw: v(40., 30.),
        ctrl_held: true,
    })
    .unwrap();
    m.end_sketch().unwrap();
    assert!(
        catalog(&m).iter().all(|p| p.index != original),
        "a split must not reuse the parent region id"
    );
    m.edit_sketch("Sketch1").unwrap();
    m.undo().unwrap();
    m.end_sketch().unwrap();
    assert_eq!(catalog(&m)[0].index, original);
    m.edit_sketch("Sketch1").unwrap();
    m.undo().unwrap();
    let replacement = rectangle(&mut m, 20., 20., 20., 20.);
    assert!(replacement.iter().all(|id| !original_entities.contains(id)));
    m.end_sketch().unwrap();
    assert_ne!(
        catalog(&m)[0].index,
        original,
        "new geometry after Undo is not the deleted region"
    );
}

#[test]
fn overlapping_circle_regions_do_not_collide_and_legacy_indices_bootstrap_unchanged() {
    let mut m = manager();
    for x in [20., 30.] {
        m.add_circle(CircleRequest {
            mode: CircleMode::CenterDiameter,
            p1: v(x, 20.),
            p2: v(x + 10., 20.),
            ctrl_held: true,
        })
        .unwrap();
    }
    m.end_sketch().unwrap();
    let original = catalog(&m);
    assert_eq!(original.len(), 3);
    let mut model: serde_json::Value =
        serde_json::from_str(&m.export_project_model().unwrap()).unwrap();
    model["schema_version"] = 7.into();
    model.as_object_mut().unwrap().remove("print_intent");
    for s in model["sketches"].as_array_mut().unwrap() {
        s.as_object_mut().unwrap().remove("profile_identities");
    }
    let mut loaded = SketchManager::new();
    load(&mut loaded, model.to_string());
    assert_eq!(catalog(&loaded), original);
    loaded.edit_sketch("Sketch1").unwrap();
    rectangle(&mut loaded, -50., -50., 5., 5.);
    loaded.end_sketch().unwrap();
    for p in original {
        assert_eq!(
            catalog(&loaded)
                .iter()
                .find(|q| q.index == p.index)
                .unwrap()
                .curves,
            p.curves
        );
    }
}
