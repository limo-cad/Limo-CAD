use super::super::reorder;
use super::*;

fn ordered_job() -> CamDocumentDto {
    let (cam, _) = duplicate(&job(), Selection::Operation(7)).unwrap();
    let (mut cam, _) = duplicate(&cam, Selection::Setup(3)).unwrap();
    cam.units = CamUnits::Inches;
    cam.tools[0].overall_length = f64::from_bits(0x4049800000000001);
    cam
}

#[test]
fn native_cam_reorder_preserves_complete_records_and_keyed_intent() {
    let cam = ordered_job();
    let next = reorder::step(&cam, Selection::Operation(7), 1).unwrap();
    let mut expected = cam.clone();
    expected.setups[0].operations.swap(0, 1);
    assert_eq!(next, expected);
    assert_eq!(
        reorder::step(&next, Selection::Operation(7), -1).unwrap(),
        cam
    );
    let next = reorder::step(&cam, Selection::Setup(3), 1).unwrap();
    let mut expected = cam.clone();
    expected.setups.swap(0, 1);
    assert_eq!(next, expected);
    assert_eq!(next.active_setup_id, cam.active_setup_id);
    assert_eq!(
        next.tools[0].overall_length.to_bits(),
        cam.tools[0].overall_length.to_bits()
    );
    assert_eq!(reorder::step(&next, Selection::Setup(3), -1).unwrap(), cam);
}

#[test]
fn native_cam_reorder_rejects_cross_setup_stale_and_nonpermutation_requests() {
    let cam = ordered_job();
    let first = cam.setups[0]
        .operations
        .iter()
        .map(CamOperationDto::id)
        .collect::<Vec<_>>();
    let other = cam.setups[1].operations[0].id();
    for ids in [
        vec![],
        vec![first[0]],
        vec![first[0], first[0]],
        vec![first[0], other],
        vec![first[0], 999],
    ] {
        assert!(reorder::apply_order(&cam, Some(3), &ids).is_err());
    }
    assert!(reorder::apply_order(&cam, Some(999), &first).is_err());
    assert!(reorder::apply_order(&cam, None, &[3, 3]).is_err());
    for selection in [
        Selection::Tool(5),
        Selection::Setup(999),
        Selection::Operation(999),
    ] {
        assert!(!reorder::can_step(&cam, selection, 1));
        assert!(reorder::step(&cam, selection, 1).is_err());
    }
    assert!(!reorder::can_step(&cam, Selection::Setup(3), -1));
    assert!(!reorder::can_step(&cam, Selection::Operation(first[1]), 1));
    assert!(!reorder::can_step(&cam, Selection::Operation(first[0]), 0));
    assert!(!reorder::can_step(&cam, Selection::Operation(first[0]), 2));
    assert!(reorder::can_step(&cam, Selection::Operation(first[0]), 1));
    assert_eq!(cam, ordered_job());
}

#[test]
fn native_cam_setup_reorder_keeps_shared_rest_stock_predecessor_validation() {
    let mut cam = ordered_job();
    let source = cam.setups[0].id;
    let dependent = cam.setups[1].id;
    cam.setups[1].stock_spec = limo_cad_cam::CamStockSpecDto::RestFromSetup { setup_id: source };
    cam.setups[1].resolved_stock = limo_cad_cam::CamResolvedStockDto::Rest {
        source_setup_id: source,
    };
    cam.validate_for_editing().unwrap();
    let error = reorder::step(&cam, Selection::Setup(dependent), -1).unwrap_err();
    assert!(
        error.contains("must follow its rest-stock source"),
        "{error}"
    );
    assert_eq!(
        reorder::apply_order(&cam, None, &[source, dependent]).unwrap(),
        cam
    );
    assert!(
        reorder::step(&cam, Selection::Operation(7), 1).is_ok(),
        "Only setup order controls rest-source precedence"
    );
}

#[test]
fn native_cam_reorder_uses_existing_stamps_and_shared_stale_detection() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let cam = ordered_job();
    parse_engine_envelope(
        fixture
            .engine
            .engine_call("cam_set_document", &serde_json::to_string(&cam).unwrap()),
    )
    .unwrap();
    parse_engine_envelope(fixture.engine.engine_call("cam_regenerate_setup", "3")).unwrap();
    let current = fixture.engine.cam_document_snapshot();
    let next = reorder::step(&current, Selection::Operation(7), 1).unwrap();
    assert_eq!(next.toolpath_generations, current.toolpath_generations);
    assert_eq!(next.height_expressions, current.height_expressions);
    parse_engine_envelope(
        fixture
            .engine
            .engine_call("cam_set_document", &serde_json::to_string(&next).unwrap()),
    )
    .unwrap();
    let statuses =
        parse_engine_envelope(fixture.engine.engine_call("cam_toolpath_statuses", "")).unwrap();
    for id in current.setups[0].operations.iter().map(CamOperationDto::id) {
        assert_eq!(
            statuses
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["operation_id"] == id)
                .unwrap()["state"],
            "stale"
        );
    }
    let restored = reorder::step(&next, Selection::Operation(7), -1).unwrap();
    assert_eq!(restored, current);
    parse_engine_envelope(fixture.engine.engine_call(
        "cam_set_document",
        &serde_json::to_string(&restored).unwrap(),
    ))
    .unwrap();
    let statuses =
        parse_engine_envelope(fixture.engine.engine_call("cam_toolpath_statuses", "")).unwrap();
    for id in current.setups[0].operations.iter().map(CamOperationDto::id) {
        assert_eq!(
            statuses
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["operation_id"] == id)
                .unwrap()["state"],
            "current"
        );
    }
}
