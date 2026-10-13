use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use serde_json::json;

#[test]
fn presentation_apply_validates_shared_metadata_and_restores_exact_issued_history() {
    use fields::{tests as form, Id};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let seeded = form::document();
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "drawing_set_document",
            &serde_json::to_value(&seeded).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let drawing = f.engine.drawing_snapshot();
    let exported =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let before = exported();
    let receipt = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let mut invalid = drawing.clone();
    if let limo_cad_sketch::DrawingAnnotationDto::LinearDimension { presentation, .. } =
        &mut invalid.sheets[0].annotations[2]
    {
        presentation.dual_units.as_mut().unwrap().precision = 7;
    }
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(invalid).unwrap(),
            || Ok(())
        )
        .is_err());
    assert_eq!(exported(), before);
    assert_eq!(
        f.bridge
            .native_document_receipt(&f.engine, &f.owner())
            .unwrap()
            .revision,
        receipt.revision
    );
    let mut draft = form::linear(&drawing);
    let mut inputs = fields::from_annotation(draft.annotation());
    for (id, value) in [
        (Id::Tolerance, "deviation"),
        (Id::Upper, "0.25"),
        (Id::Lower, "-0.1"),
        (Id::Basic, "true"),
        (Id::Fit, "g6"),
        (Id::DualUnit, "inch"),
        (Id::DualPrecision, "3"),
        (Id::DualPlacement, "bracketed"),
    ] {
        form::set(&mut inputs, id, value);
    }
    fields::apply(&mut draft, &inputs).unwrap();
    let next = draft.apply(&drawing).unwrap();
    assert_eq!(
        next.sheets[0].release.status,
        limo_cad_sketch::DrawingReleaseStatus::Draft
    );
    assert_eq!(
        next.sheets[0].release.released_revision,
        drawing.sheets[0].release.released_revision
    );
    assert_eq!(next.sheets[1], drawing.sheets[1]);
    assert_eq!(
        &next.sheets[0].annotations[..2],
        &drawing.sheets[0].annotations[..2]
    );
    assert_eq!(
        exported(),
        before,
        "Editing fields cannot mutate the shared document before Apply"
    );
    f.bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(&next).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let after = exported();
    assert_eq!(f.engine.drawing_snapshot(), next);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        exported(),
        before,
        "One Undo must restore the full issued state, without a failed-validation history entry"
    );
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(exported(), after);
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(drawing).unwrap(),
            || Ok(())
        )
        .is_err());
    assert_eq!(exported(), after);
}

#[test]
fn annotation_edits_delete_and_creation_restore_exact_released_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let seeded = tests::document();
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "drawing_set_document",
            &serde_json::to_value(&seeded).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let drawing = f.engine.drawing_snapshot();
    let exported =
        || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let before = exported();
    let receipt = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let selection = draft::Selection {
        sheet_id: 1,
        annotation_id: 1,
    };
    let mut edit = draft::Draft::new(&drawing, selection).unwrap();
    edit.note("Caf\u{e9} \u{96f6}\u{4ef6}\nEdited note".into())
        .unwrap();
    edit.move_note([80., 90.], [297., 210.]).unwrap();
    let next = edit.apply(&drawing).unwrap();
    f.bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(&next).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let after = exported();
    assert_ne!(after, before);
    assert_eq!(f.engine.drawing_snapshot(), next);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(exported(), before);
    assert_eq!(f.engine.drawing_snapshot(), drawing);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(exported(), after);
    let stale = f.bridge.apply_native_mutation_at(
        &f.engine,
        &receipt.owner,
        receipt.revision,
        "drawing_set_document",
        &serde_json::to_value(&drawing).unwrap(),
        || Ok(()),
    );
    assert!(stale.is_err());
    assert_eq!(exported(), after);
    let deleted = draft::Draft::new(&next, selection)
        .unwrap()
        .delete(&next)
        .unwrap();
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "drawing_set_document",
            &serde_json::to_value(&deleted).unwrap(),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(f.engine.drawing_snapshot(), deleted);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(exported(), after);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(exported(), before);
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "drawing_add_note",
            &json!({"sheet_id":1,"text":"Created in native","position":[70.,80.]}),
            || Ok(()),
        )
        .unwrap();
    let created = f.engine.drawing_snapshot();
    assert_eq!(
        created.sheets[0].release.status,
        limo_cad_sketch::DrawingReleaseStatus::Draft
    );
    assert_eq!(created.sheets[1], drawing.sheets[1]);
    assert_eq!(
        &created.sheets[0].annotations[..2],
        &drawing.sheets[0].annotations[..]
    );
    let after_create = exported();
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(exported(), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(exported(), after_create);
}

