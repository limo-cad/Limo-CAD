use super::straight_fixture as fixture;
use super::*;
use limo_cad_core::UnitSystem;
use serde_json::{json, Value};

fn export(
    document: &DrawingDocumentDto,
    scene: &SolidSceneDto,
    projection: &DrawingProjectionDto,
    units: UnitSystem,
    format: DrawingExportFormat,
) -> Result<String, String> {
    export_sheet_with_units(
        document,
        scene,
        &AssemblyDocumentDto::default(),
        &DrawingExportRequest {
            sheet_id: 1,
            format,
        },
        units,
        |_| Ok(projection.clone()),
    )
}

fn dxf_entities(dxf: &str, kind: &str) -> Vec<BTreeMap<String, String>> {
    dxf.split(&format!("0\n{kind}\n"))
        .skip(1)
        .map(|entity| {
            let lines: Vec<_> = entity.split("\n0\n").next().unwrap().lines().collect();
            lines
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| (pair[0].to_owned(), pair[1].to_owned()))
                .collect()
        })
        .collect()
}

fn replace_annotation(document: &mut DrawingDocumentDto, change: impl FnOnce(&mut Value)) {
    let mut value = serde_json::to_value(&document.sheets[0].annotations[0]).unwrap();
    change(&mut value);
    document.sheets[0].annotations[0] = serde_json::from_value(value).unwrap();
}

#[test]
fn straight_relations_export_current_values_and_graphical_entities_without_mutating_intent() {
    for (kind, value) in [
        ("length", "40.00 mm"),
        ("distance", "30.00 mm"),
        ("angle", "90.00°"),
        ("point-line", "30.00 mm"),
    ] {
        let (document, scene, projection) = fixture::fixture(kind, 40.);
        let before = serde_json::to_value((&document, &scene, &projection)).unwrap();
        let svg = export(
            &document,
            &scene,
            &projection,
            UnitSystem::Mm,
            DrawingExportFormat::Svg,
        )
        .unwrap();
        let dxf = export(
            &document,
            &scene,
            &projection,
            UnitSystem::Mm,
            DrawingExportFormat::Dxf,
        )
        .unwrap();
        assert!(svg.contains(&format!(">{value}</text>")), "{kind}: {svg}");
        assert_eq!(
            dxf_entities(&dxf, "TEXT")
                .iter()
                .filter(|entity| entity["1"] == dxf_text(value))
                .count(),
            1
        );
        assert_eq!(
            svg.matches("<polygon data-layer=\"DIMENSION\"").count(),
            2,
            "{kind}"
        );
        assert_eq!(
            dxf_entities(&dxf, "SOLID")
                .iter()
                .filter(|entity| entity["8"] == "DIMENSION")
                .count(),
            2
        );
        assert!(
            !dxf.contains("0\nDIMENSION\n"),
            "This exporter promises graphical DXF only"
        );
        assert!(
            !svg.contains("999.00000"),
            "diagnostic fallbacks must never be measured"
        );
        assert_eq!(
            svg,
            export(
                &document,
                &scene,
                &projection,
                UnitSystem::Mm,
                DrawingExportFormat::Svg
            )
            .unwrap()
        );
        assert_eq!(
            before,
            serde_json::to_value((&document, &scene, &projection)).unwrap(),
            "{kind}: export changed metadata, references, counters or release receipt"
        );
    }
}

