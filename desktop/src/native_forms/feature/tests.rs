use super::*;
use limo_cad_core::{Document, FaceId, Feature, UnitSystem};

#[test]
fn numeric_steps_resolve_units_and_expressions_without_reusing_invalid_input() {
    let mut fixture = Fixture::new();
    fixture.document.settings.units = UnitSystem::In;
    let parameters = vec![ParameterValue {
        name: "stock".into(),
        kind: DimensionKind::Length,
        value: 25.4,
    }];
    let mut model = fixture.model();
    model.parameters = &parameters;
    let mut form = fixture.form();
    form.set_value(SolidField::Distance, "stock/2", &model)
        .unwrap();
    form.step_value(SolidField::Distance, 1, &model).unwrap();
    let ExtrudeExtent::Distance { distance } =
        form.prepare_preview(&model).unwrap().request().extent
    else {
        panic!("A stepped length keeps its extent");
    };
    assert!((distance - 38.1).abs() < 1e-9);
    form.set_value(SolidField::Taper, "45/3 deg", &model)
        .unwrap();
    form.step_value(SolidField::Taper, -1, &model).unwrap();
    assert_eq!(
        form.prepare_preview(&model)
            .unwrap()
            .request()
            .taper_angle_deg,
        14.
    );
    form.set_value(SolidField::Distance, "unfinished+", &model)
        .unwrap();
    assert!(form.step_value(SolidField::Distance, 1, &model).is_err());
    assert!(form
        .fields(&model)
        .iter()
        .any(|r| matches!(&r.value, Field::Text{value,..} if value=="unfinished+")));
    assert!(!form.can_apply(&model));
}

#[test]
fn reference_presentation_keeps_numeric_focus_stable_until_the_accepted_source_changes() {
    let fixture = Fixture::new();
    let model = fixture.model();
    let mut form = SolidForm::new(&model);
    assert!(!form.presentation(&model).references[&SolidField::Source].has_selection);
    assert!(form.presentation(&model).auto_focus.is_none());
    form.set_source(
        ProfileSource::Profiles {
            sketch_name: "Sketch1".into(),
            indices: vec![0],
        },
        &model,
    )
    .unwrap();
    let accepted = form.presentation(&model);
    assert!(accepted.references[&SolidField::Source].has_selection);
    assert_eq!(
        accepted.references[&SolidField::Source].caption,
        "1 profile selected · Sketch1"
    );
    form.set_value(SolidField::Distance, "20", &model).unwrap();
    assert_eq!(accepted.auto_focus, form.presentation(&model).auto_focus);
    form.set_source(ProfileSource::None, &model).unwrap();
    assert!(form.presentation(&model).auto_focus.is_none());
    assert!(!form.presentation(&model).references[&SolidField::Source].has_selection);

    let mut hole = SolidForm::new_kind(SolidFormKind::Hole, &model);
    hole.set_hole_support(
        Some(PlanarFaceSourceDto {
            body_id: BodyId(1),
            face_id: FaceId(12),
        }),
        Some([5.00001335144043, 5.0000152587890625, 10.]),
        &model,
    )
    .unwrap();
    let before = hole.hole_payload(&model).unwrap().1;
    let status = hole.presentation(&model);
    assert!(status.references[&SolidField::HolePositions].has_selection);
    assert_eq!(
        status.references[&SolidField::HolePositions].caption,
        "1 position selected"
    );
    assert_eq!(before, hole.hole_payload(&model).unwrap().1);
}

