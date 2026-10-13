use super::center_fixture as fixture;
use super::*;
use serde_json::{json, Value};

fn document() -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
    let (mut document, scene, projection) = fixture::fixture("mark", 2., true);
    let data: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/hole-callout.json")).unwrap();
    document.sheets[0].annotations =
        vec![serde_json::from_value(data["annotation"].clone()).unwrap()];
    document.sheets[0].style.leader.width_mm = 0.55;
    document.sheets[0].style.leader.dash_mm = vec![3., 1.];
    (document, scene, projection)
}
fn export(
    document: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    projection: &DrawingProjectionDto,
    format: DrawingExportFormat,
) -> Result<String, String> {
    export_sheet(
        document,
        scene,
        &AssemblyDocumentDto::default(),
        &DrawingExportRequest {
            sheet_id: 1,
            format,
        },
        |_| Ok(projection.clone()),
    )
}
#[test]
fn hole_labels_share_the_react_contract_without_inferring_a_through_extent() {
    let data: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/hole-callout.json")).unwrap();
    for sample in data["cases"].as_array().unwrap() {
        let mut annotation = data["annotation"].clone();
        annotation
            .as_object_mut()
            .unwrap()
            .extend(sample["changes"].as_object().unwrap().clone());
        let annotation = serde_json::from_value(annotation).unwrap();
        let units = serde_json::from_value(sample["units"].clone()).unwrap();
        let standard = serde_json::from_value(sample["standard"].clone()).unwrap();
        assert_eq!(
            crate::drawing_presentation::text::hole(&annotation, units, standard),
            sample["expected"].as_str().unwrap(),
            "{}",
            sample["name"]
        );
    }
}
#[test]
fn hole_extent_is_explicit_validated_and_legacy_archives_keep_their_fields() {
    let (mut document, _, _) = document();
    let before = serde_json::to_value(&document).unwrap();
    assert!(before["sheets"][0]["annotations"][0]
        .get("through_all")
        .is_none());
    let loaded: DrawingDocumentDto = serde_json::from_value(before.clone()).unwrap();
    assert_eq!(serde_json::to_value(&loaded).unwrap(), before);
    if let DrawingAnnotationDto::HoleNote {
        through_all, depth, ..
    } = &mut document.sheets[0].annotations[0]
    {
        *through_all = Some(true);
        *depth = Some(8.);
    }
    assert!(document.validate().is_err());
    if let DrawingAnnotationDto::HoleNote { depth, .. } = &mut document.sheets[0].annotations[0] {
        *depth = None;
    }
    document.validate().unwrap();
    assert_eq!(
        serde_json::from_str::<DrawingDocumentDto>(&serde_json::to_string(&document).unwrap())
            .unwrap(),
        document
    );
}
#[test]
fn hole_export_uses_current_circle_filled_leader_mask_style_and_shared_multiline_label() {
    let (mut document, scene, projection) = document();
    if let DrawingAnnotationDto::HoleNote {
        through_all, note, ..
    } = &mut document.sheets[0].annotations[0]
    {
        *through_all = Some(true);
        *note = "Deburr\nInspect".into();
    }
    let before = serde_json::to_value((&document, &scene, &projection)).unwrap();
    let mut paper = Paper {
        size: [297., 210.],
        items: vec![],
    };
    hole::draw(
        &mut paper,
        &document.sheets[0],
        &BTreeMap::from([(1, projection.clone())]),
        &document.sheets[0].annotations[0],
        limo_cad_core::UnitSystem::Mm,
    )
    .unwrap();
    let center = paper_point(
        &document.sheets[0].views[0],
        projection.circles[0].center,
        &projection,
    );
    let Primitive::Line {
        points,
        width,
        dash,
        layer,
    } = &paper.items[0]
    else {
        panic!()
    };
    assert_eq!(
        (*width, layer, dash.as_slice()),
        (0.55, &"LEADER", &[3., 1.][..])
    );
    assert!(((points[0][0] - center[0]).hypot(points[0][1] - center[1]) - 10.).abs() < 1e-9);
    assert_eq!(points[1], [180., 110.]);
    assert!(matches!(
        paper.items[1],
        Primitive::Triangle {
            layer: "LEADER",
            ..
        }
    ));
    let labels: Vec<_> = paper
        .items
        .iter()
        .filter_map(|item| {
            if let Primitive::Text { value, point, .. } = item {
                Some((value.as_str(), *point))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        labels.iter().map(|l| l.0).collect::<Vec<_>>(),
        ["⌀6 THRU", "Deburr", "Inspect"]
    );
    assert!(
        (labels[1].1[1] - labels[0].1[1] - document.sheets[0].style.text_height_mm * 1.25).abs()
            < 1e-9
    );
    assert_eq!(
        paper
            .items
            .iter()
            .filter(|p| matches!(
                p,
                Primitive::Triangle {
                    layer: TEXT_MASK,
                    ..
                }
            ))
            .count(),
        6
    );
    let last_mask = paper
        .items
        .iter()
        .rposition(|p| {
            matches!(
                p,
                Primitive::Triangle {
                    layer: TEXT_MASK,
                    ..
                }
            )
        })
        .unwrap();
    let first_text = paper
        .items
        .iter()
        .position(|p| matches!(p, Primitive::Text { .. }))
        .unwrap();
    assert!(
        last_mask < first_text,
        "A later multiline mask must not erase earlier text"
    );
    for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
        let output = export(&document, &scene, &projection, format).unwrap();
        assert!(output.contains("THRU") && output.contains("Inspect") && output.contains("LEADER"));
        assert!(!output.contains("999.00000"));
    }
    assert_eq!(
        serde_json::to_value((&document, &scene, &projection)).unwrap(),
        before
    );
}
#[test]
fn hole_export_rejects_stale_missing_wrong_occurrence_and_open_geometry() {
    let (document, scene, projection) = document();
    for case in 0..5 {
        let mut changed = projection.clone();
        match case {
            0 => changed.circles.clear(),
            1 => changed.circles[0].edge_key = "replacement".into(),
            2 => changed.circles[0].closed = false,
            3 => {
                changed
                    .topology_signatures
                    .insert("1".into(), "changed".into());
            }
            _ => {
                changed.circles[0].occurrence_id = Some(serde_json::from_value(json!(99)).unwrap())
            }
        }
        for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
            assert!(
                export(&document, &scene, &changed, format).is_err(),
                "case {case}"
            );
        }
    }
}
