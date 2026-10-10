use super::*;
use limo_cad_occt::*;
use limo_cad_sketch::*;
use serde_json::json;

pub(super) fn projection() -> DrawingProjectionDto {
    serde_json::from_value(json!({"visible":[],"hidden":[],"bounds":[0.,0.,40.,30.],"topology_signatures":{"1":"exact-body-topology"},
        "anchors":[
            {"body_id":1,"edge_id":9,"edge_key":"rear","endpoint":"start","model_point":[0.,0.,0.],"point":[0.,0.],"hidden":false},
            {"body_id":1,"edge_id":8,"edge_key":"front","endpoint":"start","model_point":[0.,0.,6.],"point":[0.,0.],"hidden":false},
            {"body_id":1,"edge_id":1,"edge_key":"hidden","endpoint":"end","model_point":[0.,0.,10.],"point":[0.,0.],"hidden":true},
            {"body_id":1,"edge_id":3,"edge_key":"stable","endpoint":"end","model_point":[0.,0.,6.],"point":[0.,0.],"hidden":false},
            {"body_id":1,"edge_id":3,"edge_key":"stable","endpoint":"start","model_point":[0.,0.,6.],"point":[0.,0.],"hidden":false},
            {"body_id":1,"edge_id":4,"edge_key":"right","endpoint":"start","model_point":[40.,0.,6.],"point":[40.,0.],"hidden":false}],
        "circles":[
            {"body_id":1,"edge_id":5,"edge_key":"inner","center_model":[20.,15.,6.],"normal_model":[0.,0.,1.],"center":[20.,15.],"radius":3.,"closed":true,"hidden":false},
            {"body_id":1,"edge_id":6,"edge_key":"outer","center_model":[20.,15.,6.],"normal_model":[0.,0.,1.],"center":[20.,15.],"radius":5.,"closed":true,"hidden":false}]
    })).unwrap()
}
pub(super) fn document() -> DrawingDocumentDto {
    serde_json::from_value(json!({"active_sheet_id":1,"next_sheet_id":3,"next_view_id":2,"next_annotation_id":4,"sheets":[
        {"id":1,"name":"Released","format":"a4","orientation":"landscape","release":{"status":"released","released_revision":"A","released_at":"2026-09-26"},
        "views":[{"id":1,"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,100.],"scale":1.}],
        "annotations":[{"kind":"note","id":1,"text":"Saved\nUnicode","position":[20.,30.]},
            {"kind":"revision_cloud","id":2,"revision":"B","points":[[10.,10.],[20.,10.],[20.,20.],[10.,20.]]}]},
        {"id":2,"name":"Untouched","format":"a3","orientation":"portrait","annotations":[{"kind":"note","id":3,"text":"Other sheet","position":[10.,10.]}]}
    ]})).unwrap()
}
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

#[test]
fn coincident_pick_targets_prefer_visible_front_geometry_and_keep_exact_references() {
    let projection = projection();
    let document = document();
    let view = &document.sheets[0].views[0];
    let endpoints = anchors::endpoints(view, &projection, view.direction).unwrap();
    assert_eq!(endpoints.len(), 2);
    assert_eq!(endpoints[0].edge_key, "stable");
    assert_eq!(
        endpoints[0].endpoint,
        DrawingProjectionAnchorEndpoint::Start
    );
    let exact = anchors::endpoint_ref(endpoints[0], &projection);
    assert_eq!(
        exact.topology_signature.as_deref(),
        Some("exact-body-topology")
    );
    assert_eq!(exact.fallback_point, [0., 0., 6.]);
    assert_eq!(exact.edge_id.0, 3);
    let circles = anchors::circles(view, &projection, view.direction, false).unwrap();
    assert_eq!(circles.len(), 1);
    assert_eq!(circles[0].edge_key, "outer");
    let center = anchors::circle_ref(circles[0], &projection);
    assert!(center.circle_center);
    assert_eq!(center.fallback_point, [20., 15., 6.]);
}

#[test]
fn two_anchor_creation_rejects_duplicate_and_retired_pairs_without_new_model_state() {
    let projection = projection();
    let a = anchors::endpoint_ref(&projection.anchors[4], &projection);
    let b = anchors::endpoint_ref(&projection.anchors[5], &projection);
    let mut tool = LinearPlacement::default();
    let initial = stamp();
    assert!(tool.click(&initial, 1, a.clone()).is_none());
    assert!(tool.click(&initial, 1, a.clone()).is_none());
    let mut later = initial.clone();
    later.revision += 1;
    assert!(
        tool.click(&later, 1, b.clone()).is_none(),
        "A changed document must start a fresh pair"
    );
    let request = tool.click(&later, 1, a.clone()).unwrap();
    assert_eq!(request.first, b);
    assert_eq!(request.second, a);
    assert_eq!(request.offset, 12.);
    assert_eq!(request.precision, 2);
    assert!(tool.first.is_none());
    assert!(tool.click(&later, 1, a.clone()).is_none());
    assert!(
        tool.click(&later, 2, b.clone()).is_none(),
        "A different view starts a fresh pair"
    );
    later.owner.epoch += 1;
    tool.observe(&later);
    assert!(tool.first.is_none());
    let note = place_note(1, [18., 22.]);
    assert_eq!(note.text, "NOTE");
    assert_eq!(note.position, [18., 22.]);
}

#[test]
fn note_preview_apply_delete_preserve_shared_content_and_release_metadata() {
    let before = document();
    before.validate().unwrap();
    let selection = draft::Selection {
        sheet_id: 1,
        annotation_id: 1,
    };
    let mut edit = draft::Draft::new(&before, selection).unwrap();
    assert_eq!(edit.apply(&before).unwrap(), before);
    edit.note("Caf\u{e9} \u{96f6}\u{4ef6}\nSecond line".into())
        .unwrap();
    edit.move_note([-10., 500.], [297., 210.]).unwrap();
    assert_eq!(
        before.sheets[0].release.status,
        DrawingReleaseStatus::Released
    );
    let actual = edit.apply(&before).unwrap();
    let mut expected = before.clone();
    expected.sheets[0].annotations[0] = DrawingAnnotationDto::Note {
        id: 1,
        text: "Caf\u{e9} \u{96f6}\u{4ef6}\nSecond line".into(),
        position: [5., 205.],
    };
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(actual, expected);
    let deleted = edit.delete(&before).unwrap();
    expected = before.clone();
    expected.sheets[0].annotations.remove(0);
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(deleted, expected);
    assert!(
        edit.apply(&actual).is_err(),
        "An old draft cannot overwrite changed annotation intent"
    );
    edit.note(" ".into()).unwrap();
    assert!(edit.apply(&before).is_err());
    edit.note("\u{96f6}".repeat(4096)).unwrap();
    assert!(edit.apply(&before).is_ok());
    edit.note("\u{96f6}".repeat(4097)).unwrap();
    assert!(edit.apply(&before).is_err());
}

#[test]
fn dimension_drag_is_cumulative_paper_offset_and_preserves_topology_presentation() {
    let mut before = document();
    let p = projection();
    before.sheets[0]
        .annotations
        .push(DrawingAnnotationDto::LinearDimension {
            id: 4,
            view_id: 1,
            first: anchors::endpoint_ref(&p.anchors[4], &p),
            second: anchors::endpoint_ref(&p.anchors[5], &p),
            mode: DrawingLinearDimensionMode::Horizontal,
            offset: 12.,
            prefix: "Saved prefix".into(),
            suffix: "Saved suffix".into(),
            precision: 3,
            presentation: Default::default(),
        });
    before.next_annotation_id = 5;
    before.validate().unwrap();
    let mut edit = draft::Draft::new(
        &before,
        draft::Selection {
            sheet_id: 1,
            annotation_id: 4,
        },
    )
    .unwrap();
    edit.move_linear([10., 10.], [50., 10.], [3., 4.]).unwrap();
    edit.move_linear([10., 10.], [50., 10.], [6., 8.]).unwrap();
    let mut expected = before.clone();
    if let DrawingAnnotationDto::LinearDimension { offset, .. } =
        &mut expected.sheets[0].annotations[2]
    {
        *offset = 20.;
    }
    expected.sheets[0].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(edit.apply(&before).unwrap(), expected);
}
