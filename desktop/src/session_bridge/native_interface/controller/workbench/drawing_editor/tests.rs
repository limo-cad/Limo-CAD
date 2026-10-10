use super::model::*;
use limo_cad_sketch::*;
use serde_json::json;
mod integration;
mod tables;

fn document() -> DrawingDocumentDto {
    let sheets: Vec<_> = (1..=8).map(|id| json!({
        "id":id,"name":format!("Sheet {id}"),"format":"a4","orientation":"landscape",
        "title_block":{"title":"Saved title","checked_by":"Checker","company":"Existing company"},
        "template_name":"Issued template","tolerance_note":{"preset":"custom","custom":"Saved tolerance"},
        "revision_table_position":[14.,17.],"bom_table_position":[20.,22.]
    })).collect();
    let document: DrawingDocumentDto =
        serde_json::from_value(json!({"sheets":sheets,"active_sheet_id":1,"next_sheet_id":9}))
            .unwrap();
    document.validate().unwrap();
    document
}
fn edit(draft: &mut Draft, path: &str, value: &str) {
    let index = draft.fields.iter().position(|f| f.path == path).unwrap();
    draft.set(index, value.into()).unwrap();
}

fn release(document: &mut DrawingDocumentDto) {
    for sheet in &mut document.sheets {
        sheet.release = DrawingReleaseDto {
            status: DrawingReleaseStatus::Released,
            released_revision: "A".into(),
            released_at: "2026-09-26".into(),
        };
    }
}

#[test]
fn released_sheet_and_view_content_edits_revoke_only_the_edited_release() {
    let mut before = document();
    release(&mut before);
    let mut draft = Draft::new(&before, Selection::Sheet(7)).unwrap();
    assert_eq!(
        draft.apply(&before).unwrap(),
        before,
        "No-op must retain release"
    );
    edit(&mut draft, "/name", "Edited sheet");
    let actual = draft.apply(&before).unwrap();
    let mut expected = before.clone();
    expected.sheets[6].name = "Edited sheet".into();
    expected.sheets[6].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(actual, expected);

    let mut laid_out = auto_layout(&before, &Default::default()).unwrap();
    assert_eq!(
        laid_out.sheets[0].release.status,
        DrawingReleaseStatus::Draft
    );
    assert_eq!(laid_out.sheets[0].release.released_revision, "A");
    assert_eq!(laid_out.sheets[0].release.released_at, "2026-09-26");
    assert_eq!(laid_out.sheets[1..], before.sheets[1..]);
    release(&mut laid_out);
    let mut view = Draft::new(&laid_out, Selection::View(1)).unwrap();
    edit(&mut view, "/scale", "1.0000");
    assert_eq!(
        view.apply(&laid_out).unwrap(),
        laid_out,
        "Text-only numeric normalization retains release"
    );
    edit(&mut view, "/scale", "0.5");
    let actual = view.apply(&laid_out).unwrap();
    let mut expected = laid_out.clone();
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    for view in &mut expected.sheets[0].views {
        view.scale = 0.5;
    }
    assert_eq!(actual, expected);
}

#[test]
fn sheet_draft_preserves_untouched_sheet_content_and_all_other_sheets() {
    let before = document();
    let mut draft = Draft::new(&before, Selection::Sheet(7)).unwrap();
    assert!(!draft.dirty());
    assert_eq!(draft.apply(&before).unwrap(), before);
    edit(&mut draft, "/name", "Release drawing");
    edit(&mut draft, "/format", "ansi_d");
    edit(&mut draft, "/orientation", "portrait");
    edit(&mut draft, "/title_block/approved_by", "Reviewer");
    assert!(draft.dirty());
    let actual = draft.apply(&before).unwrap();
    let mut expected = before.clone();
    expected.sheets[6].name = "Release drawing".into();
    expected.sheets[6].format = DrawingSheetFormat::AnsiD;
    expected.sheets[6].orientation = DrawingSheetOrientation::Portrait;
    expected.sheets[6].title_block.approved_by = "Reviewer".into();
    assert_eq!(actual, expected);
    assert_eq!(sheets(&before).len(), 8);
    assert_eq!(sheets(&before)[7], (8, "Sheet 8".into()));
    assert_eq!(before.sheets[6].name, "Sheet 7");
}

