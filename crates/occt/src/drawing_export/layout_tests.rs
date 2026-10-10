use crate::drawing_presentation::{cloud::Cloud, layout, text};

fn svg_anchor(svg: &str, needle: &str) -> (f64, f64, f64) {
    let line = svg
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle}"));
    let number = |key: &str| {
        let token = format!("{key}=\"");
        let start = line.find(&token).unwrap() + token.len();
        let end = line[start..].find('"').unwrap() + start;
        line[start..end].parse::<f64>().unwrap()
    };
    (number("x"), number("y"), number("font-size"))
}

fn overlaps(first: [f64; 4], second: [f64; 4]) -> bool {
    first[0] < second[2] - 1e-3
        && second[0] < first[2] - 1e-3
        && first[1] < second[3] - 1e-3
        && second[1] < first[3] - 1e-3
}

fn box_at(point: [f64; 2], value: &str, height: f64) -> [f64; 4] {
    text::label_bounds(point, value, height, 1.)
}

#[test]
fn caption_clears_six_mm_dimension_and_labels_clear_neighbors_and_dxf_keeps_glyphs() {
    let (mut doc, scene, projection) = fixture(12.8);
    let sheet = &mut doc.sheets[0];
    let DrawingAnnotationDto::LinearDimension { offset, .. } = &mut sheet.annotations[0] else {
        panic!("fixture dimension");
    };
    *offset = 6.;
    let naive = sheet.views[0].position[1] + 6.;
    let ink = layout::dimension_ink_y(naive, naive, sheet.style.dimension.width_mm, true);
    let naive_top = text::label_bounds(
        [0., naive],
        "",
        sheet.style.small_text_height_mm,
        0.,
    )[1];
    assert!(
        naive_top < ink,
        "a caption 6 mm under the view must start on the dimension"
    );

    let weld_label = [48., 48.5];
    let points = [[200., 70.], [255., 70.], [255., 110.], [200., 110.]];
    let caption = Cloud::new(&points).unwrap().caption_baseline("REV CLOUDQA");
    let bom_at = [24., 130.];
    let revision_at = [152., 44.];
    let glyphs = "零件⌀";
    sheet.annotations[1] = serde_json::from_value(json!({
        "kind":"note","id":2,"text":glyphs,"position":weld_label
    }))
    .unwrap();
    sheet.annotations.push(serde_json::from_value(json!({
        "kind":"note","id":3,"text":"NOTE-CLOUD","position":caption
    }))
    .unwrap());
    sheet.annotations.push(serde_json::from_value(json!({
        "kind":"note","id":4,"text":"NOTE-BOM","position":bom_at
    }))
    .unwrap());
    sheet.annotations.push(serde_json::from_value(json!({
        "kind":"note","id":5,"text":"NOTE-REV","position":revision_at
    }))
    .unwrap());
    sheet.annotations.push(serde_json::from_value(json!({
        "kind":"weld_symbol","id":6,"view_id":1,
        "attachment":{"body_id":1,"edge_id":1,"edge_key":"bottom",
            "fallback_start":[999.,999.,999.],"fallback_end":[998.,999.,999.]},
        "position":[40.,50.],"weld_type":"fillet","side":"arrow","size":6.25,
        "contour":"none","finish":"","all_around":false,"field_weld":false,"tail":""
    }))
    .unwrap());
    sheet.annotations.push(serde_json::from_value(json!({
        "kind":"revision_cloud","id":7,"revision":"CLOUDQA","points":points
    }))
    .unwrap());
    doc.next_annotation_id = 8;
    doc.next_revision_id = 2;
    doc.next_bom_item_id = 2;
    sheet.bom = vec![serde_json::from_value(json!({
        "id":1,"item_number":"1","part_number":"P","description":"D",
        "quantity":1.,"material":"M","finish":""
    }))
    .unwrap()];
    sheet.bom_table_position = Some(bom_at);
    sheet.revision_table_position = Some([150., 40.]);
    sheet.revisions.push(serde_json::from_value(json!({
        "id":1,"revision":"A","description":"Moved","date":"2026-09-10",
        "author":"A","checked_by":"B","approved_by":"C","change_order":"CO","status":"draft"
    }))
    .unwrap());

    let export = |format| {
        export_sheet(
            &doc,
            &scene,
            &AssemblyDocumentDto::default(),
            &DrawingExportRequest { sheet_id: 1, format },
            |_| Ok(projection.clone()),
        )
    };
    let svg = export(DrawingExportFormat::Svg).unwrap();
    let (caption_x, caption_y, caption_h) = svg_anchor(&svg, "Front  (scale 2)");
    let _ = caption_x;
    let caption_top = text::label_bounds([0., caption_y], "", caption_h, 0.)[1];
    assert!(
        caption_top + 1e-4 >= ink + layout::GAP_MM,
        "caption top {caption_top} still meets dimension ink {ink}"
    );

    let style_h = doc.sheets[0].style.text_height_mm;
    let small = doc.sheets[0].style.small_text_height_mm;
    let pairs = [
        ("6.25</text>", "6.25", style_h, weld_label, glyphs, style_h),
        (
            "REV CLOUDQA",
            "REV CLOUDQA",
            crate::drawing_presentation::cloud::TEXT_HEIGHT_MM,
            caption,
            "NOTE-CLOUD",
            style_h,
        ),
        (
            "ITEM   PART / DESCRIPTION",
            "ITEM   PART / DESCRIPTION   QTY   MATERIAL / PROCESS",
            small,
            bom_at,
            "NOTE-BOM",
            style_h,
        ),
        (
            "REVISION HISTORY",
            "REVISION HISTORY — description, responsibility and release",
            small,
            revision_at,
            "NOTE-REV",
            style_h,
        ),
    ];
    for (needle, value, height, origin, neighbor, neighbor_h) in pairs {
        let (x, y, parsed_h) = svg_anchor(&svg, needle);
        let (nx, ny, nh) = svg_anchor(&svg, neighbor);
        assert!(
            !overlaps(box_at([x, y], value, parsed_h), box_at([nx, ny], neighbor, nh)),
            "{needle} still overlaps {neighbor}"
        );
        let before = box_at(origin, value, height);
        let after = box_at([x, y], value, parsed_h);
        assert!(
            (before[0] - after[0]).abs() > 0.2 || (before[1] - after[1]).abs() > 0.2,
            "{needle} did not move off its neighbor"
        );
        let _ = neighbor_h;
    }

    let dxf = export(DrawingExportFormat::Dxf).unwrap();
    assert!(dxf.contains(glyphs), "CJK and technical characters must remain in the DXF text");
    assert!(!dxf.contains("NOBS_EMBEDDED_FONT"));
    assert!(dxf.contains("0\nHATCH\n"), "Unicode labels have actual filled font outlines");
    let text_start = dxf.find(&format!("1\n{glyphs}\n")).unwrap();
    assert!(dxf[text_start..].starts_with(&format!("1\n{glyphs}\n60\n1\n")),
        "mixed labels must not double paint their source TEXT over the glyphs");
    assert!(
        dxf.contains("1000\nArial\n"),
        "the sheet family is still recorded"
    );
    assert!(dxf.ends_with("0\nEOF\n"));
}
