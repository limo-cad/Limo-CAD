use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use limo_cad_interface::{ControlInput, DocumentContext};
use serde_json::json;

pub(super) fn stamp() -> Stamp {
    Stamp {
        owner: DocumentContext {
            window_id: "main".into(),
            document_id: "drawing".into(),
            epoch: 7,
        },
        revision: 13,
        sheet_id: 1,
    }
}
pub(super) fn target() -> radial::Target {
    let document = fixture::document();
    radial::targets(
        &document.sheets[0].views[0],
        &fixture::projection(),
        [0., 0., 1.],
        DrawingRadialDimensionMode::Diameter,
    )
    .unwrap()
    .remove(0)
}
pub(super) fn definition() -> HoleDefinitionDto {
    serde_json::from_value(json!({"feature_id":15,"name":"Patterned threaded holes","body_id":1,"face_id":2,
        "position":{"x":20.,"y":15.},"positions":[{"position":{"x":20.,"y":15.}},{"position":{"x":30.,"y":15.}}],
        "diameter":6.,"extent":{"type":"distance","depth":16.},"style":"counterbore",
        "counterbore_diameter":10.,"counterbore_depth":2.5,"countersink_diameter":12.,"countersink_angle_deg":90.,"flip":false,
        "face_basis":{"origin":[0.,0.,6.],"u":[1.,0.,0.],"v":[0.,1.,0.],"normal":[0.,0.,1.]},
        "thread":{"standard":"iso_metric","series":"metric_coarse","designation":"M6 x 1","class":"6H","nominal_diameter":6.,"pitch":1.,"hand":"left","depth":12.}})).unwrap()
}
pub(super) fn created() -> DrawingDocumentDto {
    create(&fixture::document(), &stamp(), &target(), &[definition()]).unwrap()
}
fn last(document: &DrawingDocumentDto) -> &DrawingAnnotationDto {
    document.sheets[0].annotations.last().unwrap()
}
#[test]
fn hole_creation_uses_modeled_pattern_thread_and_exact_circle_without_touching_other_records() {
    let original = fixture::document();
    let created = created();
    let DrawingAnnotationDto::HoleNote {
        feature,
        position,
        quantity,
        diameter,
        depth,
        thread,
        note,
        source_feature_id,
        feature_name,
        hole_style,
        counterbore_diameter,
        counterbore_depth,
        countersink_diameter,
        thread_depth,
        pattern_note,
        ..
    } = last(&created)
    else {
        panic!()
    };
    assert_eq!(feature, &target().reference);
    assert_eq!(*position, [120., 115.]);
    assert_eq!((*quantity, *diameter, *depth), (2, 6., Some(16.)));
    assert_eq!(thread, "M6 x 1 - 6H LH");
    assert_eq!(
        (*source_feature_id, feature_name.as_str()),
        (Some(15), "Patterned threaded holes")
    );
    assert_eq!(*hole_style, DrawingHoleStyle::Counterbore);
    assert_eq!(
        (
            *counterbore_diameter,
            *counterbore_depth,
            *countersink_diameter,
            *thread_depth
        ),
        (Some(10.), Some(2.5), None, Some(12.))
    );
    assert_eq!(pattern_note, "2 HOLES");
    assert!(note.is_empty());
    assert_eq!(created.sheets[1], original.sheets[1]);
    assert_eq!(
        &created.sheets[0].annotations[..2],
        original.sheets[0].annotations.as_slice()
    );
    assert_eq!(
        created.sheets[0].release.status,
        DrawingReleaseStatus::Draft
    );
    assert_eq!(
        created.sheets[0].release.released_revision,
        original.sheets[0].release.released_revision
    );
    assert_eq!(
        created.sheets[0].release.released_at,
        original.sheets[0].release.released_at
    );
    assert_eq!(created.next_annotation_id, 5);
    let mut countersink = definition();
    countersink.style = HoleStyle::Countersink;
    countersink.extent = HoleExtent::ThroughAll;
    countersink.thread = None;
    let through = create(&original, &stamp(), &target(), &[countersink]).unwrap();
    let DrawingAnnotationDto::HoleNote {
        depth,
        note,
        hole_style,
        counterbore_diameter,
        countersink_diameter,
        countersink_angle_deg,
        ..
    } = last(&through)
    else {
        panic!()
    };
    assert_eq!(
        (
            *depth,
            note.as_str(),
            *hole_style,
            *counterbore_diameter,
            *countersink_diameter,
            *countersink_angle_deg
        ),
        (
            None,
            "THRU",
            DrawingHoleStyle::Countersink,
            None,
            Some(12.),
            Some(90.)
        )
    );
}
#[test]
fn hole_matching_requires_unique_body_radius_axis_and_three_dimensional_entry_position() {
    let target = target();
    let exact = definition();
    for change in [
        "body", "radius", "normal", "center", "basis", "plane", "nearby",
    ] {
        let mut wrong = exact.clone();
        match change {
            "body" => wrong.body_id.0 += 1,
            "radius" => wrong.diameter = 14.,
            "normal" => wrong.face_basis.as_mut().unwrap().normal = [1., 0., 0.],
            "center" => wrong.face_basis.as_mut().unwrap().origin = [100., 0., 6.],
            "plane" => {
                let basis = wrong.face_basis.as_mut().unwrap();
                basis.origin[2] = -10.;
                basis.normal = [0., 0., -1.];
                basis.v = [0., -1., 0.];
                for position in &mut wrong.positions {
                    position.position.y = -position.position.y;
                }
                wrong.flip = true;
            }
            "nearby" => wrong.face_basis.as_mut().unwrap().origin[0] += 0.02,
            _ => wrong.face_basis = None,
        }
        let definitions = [wrong];
        assert!(
            best_definition(&definitions, &target.reference)
                .unwrap()
                .is_none(),
            "{change}"
        );
    }
    let mut earlier = exact.clone();
    earlier.feature_id.0 -= 1;
    let definitions = [exact.clone(), earlier.clone()];
    assert!(best_definition(&definitions, &target.reference)
        .unwrap()
        .is_none());
    let mut occurrence = target.clone();
    occurrence.reference.occurrence_id = Some(serde_json::from_value(json!(91)).unwrap());
    let fallback = create(
        &fixture::document(),
        &stamp(),
        &occurrence,
        std::slice::from_ref(&exact),
    )
    .unwrap();
    let DrawingAnnotationDto::HoleNote {
        feature,
        quantity,
        diameter,
        depth,
        source_feature_id,
        ..
    } = last(&fallback)
    else {
        panic!()
    };
    assert_eq!(feature, &occurrence.reference);
    assert_eq!(
        (*quantity, *diameter, *depth, *source_feature_id),
        (1, 6., None, None)
    );
    let mut open = target.clone();
    open.reference.closed = false;
    assert!(create(&fixture::document(), &stamp(), &open, &[]).is_err());
    let too_many = vec![exact; 16_385];
    assert!(best_definition(&too_many, &target.reference).is_err());
}
#[test]
fn hole_enrichment_rejects_opposed_or_unresolved_sources_without_retiring_the_circle_association() {
    let target = target();
    let exact = definition();
    let mut flipped_samples = target.reference.clone();
    flipped_samples.fallback_normal = [0., 0., -1.];
    assert!(
        best_definition(std::slice::from_ref(&exact), &flipped_samples)
            .unwrap()
            .is_some(),
        "Circle sample winding cannot establish support-face orientation"
    );
    let mut opposed = exact.clone();
    opposed.feature_id.0 += 1;
    opposed.face_basis.as_mut().unwrap().normal = [0., 0., -1.];
    opposed.face_basis.as_mut().unwrap().v = [0., -1., 0.];
    for position in &mut opposed.positions {
        position.position.y = -position.position.y;
    }
    opposed.flip = true;
    opposed.extent = HoleExtent::Distance { depth: 3. };
    opposed.thread = None;
    let mut associative = exact.clone();
    associative.feature_id.0 += 2;
    associative.positions[0].position_reference = Some(limo_cad_solid::SketchPointRefDto {
        sketch_name: "Moved hole positions".into(),
        entity_id: 2,
        point: limo_cad_solid::SketchPointKindDto::Point,
    });
    associative.positions[0].position.x = 999.;
    for uncertain in [opposed, associative] {
        for definitions in [
            vec![exact.clone(), uncertain.clone()],
            vec![uncertain.clone(), exact.clone()],
        ] {
            let fallback = create(&fixture::document(), &stamp(), &target, &definitions).unwrap();
            let DrawingAnnotationDto::HoleNote {
                feature,
                source_feature_id,
                feature_name,
                thread,
                depth,
                quantity,
                ..
            } = last(&fallback)
            else {
                panic!()
            };
            assert_eq!(feature, &target.reference);
            assert_eq!(
                (
                    *source_feature_id,
                    feature_name.as_str(),
                    thread.as_str(),
                    *depth,
                    *quantity
                ),
                (None, "", "", None, 1)
            );
        }
    }
    let mut exit = target.clone();
    exit.reference.fallback_center[2] = -10.;
    assert!(
        best_definition(&[exact], &exit.reference)
            .unwrap()
            .is_none(),
        "Coaxial exit or counterbore shoulder circles are not the modeled entry point"
    );
}
#[test]
fn hole_fields_keep_nullable_hidden_values_metadata_and_invalid_raw_text_until_reset() {
    let saved = created();
    let selection = Selection {
        sheet_id: 1,
        annotation_id: 4,
    };
    let mut draft = Draft::new(&saved, selection).unwrap();
    let mut form = fields::from_annotation(draft.annotation());
    fields::apply(&mut draft, &form).unwrap();
    assert!(!draft.dirty());
    fields::edit(
        &mut form,
        Id::CounterboreDepth,
        &ControlInput::SetValue("unfinished".into()),
    )
    .unwrap();
    fields::edit(
        &mut form,
        Id::HoleStyle,
        &ControlInput::SetValue("simple".into()),
    )
    .unwrap();
    assert!(fields::visible(&form)
        .iter()
        .any(|i| form[*i].id == Id::CounterboreDepth));
    assert!(fields::apply(&mut draft, &form).is_err());
    assert!(!draft.dirty());
    assert_eq!(
        form.iter()
            .find(|f| f.id == Id::CounterboreDepth)
            .unwrap()
            .text,
        "unfinished"
    );
    for (id, value) in [
        (Id::CounterboreDepth, "2.5"),
        (Id::Depth, ""),
        (Id::ThreadDepth, ""),
        (Id::PatternNote, "2 PLACES"),
        (Id::Note, "Deburr\n零件"),
        (Id::X, "125.25"),
    ] {
        fields::edit(&mut form, id, &ControlInput::SetValue(value.into())).unwrap();
    }
    fields::apply(&mut draft, &form).unwrap();
    let changed = draft.apply(&saved).unwrap();
    let DrawingAnnotationDto::HoleNote {
        feature,
        source_feature_id,
        feature_name,
        counterbore_diameter,
        counterbore_depth,
        depth,
        thread_depth,
        note,
        hole_style,
        ..
    } = last(&changed)
    else {
        panic!()
    };
    assert_eq!(feature, &target().reference);
    assert_eq!(
        (*source_feature_id, feature_name.as_str()),
        (Some(15), "Patterned threaded holes")
    );
    assert_eq!(
        (
            *counterbore_diameter,
            *counterbore_depth,
            *depth,
            *thread_depth
        ),
        (Some(10.), Some(2.5), None, None)
    );
    assert_eq!(note, "Deburr\n零件");
    assert_eq!(*hole_style, DrawingHoleStyle::Simple);
    let preview = fields::hole_preview(
        draft.annotation(),
        &form,
        limo_cad_core::UnitSystem::In,
        DrawingStandard::Ansi,
    )
    .unwrap();
    assert_eq!(
        preview,
        limo_cad_occt::drawing_presentation::text::hole(
            draft.annotation(),
            limo_cad_core::UnitSystem::In,
            DrawingStandard::Ansi
        )
    );
    let reset = fields::from_annotation(last(&saved));
    assert!(!fields::dirty(&reset));
    for value in ["0", "10001", "1.5", "NaN"] {
        let mut bad = fields::from_annotation(last(&saved));
        fields::edit(
            &mut bad,
            Id::Quantity,
            &ControlInput::SetValue(value.into()),
        )
        .unwrap();
        assert!(fields::apply(&mut draft, &bad).is_err());
    }
    let mut altered = draft.annotation().clone();
    let DrawingAnnotationDto::HoleNote {
        source_feature_id, ..
    } = &mut altered
    else {
        panic!()
    };
    *source_feature_id = Some(999);
    assert!(draft.hole_note(altered).is_err());
    let mut bad = fields::from_annotation(last(&saved));
    fields::edit(
        &mut bad,
        Id::Note,
        &ControlInput::SetValue("n".repeat(4097)),
    )
    .unwrap();
    assert!(fields::apply(&mut draft, &bad).is_err());
}
#[test]
fn hole_drag_is_cumulative_clamped_and_preserves_association() {
    let saved = created();
    let mut draft = Draft::new(
        &saved,
        Selection {
            sheet_id: 1,
            annotation_id: 4,
        },
    )
    .unwrap();
    draft.move_hole([9., -4.], [297., 210.]).unwrap();
    let once = draft.annotation().clone();
    draft.move_hole([9., -4.], [297., 210.]).unwrap();
    assert_eq!(draft.annotation(), &once);
    draft.move_hole([1000., -1000.], [297., 210.]).unwrap();
    let DrawingAnnotationDto::HoleNote {
        position, feature, ..
    } = draft.annotation()
    else {
        panic!()
    };
    assert_eq!(*position, [292., 5.]);
    assert_eq!(feature, &target().reference);
    assert!(draft.move_hole([f64::NAN, 0.], [297., 210.]).is_err());
}

