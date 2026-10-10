use super::*;
use operation_editor::linking_points::picking::{self, Key, Target};

fn sources(cam: &mut CamDocumentDto) -> SolidSceneDto {
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    let mut tool = cam.tools[0].clone();
    tool.id = 6;
    tool.number = Some(2);
    tool.kind = CamToolKind::Drill;
    tool.point_angle_degrees = Some(118.);
    cam.tools.push(tool);
    cam.next_tool_id = 7;
    let op = cam.setups[0].operations.remove(0);
    cam.setups[0].operations = vec![drill(1, true), drill(2, false), op, drill(8, true)];
    cam.next_operation_id = 9;
    let body = |id| json!({"id":id,"name":format!("Body {id}"),"feature_id":1,"mesh":{"positions":[12.,23.,7.,12.,23.,7.,13.,24.,7.],"normals":[],"indices":[]},"faces":[],"edges":[]});
    serde_json::from_value(json!({"bodies":[body(11),body(12)],"errors":[]})).unwrap()
}
fn snapshot(draft: &Draft, target: Target) -> picking::SelectionState {
    picking::snapshot(draft, &picking::button(target.key())).unwrap()
}
fn fields(draft: &Draft) -> Vec<(String, String, String)> {
    draft
        .fields
        .iter()
        .map(|field| {
            (
                field.path.clone(),
                field.text.clone(),
                field.original.clone(),
            )
        })
        .collect()
}

#[test]
fn linking_viewport_sources_use_real_world_positions_prior_drills_and_typed_vertex_identity() {
    let mut cam = cam();
    let scene = sources(&mut cam);
    cam.setups[0].wcs=serde_json::from_value(json!({"origin":{"x":10.,"y":20.,"z":3.},"x_axis":[0.,1.,0.],"y_axis":[1.,0.,0.],"z_axis":[0.,0.,-1.]})).unwrap();
    let draft = operation_draft(&cam, &scene);
    let predrill = picking::candidates(&draft, &snapshot(&draft, Target::Predrill)).unwrap();
    assert_eq!(predrill.len(), 2);
    assert_eq!(
        predrill[0].key,
        Key::Drill {
            operation: 1,
            index: 0
        }
    );
    assert_eq!(predrill[0].point, [10.3, 20.1, 3.]);
    assert_eq!(predrill[1].point, [10.1, 20.3, 3.]);
    let entries = picking::candidates(&draft, &snapshot(&draft, Target::Entry)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key, Key::Vertex { body: 11, index: 0 });
    assert_eq!(entries[0].point, [12., 23., 7.]);
    assert_eq!(entries[1].key, Key::Vertex { body: 11, index: 2 });
}

#[test]
fn linking_pick_appends_predrill_and_replaces_entry_exit_using_exact_inch_copy_cache() {
    let mut cam = cam();
    let scene = sources(&mut cam);
    cam.units = CamUnits::Inches;
    cam.setups[0].wcs.origin = limo_cad_cam::Point3Dto::new(10.1, 20.3, 3.);
    cam.linking[0].predrill_positions = vec![Point2Dto::new(0.1, 0.3)];
    cam.linking[0].entry_positions = vec![Point2Dto::new(4., 5.)];
    cam.linking[0].exit_positions = vec![Point2Dto::new(6., 7.)];
    let mut draft = operation_draft(&cam, &scene);
    let record = draft.record.clone();
    set(&mut draft, "/native/linking/predrill_positions/0/x", "-");
    for target in [Target::Predrill, Target::Entry, Target::Exit] {
        let state = snapshot(&draft, target);
        let candidate = picking::candidates(&draft, &state).unwrap().remove(0);
        let before = fields(&draft);
        picking::stage(&mut draft, &cam, &state, candidate.key).unwrap();
        for (path, text, baseline) in before.iter().filter(|(path, _, _)| {
            !path.starts_with(&format!("/native/linking/{}/", target.key()))
                && *path != row(target.key())
        }) {
            let next = draft
                .fields
                .iter()
                .find(|field| &field.path == path)
                .unwrap();
            assert_eq!(&next.text, text);
            assert_eq!(&next.original, baseline);
        }
        assert_eq!(draft.record, record);
    }
    assert_eq!(
        form::text(&draft, "/native/linking/predrill_positions/0/x").unwrap(),
        "-"
    );
    set(
        &mut draft,
        "/native/linking/predrill_positions/0/x",
        &cam.units.from_mm(0.1).to_string(),
    );
    let mut expected = cam.clone();
    expected.linking[0]
        .predrill_positions
        .push(Point2Dto::new(0.1, 0.3));
    let vertex = Point2Dto::new(12. - 10.1, 23. - 20.3);
    expected.linking[0].entry_positions = vec![vertex];
    expected.linking[0].exit_positions = vec![vertex];
    assert_eq!(draft.edited(&cam).unwrap(), expected);
    edit(
        &mut draft,
        &cam,
        "/native/linking/entry_positions/0/x",
        "0.25",
    );
    let changed = draft.edited(&cam).unwrap();
    assert_eq!(changed.linking[0].entry_positions[0].x, 6.35);
    assert_eq!(
        changed.linking[0].entry_positions[0].y.to_bits(),
        vertex.y.to_bits()
    );
}

