use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use limo_cad_interface::{ControlInput, DocumentContext};
use limo_cad_occt::DrawingProjectionDto;
use limo_cad_solid::SolidSceneDto;
use serde_json::{json, Value};

fn stamp() -> Stamp {
    Stamp {
        owner: DocumentContext {
            window_id: "main".into(),
            document_id: "chamfer".into(),
            epoch: 2,
        },
        revision: 7,
        sheet_id: 1,
    }
}
fn geometry() -> (SolidSceneDto, DrawingProjectionDto, DrawingViewDto) {
    let segments = [
        [[0., 2., 0.], [2., 0., 0.]],
        [[0., 2., 0.], [0., 20., 0.]],
        [[2., 0., 0.], [30., 0., 0.]],
    ];
    let edges: Vec<Value> = segments
        .iter()
        .enumerate()
        .map(|(i, ends)| {
            json!({"id":i+1,"key":format!("edge-{}",i+1),
        "points":ends.map(|p|json!({"x":p[0],"y":p[1],"z":p[2]}))})
        })
        .collect();
    let anchors: Vec<Value> = segments
        .iter()
        .enumerate()
        .flat_map(|(i, ends)| {
            ends.iter().enumerate().map(move |(n, p)| {
                json!({
        "body_id":1,"edge_id":i+1,"edge_key":format!("edge-{}",i+1),"occurrence_id":31,
        "endpoint":if n==0{"start"}else{"end"},"model_point":p,"point":[p[0],p[1]],"hidden":false})
            })
        })
        .collect();
    let visible: Vec<Value> = segments
        .iter()
        .map(|ends| json!({"points":ends.map(|p|[p[0],p[1]])}))
        .collect();
    let scene=serde_json::from_value(json!({"bodies":[{"id":1,"name":"Bevel","feature_id":1,"mesh":{"positions":[],"indices":[],"normals":[]},"edges":edges,"faces":[]}],"errors":[]})).unwrap();
    let projection=serde_json::from_value(json!({"bounds":[0.,0.,30.,20.],"visible":visible,"hidden":[],"anchors":anchors,"topology_signatures":{"1":"exact-bevel-topology"}})).unwrap();
    (
        scene,
        projection,
        fixture::document().sheets[0].views[0].clone(),
    )
}
fn target() -> Target {
    let (s, p, v) = geometry();
    targets(&s, &v, &p, [0., 0., 1.]).unwrap().remove(0)
}
pub(super) fn created() -> DrawingDocumentDto {
    let mut p = Placement::default();
    p.pick(&stamp(), target());
    p.create(&fixture::document(), &stamp()).unwrap()
}