#[test]
fn view_edits_preserve_group_scale_and_projected_alignment() {
    let before = auto_layout(&document(), &Default::default()).unwrap();
    let mut scale = Draft::new(&before, Selection::View(2)).unwrap();
    edit(&mut scale, "/scale", "0.5");
    let scaled = scale.apply(&before).unwrap();
    assert!(scaled.sheets[0].views.iter().all(|v| v.scale == 0.5));
    let mut placement = Draft::new(&scaled, Selection::View(1)).unwrap();
    let original = scaled.sheets[0].views[0].position;
    edit(
        &mut placement,
        "/position/0",
        &(original[0] + 10.).to_string(),
    );
    edit(
        &mut placement,
        "/position/1",
        &(original[1] + 20.).to_string(),
    );
    let moved = placement.apply(&scaled).unwrap();
    let views = &moved.sheets[0].views;
    assert_eq!(views[0].position, [original[0] + 10., original[1] + 20.]);
    assert_eq!(views[1].position[0], views[0].position[0]);
    assert_eq!(views[1].position[1], scaled.sheets[0].views[1].position[1]);
    assert_eq!(views[2].position[1], views[0].position[1]);
    assert_eq!(views[2].position[0], scaled.sheets[0].views[2].position[0]);
    assert_eq!(views[3], scaled.sheets[0].views[3]);
    let mut child = Draft::new(&moved, Selection::View(2)).unwrap();
    edit(&mut child, "/position/0", "999");
    edit(&mut child, "/position/1", "190");
    let constrained = child.apply(&moved).unwrap();
    assert_eq!(
        constrained.sheets[0].views[1].position,
        [views[0].position[0], 190.]
    );
}

#[test]
fn auto_layout_matches_first_and_third_angle_and_refuses_existing_views() {
    for projection_method in [
        DrawingProjectionMethod::FirstAngle,
        DrawingProjectionMethod::ThirdAngle,
    ] {
        let mut before = document();
        before.sheets[0].projection_method = projection_method;
        let actual = auto_layout(&before, &Default::default()).unwrap();
        assert_eq!(actual.sheets[1..], before.sheets[1..]);
        assert_eq!(actual.next_view_id, 5);
        let group = &actual.sheets[0].views;
        assert_eq!(group.len(), 4);
        assert_eq!(group[0].position, [297. * 0.39, 210. * 0.47]);
        assert_eq!(group[0].parent_view_id, None);
        assert!(group[1..]
            .iter()
            .all(|v| v.parent_view_id == Some(group[0].id)));
        let third = projection_method == DrawingProjectionMethod::ThirdAngle;
        assert_eq!(group[1].position[1] < group[0].position[1], third);
        assert_eq!(group[2].position[0] > group[0].position[0], third);
        assert_eq!(group[3].position, [297. * 0.74, 210. * 0.31]);
        assert!(group.iter().all(|v| v.scale == 1.));
        assert!(auto_layout(&actual, &Default::default()).is_err());
    }
}

#[test]
fn invalid_or_stale_drafts_do_not_replace_saved_intent() {
    let before = auto_layout(&document(), &Default::default()).unwrap();
    for value in ["NaN", "inf", "0", "-0.5", "invalid"] {
        let mut draft = Draft::new(&before, Selection::View(1)).unwrap();
        edit(&mut draft, "/scale", value);
        assert!(draft.apply(&before).is_err(), "accepted {value}");
    }
    let mut draft = Draft::new(&before, Selection::Sheet(1)).unwrap();
    edit(&mut draft, "/name", "Edited");
    let mut changed = before.clone();
    changed.sheets[0].title_block.title = "Another editor changed this".into();
    assert!(draft
        .apply(&changed)
        .unwrap_err()
        .contains("Drawing changed"));
    assert_eq!(changed.sheets[0].name, before.sheets[0].name);
    let format = draft
        .fields
        .iter()
        .position(|f| f.path == "/format")
        .unwrap();
    assert!(draft.set(format, "made_up".into()).is_err());
}

#[test]
fn changing_standard_matches_react_defaults_and_retains_subsequent_overrides() {
    let before = document();
    let mut draft = Draft::new(&before, Selection::Sheet(1)).unwrap();
    edit(&mut draft, "/standard", "ansi");
    let defaults = draft.apply(&before).unwrap();
    assert_eq!(defaults.sheets[0].format, DrawingSheetFormat::Letter);
    assert_eq!(
        defaults.sheets[0].projection_method,
        DrawingProjectionMethod::ThirdAngle
    );
    assert_eq!(
        defaults.sheets[0].tolerance_note.preset,
        DrawingTolerancePreset::AnsiDecimal
    );
    assert_eq!(defaults.sheets[0].tolerance_note.custom, "");
    edit(&mut draft, "/format", "ansi_d");
    edit(&mut draft, "/projection_method", "first_angle");
    edit(&mut draft, "/tolerance_note/preset", "custom");
    edit(&mut draft, "/tolerance_note/custom", "Company tolerance");
    let actual = draft.apply(&before).unwrap();
    assert_eq!(actual.sheets[0].format, DrawingSheetFormat::AnsiD);
    assert_eq!(
        actual.sheets[0].projection_method,
        DrawingProjectionMethod::FirstAngle
    );
    assert_eq!(actual.sheets[0].tolerance_note.custom, "Company tolerance");
    assert_eq!(actual.sheets[0].title_block, before.sheets[0].title_block);
    assert_eq!(actual.sheets[1..], before.sheets[1..]);
    edit(&mut draft, "/standard", "iso");
    let iso = draft.apply(&before).unwrap();
    assert_eq!(iso.sheets[0].format, DrawingSheetFormat::A4);
    assert_eq!(
        iso.sheets[0].tolerance_note.preset,
        DrawingTolerancePreset::Iso2768Medium
    );
}