#[test]
fn existing_units_and_complete_presentation_are_shared_by_both_formats() {
    let (mut document, scene, projection) = fixture::fixture("length", 40.);
    fixture::full_presentation(&mut document);
    let before = document.clone();
    for (units, expected) in [
        (
            UnitSystem::Mm,
            "QA (40.000 mm +0.200/-0.100 H7 [1.575 in]) exact",
        ),
        (
            UnitSystem::Cm,
            "QA (4.000 cm +0.020/-0.010 H7 [1.575 in]) exact",
        ),
        (
            UnitSystem::In,
            "QA (1.575 in +0.008/-0.004 H7 [1.575 in]) exact",
        ),
    ] {
        let svg = export(
            &document,
            &scene,
            &projection,
            units,
            DrawingExportFormat::Svg,
        )
        .unwrap();
        let dxf = export(
            &document,
            &scene,
            &projection,
            units,
            DrawingExportFormat::Dxf,
        )
        .unwrap();
        assert!(svg.contains(&format!(">{}</text>", xml(expected))));
        assert!(dxf_entities(&dxf, "TEXT")
            .iter()
            .any(|e| e["1"] == dxf_text(expected)));
        let title_units = format!(
            "DIMENSIONS: {} ",
            crate::drawing_presentation::text::unit_label(units)
        );
        assert!(svg.contains(&title_units));
        assert!(dxf_entities(&dxf, "TEXT")
            .iter()
            .any(|e| e["1"].starts_with(&title_units)));
        assert!(svg.contains("105.00000,145.00000 185.00000,145.00000"));
        assert!(dxf.contains("$INSUNITS\n70\n4\n"));
        assert!(
            svg.lines()
                .any(|line| line.contains("data-layer=\"DIMENSION\"")
                    && line.matches(',').count() == 5),
            "basic box missing"
        );
        assert_eq!(document, before);
    }
    replace_annotation(&mut document, |v| {
        v["presentation"]["dual_units"]["placement"] = json!("stacked");
        v["presentation"]["tolerance"]["mode"] = json!("limits");
    });
    let svg = export(
        &document,
        &scene,
        &projection,
        UnitSystem::Cm,
        DrawingExportFormat::Svg,
    )
    .unwrap();
    assert!(svg.contains("QA (4.020 / 3.990 cm H7 / 1.575 in) exact"));
    let (mut angle, scene, projection) = fixture::fixture("angle", 40.);
    fixture::full_presentation(&mut angle);
    let saved = angle.clone();
    let text = "QA (90.000° +0.200°/-0.100° H7) exact";
    let svg = export(
        &angle,
        &scene,
        &projection,
        UnitSystem::In,
        DrawingExportFormat::Svg,
    )
    .unwrap();
    assert!(svg.contains(&xml(text)));
    assert!(
        !svg.contains("1.575 in"),
        "Angular formatter must not convert degrees as length"
    );
    assert_eq!(
        angle, saved,
        "Unused angular dual-unit intent remains stored"
    );
}

#[test]
fn dimensional_edits_resolve_current_endpoints_while_stale_missing_or_nonfinite_refs_fail() {
    let (document, scene, projection) = fixture::fixture("length", 40.);
    let (_, edited_scene, edited_projection) = fixture::fixture("length", 55.);
    for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
        let output = export(
            &document,
            &edited_scene,
            &edited_projection,
            UnitSystem::Mm,
            format,
        )
        .unwrap();
        assert!(output.contains("55.00 mm"));
        for damaged in [
            "scene_signature",
            "projection_signature",
            "missing",
            "key",
            "occurrence",
            "nonfinite",
        ] {
            let mut scene = scene.clone();
            let mut projection = projection.clone();
            match damaged {
                "scene_signature" => scene.bodies[0].topology_signature = "new connectivity".into(),
                "projection_signature" => {
                    projection
                        .topology_signatures
                        .insert("1".into(), "stale".into());
                }
                "missing" => projection.anchors.retain(|a| a.edge_id.0 != 1),
                "key" => {
                    for a in &mut projection.anchors {
                        a.edge_key.push_str("-wrong");
                    }
                }
                "occurrence" => {
                    for a in &mut projection.anchors {
                        a.occurrence_id = Some(limo_cad_sketch::OccurrenceId(7));
                    }
                }
                "nonfinite" => projection.anchors[0].point[0] = f64::NAN,
                _ => unreachable!(),
            }
            let before = document.clone();
            assert!(
                export(&document, &scene, &projection, UnitSystem::Mm, format).is_err(),
                "{damaged}"
            );
            assert_eq!(document, before);
        }
    }
}

