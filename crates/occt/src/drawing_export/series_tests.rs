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
fn multiple_series_batches_keep_independent_dimension_blocks_and_scaled_measurements() {
    use super::straight_tests::dxf_entities;
    for layout in ["chain", "baseline", "continued"] {
        for scale in [0.5, 2.] {
            for (units, unit_mm) in [
                (UnitSystem::Mm, 1.),
                (UnitSystem::Cm, 10.),
                (UnitSystem::In, 25.4),
            ] {
                let (mut d, s, p) = fixture::fixture(layout);
                d.sheets[0].views[0].scale = scale;
                let mut extra = serde_json::to_value(&d.sheets[0].annotations[0]).unwrap();
                extra["id"] = json!(3);
                extra["offset"] = json!(22.);
                d.sheets[0]
                    .annotations
                    .push(serde_json::from_value(extra).unwrap());
                d.next_annotation_id = 4;
                let output = export(&d, &s, &p, DrawingExportFormat::Dxf, units).unwrap();
                let dimensions = dxf_entities(&output, "DIMENSION");
                let styles = dxf_entities(&output, "DIMSTYLE");
                assert_eq!(dimensions.len(), 4);
                let blocks: std::collections::BTreeSet<_> =
                    dimensions.iter().map(|v| &v["2"]).collect();
                assert_eq!(blocks.len(), 4);
                for (i, dim) in dimensions.iter().enumerate() {
                    let num = |k: &str| dim[k].parse::<f64>().unwrap();
                    let angle = num("50").to_radians();
                    let paper = ((num("14") - num("13")) * angle.cos()
                        + (num("24") - num("23")) * angle.sin())
                    .abs();
                    let style = styles
                        .iter()
                        .find(|s| s.get("2") == Some(&dim["3"]))
                        .unwrap();
                    let expected = if i % 2 == 0 {
                        40.
                    } else if layout == "baseline" {
                        50.
                    } else {
                        30.
                    };
                    assert!(
                        (paper * style["144"].parse::<f64>().unwrap() - expected / unit_mm).abs()
                            < 1e-8
                    );
                }
            }
        }
    }
}

#[test]
fn series_semantic_metadata_is_charged_to_the_retained_budget() {
    let (d, _, p) = fixture::fixture("chain");
    let sheet = &d.sheets[0];
    let projections = BTreeMap::from([(1, p)]);
    let mut budget = PaperGraphicsBudget::default();
    let batch = series::draw(
        sheet,
        &projections,
        &sheet.annotations[0],
        UnitSystem::Mm,
        &mut budget,
    )
    .unwrap();
    assert_eq!(batch.dimensions.len(), 2);
    let metadata = batch
        .dimensions
        .iter()
        .map(|v| v.retained_bytes().unwrap() * 8)
        .sum::<usize>();
    let limit = budget.usage().retained_bytes - metadata;
    let mut bounded = PaperGraphicsBudget::new(graphics::PaperGraphicsLimits {
        retained_bytes: limit,
        ..Default::default()
    });
    assert!(series::draw(
        sheet,
        &projections,
        &sheet.annotations[0],
        UnitSystem::Mm,
        &mut bounded
    )
    .is_err());
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
