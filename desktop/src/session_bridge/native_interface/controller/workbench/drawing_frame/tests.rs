use super::*;
use serde_json::json;

fn sheet() -> DrawingSheetDto {
    serde_json::from_value(json!({"id":1,"name":"Production","format":"a4","orientation":"landscape",
        "title_block":{"title":"Café 零件","drawing_number":"DWG-7","revision":"B","author":"Engineer","checked_by":"Checker","approved_by":"Approver","company":"Company","material":"Aluminium","finish":"Deburr"},
        "tolerance_note":{"preset":"custom","custom":"  Company tolerance 0.25 mm  "}})).unwrap()
}
fn has(art: &Art, text: &str) -> bool {
    art.labels.iter().any(|label| label.text == text)
}

#[test]
fn sheet_frame_retains_all_title_metadata_and_uses_shared_paper_bounds() {
    let sheet = sheet();
    let original = sheet.clone();
    for (width, height) in [(297., 210.), (279.4, 431.8), (1189., 841.)] {
        let art = render(&sheet, width, height);
        assert_eq!(art.segments.len(), 17);
        let border = &art.segments[..4];
        assert_eq!((border[0].x1, border[0].y1), (5., 5.));
        assert!((border[1].x1 as f64 - (width - 5.)).abs() < 1e-4);
        assert!((border[2].y1 as f64 - (height - 5.)).abs() < 1e-4);
        for text in [
            "Café 零件",
            "DRAWING: DWG-7",
            "SHEET: Production",
            "ISO A4 · 1ST ANGLE",
            "Company tolerance 0.25 mm",
            "COMPANY: Company",
            "REV B",
            "MATERIAL: Aluminium",
            "FINISH: Deburr",
            "DRAWN: Engineer",
            "CHECKED: Checker",
            "APPROVED: Approver",
        ] {
            assert!(has(&art, text), "missing {text}");
        }
        assert!(art
            .labels
            .iter()
            .all(|l| l.align == LabelAlign::Start && l.text_height_mm >= 1.8));
        assert!(art.labels.iter().all(|l| {
            l.x - l.width_mm * 0.5 >= (width - 185.) as f32
                && l.x + l.width_mm * 0.5 <= (width - 5.) as f32
                && l.y >= (height - 49.) as f32
                && l.y <= (height - 5.) as f32
        }));
    }
    assert_eq!(sheet, original);
}

#[test]
fn title_wraps_unicode_and_reports_overflow_without_altering_source() {
    assert_eq!(wrap("one  two\r\n零件", 20.), vec!["one two", "零件"]);
    assert_eq!(wrap("零件加工", 2.), vec!["零件", "加工"]);
    let mut art = CheckedArt::default();
    cell(
        &mut art,
        "Long component title split across two rows".into(),
        [10., 20., 65., 9.],
        3.5,
    );
    assert!(art.labels.len() > 1);
    assert!(art
        .labels
        .iter()
        .all(|l| l.ink != Ink::Overflow && l.x + l.width_mm * 0.5 <= 73.51));
    let mut sheet = sheet();
    sheet.title_block.title = "零".repeat(2048);
    let before = sheet.clone();
    let art = render(&sheet, 297., 210.);
    assert!(art
        .labels
        .iter()
        .any(|l| l.text == "! TEXT TOO LONG" && l.ink == Ink::Overflow));
    assert_eq!(sheet, before);
}

