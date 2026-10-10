use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use limo_cad_interface::{ControlInput, DocumentContext};

fn stamp() -> Stamp {
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
fn corners() -> [[f64; 2]; 3] {
    [[25.25, 30.125], [65.5, 30.125], [65.5, 60.75]]
}
fn staged(document: &DrawingDocumentDto) -> Placement {
    let mut placement = Placement::default();
    for point in corners() {
        assert!(placement
            .click(&stamp(), point, document)
            .unwrap()
            .is_none());
    }
    placement
}
pub(super) fn created() -> DrawingDocumentDto {
    let document = fixture::document();
    staged(&document)
        .click(&stamp(), [25.25, 60.75], &document)
        .unwrap()
        .unwrap()
}

#[test]
fn revision_cloud_closes_at_four_millimetres_or_fourth_corner_without_staging_mutation() {
    let mut before = fixture::document();
    before.sheets[0].views.clear();
    before.sheets[0].title_block.revision = "b\u{03c9}".into();
    let mut placement = staged(&before);
    assert_eq!(placement.points, corners());
    assert_eq!(before.next_annotation_id, 4);
    let first = corners()[0];
    let triangle = placement
        .click(&stamp(), [first[0] + 4., first[1]], &before)
        .unwrap()
        .unwrap();
    let mut expected = before.clone();
    expected.next_annotation_id = 5;
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    expected.sheets[0]
        .annotations
        .push(DrawingAnnotationDto::RevisionCloud {
            id: 4,
            revision: "b\u{03c9}".into(),
            points: corners().into(),
        });
    assert_eq!(triangle, expected);
    let fourth = [first[0] + 4.000001, first[1]];
    let quad = placement.click(&stamp(), fourth, &before).unwrap().unwrap();
    if let DrawingAnnotationDto::RevisionCloud { points, .. } =
        expected.sheets[0].annotations.last_mut().unwrap()
    {
        points.push(fourth);
    }
    assert_eq!(quad, expected);
    assert_eq!(
        placement.points,
        corners(),
        "Submission failure can retain the exact staged three vertices"
    );
    placement.cancel();
    assert!(placement.points.is_empty());
    let fallback = created();
    assert!(
        matches!(fallback.sheets[0].annotations.last(),Some(DrawingAnnotationDto::RevisionCloud {revision,..}) if revision=="A")
    );
}

#[test]
fn revision_cloud_repeated_early_point_and_invalid_geometry_follow_shared_validation() {
    let document = fixture::document();
    let mut placement = Placement::default();
    for _ in 0..3 {
        assert!(placement
            .click(&stamp(), [20., 20.], &document)
            .unwrap()
            .is_none());
    }
    assert!(
        placement
            .click(&stamp(), [20., 20.], &document)
            .unwrap()
            .is_some(),
        "Cloud polygons must follow the shared DTO's validity rules"
    );
    let saved = placement.points.clone();
    assert!(placement
        .click(&stamp(), [f64::NAN, 20.], &document)
        .is_err());
    assert_eq!(placement.points, saved);
    for counter in [u64::MAX, 2] {
        let mut bad = document.clone();
        bad.next_annotation_id = counter;
        assert!(staged(&bad).click(&stamp(), [25., 60.], &bad).is_err());
    }
    let mut bad = document.clone();
    bad.sheets[0].title_block.revision = " ".into();
    assert!(staged(&bad).click(&stamp(), [25., 60.], &bad).is_err());
}

#[test]
fn revision_cloud_staging_is_owned_by_document_epoch_revision_and_sheet() {
    let document = fixture::document();
    let initial = stamp();
    let mut revisions = vec![initial.clone(); 4];
    revisions[0].revision += 1;
    revisions[1].owner.epoch += 1;
    revisions[2].owner.document_id = "other".into();
    revisions[3].sheet_id = 2;
    for changed in revisions {
        let mut placement = staged(&document);
        assert!(placement
            .click(&changed, [80., 70.], &document)
            .unwrap()
            .is_none());
        assert_eq!(placement.points, vec![[80., 70.]]);
    }
}

#[test]
fn revision_cloud_loaded_vertices_and_release_receipt_survive_edit_drag_delete() {
    let mut before = fixture::document();
    let points: Vec<_> = (0..4096)
        .map(|i| [5.125 + i as f64 / 32., 6.75 + (i % 31) as f64 / 8.])
        .collect();
    before.sheets[0].annotations[1] = DrawingAnnotationDto::RevisionCloud {
        id: 2,
        revision: "a\u{00df}".into(),
        points: points.clone(),
    };
    before.validate().unwrap();
    let selection = Selection {
        sheet_id: 1,
        annotation_id: 2,
    };
    let mut draft = Draft::new(&before, selection).unwrap();
    let mut fields = fields::from_annotation(draft.annotation());
    assert_eq!(fields.len(), 1);
    fields::apply(&mut draft, &fields).unwrap();
    assert_eq!(
        draft.apply(&before).unwrap(),
        before,
        "No-op must retain lowercase saved revision and released status"
    );
    fields::edit(
        &mut fields,
        Id::Revision,
        &ControlInput::SetValue("b\u{00df}\u{03c9}".into()),
    )
    .unwrap();
    fields::apply(&mut draft, &fields).unwrap();
    let mut expected = before.clone();
    if let DrawingAnnotationDto::RevisionCloud { revision, .. } =
        &mut expected.sheets[0].annotations[1]
    {
        *revision = "BSS\u{03a9}".into();
    }
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(draft.apply(&before).unwrap(), expected);
    draft
        .move_revision_cloud([-30., 300.], [297., 210.])
        .unwrap();
    draft
        .move_revision_cloud([-30., 300.], [297., 210.])
        .unwrap();
    if let DrawingAnnotationDto::RevisionCloud { points, .. } =
        &mut expected.sheets[0].annotations[1]
    {
        for p in points {
            *p = [(p[0] - 30.).clamp(5., 292.), 205.];
        }
    }
    assert_eq!(draft.apply(&before).unwrap(), expected);
    assert!(
        draft.apply(&expected).is_err(),
        "An old draft cannot overwrite a moved cloud"
    );
    let saved = draft.annotation().clone();
    assert!(draft
        .move_revision_cloud([f64::NAN, 0.], [297., 210.])
        .is_err());
    assert!(draft.move_revision_cloud([0., 0.], [9., 210.]).is_err());
    assert_eq!(draft.annotation(), &saved);
    let mut deleted = before.clone();
    deleted.sheets[0].annotations.remove(1);
    deleted.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(draft.delete(&before).unwrap(), deleted);
    fields::edit(
        &mut fields,
        Id::Revision,
        &ControlInput::SetValue(" \t".into()),
    )
    .unwrap();
    fields::apply(&mut draft, &fields).unwrap();
    assert!(draft.apply(&before).is_err());
}

#[test]
fn revision_cloud_scallop_hit_uses_curved_stroke_and_rejects_empty_polygon_interior() {
    let edge = [[0., 0.], [20., 0.]];
    let sagitta = 2.9 - (2.9_f64.powi(2) - 2.5_f64.powi(2)).sqrt();
    assert!(edge_distance([2.5, -sagitta], edge).unwrap() < 1e-10);
    assert!((edge_distance([2.5, -sagitta - 3.01], edge).unwrap() - 3.01).abs() < 1e-10);
    assert!(edge_distance([2.5, 4.], edge).unwrap() > 3.);
    assert!((edge_distance([-2., 0.], edge).unwrap() - 2.).abs() < 1e-10);
    assert!(edge_distance([20. + sagitta, 2.5], [[20., 0.], [20., 20.]]).unwrap() < 1e-10);
    assert!(edge_distance([17.5, sagitta], [[20., 0.], [0., 0.]]).unwrap() < 1e-10);
    for edge in [
        [[10., 10.], [80., 10.]],
        [[80., 10.], [80., 70.]],
        [[80., 70.], [10., 70.]],
        [[10., 70.], [10., 10.]],
    ] {
        assert!(edge_distance([45., 40.], edge).unwrap() > 3.);
    }
    assert!(edge_distance([1e11, 20.], [[0., 0.], [1e12, 0.]]).unwrap() > 3.);
    assert!(edge_distance([0., 0.], [[0., 0.], [0., 0.]]).is_none());
    assert!(edge_distance([f64::INFINITY, 0.], edge).is_none());
}

#[test]
fn revision_cloud_hit_resolves_overlapping_coarse_edges_and_bounds_loaded_work() {
    let mut sheet = fixture::document().sheets.remove(0);
    sheet.annotations = vec![
        DrawingAnnotationDto::RevisionCloud {
            id: 5,
            revision: "A".into(),
            points: vec![[0., 0.], [20., 0.], [20., 20.]],
        },
        DrawingAnnotationDto::RevisionCloud {
            id: 6,
            revision: "B".into(),
            points: vec![[0., 5.], [20., 5.], [20., 20.]],
        },
    ];
    assert_eq!(
        hit(&sheet, [2.5, 0.]),
        Some(5),
        "The later coarse rectangle must not hide a real underlying scallop"
    );
    let sagitta = 2.9 - (2.9_f64.powi(2) - 2.5_f64.powi(2)).sqrt();
    assert_eq!(hit(&sheet, [2.5, 5. - sagitta]), Some(6));
    assert_eq!(hit(&sheet, [10., -20.]), None);
    sheet.annotations[1] = DrawingAnnotationDto::RevisionCloud {
        id: 6,
        revision: "B".into(),
        points: vec![[0., 0.], [20., 0.], [20., 20.]],
    };
    assert_eq!(
        hit(&sheet, [2.5, 0.]),
        Some(6),
        "Exact ties use the last painted cloud"
    );
    sheet.annotations.push(DrawingAnnotationDto::RevisionCloud {
        id: 7,
        revision: "dense".into(),
        points: vec![[10., 10.]; 4096],
    });
    assert_eq!(
        hit(&sheet, [2.5, 0.]),
        None,
        "Dense sheets retain labels without unbounded path picking"
    );
}
