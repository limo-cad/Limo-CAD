use super::super::{
    draft::{Draft, Selection},
    fields::{self, Id},
    tests as fixture,
};
use super::*;
use limo_cad_interface::{ControlInput, DocumentContext};
use serde_json::json;

fn stamp() -> Stamp {
    Stamp {
        owner: DocumentContext {
            window_id: "main".into(),
            document_id: "straight".into(),
            epoch: 2,
        },
        revision: 7,
        sheet_id: 1,
    }
}
fn line(id: u64, paper: [[f64; 2]; 2]) -> LineTarget {
    LineTarget{view_id:1,paper,pick_segments:vec![paper],scale:1.,reference:serde_json::from_value(json!({
    "body_id":1,"edge_id":id,"edge_key":format!("edge-{id}"),"topology_signature":"exact-body-topology",
    "fallback_start":[paper[0][0],paper[0][1],6.],"fallback_end":[paper[1][0],paper[1][1],6.]
})).unwrap()}
}
fn point(paper: [f64; 2]) -> Target {
    Target{view_id:1,paper,reference:serde_json::from_value(json!({
    "body_id":1,"edge_id":7,"edge_key":"point","endpoint":"start","fallback_point":[paper[0],paper[1],6.],
    "topology_signature":"exact-body-topology","circle_center":true
})).unwrap()}
}
pub(in super::super) fn document(point_line: bool) -> DrawingDocumentDto {
    let mut p = Placement::default();
    p.edge(&stamp(), line(1, [[60., 70.], [100., 70.]]), None);
    if point_line {
        assert!(p.anchor(&stamp(), point([80., 95.])));
    }
    p.create(&fixture::document(), &stamp()).unwrap()
}
#[test]
fn smart_edges_stage_toggle_and_auto_classify_without_consuming_ids() {
    let before = fixture::document();
    let saved = before.clone();
    let s = stamp();
    let mut p = Placement::default();
    let a = line(1, [[60., 70.], [100., 70.]]);
    let b = line(2, [[60., 100.], [100., 100.]]);
    let c = line(3, [[100., 70.], [100., 100.]]);
    p.edge(&s, a.clone(), None);
    assert!(p.valid());
    let single = p.create(&before, &s).unwrap();
    assert!(matches!(
        &single.sheets[0].annotations[2],
        DrawingAnnotationDto::LineDimension {
            mode: DrawingLineDimensionMode::Length,
            second: None,
            precision: 2,
            ..
        }
    ));
    p.edge(&s, b.clone(), None);
    let distance = p.create(&before, &s).unwrap();
    assert!(matches!(
        &distance.sheets[0].annotations[2],
        DrawingAnnotationDto::LineDimension {
            mode: DrawingLineDimensionMode::Distance,
            second: Some(_),
            precision: 2,
            ..
        }
    ));
    p.edge(&s, b, None);
    assert!(matches!(
        p.annotation(4),
        Some(DrawingAnnotationDto::LineDimension {
            mode: DrawingLineDimensionMode::Length,
            ..
        })
    ));
    p.edge(&s, c, None);
    assert!(matches!(
        p.annotation(4),
        Some(DrawingAnnotationDto::LineDimension {
            mode: DrawingLineDimensionMode::Angle,
            precision: 1,
            ..
        })
    ));
    p.edge(&s, a, None);
    assert!(matches!(
        p.annotation(4),
        Some(DrawingAnnotationDto::LineDimension {
            mode: DrawingLineDimensionMode::Length,
            ..
        })
    ));
    assert_eq!(before, saved);
    assert_eq!(single.next_annotation_id, 5);
    assert_eq!(single.sheets[1], before.sheets[1]);
    assert_eq!(
        &single.sheets[0].annotations[..2],
        &before.sheets[0].annotations[..]
    );
    assert_eq!(
        single.sheets[0].release.released_revision,
        before.sheets[0].release.released_revision
    );
    assert_eq!(single.sheets[0].release.status, DrawingReleaseStatus::Draft);
    p.cancel();
    assert!(!p.active());
    assert!(p.create(&before, &s).is_err());
    assert_eq!(before, saved);
}
#[test]
fn point_line_accepts_both_pick_orders_and_circle_centers_but_rejects_zero_span() {
    let s = stamp();
    let edge = line(1, [[60., 70.], [100., 70.]]);
    let anchor = point([80., 95.]);
    let mut first = Placement::default();
    first.edge(&s, edge.clone(), Some(anchor.clone()));
    let mut second = Placement::default();
    second.edge(&s, edge.clone(), None);
    assert!(second.anchor(&s, anchor));
    assert_eq!(
        first.create(&fixture::document(), &s).unwrap(),
        second.create(&fixture::document(), &s).unwrap()
    );
    assert!(second.anchor(&s, point([80., 70.])));
    assert!(!second.valid());
    assert!(second.create(&fixture::document(), &s).is_err());
    first.edge(&s, line(2, [[60., 60.], [100., 60.]]), None);
    assert!(first.valid());
    assert!(
        matches!(first.annotation(4),Some(DrawingAnnotationDto::PointLineDimension{line,..}) if line.edge_id.0==2)
    );
}
#[test]
fn retired_or_cross_view_picks_cannot_complete_old_geometry_and_ids_do_not_wrap() {
    let s = stamp();
    let a = line(1, [[60., 70.], [100., 70.]]);
    let mut p = Placement::default();
    p.edge(&s, a.clone(), None);
    let mut changed = s.clone();
    changed.owner.epoch += 1;
    assert!(p.create(&fixture::document(), &changed).is_err());
    changed = s.clone();
    changed.revision += 1;
    assert!(p.create(&fixture::document(), &changed).is_err());
    let mut b = line(2, [[60., 100.], [100., 100.]]);
    b.view_id = 2;
    p.edge(&s, b, None);
    assert!(matches!(
        p.annotation(4),
        Some(DrawingAnnotationDto::LineDimension {
            view_id: 2,
            second: None,
            ..
        })
    ));
    p.edge(&s, a, None);
    let mut before = fixture::document();
    before.next_annotation_id = u64::MAX;
    assert!(p.create(&before, &s).is_err());
    assert_eq!(before.next_annotation_id, u64::MAX);
}
#[test]
fn relation_boundary_and_geometric_hit_test_are_independent_of_hit_rectangle() {
    let a = line(1, [[0., 0.], [40., 0.]]);
    for (degrees, expected) in [
        (0.99, DrawingLineDimensionMode::Distance),
        (1.01, DrawingLineDimensionMode::Angle),
    ] {
        let angle = degrees * std::f64::consts::PI / 180.;
        let b = line(2, [[0., 30.], [40. * angle.cos(), 30. + 40. * angle.sin()]]);
        assert_eq!(mode(&a, Some(&b)), expected);
    }
    let diagonal = line(3, [[0., 0.], [40., 40.]]);
    assert_eq!(hit(std::slice::from_ref(&diagonal), [2., 38.], 2.), None);
    assert_eq!(hit(&[diagonal], [20., 20.5], 2.), Some(0));
}
#[test]
fn straight_draft_preserves_full_references_presentation_and_cumulative_drag() {
    for is_point in [false, true] {
        let mut before = document(is_point);
        let a = &mut before.sheets[0].annotations[2];
        let (DrawingAnnotationDto::LineDimension {
            prefix,
            suffix,
            presentation,
            ..
        }
        | DrawingAnnotationDto::PointLineDimension {
            prefix,
            suffix,
            presentation,
            ..
        }) = a
        else {
            unreachable!()
        };
        *prefix = "Saved full prefix".into();
        *suffix = " suffix".into();
        *presentation=serde_json::from_value(json!({"tolerance":{"mode":"deviation","upper":0.2,"lower":-0.1},
            "basic":true,"reference":false,"fit_class":"H7","dual_units":{"unit":"inch","precision":3,"placement":"stacked"}})).unwrap();
        let mut d = Draft::new(
            &before,
            Selection {
                sheet_id: 1,
                annotation_id: 4,
            },
        )
        .unwrap();
        let mut inputs = fields::from_annotation(d.annotation());
        fields::edit(&mut inputs, Id::X, &ControlInput::SetValue("125".into())).unwrap();
        fields::apply(&mut d, &inputs).unwrap();
        let after = d.apply(&before).unwrap();
        let mut expected = before.clone();
        match &mut expected.sheets[0].annotations[2] {
            DrawingAnnotationDto::LineDimension { position, .. }
            | DrawingAnnotationDto::PointLineDimension { position, .. } => position[0] = 125.,
            _ => unreachable!(),
        };
        assert_eq!(after, expected);
        assert!(d.apply(&after).is_err());
        let mut d = Draft::new(
            &before,
            Selection {
                sheet_id: 1,
                annotation_id: 4,
            },
        )
        .unwrap();
        d.move_straight([20., 10.], [297., 210.]).unwrap();
        let once = d.apply(&before).unwrap();
        d.move_straight([20., 10.], [297., 210.]).unwrap();
        assert_eq!(d.apply(&before).unwrap(), once);
        d.move_straight([-999., 999.], [297., 210.]).unwrap();
        assert!(
            matches!(d.annotation(),DrawingAnnotationDto::LineDimension{position,..}|DrawingAnnotationDto::PointLineDimension{position,..} if *position==[5.,205.])
        );
        assert!(d.move_straight([f64::NAN, 0.], [297., 210.]).is_err());
        let deleted = d.delete(&before).unwrap();
        expected = before.clone();
        expected.sheets[0].annotations.remove(2);
        assert_eq!(deleted, expected);
        fields::edit(
            &mut inputs,
            Id::Precision,
            &ControlInput::SetValue("7".into()),
        )
        .unwrap();
        assert!(fields::apply(&mut d, &inputs).is_err());
    }
}