#[test]
fn repeated_occurrences_use_the_exact_projected_instance_and_reject_exclusion() {
    let (mut document, scene, mut projection) = fixture::fixture("length", 40.);
    replace_annotation(&mut document, |v| v["first"]["occurrence_id"] = json!(7));
    let mut placed = projection.anchors.clone();
    for a in &mut placed {
        a.occurrence_id = Some(limo_cad_sketch::OccurrenceId(7));
        a.point = [a.point[0] * 0.5 + 100., a.point[1] + 50.];
    }
    projection.anchors.extend(placed);
    let before = document.clone();
    for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
        let output = export(&document, &scene, &projection, UnitSystem::Mm, format).unwrap();
        assert!(output.contains("20.00 mm"));
        let mut excluded = projection.clone();
        excluded.anchors.retain(|a| a.occurrence_id.is_none());
        assert!(export(&document, &scene, &excluded, UnitSystem::Mm, format).is_err());
    }
    assert_eq!(document, before);
}

#[test]
fn invalid_current_relations_fail_and_symmetry_axes_export_center_art() {
    for kind in ["length", "distance", "angle", "point-line"] {
        let (document, scene, mut projection) = fixture::fixture(kind, 40.);
        for a in &mut projection.anchors {
            a.point = [0., 0.];
        }
        for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
            assert!(
                export(&document, &scene, &projection, UnitSystem::Mm, format).is_err(),
                "{kind}"
            );
        }
    }
    let (mut document, scene, projection) = fixture::fixture("length", 40.);
    document.sheets[0].annotations.push(serde_json::from_value(json!({"kind":"automatic_symmetry_axis","id":3,"view_id":1,"axis":"both","extension":4.})).unwrap());
    document.next_annotation_id = 4;
    for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
        let output = export(&document, &scene, &projection, UnitSystem::Mm, format).unwrap();
        assert!(
            output.contains("CENTER"),
            "Symmetry axes must emit center geometry"
        );
    }
}

#[test]
fn vertical_ansi_label_has_y_up_dxf_rotation_and_the_shaft_gap_matches_shared_layout() {
    let (mut document, scene, projection) = fixture::fixture("distance", 40.);
    document.sheets[0].standard = DrawingStandard::Ansi;
    let svg = export(
        &document,
        &scene,
        &projection,
        UnitSystem::Mm,
        DrawingExportFormat::Svg,
    )
    .unwrap();
    let dxf = export(
        &document,
        &scene,
        &projection,
        UnitSystem::Mm,
        DrawingExportFormat::Dxf,
    )
    .unwrap();
    let text = dxf_entities(&dxf, "TEXT")
        .into_iter()
        .find(|e| e["1"] == "30.00 mm")
        .unwrap();
    assert_eq!(text["50"], "-90.00000");
    assert_eq!(text["72"], "1");
    let x: f64 = text["11"].parse().unwrap();
    let y: f64 = text["21"].parse().unwrap();
    assert!(svg.contains(&format!("x=\"{x:.5}\" y=\"{:.5}\"", 297. - y)));
    let dimension_lines: Vec<_> = dxf_entities(&dxf, "LINE")
        .into_iter()
        .filter(|e| e["8"] == "DIMENSION")
        .collect();
    assert_eq!(
        dimension_lines.len(),
        2,
        "Inside ANSI value must interrupt the shaft"
    );
    assert!(dimension_lines.iter().all(|e| {
        let start: f64 = e["20"].parse().unwrap();
        let end: f64 = e["21"].parse().unwrap();
        !(start.min(end) < 182. && start.max(end) > 182.)
    }));
}

