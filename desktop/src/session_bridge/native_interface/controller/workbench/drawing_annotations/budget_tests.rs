use super::*;
use serde_json::{json, Value};

fn values(art: &Art) -> Value {
    json!({
        "segments":art.segments.iter().map(|s|json!([s.x1,s.y1,s.x2,s.y2,s.width_mm,s.hidden,s.arrow,format!("{:?}",s.ink)])).collect::<Vec<_>>(),
        "labels":art.labels.iter().map(|s|json!([s.text,s.x,s.y,s.angle,s.width_mm,s.height_mm,s.text_height_mm,s.mask,format!("{:?}",s.ink),format!("{:?}",s.align)])).collect::<Vec<_>>(),
        "fills":art.fills.iter().map(|s|json!([s.x,s.y,s.width,s.height,s.round])).collect::<Vec<_>>()
    })
}
fn valid_document(sheet: &DrawingSheetDto) -> DrawingDocumentDto {
    serde_json::from_value(json!({"sheets":[sheet],"active_sheet_id":1,"next_sheet_id":2,"next_view_id":2,"next_annotation_id":2,"next_bom_item_id":2})).unwrap()
}

#[test]
fn limits_leave_all_24_ordinary_annotation_graphics_exact() {
    let (mut sheet, projections) = tests::fixture();
    for variant in tests::variants() {
        sheet.annotations = vec![tests::annotation(variant)];
        let before = sheet.clone();
        let ordinary = try_render(&sheet, &projections, UnitSystem::Mm).unwrap();
        let generous = render_with_limits(
            &sheet,
            &projections,
            UnitSystem::Mm,
            budget::Limits {
                segments: 65_536,
                labels: 16_384,
                fills: 8_192,
                retained: 32 * 1024 * 1024,
                text: 4 * 1024 * 1024,
                scratch: 16 * 1024 * 1024,
                work: 20_000_000,
            },
        )
        .unwrap();
        assert_eq!(values(&ordinary), values(&generous));
        assert_eq!(before, sheet);
    }
}

#[test]
fn accepted_large_radial_and_angular_intent_rejects_dash_work_without_allocating_it() {
    let (mut sheet, projections) = tests::fixture();
    sheet.style.dimension.dash_mm = vec![0.05, 0.05];
    for kind in ["radial_dimension", "angular_dimension"] {
        let mut annotation = tests::variants()
            .into_iter()
            .find(|v| v["kind"] == kind)
            .unwrap();
        annotation[if kind == "radial_dimension" {
            "offset"
        } else {
            "radius"
        }] = json!(1e6);
        sheet.annotations = vec![tests::annotation(annotation)];
        let document = valid_document(&sheet);
        document.validate().unwrap();
        let before = serde_json::to_value(&document).unwrap();
        let error = try_render(&sheet, &projections, UnitSystem::Mm)
            .err()
            .unwrap();
        assert!(error.contains("work limit"), "{error}");
        assert_eq!(before, serde_json::to_value(&document).unwrap());
    }
    let mut sink = CheckedArt::default();
    sink.polyline(&[[0., 0.], [1e6, 0.]], &sheet.style.dimension, Ink::Drawing);
    assert!(sink.budget.check().unwrap_err().contains("work limit"));
    assert_eq!(
        sink.art.segments.capacity(),
        0,
        "Dash rejection allocated on-spans"
    );
}

#[test]
fn accepted_revision_cloud_rejects_scallops_before_generation() {
    let (mut sheet, projections) = tests::fixture();
    sheet.annotations = vec![tests::annotation(
        json!({"kind":"revision_cloud","revision":"A","points":[[0.,0.],[1e6,0.],[1e6,1e6],[0.,1e6]]}),
    )];
    valid_document(&sheet).validate().unwrap();
    assert!(try_render(&sheet, &projections, UnitSystem::Mm)
        .err()
        .unwrap()
        .contains("work limit"));
}