#[test]
fn curved_dimension_inspectors_commit_one_exact_issued_history_entry() {
    use fields::{tests as form, Id};
    use limo_cad_sketch::DrawingAnnotationDto;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for id in [5, 6] {
        let f = Fixture::new();
        let seeded = form::curved_document();
        f.bridge
            .apply_native_mutation(
                &f.engine,
                &f.owner(),
                "drawing_set_document",
                &serde_json::to_value(&seeded).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let drawing = f.engine.drawing_snapshot();
        let exported =
            || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
        let before = exported();
        let receipt = f
            .bridge
            .native_document_receipt(&f.engine, &f.owner())
            .unwrap();
        let mut invalid = drawing.clone();
        match invalid.sheets[0]
            .annotations
            .iter_mut()
            .find(|a| a.id() == id)
            .unwrap()
        {
            DrawingAnnotationDto::RadialDimension { offset, .. } => *offset = 0.,
            DrawingAnnotationDto::AngularDimension { radius, .. } => *radius = 0.,
            _ => unreachable!(),
        }
        assert!(f
            .bridge
            .apply_native_mutation_at(
                &f.engine,
                &receipt.owner,
                receipt.revision,
                "drawing_set_document",
                &serde_json::to_value(invalid).unwrap(),
                || Ok(())
            )
            .is_err());
        assert_eq!(exported(), before);
        let selection = draft::Selection {
            sheet_id: 1,
            annotation_id: id,
        };
        let mut edit = draft::Draft::new(&drawing, selection).unwrap();
        let mut inputs = fields::from_annotation(edit.annotation());
        for (key, value) in [
            (Id::Tolerance, "symmetric"),
            (Id::Upper, "0.1"),
            (Id::Basic, "true"),
            (Id::Fit, "g6"),
            (Id::DualUnit, "inch"),
        ] {
            form::set(&mut inputs, key, value);
        }
        form::set(
            &mut inputs,
            if id == 5 { Id::Offset } else { Id::ArcRadius },
            "18",
        );
        fields::apply(&mut edit, &inputs).unwrap();
        let next = edit.apply(&drawing).unwrap();
        assert_eq!(
            exported(),
            before,
            "Draft must not mutate the shared document"
        );
        f.bridge
            .apply_native_mutation_at(
                &f.engine,
                &receipt.owner,
                receipt.revision,
                "drawing_set_document",
                &serde_json::to_value(&next).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let after = exported();
        assert_eq!(f.engine.drawing_snapshot(), next);
        assert_eq!(
            next.sheets[0].release.status,
            limo_cad_sketch::DrawingReleaseStatus::Draft
        );
        assert_eq!(
            next.sheets[0].release.released_revision,
            drawing.sheets[0].release.released_revision
        );
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(exported(), before);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(exported(), after);
        assert!(f
            .bridge
            .apply_native_mutation_at(
                &f.engine,
                &receipt.owner,
                receipt.revision,
                "drawing_set_document",
                &serde_json::to_value(&drawing).unwrap(),
                || Ok(())
            )
            .is_err());
        assert_eq!(
            exported(),
            after,
            "Stale form cannot overwrite curved dimension edits"
        );
        let removed = draft::Draft::new(&next, selection)
            .unwrap()
            .delete(&next)
            .unwrap();
        f.bridge
            .apply_native_mutation(
                &f.engine,
                &f.owner(),
                "drawing_set_document",
                &serde_json::to_value(removed).unwrap(),
                || Ok(()),
            )
            .unwrap();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(exported(), after, "Delete is exactly one history entry");
    }
}

#[test]
fn native_radial_and_angular_requests_use_existing_shared_creation_history() {
    use limo_cad_sketch::DrawingRadialDimensionMode;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for curved in [false, true] {
        let f = Fixture::new();
        let drawing = tests::document();
        for (op, args) in [
            (
                "sketch_begin",
                json!({"plane":{"type":"origin_plane","plane":"xy"}}),
            ),
            (
                if curved {
                    "sketch_add_rectangle"
                } else {
                    "sketch_add_circle"
                },
                if curved {
                    json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true})
                } else {
                    json!({"mode":"center_diameter","p1":{"x":20.,"y":15.},"p2":{"x":25.,"y":15.},"ctrl_held":true})
                },
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
            ),
        ] {
            f.bridge
                .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
                .unwrap();
        }
        let projected = parse_engine_envelope(f.engine.engine_call(
            "drawing_projection",
            &json!({"direction":[0.,0.,1.],"up":[0.,1.,0.],"include_hidden":true}).to_string(),
        ))
        .unwrap();
        let projection: limo_cad_occt::DrawingProjectionDto =
            serde_json::from_value(projected).unwrap();
        f.bridge
            .apply_native_mutation(
                &f.engine,
                &f.owner(),
                "drawing_set_document",
                &serde_json::to_value(&drawing).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let exported =
            || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
        let before = exported();
        let receipt = f
            .bridge
            .native_document_receipt(&f.engine, &f.owner())
            .unwrap();
        let stamp = Stamp {
            owner: receipt.owner.clone(),
            revision: receipt.revision,
            sheet_id: 1,
        };
        let (operation, args) = if curved {
            let targets = anchors::endpoints(
                &drawing.sheets[0].views[0],
                &projection,
                drawing.sheets[0].views[0].direction,
            )
            .unwrap();
            assert!(targets.len() >= 3);
            let (a, b) = (targets[0], targets[1]);
            let c = *targets
                .iter()
                .skip(2)
                .find(|c| {
                    ((b.point[0] - a.point[0]) * (c.point[1] - a.point[1])
                        - (b.point[1] - a.point[1]) * (c.point[0] - a.point[0]))
                        .abs()
                        > 1e-7
                })
                .expect("Real box projection must have a noncollinear third corner");
            let mut picks = angular::Placement::default();
            picks
                .click(&stamp, 1, anchors::endpoint_ref(a, &projection), a.point)
                .unwrap();
            picks
                .click(&stamp, 1, anchors::endpoint_ref(b, &projection), b.point)
                .unwrap();
            (
                "drawing_add_angular_dimension",
                serde_json::to_value(
                    picks
                        .click(&stamp, 1, anchors::endpoint_ref(c, &projection), c.point)
                        .unwrap()
                        .unwrap(),
                )
                .unwrap(),
            )
        } else {
            let targets = radial::targets(
                &drawing.sheets[0].views[0],
                &projection,
                drawing.sheets[0].views[0].direction,
                DrawingRadialDimensionMode::Radius,
            )
            .unwrap();
            (
                "drawing_add_radial_dimension",
                serde_json::to_value(
                    radial::request(&stamp, &targets[0], DrawingRadialDimensionMode::Radius)
                        .unwrap(),
                )
                .unwrap(),
            )
        };
        f.bridge
            .apply_native_mutation_at(
                &f.engine,
                &receipt.owner,
                receipt.revision,
                operation,
                &args,
                || Ok(()),
            )
            .unwrap();
        let created = f.engine.drawing_snapshot();
        let mut expected = drawing.clone();
        let added = created.sheets[0].annotations.last().unwrap();
        expected.sheets[0].annotations.push(added.clone());
        expected.sheets[0].release.status = limo_cad_sketch::DrawingReleaseStatus::Draft;
        expected.next_annotation_id += 1;
        assert_eq!(
            created, expected,
            "Existing shared command must preserve every other drawing record"
        );
        let mut exact_args = args.clone();
        exact_args.as_object_mut().unwrap().remove("sheet_id");
        exact_args["id"] = json!(drawing.next_annotation_id);
        exact_args["kind"] = json!(if curved {
            "angular_dimension"
        } else {
            "radial_dimension"
        });
        assert_eq!(
            serde_json::to_value(added).unwrap(),
            exact_args,
            "Exact topology and presentation references survive creation"
        );
        let after = exported();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(exported(), before);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(exported(), after);
    }
}

#[test]
fn series_and_ordinate_create_edit_drag_delete_each_restore_exact_issued_history() {
    use fields::{tests as form, Id};
    use limo_cad_sketch::{DrawingAnnotationDto, DrawingReleaseStatus};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for layout in series::tests::layouts() {
        let f = Fixture::new();
        let seed = tests::document();
        let apply = |doc: &limo_cad_sketch::DrawingDocumentDto| {
            f.bridge
                .apply_native_mutation(
                    &f.engine,
                    &f.owner(),
                    "drawing_set_document",
                    &serde_json::to_value(doc).unwrap(),
                    || Ok(()),
                )
                .unwrap()
        };
        apply(&seed);
        let exported =
            || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
        let baseline = exported();
        let document = f.engine.drawing_snapshot();
        let mut refs = series::tests::refs();
        if layout.is_none() {
            refs.truncate(2);
        }
        let next = series::create(&document, 1, 1, refs, layout).unwrap();
        apply(&next);
        let created = exported();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(exported(), baseline);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(exported(), created);
        let mut saved = f.engine.drawing_snapshot();
        saved.sheets[0].release.status = DrawingReleaseStatus::Released;
        apply(&saved);
        let issued = exported();
        let saved = f.engine.drawing_snapshot();
        let selection = draft::Selection {
            sheet_id: 1,
            annotation_id: 4,
        };
        let mut d = draft::Draft::new(&saved, selection).unwrap();
        let mut fields = fields::from_annotation(d.annotation());
        form::set(&mut fields, Id::Offset, "-16");
        form::set(&mut fields, Id::Tolerance, "deviation");
        form::set(&mut fields, Id::Upper, "0.2");
        form::set(&mut fields, Id::Lower, "-0.1");
        if layout.is_some() {
            form::set(&mut fields, Id::Spacing, "11");
        } else {
            form::set(&mut fields, Id::Axis, "x");
        }
        fields::apply(&mut d, &fields).unwrap();
        assert_eq!(exported(), issued);
        let edited = d.apply(&saved).unwrap();
        apply(&edited);
        let edited_model = exported();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(exported(), issued);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(exported(), edited_model);
        let mut d = draft::Draft::new(&edited, selection).unwrap();
        if layout.is_some() {
            d.move_linear([0., 0.], [40., 0.], [3., 8.]).unwrap();
        } else {
            d.move_ordinate([3., 8.]).unwrap();
        }
        assert_eq!(
            exported(),
            edited_model,
            "Pointer previews do not mutate the model"
        );
        let dragged = d.apply(&edited).unwrap();
        apply(&dragged);
        let dragged_model = exported();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(exported(), edited_model);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(exported(), dragged_model);
        let deleted = draft::Draft::new(&dragged, selection)
            .unwrap()
            .delete(&dragged)
            .unwrap();
        apply(&deleted);
        let deleted_model = exported();
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
            .unwrap();
        assert_eq!(exported(), dragged_model);
        f.bridge
            .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
            .unwrap();
        assert_eq!(exported(), deleted_model);
        assert_eq!(deleted.sheets[1], document.sheets[1]);
        assert_eq!(
            deleted.sheets[0].annotations,
            document.sheets[0].annotations
        );
        assert_eq!(
            deleted.sheets[0].release.released_revision,
            document.sheets[0].release.released_revision
        );
        assert!(matches!(
            dragged.sheets[0].annotations.last().unwrap(),
            DrawingAnnotationDto::ChainDimension { .. }
                | DrawingAnnotationDto::OrdinateDimension { .. }
        ));
    }
}
