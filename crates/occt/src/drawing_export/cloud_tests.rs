use super::*;
use crate::drawing_presentation::{cloud as geometry, text};
use serde_json::json;
#[path = "../../tests/support/cloud_export.rs"]
mod fixture;

fn export(
    doc: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    format: DrawingExportFormat,
) -> Result<String, String> {
    export_sheet(
        doc,
        scene,
        &Default::default(),
        &DrawingExportRequest {
            sheet_id: 1,
            format,
        },
        |_| Err("Cloud asked for an unrelated projection".into()),
    )
}
fn cloud_record(doc: &DrawingDocumentDto) -> (&str, &[P]) {
    let DrawingAnnotationDto::RevisionCloud {
        revision, points, ..
    } = &doc.sheets[0].annotations[0]
    else {
        panic!("Cloud missing")
    };
    (revision, points)
}

#[test]
fn triangle_quad_and_loaded_polygon_export_exact_native_scallops_and_preserve_intent() {
    for kind in ["triangle", "quad", "loaded-seven"] {
        let (doc, scene) = fixture::fixture(kind);
        let before = serde_json::to_string(&(&doc, &scene)).unwrap();
        let (revision, points) = cloud_record(&doc);
        let native = geometry::Cloud::new(points).unwrap();
        let art = cloud::draw(
            [297., 210.],
            revision,
            points,
            &mut PaperGraphicsBudget::default(),
        )
        .unwrap();
        let actual: Vec<_> = art
            .iter()
            .filter_map(|p| match p {
                PaperPrimitive::Line {
                    points,
                    layer,
                    width,
                    dash,
                } => {
                    assert_eq!(*layer, "REVISION");
                    assert_eq!(*width, 0.45);
                    assert!(dash.is_empty());
                    Some([points[0], points[1]])
                }
                _ => None,
            })
            .collect();
        let expected: Vec<_> = native
            .arcs()
            .flat_map(|a| {
                a.points()
                    .windows(2)
                    .map(|p| [p[0], p[1]])
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(actual, expected);
        let labels: Vec<_> = art
            .iter()
            .filter_map(|p| {
                if let PaperPrimitive::Text {
                    point,
                    value,
                    layer,
                    height,
                    ..
                } = p
                {
                    assert_eq!(*layer, "REVISION");
                    assert_eq!(*height, 3.2);
                    Some((*point, value.clone()))
                } else {
                    None
                }
            })
            .collect();
        let baseline = native.caption_baseline(&format!("REV {revision}"));
        assert_eq!(labels[0], (baseline, "REV B<2> & cω".into()));
        assert_eq!(
            labels[1],
            ([baseline[0], baseline[1] + 4.], "Café 零件".into())
        );
        let svg = export(&doc, &scene, DrawingExportFormat::Svg).unwrap();
        assert!(svg.contains("fill=\"#c43b4d\""));
        assert!(svg.contains("stroke=\"#c43b4d\" stroke-width=\"0.45\""));
        assert!(svg.contains("REV B&lt;2&gt; &amp; cω"));
        let dxf = export(&doc, &scene, DrawingExportFormat::Dxf).unwrap();
        assert!(dxf.contains("420\n12860237\n"));
        assert!(dxf.contains("0\nTEXT\n8\nREVISION\n"));
        assert!(dxf.contains("370\n40\n") || dxf.contains("370\n50\n"));
        assert!(dxf.contains("REV B<2> & cω"));
        assert_eq!(export(&doc, &scene, DrawingExportFormat::Svg).unwrap(), svg);
        assert_eq!(serde_json::to_string(&(&doc, &scene)).unwrap(), before);
    }
}

#[test]
fn clouds_clip_strokes_without_rewriting_vertices_and_reject_partial_captions() {
    let (doc, scene) = fixture::fixture("clipped-right");
    let (revision, points) = cloud_record(&doc);
    assert!(points.iter().any(|p| p[0] > 297.));
    let art = cloud::draw(
        [297., 210.],
        revision,
        points,
        &mut PaperGraphicsBudget::default(),
    )
    .unwrap();
    assert!(art.iter().any(|p|matches!(p,PaperPrimitive::Line{points,..} if points.iter().any(|p|(p[0]-297.).abs()<1e-9))));
    assert!(art.iter().all(|p| match p {
        PaperPrimitive::Line { points, .. } => points
            .iter()
            .all(|p| (0. ..=297.).contains(&p[0]) && (0. ..=210.).contains(&p[1])),
        _ => true,
    }));
    for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
        assert!(export(&doc, &scene, format).is_ok());
        let (mut outside, scene) = fixture::fixture("triangle");
        if let DrawingAnnotationDto::RevisionCloud { points, .. } =
            &mut outside.sheets[0].annotations[0]
        {
            points[0][1] = 1.;
        }
        outside.validate().unwrap();
        assert!(export(&outside, &scene, format)
            .unwrap_err()
            .contains("caption extends outside"));
    }
    let bounds = text::label_bounds([260., 33.], "REV B", 3.2, 1.);
    assert!(bounds[0] > 0. && bounds[2] < 297.);
}

#[test]
fn generation_budget_fails_before_tessellation_and_accumulates_across_clouds() {
    let limits = PaperGraphicsLimits {
        work: 1_000,
        ..Default::default()
    };
    let mut budget = PaperGraphicsBudget::new(limits);
    assert!(cloud::draw(
        [297., 210.],
        "A",
        &[[20., 20.], [1e6, 20.], [1e6, 1e6]],
        &mut budget
    )
    .unwrap_err()
    .contains("work limit"));
    assert_eq!(budget.usage().primitives, 0);
    assert_eq!(budget.usage().retained_bytes, 0);
    assert_eq!(budget.usage().peak_scratch_bytes, 0);
    let points = [[20., 20.], [40., 20.], [30., 40.]];
    let mut first = PaperGraphicsBudget::default();
    cloud::draw([297., 210.], "A", &points, &mut first).unwrap();
    let mut combined = PaperGraphicsBudget::new(PaperGraphicsLimits {
        work: first.usage().work + 1,
        ..Default::default()
    });
    cloud::draw([297., 210.], "A", &points, &mut combined).unwrap();
    assert!(cloud::draw([297., 210.], "B", &points, &mut combined).is_err());
    for bad in [
        vec![[1., 1.]; 2],
        vec![[f64::NAN, 1.]; 3],
        vec![[1., 1.]; 4097],
    ] {
        assert!(cloud::draw([297., 210.], "A", &bad, &mut PaperGraphicsBudget::default()).is_err());
    }
}

#[test]
fn cloud_presentation_is_fixed_and_other_unsupported_annotations_still_fail() {
    let (mut doc, scene) = fixture::fixture("quad");
    let (revision, points) = cloud_record(&doc);
    let before = cloud::draw(
        [297., 210.],
        revision,
        points,
        &mut PaperGraphicsBudget::default(),
    )
    .unwrap();
    doc.sheets[0].style.visible.width_mm = 1.1;
    doc.sheets[0].style.text_height_mm = 7.;
    let (revision, points) = cloud_record(&doc);
    assert_eq!(
        before,
        cloud::draw(
            [297., 210.],
            revision,
            points,
            &mut PaperGraphicsBudget::default()
        )
        .unwrap()
    );
    doc.sheets[0].views.push(serde_json::from_value(json!({"id":1,"name":"Unsupported symmetry","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,70.],"scale":1.})).unwrap());
    doc.next_view_id = 2;
    doc.sheets[0].annotations.push(serde_json::from_value(json!({"kind":"automatic_symmetry_axis","id":3,"view_id":1,"axis":"both","extension":4.})).unwrap());
    doc.next_annotation_id = 4;
    doc.validate().unwrap();
    let error = draw_annotation(
        &mut Paper {
            size: [297., 210.],
            items: Vec::new(),
        },
        &doc.sheets[0],
        &BTreeMap::new(),
        &doc.sheets[0].annotations[2],
        limo_cad_core::UnitSystem::Mm,
    )
    .unwrap_err();
    assert!(error.contains("does not yet support annotation 3"));
    let _ = scene;
}