fn candidate_fixture() -> (
    limo_cad_solid::SolidSceneDto,
    limo_cad_occt::DrawingProjectionDto,
    DrawingViewDto,
) {
    let scene=serde_json::from_value(json!({"bodies":[{"id":1,"name":"Test","feature_id":1,"topology_signature":"exact-body-topology",
        "mesh":{"positions":[],"normals":[],"indices":[]},"faces":[],"edges":[
        {"id":1,"key":"edge-1","points":[{"x":0.,"y":0.,"z":0.},{"x":40.,"y":0.,"z":0.}]},
        {"id":2,"key":"edge-2","points":[{"x":0.,"y":0.,"z":0.},{"x":20.,"y":5.,"z":0.},{"x":40.,"y":0.,"z":0.}]}]}],"errors":[]})).unwrap();
    let projection=serde_json::from_value(json!({"bounds":[0.,0.,40.,30.],"visible":[{"points":[[0.,0.],[40.,0.]]}],"hidden":[],"topology_signatures":{"1":"exact-body-topology"},"anchors":[
        {"body_id":1,"edge_id":1,"edge_key":"edge-1","occurrence_id":31,"endpoint":"start","model_point":[100.,0.,0.],"point":[0.,0.],"hidden":false},
        {"body_id":1,"edge_id":1,"edge_key":"edge-1","occurrence_id":31,"endpoint":"end","model_point":[100.,40.,0.],"point":[40.,0.],"hidden":false},
        {"body_id":1,"edge_id":1,"edge_key":"edge-1","occurrence_id":32,"endpoint":"start","model_point":[100.,0.,6.],"point":[0.,0.],"hidden":false},
        {"body_id":1,"edge_id":1,"edge_key":"edge-1","occurrence_id":32,"endpoint":"end","model_point":[100.,40.,6.],"point":[40.,0.],"hidden":false}]})).unwrap();
    (
        scene,
        projection,
        fixture::document().sheets[0].views[0].clone(),
    )
}
#[test]
fn candidates_use_published_points_and_actual_depth_without_double_transforming_occurrences() {
    let (scene, p, mut view) = candidate_fixture();
    view.direction = [0., 0., -1.];
    let actual = targets(&scene, &view, &p, [0., 0., 1.]).unwrap();
    assert_eq!(actual.len(), 1);
    let line = &actual[0];
    assert_eq!(line.reference.occurrence_id.unwrap().0, 32);
    assert_eq!(line.reference.fallback_start, [100., 0., 6.]);
    assert_eq!(line.reference.fallback_end, [100., 40., 6.]);
    assert_eq!(line.paper, [[80., 115.], [120., 115.]]);
    assert_eq!(
        line.reference.topology_signature.as_deref(),
        Some("exact-body-topology")
    );
    let reverse = targets(&scene, &view, &p, [0., 0., -1.]).unwrap();
    assert_eq!(reverse[0].reference.occurrence_id.unwrap().0, 31);
}
#[test]
fn candidates_never_pair_different_instances_or_promote_curves_and_occluded_edges() {
    let (scene, mut p, view) = candidate_fixture();
    p.anchors.remove(3);
    p.anchors.remove(0);
    assert!(targets(&scene, &view, &p, [0., 0., 1.]).unwrap().is_empty());
    let (scene, mut p, view) = candidate_fixture();
    for a in &mut p.anchors {
        a.edge_id.0 = 2;
        a.edge_key = "edge-2".into();
    }
    assert!(targets(&scene, &view, &p, [0., 0., 1.]).unwrap().is_empty());
    let (scene, mut p, mut view) = candidate_fixture();
    p.visible = vec![serde_json::from_value(json!({"points":[[0.,0.],[5.,0.]]})).unwrap()];
    view.show_hidden_lines = false;
    let visible = targets(&scene, &view, &p, [0., 0., 1.]).unwrap();
    assert_eq!(visible[0].pick_segments, vec![[[80., 115.], [85., 115.]]]);
    view.show_hidden_lines = true;
    let still_visible = targets(&scene, &view, &p, [0., 0., 1.]).unwrap();
    assert_eq!(still_visible[0].pick_segments, visible[0].pick_segments);
    p.hidden = vec![serde_json::from_value(json!({"points":[[5.,0.],[40.,0.]]})).unwrap()];
    assert_eq!(targets(&scene, &view, &p, [0., 0., 1.]).unwrap().len(), 1);
}