#[test]
fn rib_uses_typed_lengths_and_extent_specific_references() {
    use limo_cad_solid::{PathRefDto, RibExtent, RibRequest};
    let mut fixture = Fixture::new();
    fixture.profiles[0].path_curves = serde_json::from_value(json!([
        {"kind":"line","entity_id":20,"start":{"x":0.,"y":0.},"end":{"x":20.,"y":0.}},
        {"kind":"line","entity_id":21,"start":{"x":30.,"y":5.},"end":{"x":40.,"y":5.}}
    ]))
    .unwrap();
    let mut model = fixture.model();
    let parameters = vec![ParameterValue {
        name: "stock".into(),
        kind: DimensionKind::Length,
        value: 20.,
    }];
    model.parameters = &parameters;
    let mut form = SolidForm::new_kind(SolidFormKind::Rib, &model);
    form.set_path(
        SolidField::Path,
        Some(PathRefDto {
            sketch_name: "Sketch1".into(),
            entity_ids: vec![20, 21],
        }),
        &model,
    )
    .unwrap();
    assert_eq!(form.parameter_sketch(), Some("Sketch1"));
    assert!(
        form.can_apply(&model),
        "Rib centerlines need not be connected"
    );
    form.set_value(SolidField::Thickness, "stock/10", &model)
        .unwrap();
    form.set_value(SolidField::Distance, "1/4 in", &model)
        .unwrap();
    let (_, request) = form.rib_payload(&model).unwrap();
    let request: RibRequest = serde_json::from_value(request).unwrap();
    assert_eq!(request.thickness, 2.);
    assert_eq!(request.extent, Some(RibExtent::Distance { depth: 6.35 }));
    form.set_value(SolidField::Thickness, "0", &model).unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::Thickness, "2", &model).unwrap();
    form.set_value(SolidField::Extent, "to_next", &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::Operation, "join", &model)
        .unwrap();
    form.set_targets(vec![BodyId(1)], &model).unwrap();
    form.set_value(SolidField::Distance, "unfinished+", &model)
        .unwrap();
    assert!(form.can_apply(&model));
    form.set_value(SolidField::Extent, "to_face", &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    form.set_stop_face(
        Some(PlanarFaceSourceDto {
            body_id: BodyId(1),
            face_id: FaceId(12),
        }),
        &model,
    )
    .unwrap();
    form.set_value(SolidField::Symmetric, "true", &model)
        .unwrap();
    let ticket = form.prepare_apply(&model).unwrap();
    assert_eq!(ticket.operation(), "solid_rib");
    assert_eq!(ticket.arguments()["extent"]["face_id"], 12);
    assert_eq!(ticket.arguments()["symmetric"], true);
    assert_eq!(ticket.arguments()["target_body_ids"], json!([1]));
}

