use super::super::tables::{self as table_model, Table};
use super::*;

fn fixture() -> DrawingDocumentDto {
    let mut document = auto_layout(&document(), &Default::default()).unwrap();
    let mut other_view = document.sheets[0].views[0].clone();
    other_view.id = 5;
    document.sheets[1].views.push(other_view);
    document.next_view_id = 6;
    document.next_revision_id = 5;
    document.next_bom_item_id = 14;
    document.next_annotation_id = 6;
    document.sheets[0].title_block.revision = "C".into();
    document.sheets[0].title_block.author = "Designer".into();
    document.sheets[0].title_block.approved_by = "Approver".into();
    document.sheets[0].revisions = (1..=3).map(|id| serde_json::from_value(json!({
        "id":id,"revision":if id == 1 {"A"} else if id == 2 {"B"} else {"C"},
        "description":format!("Saved revision {id}"),"date":"2026-09-26",
        "author":"Original designer","checked_by":"Original checker","approved_by":"Original approver",
        "change_order":format!("EC-{id}"),"status":if id == 1 {"released"} else if id == 2 {"superseded"} else {"draft"}
    })).unwrap()).collect();
    document.sheets[1].revisions = vec![serde_json::from_value(json!({
        "id":4,"revision":"Z","description":"Other sheet","status":"released"
    }))
    .unwrap()];
    let bom = |id, number| {
        serde_json::from_value(json!({
            "id":id,"item_number":number,"body_id":1,"part_number":"P-001",
            "description":"Saved stock","quantity":0.625,"material":"Aluminium","finish":"Anodized"
        }))
        .unwrap()
    };
    document.sheets[0].bom = vec![bom(11, "A-10"), bom(12, "A-20")];
    document.sheets[1].bom = vec![bom(13, "A-10")];
    let balloon = |id, view, row| {
        serde_json::from_value(json!({
        "kind":"item_balloon","id":id,"view_id":view,"bom_item_id":row,"position":[60.,70.],
        "attachment":{"type":"anchor","reference":{"body_id":1,"edge_id":1,"edge_key":"saved-edge",
            "endpoint":"start","fallback_point":[2.,3.,4.]}}
    })).unwrap()
    };
    document.sheets[0].annotations = vec![
        balloon(1, 1, 11),
        balloon(2, 1, 11),
        balloon(3, 1, 12),
        serde_json::from_value(
            json!({"kind":"note","id":4,"text":"Untouched note","position":[20.,30.]}),
        )
        .unwrap(),
    ];
    document.sheets[1].annotations = vec![balloon(5, 5, 13)];
    release(&mut document);
    document.validate().unwrap();
    document
}

fn field<'a>(draft: &'a Draft, path: &str) -> &'a str {
    &draft.fields.iter().find(|f| f.path == path).unwrap().text
}