#[test]
fn chamfer_uses_setback_and_published_basis_and_preserves_exact_endpoint_identity() {
    let (s, mut p, mut v) = geometry();
    v.direction = [1., 0., 0.];
    for a in &mut p.anchors {
        a.model_point[0] += 100.;
        a.model_point[2] = 6.;
    }
    let found = targets(&s, &v, &p, [0., 0., 1.]).unwrap();
    assert_eq!(found.len(), 1);
    let t = &found[0];
    assert!((t.length - 2.).abs() < 1e-12);
    assert!((t.angle - 45.).abs() < 1e-12);
    assert!((t.length - 8_f64.sqrt()).abs() > 0.8);
    assert_eq!(t.first.occurrence_id.unwrap().0, 31);
    assert_eq!(
        t.first.topology_signature.as_deref(),
        Some("exact-bevel-topology")
    );
    assert_eq!(t.first.fallback_point, [100., 2., 6.]);
    assert_eq!(t.second.fallback_point, [102., 0., 6.]);
    assert_eq!(t.line.paper, [[85., 108.], [87., 110.]]);
    assert!(!t.first.circle_center);
    assert_ne!(t.first.endpoint, t.second.endpoint);
    assert!(targets(&s, &v, &p, [1., 0., 0.]).unwrap().is_empty());
}
#[test]
fn unequal_chamfer_prefers_visible_carrier_before_longer_carrier() {
    let (mut s, mut p, v) = geometry();
    s.bodies[0].edges[0].points[1].x = 4.;
    s.bodies[0].edges[2].points[0].x = 4.;
    for a in &mut p.anchors {
        if (a.edge_id.0 == 1 && a.endpoint == limo_cad_occt::DrawingProjectionAnchorEndpoint::End)
            || (a.edge_id.0 == 3
                && a.endpoint == limo_cad_occt::DrawingProjectionAnchorEndpoint::Start)
        {
            a.point[0] = 4.;
            a.model_point[0] = 4.;
        }
    }
    p.visible[0].points[1][0] = 4.;
    p.visible[2].points[0][0] = 4.;
    let t = targets(&s, &v, &p, [0., 0., 1.]).unwrap().remove(0);
    assert!((t.length - 4.).abs() < 1e-12);
    assert!((t.angle - 0.5_f64.atan().to_degrees()).abs() < 1e-12);
    p.visible[2].points[1] = [6.6, 0.];
    let t = targets(&s, &v, &p, [0., 0., 1.]).unwrap().remove(0);
    assert!((t.length - 4.).abs() < 1e-12);
    assert!((t.angle - 0.5_f64.atan().to_degrees()).abs() < 1e-12);
    p.visible.pop();
    let t = targets(&s, &v, &p, [0., 0., 1.]).unwrap().remove(0);
    assert!((t.length - 2.).abs() < 1e-12);
    assert!((t.angle - 2_f64.atan().to_degrees()).abs() < 1e-12);
}
#[test]
fn chamfer_requires_both_same_occurrence_carriers_and_rejects_curve_and_empty_hlr() {
    let (s, mut p, v) = geometry();
    p.anchors.retain(|a| a.edge_id.0 != 3);
    assert!(targets(&s, &v, &p, [0., 0., 1.]).unwrap().is_empty());
    let (s, mut p, v) = geometry();
    for a in p.anchors.iter_mut().filter(|a| a.edge_id.0 == 3) {
        a.occurrence_id = Some(serde_json::from_value(json!(32)).unwrap());
    }
    assert!(targets(&s, &v, &p, [0., 0., 1.]).unwrap().is_empty());
    let (mut s, p, v) = geometry();
    s.bodies[0].edges[0].points.insert(
        1,
        limo_cad_solid::Point3Dto {
            x: 1.,
            y: 3.,
            z: 0.,
        },
    );
    assert!(targets(&s, &v, &p, [0., 0., 1.]).unwrap().is_empty());
    let (s, mut p, v) = geometry();
    p.visible.clear();
    assert!(targets(&s, &v, &p, [0., 0., 1.]).unwrap().is_empty());
}
#[test]
fn hidden_carriers_remain_available_but_targets_follow_visible_spans_and_hidden_setting() {
    let (s, mut p, mut v) = geometry();
    p.visible.truncate(1);
    let t = targets(&s, &v, &p, [0., 0., 1.]).unwrap();
    assert_eq!(t.len(), 1);
    assert!((t[0].length - 2.).abs() < 1e-12);
    p.hidden = p.visible.clone();
    p.visible.clear();
    v.show_hidden_lines = false;
    assert!(targets(&s, &v, &p, [0., 0., 1.]).unwrap().is_empty());
    v.show_hidden_lines = true;
    assert_eq!(targets(&s, &v, &p, [0., 0., 1.]).unwrap().len(), 1);
    let (s, mut p, v) = geometry();
    p.visible[0].points = vec![[0., 2.], [0.5, 1.5]];
    let t = targets(&s, &v, &p, [0., 0., 1.]).unwrap();
    assert_eq!(t[0].line.pick_segments, vec![[[85., 108.], [85.5, 108.5]]]);
    assert_eq!(hit(&t, [85.25, 108.25], 0.1), Some(0));
    assert_eq!(hit(&t, [86.5, 109.5], 0.1), None);
}
#[test]
fn short_visible_chamfer_fragment_remains_pickable_with_exact_full_edge_references() {
    let (s, mut p, mut v) = geometry();
    v.show_hidden_lines = false;
    let full = targets(&s, &v, &p, [0., 0., 1.]).unwrap().remove(0);
    p.visible[0].points = vec![[0., 2.], [0.2, 1.8]];
    let found = targets(&s, &v, &p, [0., 0., 1.]).unwrap();
    assert_eq!(found.len(), 1);
    let fragment = &found[0];
    assert_eq!(fragment.first, full.first);
    assert_eq!(fragment.second, full.second);
    assert_eq!(fragment.line.reference, full.line.reference);
    assert_eq!(fragment.line.paper, full.line.paper);
    assert_eq!((fragment.length, fragment.angle), (full.length, full.angle));
    assert_eq!(fragment.line.pick_segments.len(), 1);
    let expected = [[85., 108.], [85.2, 108.2]];
    for (actual, expected) in fragment.line.pick_segments[0]
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
    {
        assert!((actual - expected).abs() < 1e-12);
    }
    assert_eq!(hit(&found, [85.1, 108.1], 0.01), Some(0));
    assert_eq!(hit(&found, [86.5, 109.5], 0.01), None);
}
#[test]
fn chamfer_frontmost_occurrence_ties_are_deterministic_and_work_is_bounded() {
    let (s, mut p, v) = geometry();
    let mut rear = p.anchors.clone();
    for a in &mut rear {
        a.occurrence_id = Some(serde_json::from_value(json!(32)).unwrap());
        a.model_point[2] = 6.;
    }
    p.anchors.extend(rear);
    let found = targets(&s, &v, &p, [0., 0., 1.]).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].first.occurrence_id.unwrap().0, 32);
    assert_eq!(
        targets(&s, &v, &p, [0., 0., -1.]).unwrap()[0]
            .first
            .occurrence_id
            .unwrap()
            .0,
        31
    );
    p.anchors[0].model_point[0] = f64::NAN;
    assert!(targets(&s, &v, &p, [0., 0., 1.]).is_err());
    let (s, mut p, v) = geometry();
    p.anchors = vec![p.anchors[0].clone(); 200_001];
    assert!(targets(&s, &v, &p, [0., 0., 1.])
        .unwrap_err()
        .contains("anchors"));
}
#[test]
fn chamfer_caption_reuses_document_units_and_sheet_standard() {
    let mut a = created().sheets[0].annotations[2].clone();
    if let DrawingAnnotationDto::ChamferNote {
        length,
        angle_deg,
        prefix,
        ..
    } = &mut a
    {
        *length = 2.54;
        *angle_deg = 45.;
        *prefix = "2X Ω ".into();
    }
    let caption = super::super::super::drawing_paper::chamfer_caption;
    assert_eq!(
        caption(&a, limo_cad_core::UnitSystem::Mm, DrawingStandard::Iso).as_deref(),
        Some("2X Ω 2.54 × 45°")
    );
    assert_eq!(
        caption(&a, limo_cad_core::UnitSystem::In, DrawingStandard::Ansi).as_deref(),
        Some("2X Ω .1 X 45°")
    );
}
#[test]
fn chamfer_placement_is_disposable_and_stale_or_exhausted_creation_changes_nothing() {
    let before = fixture::document();
    let original = before.clone();
    let mut p = Placement::default();
    p.pick(&stamp(), target());
    assert!(p.active());
    let mut changed = stamp();
    changed.owner.epoch += 1;
    assert!(p.create(&before, &changed).is_err());
    changed = stamp();
    changed.revision += 1;
    assert!(p.create(&before, &changed).is_err());
    let mut exhausted = before.clone();
    exhausted.next_annotation_id = u64::MAX;
    assert!(p.create(&exhausted, &stamp()).is_err());
    assert!(p.move_to([f64::NAN, 0.], [297., 210.]).is_err());
    p.move_to([999., -1.], [297., 210.]).unwrap();
    let next = p.create(&before, &stamp()).unwrap();
    assert_eq!(next.next_annotation_id, 5);
    assert_eq!(next.sheets[1], before.sheets[1]);
    assert_eq!(
        &next.sheets[0].annotations[..2],
        &before.sheets[0].annotations
    );
    assert_eq!(next.sheets[0].release.status, DrawingReleaseStatus::Draft);
    assert_eq!(
        next.sheets[0].release.released_revision,
        before.sheets[0].release.released_revision
    );
    assert!(
        matches!(&next.sheets[0].annotations[2],DrawingAnnotationDto::ChamferNote{position,..} if *position==[292.,5.])
    );
    p.cancel();
    assert!(p.create(&before, &stamp()).is_err());
    assert_eq!(before, original);
}
#[test]
fn chamfer_edit_and_cumulative_drag_preserve_reference_floats_and_other_document_intent() {
    let mut before = created();
    let id = 4;
    let selection = Selection {
        sheet_id: 1,
        annotation_id: id,
    };
    if let DrawingAnnotationDto::ChamferNote {
        length,
        angle_deg,
        position,
        ..
    } = &mut before.sheets[0].annotations[2]
    {
        *length = f64::from_bits(0x4000000000000001);
        *angle_deg = 37.125;
        *position = [-20., 250.];
    }
    let mut d = Draft::new(&before, selection).unwrap();
    let mut form = fields::from_annotation(d.annotation());
    fields::apply(&mut d, &form).unwrap();
    assert!(!d.dirty());
    assert_eq!(d.apply(&before).unwrap(), before);
    fields::edit(
        &mut form,
        Id::Prefix,
        &ControlInput::SetValue("2X 零件 ".into()),
    )
    .unwrap();
    fields::apply(&mut d, &form).unwrap();
    let edited = d.apply(&before).unwrap();
    let mut expected = serde_json::to_value(&before).unwrap();
    expected["sheets"][0]["annotations"][2]["prefix"] = json!("2X 零件 ");
    assert_eq!(serde_json::to_value(&edited).unwrap(), expected);
    let mut d = Draft::new(&edited, selection).unwrap();
    d.move_chamfer([30., -100.], [297., 210.]).unwrap();
    let once = d.apply(&edited).unwrap();
    d.move_chamfer([30., -100.], [297., 210.]).unwrap();
    assert_eq!(d.apply(&edited).unwrap(), once);
    let mut expected = serde_json::to_value(&edited).unwrap();
    expected["sheets"][0]["annotations"][2]["position"] = json!([10., 150.]);
    assert_eq!(serde_json::to_value(&once).unwrap(), expected);
    let mut stale = before.clone();
    if let DrawingAnnotationDto::ChamferNote { prefix, .. } = &mut stale.sheets[0].annotations[2] {
        *prefix = "Other editor".into();
    }
    assert!(d.apply(&stale).is_err());
    let deleted = d.delete(&edited).unwrap();
    assert_eq!(deleted.sheets[0].annotations.len(), 2);
    assert_eq!(deleted.next_annotation_id, edited.next_annotation_id);
    for (id, value) in [
        (Id::ChamferSetback, "0"),
        (Id::ChamferAngle, "180"),
        (Id::ChamferAngle, "0"),
        (Id::X, "NaN"),
    ] {
        let mut d = Draft::new(&before, selection).unwrap();
        let mut form = fields::from_annotation(d.annotation());
        fields::edit(&mut form, id, &ControlInput::SetValue(value.into())).unwrap();
        assert!(fields::apply(&mut d, &form).is_err());
        assert!(!d.dirty());
    }
    assert!(Draft::new(&before, selection)
        .unwrap()
        .chamfer([0., 0.], 2., 45., "x".repeat(257))
        .is_err());
}