#[test]
fn empty_hlr_never_promotes_hidden_or_unclassified_topology_to_pick_targets() {
    let (scene, mut p, mut view) = candidate_fixture();
    p.visible.clear();
    for anchor in &mut p.anchors {
        anchor.hidden = true;
    }
    for show_hidden in [false, true] {
        view.show_hidden_lines = show_hidden;
        assert!(targets(&scene, &view, &p, [0., 0., 1.]).unwrap().is_empty());
    }
    p.hidden = vec![serde_json::from_value(json!({"points":[[0.,0.],[40.,0.]]})).unwrap()];
    view.show_hidden_lines = false;
    assert!(targets(&scene, &view, &p, [0., 0., 1.]).unwrap().is_empty());
    view.show_hidden_lines = true;
    assert_eq!(targets(&scene, &view, &p, [0., 0., 1.]).unwrap().len(), 1);
    p.hidden.clear();
    for anchor in &mut p.anchors {
        anchor.hidden = false;
    }
    assert!(targets(&scene, &view, &p, [0., 0., 1.]).unwrap().is_empty());
}

#[test]
fn derived_view_pick_strokes_follow_masks_without_shortening_associative_geometry() {
    let (scene, p, mut view) = candidate_fixture();
    let original = targets(&scene, &view, &p, [0., 0., 1.]).unwrap().remove(0);
    view.derivation = Some(serde_json::from_value(json!({
        "type":"detail", "parent_view_id":2, "radius":8., "label":"A",
        "center":{"body_id":1,"edge_id":1,"edge_key":"edge-1","occurrence_id":32,
            "endpoint":"start","fallback_point":[100.,0.,6.],"topology_signature":"exact-body-topology"}
    })).unwrap());
    let detail = targets(&scene, &view, &p, [0., 0., 1.]).unwrap();
    assert_eq!(detail.len(), 1);
    assert_eq!(detail[0].paper, original.paper);
    assert_eq!(detail[0].reference, original.reference);
    assert_eq!(detail[0].pick_segments, vec![[[80., 115.], [88., 115.]]]);
    assert_eq!(hit(&detail, [84., 115.], 0.1), Some(0));
    assert_eq!(hit(&detail, [100., 115.], 0.1), None);

    view.derivation = Some(
        serde_json::from_value(json!({
            "type":"broken", "parent_view_id":2, "axis":"horizontal",
            "first":0.25, "second":0.75, "gap_mm":8.
        }))
        .unwrap(),
    );
    let broken = targets(&scene, &view, &p, [0., 0., 1.]).unwrap();
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].paper, original.paper);
    assert_eq!(broken[0].reference, original.reference);
    assert_eq!(
        broken[0].pick_segments,
        vec![[[80., 115.], [96., 115.]], [[104., 115.], [120., 115.]]]
    );
    assert_eq!(hit(&broken, [90., 115.], 0.1), Some(0));
    assert_eq!(hit(&broken, [110., 115.], 0.1), Some(0));
    assert_eq!(hit(&broken, [100., 115.], 0.1), None);

    let mut inside = p.clone();
    for anchor in &mut inside.anchors {
        anchor.point[0] = 20.;
        anchor.point[1] =
            if anchor.endpoint == limo_cad_occt::DrawingProjectionAnchorEndpoint::Start {
                0.
            } else {
                20.
            };
    }
    inside.visible = vec![serde_json::from_value(json!({"points":[[20.,0.],[20.,20.]]})).unwrap()];
    assert!(targets(&scene, &view, &inside, [0., 0., 1.])
        .unwrap()
        .is_empty());
}