#[test]
fn sweep_paths_preserve_curve_order_validate_connectivity_and_ignore_disabled_guides() {
    use limo_cad_solid::{PathRefDto, ProfileRefDto, SweepRequest};
    let mut fixture = Fixture::new();
    fixture.profiles[0].path_curves = serde_json::from_value(json!([
        {"kind":"line","entity_id":20,"start":{"x":0.,"y":0.},"end":{"x":10.,"y":0.}},
        {"kind":"line","entity_id":21,"start":{"x":20.,"y":0.},"end":{"x":10.,"y":0.}},
        {"kind":"line","entity_id":22,"start":{"x":50.,"y":0.},"end":{"x":60.,"y":0.}}
    ]))
    .unwrap();
    let model = fixture.model();
    let mut form = SolidForm::new_kind(SolidFormKind::Sweep, &model);
    form.set_profiles(
        vec![ProfileRefDto {
            sketch_name: "Sketch1".into(),
            profile_index: 0,
        }],
        &model,
    )
    .unwrap();
    let path = |ids| {
        Some(PathRefDto {
            sketch_name: "Sketch1".into(),
            entity_ids: ids,
        })
    };
    assert!(form
        .set_path(SolidField::Path, path(vec![20, 20]), &model)
        .is_err());
    assert!(form
        .set_path(SolidField::Path, path(vec![90]), &model)
        .is_err());
    form.set_path(SolidField::Path, path(vec![20, 22]), &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    form.set_path(SolidField::Path, path(vec![20, 21]), &model)
        .unwrap();
    form.set_value(SolidField::GuideEnabled, "true", &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::GuideEnabled, "false", &model)
        .unwrap();
    for (field, value) in [
        (SolidField::Orientation, "fixed"),
        (SolidField::Transition, "round_corner"),
        (SolidField::ForceC1, "true"),
    ] {
        form.set_value(field, value, &model).unwrap();
    }
    let ticket = form.prepare_apply(&model).unwrap();
    let request: SweepRequest = serde_json::from_value(ticket.arguments().clone()).unwrap();
    assert_eq!(request.path_entity_ids, vec![20, 21]);
    assert!(request.guide_rail.is_none());
    assert!(request.force_c1);
    assert_eq!(request.orientation, limo_cad_solid::SweepOrientation::Fixed);
    assert_eq!(
        request.transition,
        limo_cad_solid::SweepTransition::RoundCorner
    );
    assert_eq!(ticket.operation(), "solid_sweep");
}

#[test]
fn loft_keeps_cross_sketch_section_order_and_edit_identity_without_silent_repair() {
    use limo_cad_solid::{EditLoftRequest, LoftDefinitionDto, ProfileRefDto};
    let mut fixture = Fixture::new();
    let mut second = fixture.profiles[0].clone();
    second.sketch_name = "Section2".into();
    second.basis.origin[2] = 30.;
    fixture.profiles.push(second);
    fixture
        .document
        .features
        .push(Feature::new(FeatureId(10), "Loft", FeatureKind::Loft));
    let model = fixture.model();
    let sections = vec![
        ProfileRefDto {
            sketch_name: "Section2".into(),
            profile_index: 0,
        },
        ProfileRefDto {
            sketch_name: "Sketch1".into(),
            profile_index: 0,
        },
    ];
    let mut form = SolidForm::new_kind(SolidFormKind::Loft, &model);
    assert!(form
        .set_profiles(vec![sections[0].clone(), sections[0].clone()], &model)
        .is_err());
    form.set_profiles(sections.clone(), &model).unwrap();
    assert!(form.can_apply(&model));
    form.set_value(SolidField::CenterlineEnabled, "true", &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::CenterlineEnabled, "false", &model)
        .unwrap();
    assert!(form.can_apply(&model));
    let d:LoftDefinitionDto=serde_json::from_value(json!({"feature_id":10,"name":"Loft","sections":sections,"ruled":true,"continuity":"g2","operation":"new_body","target_body_ids":[],"new_body_id":10})).unwrap();
    let mut edit = SolidForm::edit_loft(&d, &model).unwrap();
    let ticket = edit.prepare_apply(&model).unwrap();
    let payload: EditLoftRequest = serde_json::from_value(ticket.arguments().clone()).unwrap();
    assert_eq!(payload.feature_id, FeatureId(10));
    assert_eq!(payload.loft.sections, sections);
    assert!(payload.loft.ruled);
    assert_eq!(payload.loft.continuity, limo_cad_solid::LoftContinuity::G2);
    let mut broken = d;
    broken.sections[0].sketch_name = "Deleted".into();
    let edit = SolidForm::edit_loft(&broken, &model).unwrap();
    assert!(!edit.can_apply(&model));
    assert_eq!(edit.selected_profiles()[0].sketch_name, "Deleted");
}

#[test]
fn revolve_preserves_axis_identity_and_rejects_non_coplanar_references() {
    let mut fixture = Fixture::new();
    let mut axis = fixture.profiles[0].clone();
    axis.sketch_name = "Axis".into();
    axis.profiles.clear();
    axis.lines = vec![limo_cad_solid::SketchLineDto {
        entity_id: 20,
        start: limo_cad_solid::Point2Dto::new(0., 0.),
        end: limo_cad_solid::Point2Dto::new(10., 0.),
    }];
    fixture.profiles.push(axis);
    let mut form = SolidForm::new_kind(SolidFormKind::Revolve, &fixture.model());
    form.set_axis(Some(("Axis".into(), 20)), &fixture.model())
        .unwrap();
    form.set_source(
        ProfileSource::Profiles {
            sketch_name: "Sketch1".into(),
            indices: vec![0],
        },
        &fixture.model(),
    )
    .unwrap();
    form.set_value(SolidField::Angle, "360/2", &fixture.model())
        .unwrap();
    assert!(form.can_apply(&fixture.model()));
    assert_eq!(
        form.revolution_axis(&fixture.model()).unwrap().unwrap(),
        [[0., 0., 0.], [10., 0., 0.]]
    );
    fixture.profiles[1].basis.origin[2] = 5.;
    assert!(!form.can_apply(&fixture.model()));
    assert!(form
        .fields(&fixture.model())
        .iter()
        .any(|f| f.field == SolidField::AxisLine && f.error.is_some()));
    fixture.profiles[1].basis.origin[2] = 0.;
    let ticket = form.prepare_apply(&fixture.model()).unwrap();
    let request: limo_cad_solid::RevolveRequest =
        serde_json::from_value(ticket.arguments().clone()).unwrap();
    assert_eq!(ticket.operation(), "solid_revolve");
    assert_eq!(request.axis_line_sketch_name.as_deref(), Some("Axis"));
    assert_eq!(request.axis_line_entity_id, Some(20));
    assert_eq!(request.angle_deg, 180.);
}

#[test]
fn revolve_hidden_fields_do_not_block_presets_and_custom_units_are_typed() {
    let fixture = Fixture::new();
    let model = fixture.model();
    let mut form = SolidForm::new_kind(SolidFormKind::Revolve, &model);
    form.set_source(
        ProfileSource::Profiles {
            sketch_name: "Sketch1".into(),
            indices: vec![0],
        },
        &model,
    )
    .unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::Axis, "custom", &model).unwrap();
    form.set_value(SolidField::OriginX, "broken+", &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::Axis, "x", &model).unwrap();
    assert!(form.can_apply(&model));
    form.set_value(SolidField::Axis, "custom", &model).unwrap();
    form.set_value(SolidField::OriginX, "1 in", &model).unwrap();
    form.set_value(SolidField::DirectionY, "0", &model).unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::DirectionX, "1", &model).unwrap();
    form.set_value(SolidField::Operation, "cut", &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    form.set_targets(vec![BodyId(1)], &model).unwrap();
    assert!(form.can_apply(&model));
    for invalid in ["0", "361", "-361", "1/0"] {
        form.set_value(SolidField::Angle, invalid, &model).unwrap();
        assert!(!form.can_apply(&model));
    }
    form.set_value(SolidField::Angle, "90", &model).unwrap();
    let ticket = form.prepare_apply(&model).unwrap();
    assert_eq!(ticket.arguments()["axis_origin"]["x"], 25.4);
    assert_eq!(ticket.arguments()["target_body_ids"], json!([1]));
}