#[test]
fn revision_and_bom_tables_keep_saved_positions_complete_rows_and_optional_visibility() {
    let mut sheet = sheet();
    sheet.revisions = serde_json::from_value(json!([
        {"id":1,"revision":"A","date":"2026-09-25","description":"Initial","approved_by":"QA"},
        {"id":2,"revision":"B","date":"2026-09-26","description":"","change_order":"ECO-7"}
    ]))
    .unwrap();
    sheet.bom=serde_json::from_value(json!([
        {"id":1,"item_number":"1","part_number":"P-7","description":"Plate 零件","quantity":2.5,"material":"Al"},
        {"id":2,"item_number":"2","description":"Pin","quantity":10.,"material":"Steel"}
    ])).unwrap();
    assert_eq!(render(&sheet, 297., 210.).fills.len(), 0);
    sheet.revision_table_position = Some([12., 20.]);
    sheet.bom_table_position = Some([150., 20.]);
    let before = sheet.clone();
    let art = render(&sheet, 297., 210.);
    assert_eq!(art.fills.len(), 2);
    assert_eq!(
        (
            art.fills[0].x,
            art.fills[0].y,
            art.fills[0].width,
            art.fills[0].height
        ),
        (12., 20., 112., 18.)
    );
    assert_eq!(
        (
            art.fills[1].x,
            art.fills[1].y,
            art.fills[1].width,
            art.fills[1].height
        ),
        (150., 20., 132., 18.)
    );
    for text in [
        "DESCRIPTION / APPROVAL",
        "Initial · QA",
        "ECO-7",
        "Plate 零件",
        "P-7",
        "2.5",
        "10",
        "MATERIAL",
    ] {
        assert!(has(&art, text), "missing table value {text}");
    }
    let part = art.labels.iter().find(|l| l.text == "P-7").unwrap();
    assert!((part.x - part.width_mm * 0.5 - 164.).abs() < 1e-4);
    assert_eq!(sheet, before);
}

#[test]
fn standard_projection_and_tolerance_text_follow_shared_sheet_settings() {
    let mut sheet = sheet();
    sheet.format = DrawingSheetFormat::Letter;
    sheet.projection_method = DrawingProjectionMethod::ThirdAngle;
    sheet.tolerance_note = DrawingToleranceNoteDto {
        preset: DrawingTolerancePreset::AnsiDecimal,
        custom: String::new(),
    };
    let art = render(&sheet, 279.4, 215.9);
    assert!(has(&art, "ANSI A · 3RD ANGLE"));
    assert!(art.labels.iter().any(|l| l.text.contains(".XXX ±.005")));
    sheet.tolerance_note.preset = DrawingTolerancePreset::None;
    assert!(has(&render(&sheet, 297., 210.), "TOLERANCES: AS SPECIFIED"));
}

fn lines(art: &CheckedArt) -> Vec<[f32; 4]> {
    art.segments
        .iter()
        .map(|s| [s.x1, s.y1, s.x2, s.y2])
        .collect()
}

#[test]
fn odd_dash_rectangles_keep_svg_phase_at_corners_and_separate_lines_restart() {
    let style = DrawingLineStyleDto {
        width_mm: 0.27,
        dash_mm: vec![5., 2., 1.],
    };
    let mut art = CheckedArt::default();
    rectangle(&mut art, 0., 0., 7., 3., &style);
    assert_eq!(
        lines(&art),
        vec![
            [0., 0., 5., 0.],
            [7., 0., 7., 1.],
            [4., 3., 2., 3.],
            [1., 3., 0., 3.],
            [0., 3., 0., 0.],
        ]
    );
    stroke(&mut art, [0., 10.], [7., 10.], &style);
    stroke(&mut art, [0., 20.], [7., 20.], &style);
    assert_eq!(&lines(&art)[5..], &[[0., 10., 5., 10.], [0., 20., 5., 20.]]);
    assert!(art
        .segments
        .iter()
        .all(|s| s.width_mm == 0.27 && s.ink == Ink::Frame));
    art.finish().unwrap();
}

