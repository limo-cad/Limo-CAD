use super::*;
use limo_cad_solid::SolidSceneDto;

pub(super) fn scene() -> SolidSceneDto {
    let vertices = [
        [2., 2., -1.],
        [20., 2., -1.],
        [20., 10., -1.],
        [2., 10., -1.],
    ];
    let edges=(0..4).map(|i|json!({"id":i+1,"key":format!("rim-{i}"),"points":vertices[i..=i].iter().chain(vertices[(i+1)%4..=(i+1)%4].iter())
        .map(|p|json!({"x":p[0],"y":p[1],"z":p[2]})).collect::<Vec<_>>()})).collect::<Vec<_>>();
    serde_json::from_value(json!({"bodies":[{"id":11,"name":"Test block","feature_id":1,
        "mesh":{"positions":[0.,0.,-6.,20.,0.,-6.,20.,10.,-6.,0.,10.,-6.,0.,0.,-1.,20.,0.,-1.,20.,10.,-1.,0.,10.,-1.],"normals":[],"indices":[0,1,4,1,4,5]},
        "faces":[{"id":1,"key":"cylinder","first_index":0,"index_count":6,"cylinder":{"origin":{"x":5.,"y":5.,"z":-3.},"axis":{"x":0.,"y":0.,"z":1.},"reference":{"x":1.,"y":0.,"z":0.},"radius":1.5}}],"edges":edges}],"errors":[]})).unwrap()
}
pub(super) fn cam(kind: &str) -> CamDocumentDto {
    let mut cam = job();
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    cam.height_expressions.clear();
    let mut record = json!({"id":7,"name":"Geometry","kind":kind,"tool_id":5,"enabled":true,"top_z":-1.,"bottom_z":-4.,"step_down":1.,"step_over":2.,"clearance_z":8.,"retract_z":2.,"feed_height_z":1.,"cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.},"points":[{"x":4.,"y":4.}],"holes":[],
        "path":[{"x":2.,"y":2.},{"x":20.,"y":2.},{"x":20.,"y":10.},{"x":2.,"y":10.}],"closed":true,"compensation":"outside","outline":[{"x":2.,"y":2.},{"x":20.,"y":2.},{"x":20.,"y":10.},{"x":2.,"y":10.}],"chamfer_width":0.5,"wall_side":"inside","tip_offset":0.2});
    if kind == "thread" {
        record.as_object_mut().unwrap().remove("step_over");
        record["pitch"] = json!(1.);
        record["major_diameter"] = json!(8.);
        record["minor_diameter"] = json!(6.);
    }
    if kind == "chamfer2d" {
        cam.tools[0].kind = limo_cad_cam::CamToolKind::ChamferMill;
        cam.tools[0].point_angle_degrees = Some(90.);
        record["additional_chains"] = json!([]);
    }
    cam.setups[0].operations[0] = serde_json::from_value(record).unwrap();
    cam.validate_for_editing().unwrap();
    cam
}
pub(super) fn draft(cam: &CamDocumentDto) -> Draft {
    let mut draft = Draft::new(cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, cam, &scene(), &[]).unwrap();
    draft
}
pub(super) fn edit(draft: &mut Draft, cam: &CamDocumentDto, path: &str, value: &str) {
    set(draft, path, value);
    operation_editor::changed(draft, cam, path).unwrap();
}

#[test]
fn native_cam_model_chain_choices_use_shared_resolution_and_preserve_other_geometry() {
    let cam = cam("contour2d");
    let mut draft = draft(&cam);
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/source",
        "model",
    );
    edit(&mut draft, &cam, "/native/geometry/chains/0/mode", "closed");
    let field = draft
        .fields
        .iter()
        .find(|field| field.path == "/native/geometry/chains/0/keys/0")
        .unwrap();
    assert!(field
        .options
        .as_ref()
        .unwrap()
        .iter()
        .any(|option| option.label.contains("Test block") && option.value == "edge:11:rim-0"));
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/keys/0",
        "edge:11:rim-0",
    );
    let next = draft.edited(&cam).unwrap();
    let actual = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(actual["chain_ref"]["source"], "model");
    assert_eq!(actual["chain_ref"]["keys"].as_array().unwrap().len(), 4);
    assert_eq!(
        actual["path"],
        serde_json::to_value(&cam.setups[0].operations[0]).unwrap()["path"]
    );
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.setups[0].wcs, cam.setups[0].wcs);
    assert_eq!(next.toolpath_generations, cam.toolpath_generations);
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/keys/0",
        "edge:11:missing",
    );
    assert!(
        draft.edited(&cam).is_err(),
        "Broken provenance must not retarget a different edge"
    );
}

