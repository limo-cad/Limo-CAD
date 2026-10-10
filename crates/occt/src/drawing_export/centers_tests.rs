use super::center_fixture as fixture;
use super::*;
use serde_json::json;

#[test]
fn dxf_custom_strokes_use_supported_pen_weights() {
    for (width, expected) in [(0.18, 18), (0.36, 35), (0.55, 53), (0.59, 60), (3., 211)] {
        assert_eq!(dxf_lineweight(width), expected);
    }
    let (doc, scene, projection) = fixture::fixture("mark", 1., true);
    let output = export(&doc, &scene, &projection, DrawingExportFormat::Dxf).unwrap();
    assert!(output.contains("370\n53\n"));
    assert!(output.contains("370\n35\n"));
    assert!(!output.contains("370\n55\n"));
    assert!(!output.contains("370\n36\n"));
}

#[test]
fn automatic_caption_clears_current_center_strokes_at_both_scales() {
    for kind in ["mark", "line"] {
        for scale in [1., 2.] {
            for custom in [false, true] {
                let (mut doc, scene, projection) = fixture::fixture(kind, scale, custom);
                let sheet = &doc.sheets[0];
                let view = &sheet.views[0];
                let normal = view.position[1]
                    + (projection.bounds[3] - projection.bounds[1]) * 0.5 * view.scale
                    + 6.;
                let baseline = centers::caption_baseline(normal, sheet, view, &projection).unwrap();
                let art = art(&doc, &projection).unwrap();
                let bottom = art
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Primitive::Line { points, width, .. } => Some(
                            points
                                .iter()
                                .map(|p| p[1])
                                .fold(f64::NEG_INFINITY, f64::max)
                                + width * 0.5,
                        ),
                        _ => None,
                    })
                    .fold(f64::NEG_INFINITY, f64::max);
                assert!(baseline - sheet.style.small_text_height_mm >= bottom + 1. - 1e-9);
                let svg = export(&doc, &scene, &projection, DrawingExportFormat::Svg).unwrap();
                let caption = svg.lines().find(|line| line.contains("(scale ")).unwrap();
                let actual_baseline = caption
                    .split("y=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap();
                assert!(actual_baseline >= baseline - 1e-5);
                let dxf = export(&doc, &scene, &projection, DrawingExportFormat::Dxf).unwrap();
                assert!(dxf.contains(&format!("20\n{:.5}\n40\n", 210. - actual_baseline)));
                doc.sheets[0].annotations.clear();
                let sheet = &doc.sheets[0];
                assert_eq!(
                    centers::caption_baseline(normal, sheet, &sheet.views[0], &projection).unwrap(),
                    normal
                );
            }
        }
    }
}

fn export(
    doc: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    projection: &DrawingProjectionDto,
    format: DrawingExportFormat,
) -> Result<String, String> {
    export_sheet(
        doc,
        scene,
        &AssemblyDocumentDto::default(),
        &DrawingExportRequest {
            sheet_id: 1,
            format,
        },
        |_| Ok(projection.clone()),
    )
}
fn art(doc: &DrawingDocumentDto, projection: &DrawingProjectionDto) -> Result<Paper, String> {
    let mut paper = Paper {
        size: [297., 210.],
        items: vec![],
    };
    centers::draw(
        &mut paper,
        &doc.sheets[0],
        &BTreeMap::from([(1, projection.clone())]),
        &doc.sheets[0].annotations[0],
    )?;
    Ok(paper)
}