struct Fixture {
    owner: DocumentContext,
    document: DocumentDto,
    profiles: Vec<ProfileCatalogItemDto>,
    scene: SolidSceneDto,
}
impl Fixture {
    fn new() -> Self {
        let basis = json!({"origin":[0.,0.,0.],"u":[1.,0.,0.],"v":[0.,1.,0.],"normal":[0.,0.,1.]});
        Self {
            owner:DocumentContext {window_id:"main".into(),document_id:"tab-a".into(),epoch:3},
            document:DocumentDto::from(&Document::new("Form test")),
            profiles:serde_json::from_value(json!([{"sketch_name":"Sketch1","feature_id":1,"basis":basis,
                "profiles":[{"index":0,"points":[{"x":0.,"y":0.},{"x":10.,"y":0.},{"x":10.,"y":10.},{"x":0.,"y":10.}],"area":100.,"nesting_depth":0},
                {"index":1,"points":[],"area":1.,"nesting_depth":1,"parent_index":0}]}])).unwrap(),
            scene:serde_json::from_value(json!({"bodies":[{"id":1,"name":"Base","feature_id":2,"mesh":{"positions":[],"normals":[],"indices":[]},"edges":[],
                "faces":[{"id":11,"key":"bottom","first_index":0,"index_count":0,"plane":basis},{"id":12,"key":"top","first_index":0,"index_count":0,"plane":{"origin":[0.,0.,10.],"u":[1.,0.,0.],"v":[0.,1.,0.],"normal":[0.,0.,1.]}}]}],"errors":[]})).unwrap(),
        }
    }
    fn model(&self) -> FormModel<'_> {
        FormModel {
            owner: &self.owner,
            engine_revision: 7,
            document: &self.document,
            profiles: &self.profiles,
            scene: &self.scene,
            datum_planes: &[],
            parameters: &[],
            assembly: None,
            assembly_solution: None,
        }
    }
    fn form(&self) -> SolidForm {
        let model = self.model();
        let mut form = SolidForm::new(&model);
        form.set_source(
            ProfileSource::Profiles {
                sketch_name: "Sketch1".into(),
                indices: vec![0],
            },
            &model,
        )
        .unwrap();
        form
    }
}