#[test]
fn native_cam_manual_point_rows_are_lazy_and_preserve_untouched_inch_precision() {
    let mut cam = cam("contour2d");
    cam.units = CamUnits::Inches;
    let mut record = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    record["path"][1]["x"] = json!(0.1);
    cam.setups[0].operations[0] = serde_json::from_value(record.clone()).unwrap();
    let mut draft = draft(&cam);
    let fields = draft.fields.len();
    edit(
        &mut draft,
        &cam,
        "/native/ui/geometry_rowchains/0/points",
        "4",
    );
    assert!(!draft.dirty(), "Selecting a point row is presentation only");
    assert_eq!(
        draft.fields.len(),
        fields + 2,
        "Only the selected row should allocate coordinate fields"
    );
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/points/3/x",
        "0.2",
    );
    let next = draft.edited(&cam).unwrap();
    let actual = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(actual["path"][3]["x"], 5.08);
    assert_eq!(
        actual["path"][1]["x"].as_f64().unwrap().to_bits(),
        0.1_f64.to_bits()
    );
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/0/points/count",
        "5",
    );
    assert!(
        draft.edited(&cam).is_err(),
        "Adding a point requires entered coordinates"
    );
}

#[test]
fn native_cam_hole_choices_share_current_face_span_and_retain_manual_centers() {
    let cam = cam("drill");
    let mut draft = draft(&cam);
    edit(&mut draft, &cam, "/native/geometry/hole_count", "1");
    assert!(
        draft.edited(&cam).is_err(),
        "A hole face must be explicitly selected"
    );
    edit(&mut draft, &cam, "/native/geometry/holes/0/face", "11:1");
    let next = draft.edited(&cam).unwrap();
    let actual = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(
        actual["holes"][0],
        json!({"point":{"x":5.,"y":5.},"top_z":-1.,"bottom_z":-6.,"axis":[0.,0.,1.],"face_key":"11:1"})
    );
    assert_eq!(
        actual["points"],
        serde_json::to_value(&cam.setups[0].operations[0]).unwrap()["points"]
    );
    assert!(draft
        .fields
        .iter()
        .find(|field| field.path == "/native/heights/top/reference")
        .unwrap()
        .options
        .as_ref()
        .unwrap()
        .iter()
        .any(|option| option.value == "hole_top"));
}

#[test]
fn native_cam_chamfer_geometry_edits_preserve_other_chain_and_independent_top() {
    let mut cam = cam("chamfer2d");
    let mut record = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    record["additional_chains"] = json!([{"path":[{"x":3.,"y":3.},{"x":15.,"y":3.},{"x":15.,"y":9.},{"x":3.,"y":9.}],"closed":true,"chain_ref":null,"modeled_chamfer":null,"top_z":-3.,"chamfer_width":0.25,"wall_side":"inside"}]);
    cam.setups[0].operations[0] = serde_json::from_value(record.clone()).unwrap();
    let mut draft = draft(&cam);
    edit(&mut draft, &cam, "/native/ui/geometry_chain", "2");
    assert!(!draft.dirty());
    edit(
        &mut draft,
        &cam,
        "/native/geometry/chains/1/chamfer_width",
        "0.4",
    );
    let next = draft.edited(&cam).unwrap();
    let actual = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    let mut expected = record;
    expected["additional_chains"][0]["chamfer_width"] = json!(0.4);
    assert_eq!(
        actual, expected,
        "Editing one chain must not overwrite independent top planes"
    );
}

#[test]
fn native_cam_failed_creation_setup_change_cannot_commit_the_previous_setup() {
    let mut cam = cam("contour2d");
    cam.setups[0].operations.clear();
    let mut unavailable = cam.setups[0].clone();
    unavailable.id = 4;
    unavailable.name = "Missing model".into();
    unavailable.body_ids = vec![limo_cad_core::BodyId(999)];
    cam.setups.push(unavailable);
    cam.next_setup_id = 5;
    let mut draft = creation::draft(
        Tab::Toolpaths,
        &cam,
        creation::Context::new(&scene(), &cam).unwrap(),
    );
    set(&mut draft, "/native/create/setup_id", "3");
    set(&mut draft, "/tool_id", "5");
    creation::seed_choices(&mut draft, &cam).unwrap();
    assert!(creation::create(&draft, &cam).is_ok());
    set(&mut draft, "/native/create/setup_id", "4");
    assert!(creation::seed_choices(&mut draft, &cam).is_err());
    assert!(
        creation::create(&draft, &cam).is_err(),
        "A failed selection must not commit to the previously selected setup"
    );
}

