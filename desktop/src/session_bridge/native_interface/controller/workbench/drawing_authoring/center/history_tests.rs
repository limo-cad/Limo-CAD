use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use limo_cad_interface::ControlInput;

#[test]
fn centers_create_edit_drag_delete_have_exact_shared_history_and_stale_receipt_fences() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    {
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
        let check_history = |before: &serde_json::Value, after: &serde_json::Value| {
            f.bridge
                .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
                .unwrap();
            assert_eq!(&exported(), before);
            f.bridge
                .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
                .unwrap();
            assert_eq!(&exported(), after);
        };
        apply(&fixture::document());
        let baseline = exported();
        let old = f
            .bridge
            .native_document_receipt(&f.engine, &f.owner())
            .unwrap();
        let created = super::tests::created(true);
        apply(&created);
        let created_model = exported();
        check_history(&baseline, &created_model);
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
        let saved = f.engine.drawing_snapshot();
        let selection = Selection {
            sheet_id: 1,
            annotation_id: 4,
        };
        let mut d = Draft::new(&saved, selection).unwrap();
        let mut form = fields::from_annotation(d.annotation());
        fields::edit(
            &mut form,
            Id::Extension,
            &ControlInput::SetValue("13.125".into()),
        )
        .unwrap();
        fields::apply(&mut d, &form).unwrap();
        assert_eq!(exported(), created_model);
        let edited = d.apply(&saved).unwrap();
        apply(&edited);
        let edited_model = exported();
        check_history(&created_model, &edited_model);
        let mut d = Draft::new(&edited, selection).unwrap();
        let (_, projection) = super::tests::fixture();
        let g = geometry(d.annotation(), &edited.sheets[0].views[0], &projection).unwrap();
        let point = add(g.grips[1].point, scale(g.grips[1].direction, 8.));
        d.center_extension(extension_at(g.grips[1], point).unwrap())
            .unwrap();
        d.center_extension(extension_at(g.grips[1], point).unwrap())
            .unwrap();
        assert_eq!(exported(), edited_model);
        let dragged = d.apply(&edited).unwrap();
        apply(&dragged);
        let dragged_model = exported();
        check_history(&edited_model, &dragged_model);
        let deleted = Draft::new(&dragged, selection)
            .unwrap()
            .delete(&dragged)
            .unwrap();
        apply(&deleted);
        let deleted_model = exported();
        check_history(&dragged_model, &deleted_model);
        assert_eq!(deleted.sheets[1], fixture::document().sheets[1]);
        assert_eq!(
            deleted.sheets[0].annotations,
            fixture::document().sheets[0].annotations
        );
        assert_eq!(
            deleted.sheets[0].release.released_revision,
            fixture::document().sheets[0].release.released_revision
        );
    }
}