#[test]
fn typed_measurements_and_field_errors_drive_the_actual_canonical_request() {
    let mut fixture = Fixture::new();
    fixture.document.settings.units = UnitSystem::In;
    let model = fixture.model();
    let mut form = fixture.form();
    assert!(form.can_apply(&model));
    form.set_value(SolidField::Distance, "=1/4 in", &model)
        .unwrap();
    form.set_value(SolidField::Taper, "=sin(30)*10 deg", &model)
        .unwrap();
    let preview = form.prepare_preview(&model).unwrap();
    assert_eq!(
        preview.request().extent,
        ExtrudeExtent::Distance { distance: 6.35 }
    );
    assert!((preview.request().taper_angle_deg - 5.).abs() < 1e-12);
    form.set_value(SolidField::Distance, "1/0", &model).unwrap();
    assert!(!form.can_apply(&model));
    assert!(form.prepare_apply(&model).is_err());
    let fields = form.fields(&model);
    let distance = fields
        .iter()
        .find(|field| field.field == SolidField::Distance)
        .unwrap();
    assert_eq!(distance.label, "Distance (in)");
    assert!(distance
        .error
        .as_ref()
        .unwrap()
        .contains("division by zero"));
    assert!(matches!(&distance.value,Field::Text{value,..} if value=="1/0"));
    assert!(!form.accepts_preview(&preview, &model));
}

#[test]
fn stop_face_and_target_picks_never_overwrite_the_accepted_source() {
    let fixture = Fixture::new();
    let model = fixture.model();
    let mut form = SolidForm::new(&model);
    let source = PlanarFaceSourceDto {
        body_id: BodyId(1),
        face_id: FaceId(11),
    };
    let stop = PlanarFaceSourceDto {
        body_id: BodyId(1),
        face_id: FaceId(12),
    };
    form.set_source(ProfileSource::Face(source), &model)
        .unwrap();
    form.set_value(SolidField::Extent, "to_face", &model)
        .unwrap();
    form.set_stop_face(Some(stop), &model).unwrap();
    form.set_targets(vec![BodyId(1)], &model).unwrap();
    let ticket = form.prepare_apply(&model).unwrap();
    assert_eq!(ticket.operation(), "solid_extrude");
    let request: ExtrudeRequest = serde_json::from_value(ticket.arguments().clone()).unwrap();
    assert_eq!(request.source_face, Some(source));
    assert_eq!(
        request.extent,
        ExtrudeExtent::ToFace {
            face_id: stop.face_id
        }
    );
    assert_eq!(request.operation, ExtrudeOperation::Join);
    assert_eq!(request.target_body_ids, vec![BodyId(1)]);
    assert!(request.sketch_name.is_empty() && request.profile_indices.is_empty());
}

#[test]
fn stop_face_feedback_matches_the_kernel_parallel_and_nonzero_extent_contract() {
    let mut fixture = Fixture::new();
    let stop = PlanarFaceSourceDto {
        body_id: BodyId(1),
        face_id: FaceId(12),
    };
    fixture.scene.bodies[0].faces[1]
        .plane
        .as_mut()
        .unwrap()
        .normal = [0., 1., 0.];
    let model = fixture.model();
    let mut form = fixture.form();
    form.set_value(SolidField::Extent, "to_face", &model)
        .unwrap();
    form.set_stop_face(Some(stop), &model).unwrap();
    assert!(!form.can_apply(&model));
    assert!(form
        .fields(&model)
        .into_iter()
        .find(|field| field.field == SolidField::StopFace)
        .unwrap()
        .error
        .unwrap()
        .contains("parallel"));
    fixture.scene.bodies[0].faces[1].plane = fixture.scene.bodies[0].faces[0].plane;
    assert!(!form.can_apply(&fixture.model()));
    assert!(form
        .fields(&fixture.model())
        .into_iter()
        .find(|field| field.field == SolidField::StopFace)
        .unwrap()
        .error
        .unwrap()
        .contains("source plane"));
}

#[test]
fn stale_references_and_invalid_profile_regions_cannot_be_applied() {
    let fixture = Fixture::new();
    let model = fixture.model();
    let mut form = fixture.form();
    for indices in [vec![1], vec![0, 0], vec![99]] {
        assert!(form
            .set_source(
                ProfileSource::Profiles {
                    sketch_name: "Sketch1".into(),
                    indices
                },
                &model
            )
            .is_err());
    }
    assert!(form.set_targets(vec![BodyId(999)], &model).is_err());
    assert!(form
        .set_stop_face(
            Some(PlanarFaceSourceDto {
                body_id: BodyId(999),
                face_id: FaceId(12)
            }),
            &model
        )
        .is_err());
    let changed = FormModel {
        engine_revision: 8,
        ..fixture.model()
    };
    assert!(!form.can_apply(&changed));
    assert!(form.prepare_apply(&changed).is_err());
    assert!(
        !form.cancel(&changed).unwrap(),
        "stale Cancel closes without restoring old preview over the new document"
    );
    assert!(!form.is_open());
}