#[test]
fn native_cam_each_toolpath_type_can_be_created_only_with_explicit_geometry() {
    for kind in [
        "face",
        "contour2d",
        "pocket2d",
        "chamfer2d",
        "drill",
        "thread",
        "adaptive3d",
        "flat3d",
    ] {
        let mut cam = cam("contour2d");
        cam.setups[0].operations.clear();
        if kind == "chamfer2d" {
            cam.tools[0].kind = limo_cad_cam::CamToolKind::ChamferMill;
            cam.tools[0].point_angle_degrees = Some(90.);
        }
        if kind == "drill" {
            cam.tools[0].kind = limo_cad_cam::CamToolKind::Drill;
            cam.tools[0].point_angle_degrees = Some(118.);
        }
        if kind == "thread" {
            cam.tools[0].kind = limo_cad_cam::CamToolKind::ThreadMill;
        }
        let mut draft = creation::draft(
            Tab::Toolpaths,
            &cam,
            creation::Context::new(&scene(), &cam).unwrap(),
        );
        set(&mut draft, "/native/create/kind", kind);
        set(&mut draft, "/native/create/setup_id", "3");
        set(&mut draft, "/tool_id", "5");
        creation::seed_choices(&mut draft, &cam).unwrap();
        if !matches!(kind, "face" | "adaptive3d" | "flat3d") {
            assert!(
                creation::create(&draft, &cam).is_err(),
                "{kind} must not guess geometry"
            );
        }
        if matches!(kind, "contour2d" | "pocket2d" | "chamfer2d") {
            edit(
                &mut draft,
                &cam,
                "/native/geometry/chains/0/source",
                "model",
            );
            edit(&mut draft, &cam, "/native/geometry/chains/0/mode", "closed");
            edit(
                &mut draft,
                &cam,
                "/native/geometry/chains/0/keys/0",
                "edge:11:rim-0",
            );
        }
        if matches!(kind, "drill" | "thread") {
            edit(&mut draft, &cam, "/native/geometry/hole_count", "1");
            edit(&mut draft, &cam, "/native/geometry/holes/0/face", "11:1");
        }
        if kind == "thread" {
            for (path, value) in [
                ("/pitch", "1.25"),
                ("/major_diameter", "10"),
                ("/minor_diameter", "8"),
            ] {
                set(&mut draft, path, value);
            }
        }
        let (next, selection) =
            creation::create(&draft, &cam).unwrap_or_else(|error| panic!("{kind}: {error}"));
        assert_eq!(selection, Selection::Operation(8));
        assert_eq!(next.setups[0].operations.len(), 1);
        assert_eq!(next.setups[0].operations[0].id(), 8);
        assert!(next
            .height_expressions
            .iter()
            .all(|entry| entry.operation_id == 8));
        assert_eq!(next.tools, cam.tools);
        assert_eq!(next.toolpath_generations, cam.toolpath_generations);
    }
}

#[test]
fn cam_forms_share_viewport_geometry_and_release_their_retained_scene() {
    let cam = cam("contour2d");
    let scene = std::sync::Arc::new(scene());
    let retired = std::sync::Arc::downgrade(&scene);
    let mut existing = Draft::new(&cam, Selection::Operation(7)).unwrap();
    operation_editor::extend_shared(&mut existing, &cam, &scene, &[]).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &operation_editor::geometry(&existing).unwrap().scene,
        &scene,
    ));

    let mut creating = creation::draft(
        Tab::Toolpaths,
        &cam,
        creation::Context::shared(&scene, &cam).unwrap(),
    );
    set(&mut creating, "/native/create/setup_id", "3");
    set(&mut creating, "/tool_id", "5");
    for kind in ["contour2d", "pocket2d", "contour2d"] {
        set(&mut creating, "/native/create/kind", kind);
        creation::seed_choices(&mut creating, &cam).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &operation_editor::geometry(&creating).unwrap().scene,
            &scene,
        ));
    }

    // A replacement viewport can release its reference while an open draft
    // keeps a coherent old scene. Closing both drafts releases that snapshot.
    drop(scene);
    assert!(retired.upgrade().is_some());
    drop(existing);
    assert!(retired.upgrade().is_some());
    drop(creating);
    assert!(retired.upgrade().is_none());
}

#[test]
fn inactive_cam_geometry_retires_only_its_owner_and_preserves_dirty_drafts() {
    for dirty in [false, true] {
        let cam = cam("contour2d");
        let scene = std::sync::Arc::new(scene());
        let retired = std::sync::Arc::downgrade(&scene);
        let mut draft = Draft::new(&cam, Selection::Operation(7)).unwrap();
        operation_editor::extend_shared(&mut draft, &cam, &scene, &[]).unwrap();
        if dirty {
            set(&mut draft, "/name", "Unapplied name");
        }
        assert_eq!(draft.dirty(), dirty);
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: "cam-document".into(),
            epoch: 3,
        };
        let mut world = World::new();
        world.insert_resource(Editor {
            owner: Some(owner.clone()),
            cam,
            draft: Some(draft),
            ..Default::default()
        });
        drop(scene);
        let mut foreign = owner.clone();
        foreign.window_id = "another-window".into();
        super::super::super::retire_document(&mut world, &foreign);
        assert!(retired.upgrade().is_some());
        super::super::super::evict_document_geometry(&mut world, &owner);
        assert_eq!(retired.upgrade().is_some(), dirty);
        if dirty {
            assert!(world.resource::<Editor>().draft.as_ref().unwrap().dirty());
            super::super::super::retire_document(&mut world, &owner);
            assert!(retired.upgrade().is_none());
        }
        assert!(!world.contains_resource::<Editor>());
    }
}
