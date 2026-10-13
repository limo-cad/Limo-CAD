use super::*;
fn catalog() -> ProfileCatalogItemDto {
    serde_json::from_str(include_str!("../../tests/fixtures/profile-dxf.json")).unwrap()
}
fn entities(text: &str) -> Vec<Vec<(String, String)>> {
    let lines: Vec<_> = text.lines().collect();
    let pairs: Vec<_> = lines
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| (p[0].to_string(), p[1].to_string()))
        .collect();
    let start = pairs
        .windows(2)
        .position(|p| {
            p[0] == ("0".into(), "SECTION".into()) && p[1] == ("2".into(), "ENTITIES".into())
        })
        .unwrap()
        + 2;
    let mut result: Vec<Vec<(String, String)>> = Vec::new();
    for pair in &pairs[start..] {
        if pair == &("0".into(), "ENDSEC".into()) {
            break;
        }
        if pair.0 == "0" {
            result.push(Vec::new());
        }
        result.last_mut().unwrap().push(pair.clone());
    }
    result
}
fn value<'a>(row: &'a [(String, String)], code: &str) -> &'a str {
    &row.iter().find(|p| p.0 == code).unwrap().1
}
#[test]
fn exact_nested_regions_and_holes_use_local_mm_without_paper_or_placement() {
    let catalog = catalog();
    let before = catalog.clone();
    let text = write_profile_dxf(&catalog, 0).unwrap();
    assert!(text.contains("$INSUNITS\r\n70\r\n4\r\n"));
    let rows = entities(&text);
    assert_eq!(rows.len(), 2);
    for (row, radius, layer) in [
        (&rows[0], "10", "PROFILE_OUTER"),
        (&rows[1], "4", "PROFILE_HOLES"),
    ] {
        assert_eq!(value(row, "0"), "CIRCLE");
        assert_eq!(value(row, "10"), "-5");
        assert_eq!(value(row, "20"), "20");
        assert_eq!(value(row, "40"), radius);
        assert_eq!(value(row, "8"), layer);
        assert_eq!(value(row, "330"), "27");
    }
    assert!(write_profile_dxf(&catalog, 1)
        .unwrap_err()
        .contains("hole wire"));
    let island = entities(&write_profile_dxf(&catalog, 2).unwrap());
    assert_eq!(island.len(), 1);
    assert_eq!(value(&island[0], "40"), "2");
    assert_eq!(catalog, before);
}
#[test]
fn both_arc_directions_retain_analytic_radius_and_midpoint_selected_sweep() {
    for (index, start, end) in [(3, "0", "180"), (4, "180", "0")] {
        let rows = entities(&write_profile_dxf(&catalog(), index).unwrap());
        assert_eq!(rows.len(), 2);
        let arc = &rows[0];
        for (code, expected) in [
            ("10", "40"),
            ("20", "0"),
            ("40", "5"),
            ("50", start),
            ("51", end),
        ] {
            assert_eq!(
                value(arc, code).parse::<f64>().ok(),
                expected.parse::<f64>().ok(),
                "{code}"
            );
        }
        assert_eq!(value(arc, "0"), "ARC");
        assert_eq!(value(&rows[1], "0"), "LINE");
    }
    let rows = entities(&write_profile_dxf(&catalog(), 5).unwrap());
    assert_eq!(value(&rows[0], "0"), "LWPOLYLINE");
    assert_eq!(value(&rows[0], "70"), "1");
    assert_eq!(value(&rows[0], "90"), "3");
}