#[test]
fn preview_and_apply_receipts_cannot_complete_newer_edits_or_retry_attempts() {
    let fixture = Fixture::new();
    let model = fixture.model();
    let mut form = fixture.form();
    let preview = form.prepare_preview(&model).unwrap();
    assert!(form.accepts_preview(&preview, &model));
    let first = form.prepare_apply(&model).unwrap();
    assert!(form.is_busy());
    assert!(!form.accepts_preview(&preview, &model));
    assert!(form.set_value(SolidField::Distance, "20", &model).is_err());
    assert!(form.cancel(&model).is_err());
    form.apply_failed(&first, &model, "Exact kernel failure".into())
        .unwrap();
    assert_eq!(form.engine_error(), Some("Exact kernel failure"));
    assert!(form.can_apply(&model));
    let retry = form.prepare_apply(&model).unwrap();
    assert!(form
        .apply_failed(&first, &model, "Late failure from the old attempt".into())
        .is_err());
    assert!(form.is_busy());
    assert_eq!(retry.owner(), &fixture.owner);
    assert_eq!(retry.model_revision(), 7);
    form.apply_succeeded(&retry, &fixture.owner, 8).unwrap();
    assert!(!form.is_open());
    assert!(form.apply_succeeded(&retry, &fixture.owner, 9).is_err());
    let mut cancelled = fixture.form();
    let ticket = cancelled.prepare_preview(&model).unwrap();
    assert!(cancelled.cancel(&model).unwrap());
    assert!(!cancelled.accepts_preview(&ticket, &model));
    assert!(
        !fixture.form().accepts_preview(&ticket, &model),
        "new forms never inherit old preview identity"
    );
}

#[test]
fn extent_specific_validation_ignores_hidden_inputs_and_validates_both_sides() {
    let fixture = Fixture::new();
    let model = fixture.model();
    let mut form = fixture.form();
    form.set_value(SolidField::Extent, "two_sides", &model)
        .unwrap();
    form.set_value(SolidField::SecondDistance, "-2", &model)
        .unwrap();
    assert!(!form.can_apply(&model));
    let fields = form.fields(&model);
    assert!(fields
        .iter()
        .find(|field| field.field == SolidField::SecondDistance)
        .unwrap()
        .error
        .is_some());
    form.set_value(SolidField::Extent, "through_all", &model)
        .unwrap();
    form.set_value(SolidField::Distance, "incomplete+", &model)
        .unwrap();
    assert!(
        form.can_apply(&model),
        "hidden distance input does not block Through All"
    );
    assert!(
        !form
            .fields(&model)
            .iter()
            .find(|field| field.field == SolidField::Distance)
            .unwrap()
            .visible
    );
    form.set_value(SolidField::Taper, "90", &model).unwrap();
    assert!(!form.can_apply(&model));
    form.set_value(SolidField::Taper, "2 mm", &model).unwrap();
    assert!(!form.can_apply(&model));
}

#[test]
fn editing_preserves_the_existing_feature_id_and_explicit_operation() {
    let mut fixture = Fixture::new();
    fixture
        .document
        .features
        .push(Feature::new(FeatureId(9), "Extrude", FeatureKind::Extrude));
    let model = fixture.model();
    let definition:ExtrudeDefinitionDto=serde_json::from_value(json!({"feature_id":9,"name":"Extrude","source_face":null,"sketch_name":"Sketch1","profile_indices":[0],"operation":"cut","extent":{"type":"distance","distance":12.},"taper_angle_deg":0.,"flip":true,"target_body_ids":[1],"new_body_ids":[]})).unwrap();
    let mut form = SolidForm::edit(&definition, &model).unwrap();
    let apply = form.prepare_apply(&model).unwrap();
    assert_eq!(apply.operation(), "solid_edit_extrude");
    let request: EditExtrudeRequest = serde_json::from_value(apply.arguments().clone()).unwrap();
    assert_eq!(request.feature_id, FeatureId(9));
    assert_eq!(request.extrude.operation, ExtrudeOperation::Cut);
    assert!(request.extrude.flip);
    assert_eq!(
        request.extrude.extent,
        ExtrudeExtent::Distance { distance: 12. }
    );
}
