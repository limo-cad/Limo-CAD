use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use limo_cad_interface::{ControlInput, DocumentContext};
pub(super) fn stamp() -> Stamp {
    Stamp {
        owner: DocumentContext {
            window_id: "main".into(),
            document_id: "centers".into(),
            epoch: 7,
        },
        revision: 12,
        sheet_id: 1,
    }
}
pub(super) fn fixture() -> (DrawingDocumentDto, DrawingProjectionDto) {
    let d = fixture::document();
    let mut p = fixture::projection();
    p.circles[0].center = [10., 15.];
    p.circles[0].center_model = [110., 15., 6.];
    p.circles[0].occurrence_id = Some(serde_json::from_value(serde_json::json!(31)).unwrap());
    p.circles[1] = p.circles[0].clone();
    p.circles[1].center = [30., 15.];
    p.circles[1].center_model = [130., 15., 6.];
    p.circles[1].occurrence_id = Some(serde_json::from_value(serde_json::json!(32)).unwrap());
    (d, p)
}
pub(super) fn created(line: bool) -> DrawingDocumentDto {
    let (d, p) = fixture();
    let view = &d.sheets[0].views[0];
    let targets = targets(view, &p, view.direction).unwrap();
    let mut placement = Placement::default();
    if line {
        assert!(placement
            .click(&stamp(), &targets[0], true, &d)
            .unwrap()
            .is_none());
    }
    placement
        .click(&stamp(), &targets[if line { 1 } else { 0 }], line, &d)
        .unwrap()
        .unwrap()
}
#[test]
fn filters_before_center_dedup_and_retains_exact_occurrence_radius_and_signature() {
    let d = fixture::document();
    let mut p = fixture::projection();
    let mut view = d.sheets[0].views[0].clone();
    view.show_hidden_lines = false;
    p.circles[1].closed = false;
    assert_eq!(
        targets(&view, &p, view.direction).unwrap()[0]
            .reference
            .edge_key,
        "inner"
    );
    p.circles[1].closed = true;
    p.circles[1].hidden = true;
    assert_eq!(
        targets(&view, &p, view.direction).unwrap()[0]
            .reference
            .edge_key,
        "inner"
    );
    view.show_hidden_lines = true;
    assert_eq!(
        targets(&view, &p, view.direction).unwrap()[0]
            .reference
            .edge_key,
        "inner"
    );
    p.circles[1].hidden = false;
    assert_eq!(
        targets(&view, &p, view.direction).unwrap()[0]
            .reference
            .edge_key,
        "outer"
    );
    let (d, p) = fixture();
    let found = targets(&d.sheets[0].views[0], &p, [0., 0., 1.]).unwrap();
    assert_eq!(found.len(), 2);
    for (i, t) in found.iter().enumerate() {
        assert_eq!(t.reference.occurrence_id, p.circles[i].occurrence_id);
        assert_eq!(t.reference.fallback_center, p.circles[i].center_model);
        assert_eq!(t.reference.fallback_normal, p.circles[i].normal_model);
        assert_eq!(t.reference.fallback_radius, p.circles[i].radius);
        assert_eq!(
            t.reference.topology_signature.as_deref(),
            Some("exact-body-topology")
        );
    }
    p.circles.iter().for_each(|c| assert!(c.closed));
}
#[test]
fn two_orders_duplicate_cancel_view_owner_revision_and_overflow_are_disposable() {
    let (d, p) = fixture();
    let v = &d.sheets[0].views[0];
    let targets = targets(v, &p, v.direction).unwrap();
    for [a, b] in [[0, 1], [1, 0]] {
        let mut placement = Placement::default();
        assert!(placement
            .click(&stamp(), &targets[a], true, &d)
            .unwrap()
            .is_none());
        assert!(placement
            .click(&stamp(), &targets[a], true, &d)
            .unwrap()
            .is_none());
        let next = placement
            .click(&stamp(), &targets[b], true, &d)
            .unwrap()
            .unwrap();
        let DrawingAnnotationDto::CenterLine {
            first,
            second,
            extension,
            ..
        } = next.sheets[0].annotations.last().unwrap()
        else {
            panic!()
        };
        assert_eq!(first, &targets[a].reference);
        assert_eq!(second, &targets[b].reference);
        assert_eq!(*extension, 2.5);
        assert_eq!(next.sheets[1], d.sheets[1]);
        assert_eq!(&next.sheets[0].annotations[..2], &d.sheets[0].annotations);
        assert_eq!(
            next.sheets[0].release.released_revision,
            d.sheets[0].release.released_revision
        );
        assert_eq!(next.sheets[0].release.status, DrawingReleaseStatus::Draft);
        assert!(!placement.active());
    }
    for cause in 0..4 {
        let mut placement = Placement::default();
        placement.click(&stamp(), &targets[0], true, &d).unwrap();
        let mut changed = stamp();
        let mut next = targets[1].clone();
        match cause {
            0 => changed.revision += 1,
            1 => changed.owner.epoch += 1,
            2 => next.view_id += 1,
            _ => placement.cancel(),
        }
        assert!(placement
            .click(&changed, &next, true, &d)
            .unwrap()
            .is_none());
        assert!(placement.selected(&next));
    }
    let mut exhausted = d.clone();
    exhausted.next_annotation_id = u64::MAX;
    assert!(Placement::default()
        .click(&stamp(), &targets[0], false, &exhausted)
        .is_err());
    assert_eq!(d.next_annotation_id, 4);
    let mut same_center = targets[1].clone();
    same_center.center = targets[0].center;
    let mut placement = Placement::default();
    placement.click(&stamp(), &targets[0], true, &d).unwrap();
    assert!(placement.click(&stamp(), &same_center, true, &d).is_err());
}
#[test]
fn geometry_uses_current_projection_and_extension_is_paper_mm_in_both_orders() {
    for line in [false, true] {
        let mut d = created(line);
        let (_, mut p) = fixture();
        let v = &mut d.sheets[0].views[0];
        v.scale = 2.;
        v.position = [120., 90.];
        let annotation = d.sheets[0].annotations.last().unwrap().clone();
        let v = &d.sheets[0].views[0];
        let g = geometry(&annotation, v, &p).unwrap();
        assert_eq!(g.grips.len(), if line { 2 } else { 4 });
        for grip in &g.grips {
            assert!((length(sub(grip.point, grip.origin)) - 2.5).abs() < 1e-12);
            let moved = add(
                grip.point,
                add(
                    scale(grip.direction, 7.),
                    [-grip.direction[1] * 19., grip.direction[0] * 19.],
                ),
            );
            assert!((extension_at(*grip, moved).unwrap() - 9.5).abs() < 1e-12);
            assert_eq!(
                extension_at(*grip, add(grip.origin, scale(grip.direction, -2.))).unwrap(),
                0.
            );
        }
        p.circles[0].center[0] += 3.;
        assert_ne!(
            geometry(&annotation, v, &p).unwrap().grips[0].point,
            g.grips[0].point
        );
        p.topology_signatures.insert("1".into(), "changed".into());
        assert!(geometry(&annotation, v, &p).is_none());
        p.topology_signatures
            .insert("1".into(), "exact-body-topology".into());
        p.circles[0].occurrence_id = None;
        assert!(
            geometry(&annotation, v, &p).is_none(),
            "Another occurrence cannot substitute for this association"
        );
    }
}
#[test]
fn extension_fields_preserve_arbitrary_loaded_intent_and_stale_drafts_reject() {
    for line in [false, true] {
        let mut before = created(line);
        before.sheets[0].release.status = DrawingReleaseStatus::Released;
        let selected = Selection {
            sheet_id: 1,
            annotation_id: 4,
        };
        let mut edit = Draft::new(&before, selected).unwrap();
        let mut fields = fields::from_annotation(edit.annotation());
        fields::edit(
            &mut fields,
            Id::Extension,
            &ControlInput::SetValue("13.125".into()),
        )
        .unwrap();
        fields::apply(&mut edit, &fields).unwrap();
        let actual = edit.apply(&before).unwrap();
        let mut expected = before.clone();
        match expected.sheets[0].annotations.last_mut().unwrap() {
            DrawingAnnotationDto::CenterMark { extension, .. }
            | DrawingAnnotationDto::CenterLine { extension, .. } => *extension = 13.125,
            _ => panic!(),
        }
        expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
        assert_eq!(actual, expected);
        assert!(edit.apply(&actual).is_err());
        for bad in [-1., f64::NAN, f64::INFINITY, 1e6 + 1.] {
            assert!(edit.center_extension(bad).is_err());
        }
        assert_eq!(edit.apply(&before).unwrap(), actual);
        let deleted = edit.delete(&before).unwrap();
        assert_eq!(
            deleted.sheets[0].annotations,
            fixture::document().sheets[0].annotations
        );
        assert_eq!(
            deleted.sheets[0].release.released_at,
            before.sheets[0].release.released_at
        );
    }
}
