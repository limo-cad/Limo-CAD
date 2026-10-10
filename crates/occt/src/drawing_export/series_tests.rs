use super::straight_fixture as rectangle;
use super::*;
use crate as occt;
use limo_cad_core::UnitSystem;
use serde_json::json;
#[path = "../../tests/support/series_export.rs"]
mod fixture;

fn export(
    d: &DrawingDocumentDto,
    s: &SolidSceneDto,
    p: &DrawingProjectionDto,
    format: DrawingExportFormat,
    units: UnitSystem,
) -> Result<String, String> {
    export_sheet_with_units(
        d,
        s,
        &AssemblyDocumentDto::default(),
        &DrawingExportRequest {
            sheet_id: 1,
            format,
        },
        units,
        |_| Ok(p.clone()),
    )
}

#[test]
fn chain_baseline_and_continued_export_every_exact_span_without_mutating_intent() {
    for (layout, expected) in [
        ("chain", "30.00 mm"),
        ("continued", "30.00 mm"),
        ("baseline", "50.00 mm"),
    ] {
        let (d, s, p) = fixture::fixture(layout);
        let before = serde_json::to_value((&d, &s, &p)).unwrap();
        let svg = export(&d, &s, &p, DrawingExportFormat::Svg, UnitSystem::Mm).unwrap();
        let dxf = export(&d, &s, &p, DrawingExportFormat::Dxf, UnitSystem::Mm).unwrap();
        assert!(svg.contains(">40.00 mm</text>"), "{layout}");
        assert!(svg.contains(&format!(">{expected}</text>")), "{layout}");
        assert!(dxf.contains(&format!("1\n{}\n", dxf_text(expected))));
        assert_eq!(svg.matches("<polygon data-layer=\"DIMENSION\"").count(), 4);
        assert!(!svg.contains("999.00000"));
        assert_eq!(before, serde_json::to_value((&d, &s, &p)).unwrap());
    }
}

#[test]
fn series_uses_existing_units_tolerances_and_atomic_stale_reference_rejection() {
    let (mut d, s, mut p) = fixture::fixture("baseline");
    fixture::full_presentation(&mut d);
    let svg = export(&d, &s, &p, DrawingExportFormat::Svg, UnitSystem::In).unwrap();
    assert!(svg.contains("QA (1.575 in +0.008/-0.004 H7 [1.575 in]) exact"));
    assert!(svg.contains("QA (1.969 in +0.008/-0.004 H7 [1.969 in]) exact"));
    assert!(export(&d, &s, &p, DrawingExportFormat::Dxf, UnitSystem::In)
        .unwrap()
        .contains("$INSUNITS\n70\n4\n"));
    p.topology_signatures.insert("1".into(), "stale".into());
    for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
        assert!(export(&d, &s, &p, format, UnitSystem::Mm)
            .unwrap_err()
            .contains("stale"));
    }
}

#[test]
fn shared_series_geometry_keeps_offsets_on_paper_and_rejects_overflow() {
    use crate::drawing_presentation::geometry as g;
    let a = g::dimension_span(
        DrawingLinearDimensionMode::Horizontal,
        [10., 20.],
        [90., 80.],
        12.,
        2.,
    )
    .unwrap();
    assert_eq!(a.value, 40.);
    assert_eq!((a.start, a.end), ([10., 32.], [90., 32.]));
    let o = g::ordinate([10., 20.], [90., 80.], -12., 2.).unwrap();
    assert_eq!((o.x_value, o.y_value), (40., -30.));
    assert_eq!((o.elbow, o.position), ([90., 68.], [90., 66.]));
    assert!(g::ordinate([0., 0.], [1., 1.], f64::MAX, 1e-310).is_none());
    assert!(g::dimension_span(
        DrawingLinearDimensionMode::Horizontal,
        [f64::MAX, 0.],
        [-f64::MAX, 0.],
        0.,
        1.
    )
    .is_none());
}

#[test]
fn ordinate_exports_signed_axes_origin_ring_complete_leader_and_shared_presentation() {
    let (mut d, s, p) = fixture::fixture("chain");
    let data = serde_json::to_value(&d.sheets[0].annotations[0]).unwrap();
    for (axis, expected) in [
        ("x", "X40.00 mm"),
        ("y", "Y30.00 mm"),
        ("both", "X40.00 mm  Y30.00 mm"),
    ] {
        d.sheets[0].annotations = vec![serde_json::from_value(json!({
            "id":1,"kind":"ordinate_dimension","view_id":1,
            "origin":data["anchors"][0],"target":data["anchors"][2],
            "axis":axis,"offset":-12.,"precision":2
        }))
        .unwrap()];
        let before = d.clone();
        let svg = export(&d, &s, &p, DrawingExportFormat::Svg, UnitSystem::Mm).unwrap();
        let dxf = export(&d, &s, &p, DrawingExportFormat::Dxf, UnitSystem::Mm).unwrap();
        assert!(svg.contains(&format!(">{expected}</text>")), "{svg}");
        assert!(dxf.contains(&format!("1\n{}\n", dxf_text(expected))));
        assert!(svg.contains("185.00000,85.00000 185.00000,73.00000 185.00000,71.00000"));
        assert_eq!(svg.matches("<polygon data-layer=\"DIMENSION\"").count(), 1);
        assert!(svg.contains("data-layer=\"TEXT_MASK\""));
        assert_eq!(d, before);
    }
}