#[test]
fn limits_fail_the_whole_sheet_and_stale_references_keep_their_separate_diagnostic() {
    let (mut sheet, projections) = tests::fixture();
    let note =
        tests::annotation(json!({"kind":"note","text":"Earlier valid note","position":[20.,20.]}));
    let radial = tests::annotation(
        tests::variants()
            .into_iter()
            .find(|v| v["kind"] == "radial_dimension")
            .unwrap(),
    );
    sheet.annotations = vec![note, radial];
    for limits in [
        budget::Limits {
            segments: 1,
            ..Default::default()
        },
        budget::Limits {
            labels: 1,
            ..Default::default()
        },
        budget::Limits {
            retained: 32,
            ..Default::default()
        },
        budget::Limits {
            text: 8,
            ..Default::default()
        },
    ] {
        assert!(render_with_limits(&sheet, &projections, UnitSystem::Mm, limits).is_err());
    }
    if let DrawingAnnotationDto::RadialDimension { feature, .. } = &mut sheet.annotations[1] {
        feature.edge_key = "retired".into();
    }
    let art = try_render(&sheet, &projections, UnitSystem::Mm).unwrap();
    assert_eq!(
        art.labels
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        vec!["Earlier valid note", "!"]
    );
}

#[test]
fn finite_saved_text_height_cannot_publish_infinite_ui_coordinates() {
    let (mut sheet, projections) = tests::fixture();
    sheet.style.text_height_mm = 1e100;
    sheet.annotations = vec![tests::annotation(
        json!({"kind":"note","text":"Finite input","position":[20.,20.]}),
    )];
    valid_document(&sheet).validate().unwrap();
    assert!(try_render(&sheet, &projections, UnitSystem::Mm)
        .err()
        .unwrap()
        .contains("finite render coordinates"));
}

#[test]
fn single_pass_marks_reuse_radial_angular_label_geometry() {
    let (mut sheet, projections) = tests::fixture();
    for kind in ["radial_dimension", "angular_dimension"] {
        sheet.annotations = vec![tests::annotation(
            tests::variants()
                .into_iter()
                .find(|v| v["kind"] == kind)
                .unwrap(),
        )];
        let art = try_render(&sheet, &projections, UnitSystem::Mm).unwrap();
        assert_eq!(art.marks.len(), 1);
        assert_eq!(
            art.marks[0].center,
            [art.labels[0].x as f64, art.labels[0].y as f64]
        );
        assert_eq!(
            art.marks[0].size,
            [
                art.labels[0].width_mm as f64,
                art.labels[0].height_mm as f64
            ]
        );
        assert_eq!(art.marks[0].radial.is_some(), kind == "radial_dimension");
        assert_eq!(art.marks[0].angular.is_some(), kind == "angular_dimension");
    }
}

#[test]
fn annotations_view_names_and_source_captions_share_one_final_label_limit() {
    let (mut sheet, mut projections) = tests::fixture();
    sheet.annotations.clear();
    let source = Label {
        text: "Source".into(),
        x: 20.,
        y: 20.,
        width_mm: 16.,
        height_mm: 5.,
        text_height_mm: 3.5,
        ..Default::default()
    };
    let sources = vec![source; budget::Limits::default().labels - 1];
    let art = try_render_decorated(&sheet, &projections, UnitSystem::Mm, &sources).unwrap();
    assert_eq!(art.labels.len(), budget::Limits::default().labels);
    assert_eq!(
        art.labels[0].text,
        format!("{} \u{b7} 2:1", projections[&1].0.name)
    );
    assert!(art.labels[1..].iter().all(|label| label.text == "Source"));
    sheet.annotations.push(tests::annotation(
        json!({"kind":"note","text":"One saved note","position":[20.,20.]}),
    ));
    let before = sheet.clone();
    let error = try_render_decorated(&sheet, &projections, UnitSystem::Mm, &sources)
        .err()
        .unwrap();
    assert!(error.contains("primitive limit"), "{error}");
    assert_eq!(sheet, before, "Aggregate rejection altered saved intent");
    sheet.annotations.clear();
    projections.get_mut(&1).unwrap().0.name = "x".repeat(budget::Limits::default().text + 1);
    assert!(
        try_render_decorated(&sheet, &projections, UnitSystem::Mm, &[])
            .err()
            .unwrap()
            .contains("text limit")
    );
}