#[test]
fn center_exports_preserve_current_geometry_scale_style_and_white_ring() {
    for kind in ["mark", "line"] {
        for scale in [1., 2.] {
            let (doc, scene, projection) = fixture::fixture(kind, scale, true);
            let before = serde_json::to_string(&(&doc, &scene, &projection)).unwrap();
            let paper = art(&doc, &projection).unwrap();
            let strokes: Vec<_> = paper
                .items
                .iter()
                .filter_map(|item| match item {
                    Primitive::Line {
                        points,
                        layer: "CENTER",
                        width,
                        dash,
                    } => {
                        assert_eq!(*width, 0.55);
                        assert_eq!(dash, &[5., 1., 0.8, 1.]);
                        Some(points.clone())
                    }
                    _ => None,
                })
                .collect();
            let c = paper_point(&doc.sheets[0].views[0], [10., 20.], &projection);
            if kind == "mark" {
                assert_eq!(strokes.len(), 2);
                assert_eq!(
                    strokes[0],
                    vec![
                        [c[0] - 5. * scale - 4., c[1]],
                        [c[0] + 5. * scale + 4., c[1]]
                    ]
                );
            } else {
                assert_eq!(strokes.len(), 1);
                let b = paper_point(&doc.sheets[0].views[0], [40., 40.], &projection);
                let distance = |p: P, q: P| (p[0] - q[0]).hypot(p[1] - q[1]);
                assert!((distance(strokes[0][0], c) - (5. * scale + 4.)).abs() < 1e-9);
                assert!((distance(strokes[0][1], b) - (8. * scale + 4.)).abs() < 1e-9);
            }
            let rings = paper
                .items
                .iter()
                .filter(
                    |p| matches!(p,Primitive::Line {layer:"CENTER_MARK",width,..} if *width==0.36),
                )
                .count();
            assert_eq!(rings, if kind == "mark" { 1 } else { 2 });
            assert!(paper.items.iter().any(|p| matches!(
                p,
                Primitive::Triangle {
                    layer: TEXT_MASK,
                    ..
                }
            )));
            for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
                let output = export(&doc, &scene, &projection, format).unwrap();
                assert!(!output.contains("999.00000"));
                assert!(output.contains("CENTER_MARK"));
                match format {
                    DrawingExportFormat::Svg => {
                        assert!(output.contains("#356170"));
                        assert!(output.contains("fill=\"white\""));
                    }
                    DrawingExportFormat::Dxf => {
                        assert!(output.contains("420\n3498352\n"));
                        assert!(output.contains("420\n16777215\n"));
                    }
                }
            }
            assert_eq!(
                serde_json::to_string(&(&doc, &scene, &projection)).unwrap(),
                before
            );
        }
    }
}

#[test]
fn center_export_tracks_current_radius_and_rejects_lost_or_open_circles() {
    let (doc, scene, projection) = fixture::fixture("mark", 2., false);
    let mut changed = projection.clone();
    changed.circles[0].radius = 9.;
    let current = art(&doc, &changed).unwrap();
    let Primitive::Line { points, .. } = &current.items[0] else {
        panic!("Center cross missing")
    };
    assert!((points[1][0] - points[0][0] - 44.).abs() < 1e-9);
    for change in 0..6 {
        let mut broken = projection.clone();
        match change {
            0 => broken.circles.clear(),
            1 => broken.circles[0].edge_key = "Lost circular key".into(),
            2 => broken.circles[0].closed = false,
            3 => {
                broken
                    .topology_signatures
                    .insert("1".into(), "different".into());
            }
            4 => broken.circles[0].radius = f64::INFINITY,
            _ => broken.circles[0].occurrence_id = Some(limo_cad_assembly::OccurrenceId(91)),
        }
        for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
            assert!(
                export(&doc, &scene, &broken, format).is_err(),
                "case{change}"
            );
        }
    }
    let mut renumbered = projection.clone();
    renumbered.circles[0].edge_id = limo_cad_core::EdgeId(71);
    assert!(export(&doc, &scene, &renumbered, DrawingExportFormat::Svg).is_ok());
}

#[test]
fn centerline_keeps_occurrence_identity_and_rejects_coincident_centers() {
    let (mut doc, _scene, mut projection) = fixture::fixture("line", 1., false);
    let mut value = serde_json::to_value(&doc.sheets[0].annotations[0]).unwrap();
    value["first"]["occurrence_id"] = json!(7);
    value["second"]["occurrence_id"] = json!(9);
    doc.sheets[0].annotations[0] = serde_json::from_value(value).unwrap();
    projection.circles[0].occurrence_id = Some(limo_cad_assembly::OccurrenceId(7));
    projection.circles[1].occurrence_id = Some(limo_cad_assembly::OccurrenceId(9));
    assert!(art(&doc, &projection).is_ok());
    projection.circles[1].occurrence_id = Some(limo_cad_assembly::OccurrenceId(7));
    assert!(art(&doc, &projection).is_err());
    projection.circles[1].occurrence_id = Some(limo_cad_assembly::OccurrenceId(9));
    projection.circles[1].center = projection.circles[0].center;
    assert!(art(&doc, &projection).is_err());
}