#[test]
fn linking_pick_rejects_stale_owner_form_source_limits_and_invalid_candidate_atomically() {
    let mut cam = cam();
    let scene = sources(&mut cam);
    let mut draft = operation_draft(&cam, &scene);
    let expected = snapshot(&draft, Target::Entry);
    let key = Key::Vertex { body: 11, index: 0 };
    let before = fields(&draft);
    assert!(picking::stage(
        &mut draft,
        &cam,
        &expected,
        Key::Vertex { body: 12, index: 0 }
    )
    .is_err());
    assert_eq!(fields(&draft), before);
    set(&mut draft, "/native/linking/high_feed", "12");
    let before = fields(&draft);
    assert!(picking::stage(&mut draft, &cam, &expected, key).is_err());
    assert_eq!(fields(&draft), before);
    let mut reopened = operation_draft(&cam, &scene);
    let before = fields(&reopened);
    assert!(picking::stage(&mut reopened, &cam, &expected, key).is_err());
    assert_eq!(fields(&reopened), before);
    cam.setups[0].operations.swap(0, 2);
    let reordered = operation_draft(&cam, &scene);
    assert!(picking::candidates(&reordered, &snapshot(&reordered, Target::Predrill)).is_err());
    set(&mut draft, "/native/linking/predrill_positions/count", "32");
    assert!(picking::snapshot(&draft, &picking::button("predrill_positions")).is_err());
    set(
        &mut draft,
        "/native/linking/high_feed",
        &"x".repeat(1024 * 1024),
    );
    assert!(picking::snapshot(&draft, &picking::button("entry_positions")).is_err());
}

#[test]
fn linking_picker_source_scan_and_predrill_count_budgets_fail_without_truncating_saved_form() {
    let mut cam = cam();
    let mut scene = sources(&mut cam);
    scene.bodies[0].mesh.positions = vec![1.; 3 * 131_073];
    let draft = operation_draft(&cam, &scene);
    assert!(picking::snapshot(&draft, &picking::button("entry_positions")).is_err());
    if let CamOperationDto::Drill { points, .. } = &mut cam.setups[0].operations[0] {
        *points = vec![Point2Dto::new(0.1, 0.3); 4097];
    }
    let draft = operation_draft(&cam, &SolidSceneDto::default());
    assert!(picking::snapshot(&draft, &picking::button("predrill_positions")).is_err());
    let mut draft = draft;
    edit(
        &mut draft,
        &cam,
        "/native/linking/predrill_positions/count",
        "1",
    );
    let options = draft
        .fields
        .iter()
        .find(|field| field.path == "/native/linking/predrill_positions/0/candidate")
        .unwrap()
        .options
        .as_ref()
        .unwrap();
    assert_eq!(
        options.len(),
        4099,
        "The existing full chooser remains available; viewport budget never truncates it"
    );
}

#[test]
fn linking_viewport_copy_keeps_shared_closed_contour_rules_and_rejects_later_drills() {
    let mut cam = cam();
    let scene = sources(&mut cam);
    let mut draft = operation_draft(&cam, &scene);
    let state = snapshot(&draft, Target::Predrill);
    let before = fields(&draft);
    assert!(picking::stage(
        &mut draft,
        &cam,
        &state,
        Key::Drill {
            operation: 8,
            index: 0
        }
    )
    .is_err());
    assert_eq!(fields(&draft), before);
    edit(&mut draft, &cam, "/native/linking/mode", "legacy");
    assert!(!operation_editor::visible(
        &draft,
        &picking::button("entry_positions")
    ));
    assert!(picking::snapshot(&draft, &picking::button("entry_positions")).is_err());
    if let CamOperationDto::Contour2d { closed, .. } = &mut cam.setups[0].operations[2] {
        *closed = false;
    }
    let mut draft = operation_draft(&cam, &scene);
    let state = snapshot(&draft, Target::Entry);
    picking::stage(&mut draft, &cam, &state, Key::Vertex { body: 11, index: 0 }).unwrap();
    assert!(draft.edited(&cam).unwrap_err().contains("closed contour"));
}