#[test]
fn angular_mask_covers_crossing_art_before_the_basic_box_and_text_in_both_formats() {
    let (mut document, _, projection) = fixture::fixture("angle", 40.);
    fixture::full_presentation(&mut document);
    let sheet = &document.sheets[0];
    let projections = BTreeMap::from([(1, projection)]);
    let mut paper = Paper {
        size: [420., 297.],
        items: vec![],
    };
    draw_annotation(
        &mut paper,
        sheet,
        &projections,
        &sheet.annotations[0],
        UnitSystem::Mm,
    )
    .unwrap();
    let (arc_index, arc) = paper
        .items
        .iter()
        .enumerate()
        .find_map(|(i, item)| match item {
            Primitive::Line { points, .. } if points.len() > 100 => Some((i, points)),
            _ => None,
        })
        .unwrap();
    let radius = 25_f64.hypot(30.);
    assert!((arc[0][0] - (185. + radius)).abs() < 1e-9);
    assert_eq!(arc[0][1], 145.);
    assert!((arc.last().unwrap()[1] - (145. + radius)).abs() < 1e-9);
    let mid_arc = arc[arc.len() / 2];
    let masks: Vec<_> = paper
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match item {
            Primitive::Triangle { points, layer } if *layer == TEXT_MASK => Some((i, points)),
            _ => None,
        })
        .collect();
    assert_eq!(masks.len(), 2);
    let first_mask = masks[0].0;
    assert!(arc_index < first_mask);
    let min = masks
        .iter()
        .flat_map(|(_, p)| p.iter())
        .fold([f64::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let max = masks
        .iter()
        .flat_map(|(_, p)| p.iter())
        .fold([f64::NEG_INFINITY; 2], |a, p| {
            [a[0].max(p[0]), a[1].max(p[1])]
        });
    assert!(
        mid_arc[0] > min[0] && mid_arc[0] < max[0] && mid_arc[1] > min[1] && mid_arc[1] < max[1],
        "Regression fixture must actually cross the label mask"
    );
    assert!(
        matches!(paper.items.get(first_mask + 2), Some(Primitive::Line { points, .. }) if points.len() == 5),
        "Basic box must be painted after the white mask"
    );
    assert!(matches!(
        paper.items.get(first_mask + 3),
        Some(Primitive::Text { .. })
    ));
    let svg = svg(&paper, &sheet.style.font_family);
    let dxf = dxf(&paper, "Arial").unwrap();
    assert_eq!(
        svg.lines()
            .filter(
                |line| line.contains("data-layer=\"TEXT_MASK\"") && line.contains("fill=\"white\"")
            )
            .count(),
        2
    );
    let masks = dxf_entities(&dxf, "SOLID")
        .into_iter()
        .filter(|e| e["8"] == TEXT_MASK)
        .collect::<Vec<_>>();
    assert_eq!(masks.len(), 2);
    assert!(masks.iter().all(|e| e["420"] == "16777215"));
    assert!(dxf.find("0\nSOLID\n8\nTEXT_MASK\n").unwrap() < dxf.find("0\nTEXT\n").unwrap());
}

#[test]
fn r2007_text_keeps_utf8_degrees_symbols_and_supplementary_scalars() {
    let (mut document, scene, projection) = fixture::fixture("angle", 40.);
    let label = "Ø± 零件 𐐷";
    replace_annotation(&mut document, |a| a["prefix"] = json!(format!("{label} ")));
    document.sheets[0].title_block.title = label.into();
    let before = document.clone();
    let output = export(
        &document,
        &scene,
        &projection,
        UnitSystem::Mm,
        DrawingExportFormat::Dxf,
    )
    .unwrap();
    assert!(output.contains("$ACADVER\n1\nAC1021\n"));
    let texts = dxf_entities(&output, "TEXT");
    assert!(texts.iter().any(|e| e["1"] == label));
    assert!(texts.iter().any(|e| e["1"] == format!("{label} 90.00°")));
    assert!(
        !output.contains("\\U+"),
        "UTF-8 labels must not require legacy escape decoding"
    );
    assert!(output
        .as_bytes()
        .windows(4)
        .any(|bytes| bytes == [0xF0, 0x90, 0x90, 0xB7]));
    assert_eq!(
        document, before,
        "Export must preserve all source text and metadata"
    );
    assert_eq!(dxf_text("first\nsecond\rthird"), "first second third");
    assert_eq!(dxf_text(r"literal \U+00B0"), r"literal \U+005CU+00B0");
}

#[test]
fn basic_linear_masks_cover_crossing_extensions_before_painting_complete_arrows() {
    for (kind, tips) in [
        ("length", [[105., 165.], [185., 165.]]),
        ("distance", [[220., 85.], [220., 145.]]),
    ] {
        let (mut document, _, projection) = fixture::fixture(kind, 40.);
        fixture::full_presentation(&mut document);
        let sheet = &document.sheets[0];
        let mut paper = Paper {
            size: [420., 297.],
            items: vec![],
        };
        draw_annotation(
            &mut paper,
            sheet,
            &BTreeMap::from([(1, projection)]),
            &sheet.annotations[0],
            UnitSystem::Cm,
        )
        .unwrap();
        let masks: Vec<_> = paper
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                Primitive::Triangle { points, layer } if *layer == TEXT_MASK => Some((i, points)),
                _ => None,
            })
            .collect();
        assert_eq!(masks.len(), 2);
        let inside = |point: P, triangle: &[P; 3]| {
            let crosses = (0..3)
                .map(|i| {
                    let a = triangle[i];
                    let b = triangle[(i + 1) % 3];
                    (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
                })
                .collect::<Vec<_>>();
            crosses.iter().all(|c| *c > 1e-6) || crosses.iter().all(|c| *c < -1e-6)
        };
        let crosses_mask = paper.items.iter().enumerate().any(|(i, item)| match item {
            Primitive::Line {
                points,
                layer: "EXTENSION",
                ..
            } => {
                assert!(i < masks[0].0);
                (1..100).any(|n| {
                    let t = f64::from(n) / 100.;
                    let p = [
                        points[0][0] + t * (points[1][0] - points[0][0]),
                        points[0][1] + t * (points[1][1] - points[0][1]),
                    ];
                    masks.iter().any(|(_, triangle)| inside(p, triangle))
                })
            }
            _ => false,
        });
        assert!(
            crosses_mask,
            "{kind}: regression must cross the actual label frame"
        );
        let arrows: Vec<_> = paper
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| match item {
                Primitive::Triangle {
                    points,
                    layer: "DIMENSION",
                } => Some((i, points)),
                _ => None,
            })
            .collect();
        assert_eq!(arrows.len(), 2);
        for ((i, triangle), tip) in arrows.iter().zip(tips) {
            assert!(*i > masks[1].0, "Mask must not erase any arrowhead");
            assert_eq!(triangle[0], tip, "Measured tip must not move");
        }
        let frame = paper
            .items
            .iter()
            .enumerate()
            .find_map(|(i, item)| match item {
                Primitive::Line { points, .. } if points.len() == 5 => Some((i, points)),
                _ => None,
            })
            .unwrap();
        assert!(frame.0 > arrows[1].0);
        assert_eq!(&frame.1[..3], masks[0].1);
        assert_eq!(frame.1[3], masks[1].1[2]);
        for format in [DrawingExportFormat::Svg, DrawingExportFormat::Dxf] {
            let output = match format {
                DrawingExportFormat::Svg => svg(&paper, &sheet.style.font_family),
                DrawingExportFormat::Dxf => dxf(&paper, &sheet.style.font_family).unwrap(),
            };
            let (mask, arrow) = match format {
                DrawingExportFormat::Svg => (
                    "<polygon data-layer=\"TEXT_MASK\"",
                    "<polygon data-layer=\"DIMENSION\"",
                ),
                DrawingExportFormat::Dxf => {
                    ("0\nSOLID\n8\nTEXT_MASK\n", "0\nSOLID\n8\nDIMENSION\n")
                }
            };
            assert!(output.find(mask).unwrap() < output.find(arrow).unwrap());
        }
    }
}