#[test]
fn actual_rotated_sketch_catalog_exports_the_closed_region_and_analytic_hole() {
    let mut manager = limo_cad_sketch::SketchManager::new();
    manager
        .begin_sketch(limo_cad_core::PlaneRef::OriginPlane {
            plane: limo_cad_core::OriginPlane::Yz,
        })
        .unwrap();
    manager.add_rectangle(serde_json::from_value(serde_json::json!({"mode":"two_point","p1":{"x":-20.,"y":10.},"p2":{"x":40.,"y":50.},"ctrl_held":true})).unwrap()).unwrap();
    manager.add_circle(serde_json::from_value(serde_json::json!({"mode":"center_diameter","p1":{"x":10.,"y":30.},"p2":{"x":14.,"y":30.},"ctrl_held":true})).unwrap()).unwrap();
    manager.end_sketch().unwrap();
    let before = manager.export_project_model().unwrap();
    let catalog = manager.profile_catalog();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].profiles.len(), 2);
    let outer = catalog[0]
        .profiles
        .iter()
        .find(|p| p.nesting_depth == 0)
        .unwrap();
    let rows = entities(&write_profile_dxf(&catalog[0], outer.index).unwrap());
    assert_eq!(rows.len(), 5);
    let circles: Vec<_> = rows.iter().filter(|r| value(r, "0") == "CIRCLE").collect();
    assert_eq!(circles.len(), 1);
    assert_eq!(value(circles[0], "8"), "PROFILE_HOLES");
    for (code, expected) in [("10", 10.), ("20", 30.), ("40", 4.)] {
        assert!((value(circles[0], code).parse::<f64>().unwrap() - expected).abs() < 1e-8);
    }
    let lines: Vec<_> = rows.iter().filter(|r| value(r, "0") == "LINE").collect();
    assert_eq!(lines.len(), 4);
    for (codes, min, max) in [(["10", "11"], -20., 40.), (["20", "21"], 10., 50.)] {
        let values: Vec<_> = lines
            .iter()
            .flat_map(|r| codes.map(|code| value(r, code).parse::<f64>().unwrap()))
            .collect();
        assert_eq!(values.iter().copied().fold(f64::INFINITY, f64::min), min);
        assert_eq!(
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            max
        );
    }
    assert_eq!(manager.export_project_model().unwrap(), before);
}
#[test]
fn malformed_missing_open_ambiguous_and_nonfinite_profiles_fail_instead_of_partial_geometry() {
    assert!(write_profile_dxf(&catalog(), 99).is_err());
    let mut bad = catalog();
    bad.profiles.push(bad.profiles[0].clone());
    assert!(write_profile_dxf(&bad, 0)
        .unwrap_err()
        .contains("ambiguous"));
    let mut bad = catalog();
    bad.profiles[1].nesting_depth = 4;
    assert!(write_profile_dxf(&bad, 0).unwrap_err().contains("nesting"));
    let mut bad = catalog();
    bad.profiles[3].curves.pop();
    assert!(write_profile_dxf(&bad, 3)
        .unwrap_err()
        .contains("closed wire"));
    let mut bad = catalog();
    if let ProfileCurveDto::Circle { radius, .. } = &mut bad.profiles[1].curves[0] {
        *radius = f64::NAN;
    }
    assert!(write_profile_dxf(&bad, 0).is_err());
    let mut bad = catalog();
    bad.profiles[5].points[0].x = f64::INFINITY;
    assert!(write_profile_dxf(&bad, 5).is_err());
}
#[test]
fn dxf_tables_handles_and_entity_owners_are_consistent() {
    let text = write_profile_dxf(&catalog(), 0).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len() % 2, 0);
    let pairs = lines.as_chunks::<2>().0;
    let mut handles = std::collections::BTreeSet::new();
    let mut seed = 0;
    for (i, pair) in pairs.iter().enumerate().filter(|(_, p)| p[0] == "5") {
        let value = u32::from_str_radix(pair[1], 16).unwrap();
        if pairs[i - 1] == ["9", "$HANDSEED"] {
            seed = value;
        } else {
            assert!(handles.insert(value));
        }
    }
    assert!(handles.iter().all(|v| *v < seed));
    for pair in pairs.iter().filter(|p| p[0] == "330" && p[1] != "0") {
        assert!(handles.contains(&u32::from_str_radix(pair[1], 16).unwrap()));
    }
    assert!(text.ends_with("0\r\nEOF\r\n"));
}