#[test]
fn hole_extent_controls_are_exclusive_and_unmatched_circles_do_not_claim_through() {
    let saved = created();
    let mut draft = Draft::new(
        &saved,
        Selection {
            sheet_id: 1,
            annotation_id: 4,
        },
    )
    .unwrap();
    let mut form = fields::from_annotation(draft.annotation());
    fields::edit(
        &mut form,
        Id::ThroughAll,
        &ControlInput::SetValue("true".into()),
    )
    .unwrap();
    assert!(form
        .iter()
        .find(|f| f.id == Id::Depth)
        .unwrap()
        .text
        .is_empty());
    fields::apply(&mut draft, &form).unwrap();
    let next = draft.apply(&saved).unwrap();
    assert!(matches!(
        last(&next),
        DrawingAnnotationDto::HoleNote {
            through_all: Some(true),
            depth: None,
            ..
        }
    ));
    fields::edit(&mut form, Id::Depth, &ControlInput::SetValue("8".into())).unwrap();
    assert_eq!(
        form.iter().find(|f| f.id == Id::ThroughAll).unwrap().text,
        "false"
    );
    fields::apply(&mut draft, &form).unwrap();
    let next = draft.apply(&saved).unwrap();
    assert!(matches!(
        last(&next),
        DrawingAnnotationDto::HoleNote {
            through_all: Some(false),
            depth: Some(8.),
            ..
        }
    ));
    let unknown = create(&fixture::document(), &stamp(), &target(), &[]).unwrap();
    assert!(matches!(
        last(&unknown),
        DrawingAnnotationDto::HoleNote {
            source_feature_id: None,
            through_all: Some(false),
            depth: None,
            ..
        }
    ));
    assert_eq!(
        limo_cad_occt::drawing_presentation::text::hole(
            last(&unknown),
            limo_cad_core::UnitSystem::Mm,
            DrawingStandard::Iso
        ),
        "⌀6"
    );
    let mut encoded = serde_json::to_value(&unknown).unwrap();
    let last = encoded["sheets"][0]["annotations"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last.as_object_mut().unwrap().remove("through_all");
    last["note"] = json!("THRU");
    let legacy: DrawingDocumentDto = serde_json::from_value(encoded).unwrap();
    let mut draft = Draft::new(
        &legacy,
        Selection {
            sheet_id: 1,
            annotation_id: 4,
        },
    )
    .unwrap();
    let form = fields::from_annotation(draft.annotation());
    fields::apply(&mut draft, &form).unwrap();
    assert!(!draft.dirty());
    assert_eq!(draft.apply(&legacy).unwrap(), legacy);
}
