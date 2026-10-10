use super::super::tests::{drain, path, setup};
use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

fn mutate(fixture: &Fixture, operation: &str, arguments: Value) {
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            operation,
            &arguments,
            || Ok(()),
        )
        .unwrap();
}
fn model(fixture: &Fixture) -> Value {
    parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()
}
fn seed(fixture: &Fixture) {
    for (operation, arguments) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
        ),
        (
            "drawing_create_sheet",
            json!({"name":"Real solid","format":"a4","orientation":"landscape"}),
        ),
        (
            "drawing_add_view",
            json!({"sheet_id":1,"view":{"name":"Top","kind":"top","body_ids":[1],"direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,70.],"scale":1.}}),
        ),
    ] {
        mutate(fixture, operation, arguments);
    }
}

#[test]
fn drawing_files_match_engine_output_without_changing_history_or_project_destination() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    seed(&fixture);
    let (mut app, services, handle) = setup(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let project = path("drawing-output-project.limo");
    request(
        app.world_mut(),
        &handle,
        &services,
        &fixture.owner(),
        &json!({"command":"save","path":project}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let before = model(&fixture);
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    for format in [Format::Svg, Format::Dxf] {
        let destination = path(&format!("real-sheet.{}", format.extension()));
        let expected = parse_engine_envelope(
            fixture
                .engine
                .drawing_export(&json!({"sheet_id":1,"format":format.extension()}).to_string()),
        )
        .unwrap();
        request(
            app.world_mut(),
            &handle,
            &services,
            &fixture.owner(),
            &json!({"command":format!("export_drawing_{}",format.extension()),"path":destination}),
        )
        .unwrap();
        let result = drain(app.world_mut(), &services).unwrap();
        assert_eq!(result["sheet_id"], 1);
        assert_eq!(result["exported"], true);
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            expected["content"].as_str().unwrap()
        );
        assert_eq!(model(&fixture), before);
        assert_eq!(
            current(app.world(), &services, &fixture.owner()).unwrap(),
            receipt
        );
        let tab = tabs(app.world(), &services, &fixture.owner())
            .unwrap()
            .remove(0);
        assert_eq!(tab.path, Some(project.clone()));
        assert!(!tab.dirty);
        request(
            app.world_mut(),
            &handle,
            &services,
            &fixture.owner(),
            &json!({"command":format!("export_drawing_{}",format.extension()),"path":destination}),
        )
        .unwrap();
        assert!(drain(app.world_mut(), &services).is_err());
        assert_eq!(
            std::fs::read_to_string(destination).unwrap(),
            expected["content"].as_str().unwrap()
        );
    }
}

#[test]
fn drawing_picker_cancellation_and_stale_receipts_preserve_files() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _) = setup(&fixture);
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    assert!(capture(&services, &receipt, Format::Dxf)
        .unwrap_err()
        .contains("Select a drawing sheet"));
    seed(&fixture);
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let intent = capture(&services, &receipt, Format::Svg).unwrap();
    let before = model(&fixture);
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<Files>().picker = Some(Picker {
        receipt: receipt.clone(),
        kind: PickerKind::Drawing(intent.clone()),
        result: Mutex::new(receive),
    });
    send.send(None).unwrap();
    poll(app.world_mut(), &services).unwrap();
    assert!(!awaiting(app.world()));
    assert!(!worker::busy(app.world()));
    assert_eq!(model(&fixture), before);
    fixture
        .rename(&fixture.owner(), "Changed while choosing")
        .unwrap();
    let destination = path("stale-sheet.svg");
    std::fs::write(&destination, b"Retain prior output").unwrap();
    export(app.world_mut(), receipt, intent, destination.clone(), true).unwrap();
    assert!(drain(app.world_mut(), &services)
        .unwrap_err()
        .contains("document changed"));
    assert_eq!(std::fs::read(destination).unwrap(), b"Retain prior output");
}

#[test]
fn stale_annotation_references_fail_before_replacing_drawing_output() {
    rejected_annotation_preserves_output(
        json!({
            "kind":"center_line_between_edges", "id":1, "view_id":1, "extension":4.,
            "first":{"body_id":1,"edge_id":999,"edge_key":"missing-first","fallback_start":[0.,0.,0.],"fallback_end":[40.,0.,0.]},
            "second":{"body_id":1,"edge_id":998,"edge_key":"missing-second","fallback_start":[0.,25.,0.],"fallback_end":[40.,25.,0.]}
        }),
        "stale or incompatible",
    );
}

#[test]
fn partially_clipped_cloud_caption_fails_before_replacing_drawing_output() {
    rejected_annotation_preserves_output(
        json!({"kind":"revision_cloud","id":1,"revision":"A","points":[[20.,1.],[40.,20.],[30.,40.]]}),
        "caption extends outside",
    );
}

fn rejected_annotation_preserves_output(annotation: Value, expected_error: &str) {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    seed(&fixture);
    let mut drawing = fixture.engine.drawing_snapshot();
    drawing.sheets[0]
        .annotations
        .push(serde_json::from_value(annotation).unwrap());
    drawing.next_annotation_id = 2;
    mutate(
        &fixture,
        "drawing_set_document",
        serde_json::to_value(drawing).unwrap(),
    );
    let (mut app, services, _) = setup(&fixture);
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let before = model(&fixture);
    for format in [Format::Svg, Format::Dxf] {
        let intent = capture(&services, &receipt, format).unwrap();
        assert!(export(
            app.world_mut(),
            receipt.clone(),
            intent.clone(),
            "relative.svg".into(),
            true
        )
        .is_err());
        let destination = path(&format!("unsupported.{}", format.extension()));
        std::fs::write(&destination, b"Previous reviewed drawing").unwrap();
        export(
            app.world_mut(),
            receipt.clone(),
            intent,
            destination.clone(),
            true,
        )
        .unwrap();
        assert!(drain(app.world_mut(), &services)
            .unwrap_err()
            .contains(expected_error));
        assert_eq!(
            std::fs::read(destination).unwrap(),
            b"Previous reviewed drawing"
        );
        assert_eq!(model(&fixture), before);
        assert_eq!(
            current(app.world(), &services, &fixture.owner()).unwrap(),
            receipt
        );
    }
}