#[test]
fn saved_visible_and_dimension_dashes_change_only_frame_lines() {
    let mut sheet = sheet();
    sheet.revision_table_position = Some([10., 140.]);
    sheet.bom_table_position = Some([150., 140.]);
    sheet.revisions = serde_json::from_value(json!([
        {"id":1,"revision":"A","date":"2026-09-27","description":"Released","approved_by":"QA"}
    ]))
    .unwrap();
    sheet.bom = serde_json::from_value(json!([
        {"id":1,"item_number":"1","part_number":"P1","description":"Plate","quantity":2.,"material":"Al"}
    ])).unwrap();
    let solid = try_render(&Source::new(&sheet, [297., 210.])).unwrap();
    sheet.style.visible = DrawingLineStyleDto {
        width_mm: 0.5,
        dash_mm: vec![5., 2., 1.],
    };
    sheet.style.dimension = DrawingLineStyleDto {
        width_mm: 0.25,
        dash_mm: vec![3., 1.],
    };
    let saved = sheet.clone();
    let art = try_render(&Source::new(&sheet, [297., 210.])).unwrap();
    assert_eq!(sheet, saved);
    let label_values = |art: &Art| {
        art.labels
            .iter()
            .map(|l| {
                (
                    l.x,
                    l.y,
                    l.width_mm,
                    l.height_mm,
                    l.text_height_mm,
                    l.text.clone(),
                    l.ink,
                    l.align,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(label_values(&art), label_values(&solid));
    assert_eq!(art.fills.len(), solid.fills.len());
    for (actual, old) in art.fills.iter().zip(&solid.fills) {
        assert_eq!(
            (
                actual.x,
                actual.y,
                actual.width,
                actual.height,
                actual.round
            ),
            (old.x, old.y, old.width, old.height, old.round)
        );
    }
    for (start, end, width) in [
        ([5., 5.], [10., 5.], 0.5),
        ([112., 161.], [115., 161.], 0.25),
        ([112., 175.], [115., 175.], 0.25),
        ([10., 140.], [13., 140.], 0.25),
        ([10., 146.], [13., 146.], 0.25),
        ([150., 140.], [153., 140.], 0.25),
        ([162., 140.], [162., 143.], 0.25),
    ] {
        assert!(
            art.segments
                .iter()
                .any(|s| [s.x1, s.y1] == start && [s.x2, s.y2] == end && s.width_mm == width),
            "missing {start:?} -> {end:?}"
        );
    }
}

#[test]
fn saved_sub_minimum_dashes_are_exact_and_pathological_work_is_rejected_before_allocation() {
    let style = DrawingLineStyleDto {
        width_mm: 0.25,
        dash_mm: vec![0.01, 0.02],
    };
    let mut art = CheckedArt::default();
    stroke(&mut art, [0., 0.], [0.065, 0.], &style);
    let actual = lines(&art);
    assert_eq!(actual.len(), 3);
    for (actual, expected) in actual.iter().zip([
        [0., 0., 0.01, 0.],
        [0.03, 0., 0.04, 0.],
        [0.06, 0., 0.065, 0.],
    ]) {
        assert!(actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (*a - b).abs() < 1e-7));
    }
    art.finish().unwrap();
    let mut art = CheckedArt::default();
    rectangle(
        &mut art,
        5.,
        5.,
        287.,
        200.,
        &DrawingLineStyleDto {
            width_mm: 0.25,
            dash_mm: vec![1e-100, 1e-100],
        },
    );
    assert!(art.segments.is_empty());
    assert_eq!(art.segments.capacity(), 0);
    assert!(art.finish().err().unwrap().contains("work"));
    let mut sheet = sheet();
    sheet.style.visible.dash_mm = vec![1e-100, 1e-100];
    assert!(try_render(&Source::new(&sheet, [297., 210.]))
        .err()
        .unwrap()
        .starts_with("Drawing frame:"));
    sheet.style.visible.dash_mm.clear();
    assert_eq!(
        try_render(&Source::new(&sheet, [297., 210.]))
            .unwrap()
            .segments
            .len(),
        17
    );
}

#[test]
fn dash_generation_stops_at_the_shared_primitive_limit() {
    let mut art = CheckedArt::default();
    rectangle(
        &mut art,
        5.,
        5.,
        287.,
        200.,
        &DrawingLineStyleDto {
            width_mm: 0.25,
            dash_mm: vec![0.001, 0.001],
        },
    );
    assert_eq!(art.segments.len(), 32_768);
    assert_eq!(art.segments.capacity(), 32_768);
    assert!(art.finish().err().unwrap().contains("primitive limit"));
}
