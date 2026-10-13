use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use limo_cad_interface::ControlInput;
#[test]
fn hole_create_edit_drag_delete_restore_exact_project_history_and_reject_stale_receipts() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let exported =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let apply = |document: &DrawingDocumentDto| {
        let receipt = f
            .bridge
            .native_document_receipt(&f.engine, &f.owner())
            .unwrap();
        f.bridge
            .apply_native_mutation_at(
                &f.engine,
                &receipt.owner,
                receipt.revision,
                "drawing_set_document",
                &serde_json::to_value(document).unwrap(),
                || Ok(()),
            )
            .unwrap();
    };
    let history = |before: &serde_json::Value, after: &serde_json::Value| {
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(&exported(), before);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(&exported(), after);
    };
    let original = fixture::document();
    apply(&original);
    let baseline = exported();
    let old = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let created = super::tests::created();
    apply(&created);
    let created_model = exported();
    history(&baseline, &created_model);
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &old.owner,
            old.revision,
            "drawing_set_document",
            &serde_json::to_value(&created).unwrap(),
            || Ok(())
        )
        .is_err());
    assert_eq!(exported(), created_model);
    let selection = Selection {
        sheet_id: 1,
        annotation_id: 4,
    };
    let mut draft = Draft::new(&created, selection).unwrap();
    let mut form = fields::from_annotation(draft.annotation());
    for (id, value) in [
        (Id::Diameter, "6.25"),
        (Id::Note, "INSPECT\n孔"),
        (Id::Quantity, "3"),
        (Id::ThreadDepth, ""),
    ] {
        fields::edit(&mut form, id, &ControlInput::SetValue(value.into())).unwrap();
    }
    fields::apply(&mut draft, &form).unwrap();
    assert_eq!(exported(), created_model);
    let edited = draft.apply(&created).unwrap();
    apply(&edited);
    let edited_model = exported();
    history(&created_model, &edited_model);
    let mut draft = Draft::new(&edited, selection).unwrap();
    draft.move_hole([8., -4.], [297., 210.]).unwrap();
    draft.move_hole([8., -4.], [297., 210.]).unwrap();
    assert_eq!(exported(), edited_model);
    let dragged = draft.apply(&edited).unwrap();
    apply(&dragged);
    let dragged_model = exported();
    history(&edited_model, &dragged_model);
    let deleted = Draft::new(&dragged, selection)
        .unwrap()
        .delete(&dragged)
        .unwrap();
    apply(&deleted);
    let deleted_model = exported();
    history(&dragged_model, &deleted_model);
    assert_eq!(
        deleted.sheets[0].annotations,
        original.sheets[0].annotations
    );
    assert_eq!(deleted.sheets[1], original.sheets[1]);
    assert_eq!(
        deleted.sheets[0].release.released_revision,
        original.sheets[0].release.released_revision
    );
    assert_eq!(
        deleted.sheets[0].release.released_at,
        original.sheets[0].release.released_at
    );
}
