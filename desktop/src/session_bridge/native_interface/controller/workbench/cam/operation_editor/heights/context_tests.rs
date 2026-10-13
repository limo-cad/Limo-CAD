use super::*;

fn baseline() -> Context {
    Context {
        model: Ok((12.125, -6.25)),
        stock: (16.5, -10.75),
        holes: Err("no original holes".into()),
        selection: Err("no original chain".into()),
        geometry: HashMap::new(),
        picker: None,
        staged: HashMap::new(),
        has_holes: false,
        has_selection: false,
        modeled_top: None,
    }
}
fn operation(kind: &str, holes: Vec<CamHoleDto>) -> CamOperationDto {
    serde_json::from_value(json!({"kind":kind,"id":1,"name":"Resolved heights","tool_id":1,
        "points":[],"holes":holes,"top_z":0.,"bottom_z":-4.,"clearance_z":10.,"retract_z":5.,"feed_height_z":2.,
        "cutting":{"spindle_rpm":1000,"feed_xy":100.,"feed_z":50.},"pitch":1.,"major_diameter":8.,"minor_diameter":6.})).unwrap()
}
fn hole(top_z: f64, bottom_z: f64, associated: bool) -> CamHoleDto {
    CamHoleDto {
        point: limo_cad_cam::Point2Dto::new(0.125, 0.375),
        top_z,
        bottom_z,
        axis: [0., 0., -1.],
        face_key: associated.then(|| "11:31".into()),
    }
}
#[test]
fn resolved_drill_and_thread_heights_keep_immutable_model_stock_and_manual_spans() {
    for kind in ["drill", "thread"] {
        let original = baseline();
        let operation = operation(
            kind,
            vec![hole(-1.125, -9.375, true), hole(2.75, -12.5, false)],
        );
        let updated = original.with_resolved_holes(&operation).unwrap();
        let values = HashMap::new();
        assert_eq!(
            updated.base(CamHeightReferenceDto::HoleTop, &values),
            Ok(2.75)
        );
        assert_eq!(
            updated.base(CamHeightReferenceDto::HoleBottom, &values),
            Ok(-12.5)
        );
        assert!(updated.has_holes);
        for reference in [
            CamHeightReferenceDto::ModelTop,
            CamHeightReferenceDto::ModelBottom,
            CamHeightReferenceDto::StockTop,
            CamHeightReferenceDto::StockBottom,
            CamHeightReferenceDto::Selection,
        ] {
            assert_eq!(
                updated.base(reference, &values),
                original.base(reference, &values)
            );
        }
        assert_eq!(updated.has_selection, original.has_selection);
        assert_eq!(updated.modeled_top, original.modeled_top);
        assert!(
            original.holes.is_err(),
            "refresh cannot mutate the cached source context"
        );
        let mut stale_model = original.clone();
        stale_model.model = Err("existing model error".into());
        let refreshed = stale_model.with_resolved_holes(&operation).unwrap();
        assert_eq!(
            refreshed.model, stale_model.model,
            "a hole edit cannot clear the source model error"
        );
    }
}
#[test]
fn resolved_hole_height_refresh_rejects_empty_and_invalid_spans_without_fallback() {
    for kind in ["drill", "thread"] {
        let original = baseline();
        let empty = original
            .with_resolved_holes(&operation(kind, vec![]))
            .unwrap();
        assert!(!empty.has_holes);
        assert!(empty
            .base(CamHeightReferenceDto::HoleTop, &HashMap::new())
            .is_err());
        let manual = original
            .with_resolved_holes(&operation(kind, vec![hole(1., -3., false)]))
            .unwrap();
        assert!(
            !manual.has_holes,
            "manual spans do not invent a face association"
        );
        assert_eq!(manual.holes, Ok((1., -3.)));
        for (top, bottom) in [(f64::NAN, -3.), (1., f64::INFINITY), (1., 1.), (-3., 1.)] {
            let mut invalid = operation(kind, vec![hole(1., -3., true)]);
            match &mut invalid {
                CamOperationDto::Drill { holes, .. } | CamOperationDto::Thread { holes, .. } => {
                    holes[0].top_z = top;
                    holes[0].bottom_z = bottom;
                }
                _ => unreachable!(),
            }
            let updated = original.with_resolved_holes(&invalid).unwrap();
            assert!(updated.holes.is_err());
            assert_eq!(updated.model, original.model);
            assert_eq!(updated.stock, original.stock);
        }
    }
}
