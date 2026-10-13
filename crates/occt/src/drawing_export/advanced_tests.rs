use super::straight_fixture as rectangle;
use super::*;
use crate as occt;
#[path = "../../tests/support/advanced_export.rs"]
mod fixture;

fn export(
    d: &DrawingDocumentDto,
    s: &SolidSceneDto,
    p: &DrawingProjectionDto,
    format: DrawingExportFormat,
    units: limo_cad_core::UnitSystem,
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
fn advanced_mixed_sheet_exports_every_family_and_preserves_saved_intent() {
    let (d, s, p) = fixture::fixture();
    let before = serde_json::to_value((&d, &s, &p)).unwrap();
    let sheet = &d.sheets[0];
    let projections = sheet.views.iter().map(|v| (v.id, p.clone())).collect();
    let mut budget = PaperGraphicsBudget::default();
    for a in &sheet.annotations {
        let art = advanced::draw(
            sheet,
            &projections,
            a,
            limo_cad_core::UnitSystem::Mm,
            &mut budget,
        )
        .unwrap();
        assert!(!art.is_empty(), "annotation {}", a.id());
    }
    for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
        let output = export(&d, &s, &p, format, limo_cad_core::UnitSystem::Mm).unwrap();
        for value in [
            "R6.000 mm",
            "9.425 mm",
            "A2",
            "Ra 3.2 GRIND",
            "+0.1 / -0.2 BREAK",
            "3-10 (20)",
            "TIG",
            "P-7",
        ] {
            assert!(output.contains(value), "missing {value:?} in {format:?}");
        }
        assert!(
            !output.contains("999.00000"),
            "fallback coordinates must never become export art"
        );
        let inches = export(&d, &s, &p, format, limo_cad_core::UnitSystem::In).unwrap();
        assert!(inches.contains("R0.236 in"));
        assert!(inches.contains("0.371 in"));
    }
    assert_eq!(before, serde_json::to_value((&d, &s, &p)).unwrap());
}

#[test]
fn every_associative_advanced_family_rejects_missing_exact_projection_without_fallback() {
    let (mut d, s, p) = fixture::fixture();
    let annotations = d.sheets[0].annotations.clone();
    for a in annotations {
        if matches!(a, DrawingAnnotationDto::AutomaticSymmetryAxis { .. }) {
            continue;
        }
        let id = a.id();
        d.sheets[0].annotations = vec![a];
        for mismatch in ["missing", "signature", "key"] {
            let mut stale = p.clone();
            match mismatch {
                "missing" => {
                    stale.anchors.clear();
                    stale.circles.clear();
                }
                "signature" => {
                    stale
                        .topology_signatures
                        .insert("1".into(), "different-topology".into());
                }
                "key" => {
                    for a in &mut stale.anchors {
                        a.edge_key = "different-edge".into();
                    }
                    for c in &mut stale.circles {
                        c.edge_key = "different-edge".into();
                    }
                }
                _ => unreachable!(),
            }
            for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
                let error =
                    export(&d, &s, &stale, format, limo_cad_core::UnitSystem::Mm).unwrap_err();
                assert!(
                    error.contains("stale or incompatible"),
                    "annotation {id}, {mismatch}: {error}"
                );
            }
        }
    }
}

#[test]
fn mixed_advanced_annotations_share_the_sheet_graphics_budget() {
    let (d, _, p) = fixture::fixture();
    let sheet = &d.sheets[0];
    let projections = sheet.views.iter().map(|v| (v.id, p.clone())).collect();
    let mut first = PaperGraphicsBudget::default();
    let first_art = advanced::draw(
        sheet,
        &projections,
        &sheet.annotations[0],
        limo_cad_core::UnitSystem::Mm,
        &mut first,
    )
    .unwrap();
    let mut shared = PaperGraphicsBudget::new(PaperGraphicsLimits {
        primitives: first_art.len(),
        ..Default::default()
    });
    advanced::draw(
        sheet,
        &projections,
        &sheet.annotations[0],
        limo_cad_core::UnitSystem::Mm,
        &mut shared,
    )
    .unwrap();
    assert!(advanced::draw(
        sheet,
        &projections,
        &sheet.annotations[1],
        limo_cad_core::UnitSystem::Mm,
        &mut shared
    )
    .is_err());
}

#[test]
fn dashed_curves_keep_one_continuous_dxf_path_across_short_tessellation_segments() {
    let mut paper = Paper {
        size: [50., 40.],
        items: vec![],
    };
    paper.line(
        vec![[1., 2.], [1.2, 2.1], [1.4, 2.2], [1.6, 2.3]],
        "CENTER",
        &DrawingLineStyleDto {
            width_mm: 0.25,
            dash_mm: vec![4., 2.],
        },
    );
    let output = dxf(&paper, "Arial").unwrap();
    assert_eq!(
        output.matches("0\nLWPOLYLINE\n").count(),
        1,
        "Independent LINE entities restart the dash and render short curve segments solid"
    );
    assert!(output.contains("100\nAcDbPolyline\n90\n4\n70\n128\n"));
    assert!(output.contains("6\nNBS_CENTER\n370\n25\n"));
    assert!(output.contains("10\n1.00000\n20\n38.00000\n"));
    assert!(output.contains("10\n1.60000\n20\n37.70000\n"));
    assert!(!output.contains("0\nLINE\n"));
}