#[test]
fn partially_occluded_edges_expose_only_the_rendered_spans_without_shortening_measurements() {
    let (scene, mut p, mut view) = candidate_fixture();
    view.show_hidden_lines = false;
    p.visible = serde_json::from_value(json!([
        {"points":[[0.,0.],[12.,0.]]}, {"points":[[13.,0.],[40.,0.]]}
    ]))
    .unwrap();
    p.hidden = serde_json::from_value(json!([{ "points":[[12.,0.],[13.,0.]] }])).unwrap();
    let targets = targets(&scene, &view, &p, [0., 0., 1.]).unwrap();
    assert_eq!(targets.len(), 1);
    let line = &targets[0];
    assert_eq!(line.paper, [[80., 115.], [120., 115.]]);
    assert_eq!(
        line.pick_segments,
        vec![[[80., 115.], [92., 115.]], [[93., 115.], [120., 115.]]]
    );
    assert_eq!(hit(&targets, [92.5, 115.], 0.1), None);
    assert_eq!(hit(&targets, [91., 115.], 0.1), Some(0));
    view.show_hidden_lines = true;
    let shown = super::targets(&scene, &view, &p, [0., 0., 1.]).unwrap();
    assert_eq!(shown[0].paper, line.paper);
    assert_eq!(shown[0].reference, line.reference);
    assert_eq!(hit(&shown, [92.5, 115.], 0.1), Some(0));
}