#[test]
fn table_choice_captions_bound_unicode_text_without_truncating_rows_or_saved_metadata() {
    let mut before = fixture();
    let description = "\u{96f6}\u{1f980}".repeat(2048);
    before.sheets[0].revisions[2].revision = "\u{1f980}".repeat(32);
    before.sheets[0].revisions[2].description = description.clone();
    before.sheets[0].bom[0].item_number = "\u{96f6}\u{1f980}".repeat(4096);
    before.sheets[0].bom[0].description = description.clone();
    before.sheets[0].bom[1].item_number = "P".into();
    before.sheets[0].bom[1].description = "\u{1f980}".repeat(76);
    before.validate().unwrap();
    let saved = before.clone();
    let revisions = table_model::rows(&before, 1, Table::Revisions).unwrap();
    let bom = table_model::rows(&before, 1, Table::Bom).unwrap();
    assert_eq!(
        revisions.iter().map(|r| r.0).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(bom.iter().map(|r| r.0).collect::<Vec<_>>(), vec![0, 11, 12]);
    assert!(revisions
        .iter()
        .chain(&bom)
        .all(|(_, caption)| caption.chars().count() <= 81));
    assert_eq!(
        revisions[3].1,
        format!(
            "{} · {}\u{2026}",
            "\u{1f980}".repeat(32),
            description.chars().take(45).collect::<String>()
        )
    );
    assert_eq!(
        bom[1].1,
        format!("{}\u{2026}", "\u{96f6}\u{1f980}".repeat(40))
    );
    assert_eq!(
        bom[2].1,
        format!("P · {}", "\u{1f980}".repeat(76)),
        "An exactly 80-character caption has no ellipsis"
    );
    assert_eq!(
        before, saved,
        "Building display choices must not rewrite stored strings"
    );

    let mut revision = Draft::new(&before, Selection::Row(1, Table::Revisions, 3)).unwrap();
    assert_eq!(field(&revision, "/description"), description);
    edit(&mut revision, "/date", "2026-10-01");
    let mut expected = before.clone();
    expected.sheets[0].revisions[2].date = "2026-10-01".into();
    expected.sheets[0].title_block.revision = before.sheets[0].revisions[2].revision.clone();
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(revision.apply(&before).unwrap(), expected);
    let mut item = Draft::new(&before, Selection::Row(1, Table::Bom, 11)).unwrap();
    assert_eq!(field(&item, "/description"), description);
    edit(&mut item, "/quantity", "1.25");
    let mut expected = before.clone();
    expected.sheets[0].bom[0].quantity = 1.25;
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(item.apply(&before).unwrap(), expected);
}

#[test]
fn new_table_rows_are_disposable_and_allocate_one_checked_shared_identity_on_apply() {
    let before = fixture();
    let saved = before.clone();
    for (table, next_id) in [(Table::Revisions, 5), (Table::Bom, 14)] {
        let mut canceled = Draft::new(&before, Selection::NewRow(1, table)).unwrap();
        assert!(canceled.dirty());
        assert!(!canceled.read_only());
        assert_eq!(
            canceled.committed_selection(),
            Selection::Row(1, table, next_id)
        );
        edit(&mut canceled, "/description", "Unapplied draft");
        drop(canceled);
        assert_eq!(
            before, saved,
            "Cancel cannot allocate IDs or mutate any saved table"
        );
        let mut draft = Draft::new(&before, Selection::NewRow(1, table)).unwrap();
        if table == Table::Revisions {
            assert_eq!(field(&draft, "/author"), "Designer");
            assert_eq!(field(&draft, "/checked_by"), "Checker");
            assert_eq!(field(&draft, "/approved_by"), "Approver");
            assert_eq!(field(&draft, "/status"), "draft");
            assert!(!field(&draft, "/date").is_empty());
            edit(&mut draft, "/revision", "D");
            edit(&mut draft, "/date", "2026-09-27");
        }
        edit(&mut draft, "/description", "New shared row");
        let actual = draft.apply(&before).unwrap();
        let mut expected = before.clone();
        expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
        if table == Table::Revisions {
            expected.next_revision_id += 1;
            expected.sheets[0].title_block.revision = "D".into();
            expected.sheets[0].revisions.push(DrawingRevisionDto {
                id: 5,
                revision: "D".into(),
                description: "New shared row".into(),
                date: "2026-09-27".into(),
                author: "Designer".into(),
                checked_by: "Checker".into(),
                approved_by: "Approver".into(),
                change_order: String::new(),
                status: DrawingReleaseStatus::Draft,
            });
        } else {
            expected.next_bom_item_id += 1;
            expected.sheets[0].bom.push(DrawingBomItemDto {
                id: 14,
                item_number: "3".into(),
                body_id: None,
                part_number: String::new(),
                description: "New shared row".into(),
                quantity: 1.,
                material: String::new(),
                finish: String::new(),
            });
        }
        assert_eq!(actual, expected);
        assert_eq!(before, saved);
    }
}

#[test]
fn new_revision_release_and_existing_draft_release_preserve_prior_issued_rows() {
    for selection in [
        Selection::NewRow(1, Table::Revisions),
        Selection::Row(1, Table::Revisions, 3),
    ] {
        let before = fixture();
        let mut draft = Draft::new(&before, selection).unwrap();
        edit(&mut draft, "/revision", "D");
        edit(&mut draft, "/description", "Released change");
        edit(&mut draft, "/date", "2026-09-27");
        edit(&mut draft, "/status", "released");
        let actual = draft.apply(&before).unwrap();
        let mut expected = before.clone();
        let row = if matches!(selection, Selection::NewRow(..)) {
            expected.next_revision_id += 1;
            expected.sheets[0].revisions.push(DrawingRevisionDto {
                id: 5,
                revision: "D".into(),
                description: "Released change".into(),
                date: "2026-09-27".into(),
                author: "Designer".into(),
                checked_by: "Checker".into(),
                approved_by: "Approver".into(),
                change_order: String::new(),
                status: DrawingReleaseStatus::Released,
            });
            expected.sheets[0].revisions.last_mut().unwrap()
        } else {
            &mut expected.sheets[0].revisions[2]
        };
        row.revision = "D".into();
        row.description = "Released change".into();
        row.date = "2026-09-27".into();
        row.status = DrawingReleaseStatus::Released;
        expected.sheets[0].title_block.revision = "D".into();
        expected.sheets[0].release = DrawingReleaseDto {
            status: DrawingReleaseStatus::Released,
            released_revision: "D".into(),
            released_at: "2026-09-27".into(),
        };
        assert_eq!(actual, expected);
        assert_eq!(actual.sheets[0].revisions[0], before.sheets[0].revisions[0]);
    }
}

#[test]
fn released_rows_reject_form_and_final_mutation_but_superseded_rows_remain_editable() {
    let before = fixture();
    let selection = Selection::Row(1, Table::Revisions, 1);
    let mut draft = Draft::new(&before, selection).unwrap();
    assert!(draft.read_only());
    assert_eq!(
        draft.apply(&before).unwrap(),
        before,
        "An unchanged issued row is a no-op"
    );
    let index = draft
        .fields
        .iter()
        .position(|f| f.path == "/description")
        .unwrap();
    assert!(draft.set(index, "Forbidden edit".into()).is_err());
    draft.fields[index].text = "Bypass form setter".into();
    assert!(draft.apply(&before).unwrap_err().contains("Released"));
    assert!(table_model::delete(&before, selection)
        .unwrap_err()
        .contains("Released"));
    let mut forged = table_model::record(&before, selection).unwrap();
    forged["status"] = json!("draft");
    assert!(
        table_model::apply(&before, selection, forged).is_err(),
        "Final adapter must not downgrade an issued row"
    );

    for status in [
        DrawingReleaseStatus::Superseded,
        DrawingReleaseStatus::Obsolete,
    ] {
        let mut saved = before.clone();
        saved.sheets[0].revisions[1].status = status;
        let selection = Selection::Row(1, Table::Revisions, 2);
        let mut draft = Draft::new(&saved, selection).unwrap();
        assert!(!draft.read_only());
        edit(
            &mut draft,
            "/description",
            "Corrected historical description",
        );
        let actual = draft.apply(&saved).unwrap();
        let mut expected = saved.clone();
        expected.sheets[0].revisions[1].description = "Corrected historical description".into();
        expected.sheets[0].title_block.revision = "B".into();
        expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
        assert_eq!(actual, expected);
        let actual = table_model::delete(&saved, selection).unwrap();
        let mut expected = saved.clone();
        expected.sheets[0].revisions.remove(1);
        expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
        assert_eq!(
            actual, expected,
            "Delete preserves the title revision and counters"
        );
    }
}

#[test]
fn bom_edits_preserve_shared_ids_fractional_quantities_and_balloon_identity() {
    let before = fixture();
    let selection = Selection::Row(1, Table::Bom, 11);
    let mut draft = Draft::new(&before, selection).unwrap();
    assert_eq!(field(&draft, "/quantity"), "0.625");
    edit(&mut draft, "/quantity", "0.62500");
    assert_eq!(
        draft.apply(&before).unwrap(),
        before,
        "Numeric normalization is a full no-op"
    );
    edit(&mut draft, "/item_number", "ALT-7");
    edit(&mut draft, "/description", "Caf\u{e9} \u{96f6}\u{4ef6}");
    let actual = draft.apply(&before).unwrap();
    let mut expected = before.clone();
    expected.sheets[0].bom[0].item_number = "ALT-7".into();
    expected.sheets[0].bom[0].description = "Caf\u{e9} \u{96f6}\u{4ef6}".into();
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(actual, expected);
    assert_eq!(actual.sheets[0].annotations, before.sheets[0].annotations);
    let mut detached = Draft::new(&actual, selection).unwrap();
    edit(&mut detached, "/body_id", "");
    expected.sheets[0].bom[0].body_id = None;
    assert_eq!(detached.apply(&actual).unwrap(), expected);
    let mut rebound = Draft::new(&expected, selection).unwrap();
    edit(&mut rebound, "/body_id", "42");
    expected.sheets[0].bom[0].body_id = Some(limo_cad_core::BodyId(42));
    assert_eq!(
        rebound.apply(&detached.apply(&actual).unwrap()).unwrap(),
        expected,
        "Existing DTO permits a nonzero saved body reference"
    );
}

#[test]
fn deleting_a_bom_row_removes_only_its_same_sheet_balloons_and_keeps_counters() {
    let before = fixture();
    let actual = table_model::delete(&before, Selection::Row(1, Table::Bom, 11)).unwrap();
    let mut expected = before.clone();
    expected.sheets[0].bom.remove(0);
    expected.sheets[0].annotations.drain(..2);
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(actual, expected);
    assert!(table_model::delete(&actual, Selection::Row(1, Table::Bom, 11)).is_err());
    assert!(table_model::delete(&before, Selection::NewRow(1, Table::Bom)).is_err());
}

#[test]
fn table_visibility_and_position_use_paper_mm_and_react_show_defaults() {
    for (table, default) in [(Table::Revisions, [10., 10.]), (Table::Bom, [10., 45.])] {
        let before = fixture();
        let selection = Selection::Table(1, table);
        let mut draft = Draft::new(&before, selection).unwrap();
        assert_eq!(draft.apply(&before).unwrap(), before);
        edit(&mut draft, "/x", "37.5");
        edit(&mut draft, "/y", "64.25");
        let moved = draft.apply(&before).unwrap();
        let mut expected = before.clone();
        let position = match table {
            Table::Revisions => &mut expected.sheets[0].revision_table_position,
            Table::Bom => &mut expected.sheets[0].bom_table_position,
        };
        *position = Some([37.5, 64.25]);
        expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
        assert_eq!(moved, expected);
        let mut hide = Draft::new(&moved, selection).unwrap();
        edit(&mut hide, "/visibility", "hidden");
        let hidden = hide.apply(&moved).unwrap();
        let position = match table {
            Table::Revisions => &mut expected.sheets[0].revision_table_position,
            Table::Bom => &mut expected.sheets[0].bom_table_position,
        };
        *position = None;
        assert_eq!(hidden, expected);
        let mut show = Draft::new(&hidden, selection).unwrap();
        edit(&mut show, "/x", "123");
        edit(&mut show, "/visibility", "shown");
        let position = match table {
            Table::Revisions => &mut expected.sheets[0].revision_table_position,
            Table::Bom => &mut expected.sheets[0].bom_table_position,
        };
        *position = Some(default);
        assert_eq!(
            show.apply(&hidden).unwrap(),
            expected,
            "Showing resets the prior hidden position"
        );
        let mut toggle = Draft::new(&before, selection).unwrap();
        edit(&mut toggle, "/visibility", "hidden");
        edit(&mut toggle, "/visibility", "shown");
        let toggled = toggle.apply(&before).unwrap();
        assert_eq!(
            match table {
                Table::Revisions => toggled.sheets[0].revision_table_position,
                Table::Bom => toggled.sheets[0].bom_table_position,
            },
            Some(default)
        );
    }
}

#[test]
fn invalid_and_stale_table_drafts_leave_saved_records_and_counters_exact() {
    let before = fixture();
    for (selection, path, values) in [
        (
            Selection::Row(1, Table::Revisions, 3),
            "/revision",
            vec!["", " "],
        ),
        (
            Selection::Row(1, Table::Bom, 11),
            "/quantity",
            vec!["0", "-1", "NaN", "inf", "text"],
        ),
        (
            Selection::Row(1, Table::Bom, 11),
            "/body_id",
            vec!["0", "-1", "1.5", "text"],
        ),
        (
            Selection::Table(1, Table::Bom),
            "/x",
            vec!["NaN", "inf", "text"],
        ),
    ] {
        for value in values {
            let mut draft = Draft::new(&before, selection).unwrap();
            edit(&mut draft, path, value);
            assert!(draft.apply(&before).is_err(), "Accepted {path}={value}");
            assert_eq!(
                field(&draft, path),
                value,
                "Invalid form text stays available for correction"
            );
        }
    }
    for table in [Table::Revisions, Table::Bom] {
        let mut exhausted = before.clone();
        match table {
            Table::Revisions => exhausted.next_revision_id = u64::MAX,
            Table::Bom => exhausted.next_bom_item_id = u64::MAX,
        };
        let saved = exhausted.clone();
        let draft = Draft::new(&exhausted, Selection::NewRow(1, table)).unwrap();
        assert!(draft.apply(&exhausted).unwrap_err().contains("exhausted"));
        assert_eq!(exhausted, saved);
        let draft = Draft::new(&before, Selection::NewRow(1, table)).unwrap();
        let mut changed = before.clone();
        match table {
            Table::Revisions => changed.next_revision_id += 1,
            Table::Bom => changed.next_bom_item_id += 1,
        };
        assert!(draft
            .apply(&changed)
            .unwrap_err()
            .contains("Drawing changed"));
        let mut changed = before.clone();
        changed.sheets[0].title_block.author = "Changed seed".into();
        assert!(draft
            .apply(&changed)
            .unwrap_err()
            .contains("Drawing changed"));
    }
    for (selection, change) in [
        (Selection::Row(1, Table::Revisions, 3), true),
        (Selection::Row(1, Table::Bom, 11), false),
    ] {
        let mut draft = Draft::new(&before, selection).unwrap();
        edit(&mut draft, "/description", "Unapplied");
        let mut changed = before.clone();
        if change {
            changed.sheets[0].revisions[2].description = "Other writer".into();
        } else {
            changed.sheets[0].bom[0].quantity = 2.;
        }
        assert!(draft
            .apply(&changed)
            .unwrap_err()
            .contains("Drawing changed"));
    }
    assert_eq!(before, fixture());
}

#[test]
fn table_release_and_linked_bom_deletion_restore_exact_shared_model_history() {
    use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let apply = |document: &DrawingDocumentDto| {
        f.bridge
            .apply_native_mutation(
                &f.engine,
                &f.owner(),
                "drawing_set_document",
                &serde_json::to_value(document).unwrap(),
                || Ok(()),
            )
            .unwrap()
    };
    let export =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    apply(&fixture());
    let seeded = f.engine.drawing_snapshot();
    let initial = export();
    let receipt = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let mut draft = Draft::new(&seeded, Selection::NewRow(1, Table::Revisions)).unwrap();
    edit(&mut draft, "/revision", "D");
    edit(&mut draft, "/date", "2026-09-27");
    edit(&mut draft, "/status", "released");
    assert_eq!(
        export(),
        initial,
        "A staged Release does not touch document or history"
    );
    let issued = draft.apply(&seeded).unwrap();
    apply(&issued);
    let issued_model = export();
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(), initial);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(), issued_model);
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(&issued).unwrap(),
            || Ok(())
        )
        .is_err());
    assert_eq!(
        export(),
        issued_model,
        "Stale Apply cannot create another history entry"
    );
    let deleted = table_model::delete(
        &f.engine.drawing_snapshot(),
        Selection::Row(1, Table::Bom, 11),
    )
    .unwrap();
    apply(&deleted);
    let deleted_model = export();
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        export(),
        issued_model,
        "One Undo restores both BOM row and all linked balloons"
    );
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(), deleted_model);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        export(),
        initial,
        "Two Undos restore the exact pre-release document"
    );
}
