use super::*;
use operation_editor::heights::picking;

fn draft(cam: &CamDocumentDto, scene: &limo_cad_solid::SolidSceneDto) -> Draft {
    let mut draft = Draft::new(cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, cam, scene, &[]).unwrap();
    set(&mut draft, "/native/ui/operation_section", "heights");
    set(&mut draft, "/native/heights/mode", "associative");
    set(&mut draft, "/native/heights/bottom/reference", "geometry");
    set(&mut draft, "/native/ui/height_kind/bottom", "edge");
    draft
}

fn pick(draft: &Draft) -> (picking::SelectionState, picking::Key) {
    let state = picking::snapshot(draft, &picking::button("bottom")).unwrap();
    let candidates = picking::candidates(picking::source(draft).unwrap(), &state).unwrap();
    let key = candidates
        .into_iter()
        .find(|candidate| candidate.key.reference.contains("rim-0"))
        .unwrap()
        .key;
    (state, key)
}

#[test]
fn native_cam_height_pick_stages_identity_and_converts_offset_once() {
    let mut cam = job();
    cam.units = CamUnits::Inches;
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    let before = cam.clone();
    let mut draft = draft(&cam, &geometry::scene());
    set(&mut draft, "/native/heights/bottom/offset", "-0.1");
    let (state, key) = pick(&draft);
    picking::stage(&mut draft, &state, &key).unwrap();
    assert_eq!(cam, before, "Picking is only a draft change");
    let next = draft.edited(&cam).unwrap();
    let bottom = next.height_expressions[0].bottom.as_ref().unwrap();
    assert_eq!(
        bottom.reference,
        limo_cad_cam::CamHeightReferenceDto::Geometry
    );
    assert_eq!(
        bottom.geometry,
        Some(limo_cad_cam::CamHeightGeometryDto::Edge {
            body_id: 11,
            key: "rim-0".into(),
        })
    );
    assert_eq!(bottom.offset, -2.54);
    let record = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert!((record["target_z"].as_f64().unwrap() + 3.54).abs() < 1e-12);
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.setups[0].wcs, cam.setups[0].wcs);
}

#[test]
fn native_cam_height_pick_rejects_changed_drafts_and_missing_picks() {
    let mut cam = job();
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    let mut draft = draft(&cam, &geometry::scene());
    assert!(draft.edited(&cam).unwrap_err().contains("Pick geometry"));
    let (state, key) = pick(&draft);
    set(&mut draft, "/native/heights/bottom/offset", "unfinished");
    assert!(picking::stage(&mut draft, &state, &key).is_err());
    assert_eq!(
        form::text(&draft, "/native/heights/bottom/offset").unwrap(),
        "unfinished"
    );
    set(&mut draft, "/native/heights/mode", "absolute");
    assert!(picking::snapshot(&draft, &picking::button("bottom")).is_err());
}

#[test]
fn native_cam_height_pick_survives_operation_geometry_rebuilds() {
    let cam = geometry::cam("contour2d");
    let mut draft = draft(&cam, &geometry::scene());
    set(&mut draft, "/native/heights/bottom/offset", "-2");
    let (state, key) = pick(&draft);
    picking::stage(&mut draft, &state, &key).unwrap();
    geometry::edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "model",
    );
    geometry::edit(&mut draft, &cam, "/native/geometry/chains/0/mode", "closed");
    geometry::edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/keys/0",
        "edge:11:rim-0",
    );
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.height_expressions[0].bottom.as_ref().unwrap().geometry,
        Some(limo_cad_cam::CamHeightGeometryDto::Edge {
            body_id: 11,
            key: "rim-0".into()
        })
    );
    let record = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(record["bottom_z"], -3.);
    assert!(draft.dirty());
}

#[test]
fn native_cam_height_pick_removed_geometry_fails_closed_and_can_be_repaired() {
    let mut cam = job();
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    let mut initial = draft(&cam, &geometry::scene());
    set(&mut initial, "/native/heights/bottom/offset", "-1");
    let (state, key) = pick(&initial);
    picking::stage(&mut initial, &state, &key).unwrap();
    let saved = initial.edited(&cam).unwrap();
    let mut scene = geometry::scene();
    scene.bodies[0].edges.retain(|edge| edge.key != "rim-0");
    let mut repair = draft(&saved, &scene);
    set(&mut repair, "/native/heights/bottom/offset", "-2");
    assert!(repair.edited(&saved).is_err());
    let state = picking::snapshot(&repair, &picking::button("bottom")).unwrap();
    let candidates = picking::candidates(picking::source(&repair).unwrap(), &state).unwrap();
    picking::stage(&mut repair, &state, &candidates[0].key).unwrap();
    let next = repair.edited(&saved).unwrap();
    assert_ne!(
        next.height_expressions[0].bottom,
        saved.height_expressions[0].bottom
    );
}

#[test]
fn native_cam_height_pick_offers_all_shared_geometry_types_without_dirtying_type_navigation() {
    let mut cam = job();
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    let mut scene = geometry::scene();
    scene.bodies[0].mesh.indices.extend([4, 5, 6]);
    scene.bodies[0].faces.push(
        serde_json::from_value(json!({
            "id":2, "key":"top", "first_index":6, "index_count":3,
        "plane":{"origin":[0.,0.,-1.], "u":[1.,0.,0.], "v":[0.,1.,0.], "normal":[0.,0.,1.]}
        }))
        .unwrap(),
    );
    let sketches = vec![serde_json::from_value(json!({
        "name":"Height datum", "plane":{"type":"origin_plane","plane":"xy"},
        "basis":{"origin":[0.,0.,-1.],"u":[1.,0.,0.],"v":[0.,1.,0.],"normal":[0.,0.,1.]},
        "entities":[
            {"kind":"point","id":7,"position":{"x":4.,"y":5.},"fully_defined":true},
            {"kind":"line","id":8,"start_id":71,"end_id":72,"start":{"x":2.,"y":2.},"end":{"x":20.,"y":2.},"consumed":false,"fully_defined":true}
        ], "constraints":[], "dimensions":[],
        "dimension_style":limo_cad_core::DimensionStyle::default(),
        "dof":{"value":0,"fully_defined":true}, "can_undo":false, "can_redo":false
    })).unwrap()];
    let mut draft = Draft::new(&cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, &cam, &scene, &sketches).unwrap();
    set(&mut draft, "/native/ui/operation_section", "heights");
    for kind in ["face", "edge", "vertex", "sketch_point", "sketch_line"] {
        set(&mut draft, "/native/ui/height_kind/bottom", kind);
        assert!(!draft.dirty());
        assert_eq!(draft.edited(&cam).unwrap(), cam);
    }
    set(&mut draft, "/native/heights/bottom/reference", "geometry");
    for (kind, count) in [
        ("face", 1),
        ("edge", 4),
        ("vertex", 8),
        ("sketch_point", 1),
        ("sketch_line", 1),
    ] {
        set(&mut draft, "/native/ui/height_kind/bottom", kind);
        let state = picking::snapshot(&draft, &picking::button("bottom")).unwrap();
        let candidates = picking::candidates(picking::source(&draft).unwrap(), &state).unwrap();
        assert_eq!(candidates.len(), count, "{kind}");
        for candidate in candidates {
            let reference: Value = serde_json::from_str(&candidate.key.reference).unwrap();
            assert_eq!(reference["kind"], kind);
        }
    }
}
