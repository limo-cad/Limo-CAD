use super::*;
use serde_json::{json, Value};

#[test]
fn material_condition_keeps_text_presentation_without_changing_cell_width() {
    let mut plain = CheckedArt::default();
    plain.label([20., 20.], "Ⓜ".into(), 3.5, 0., false, Ink::Drawing);
    let mut text = CheckedArt::default();
    text.label([20., 20.], "Ⓜ\u{fe0e}".into(), 3.5, 0., false, Ink::Drawing);
    assert_eq!(plain.labels[0].width_mm, text.labels[0].width_mm);
    let annotation: DrawingAnnotationDto = serde_json::from_value(json!({
        "kind":"gdt_frame", "id":1, "view_id":1,
        "attachment":{"type":"anchor","reference":anchor(1,"start")},
        "position":[20.,20.],"characteristic":"position","tolerance":0.1,
        "material_condition":"maximum", "datums":[]
    }))
    .unwrap();
    assert!(super::text::gdt_cells(&annotation)[1].ends_with("Ⓜ\u{fe0e}"));
}

#[test]
fn label_alignment_keeps_saved_left_center_and_right_anchors() {
    for (align, expected) in [
        (1., LabelAlign::Start),
        (0., LabelAlign::Center),
        (-1., LabelAlign::End),
    ] {
        let mut art = CheckedArt::default();
        art.label(
            [120., 80.],
            "Saved note".into(),
            3.5,
            align,
            false,
            Ink::Drawing,
        );
        let label = &art.labels[0];
        assert_eq!(label.align, expected);
        assert!(
            (f64::from(label.x) - align * f64::from(label.width_mm) * 0.5 - 120.).abs() < 0.0001
        );
    }
}

fn anchor(edge: u64, endpoint: &str) -> Value {
    json!({"body_id":1,"edge_id":edge,"edge_key":format!("e{edge}"),"endpoint":endpoint,"fallback_point":[999.,999.,999.]})
}
fn line(edge: u64) -> Value {
    json!({"body_id":1,"edge_id":edge,"edge_key":format!("e{edge}"),"fallback_start":[999.,999.,999.],"fallback_end":[999.,999.,999.]})
}
fn circle(edge: u64) -> Value {
    json!({"body_id":1,"edge_id":edge,"edge_key":format!("e{edge}"),"fallback_center":[999.,999.,999.],"fallback_normal":[0.,0.,1.],"fallback_radius":999.,"closed":true})
}
pub(super) fn fixture() -> (
    DrawingSheetDto,
    BTreeMap<u64, (DrawingViewDto, DrawingProjectionDto)>,
) {
    let view:DrawingViewDto=serde_json::from_value(json!({"id":1,"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,100.],"scale":2.})).unwrap();
    let mut anchors = Vec::new();
    for (edge, start, end) in [
        (1, [0., 0.], [40., 0.]),
        (2, [0., 20.], [40., 20.]),
        (3, [0., 0.], [0., 20.]),
    ] {
        for (endpoint, point) in [("start", start), ("end", end)] {
            anchors.push(json!({"body_id":1,"edge_id":edge,"edge_key":format!("e{edge}"),"endpoint":endpoint,"model_point":[point[0],point[1],0.],"point":point,"hidden":false}));
        }
    }
    let circles:Vec<_>=[(4,[0.,0.]),(5,[20.,0.]),(6,[10.,17.3205080757])].into_iter().map(|(edge,center)|json!({"body_id":1,"edge_id":edge,"edge_key":format!("e{edge}"),"center_model":[center[0],center[1],0.],"normal_model":[0.,0.,1.],"center":center,"radius":3.,"closed":true,"hidden":false})).collect();
    let projection=serde_json::from_value(json!({"visible":[],"hidden":[],"anchors":anchors,"circles":circles,"bounds":[-3.,-3.,43.,23.]})).unwrap();
    let sheet=serde_json::from_value(json!({"id":1,"name":"Existing sheet","format":"a4","orientation":"landscape","views":[view],"bom":[{"id":1,"item_number":"7","part_number":"PART-7","description":"Existing part","quantity":1}]})).unwrap();
    (sheet, BTreeMap::from([(1, (view, projection))]))
}
pub(super) fn variants() -> Vec<Value> {
    let a = anchor(1, "start");
    let b = anchor(1, "end");
    let c = anchor(3, "end");
    let l = line(1);
    let attachment = json!({"type":"anchor","reference":a});
    let circular = circle(4);
    vec![
        json!({"kind":"linear_dimension","first":a,"second":b,"mode":"horizontal","offset":12.}),
        json!({"kind":"line_dimension","first":l,"second":line(2),"mode":"distance","position":[160.,60.]}),
        json!({"kind":"point_line_dimension","point":c,"line":l,"position":[160.,60.]}),
        json!({"kind":"note","text":"Saved note\nSecond line","position":[20.,20.]}),
        json!({"kind":"radial_dimension","feature":circular,"mode":"diameter","leader_angle_deg":35.,"offset":15.}),
        json!({"kind":"angular_dimension","vertex":a,"first":b,"second":c,"radius":15.}),
        json!({"kind":"hole_note","feature":circular,"position":[120.,140.],"quantity":4,"diameter":6.,"depth":12.,"hole_style":"counterbore","counterbore_diameter":10.,"counterbore_depth":3.,"note":"Saved hole note"}),
        json!({"kind":"chamfer_note","first":a,"second":b,"position":[90.,140.],"length":2.,"angle_deg":45.}),
        json!({"kind":"center_mark","feature":circular,"extension":3.}),
        json!({"kind":"center_line","first":circular,"second":circle(5),"extension":3.}),
        json!({"kind":"center_line_between_edges","first":l,"second":line(2),"extension":3.}),
        json!({"kind":"automatic_symmetry_axis","axis":"both","extension":3.}),
        json!({"kind":"bolt_circle_center_line","features":[circular,circle(5),circle(6)],"extension":3.}),
        json!({"kind":"chain_dimension","anchors":[a,b,c],"mode":"aligned","layout":"baseline","offset":12.,"spacing":8.}),
        json!({"kind":"ordinate_dimension","origin":a,"target":b,"axis":"both","offset":12.}),
        json!({"kind":"arc_length_dimension","feature":circular,"first":b,"second":c,"offset":5.}),
        json!({"kind":"jogged_radius_dimension","feature":circular,"jog":[100.,150.],"position":[150.,160.]}),
        json!({"kind":"datum_feature","attachment":attachment,"label":"A","position":[140.,120.],"target_index":2}),
        json!({"kind":"gdt_frame","attachment":attachment,"position":[140.,130.],"characteristic":"position","tolerance":0.1,"diameter_zone":true,"material_condition":"maximum","datums":[{"label":"A"}]}),
        json!({"kind":"surface_texture","attachment":attachment,"position":[140.,140.],"roughness_ra":1.6,"process":"Grind"}),
        json!({"kind":"edge_requirement","attachment":l,"position":[140.,150.],"upper_deviation":0.2,"lower_deviation":-0.1,"note":"Deburr"}),
        json!({"kind":"weld_symbol","attachment":l,"position":[140.,160.],"weld_type":"fillet","side":"arrow","size":3.,"all_around":true,"field_weld":true,"tail":"W1"}),
        json!({"kind":"item_balloon","attachment":attachment,"position":[140.,170.],"bom_item_id":1}),
        json!({"kind":"revision_cloud","revision":"B","points":[[20.,20.],[50.,20.],[50.,40.],[20.,40.]]}),
    ]
}
pub(super) fn annotation(mut value: Value) -> DrawingAnnotationDto {
    value["id"] = json!(1);
    value["view_id"] = json!(1);
    serde_json::from_value(value).unwrap()
}

#[test]
fn every_saved_annotation_variant_produces_finite_paper_graphics() {
    let (mut sheet, projections) = fixture();
    for value in variants() {
        let kind = value["kind"].as_str().unwrap().to_owned();
        sheet.annotations = vec![annotation(value)];
        let before = sheet.clone();
        let art = render(&sheet, &projections, UnitSystem::Mm);
        assert!(
            !art.segments.is_empty() || !art.labels.is_empty(),
            "{kind} disappeared"
        );
        assert!(
            art.labels.iter().all(|label| label.text != "!"),
            "{kind} failed to resolve current topology"
        );
        for segment in &art.segments {
            assert!(
                [
                    segment.x1,
                    segment.y1,
                    segment.x2,
                    segment.y2,
                    segment.width_mm
                ]
                .iter()
                .all(|n| n.is_finite()),
                "{kind}"
            );
        }
        for label in &art.labels {
            assert!(
                [
                    label.x,
                    label.y,
                    label.angle,
                    label.width_mm,
                    label.height_mm,
                    label.text_height_mm
                ]
                .iter()
                .all(|n| n.is_finite()),
                "{kind}"
            );
        }
        assert_eq!(
            sheet, before,
            "presentation must not mutate saved drawing intent"
        );
    }
}

#[test]
fn straight_label_hits_follow_painted_text_and_reject_missing_occurrences_for_drag() {
    let (mut sheet, projections) = fixture();
    for mut value in [variants()[1].clone(), variants()[2].clone()] {
        sheet.annotations = vec![annotation(value.clone())];
        let art = render(&sheet, &projections, UnitSystem::Mm);
        assert_eq!(art.marks.len(), 1);
        let mark = &art.marks[0];
        let label = &art.labels[0];
        assert!(mark.position_resolved);
        assert_eq!(mark.center, [label.x as f64, label.y as f64]);
        assert_eq!(mark.size, [label.width_mm as f64, label.height_mm as f64]);
        assert_eq!(mark.angle, label.angle);
        let field = if value["kind"] == "line_dimension" {
            "first"
        } else {
            "line"
        };
        value[field]["occurrence_id"] = json!(9999);
        sheet.annotations = vec![annotation(value)];
        let broken = render(&sheet, &projections, UnitSystem::Mm);
        assert!(broken.marks.iter().all(|mark| !mark.position_resolved));
        assert!(broken.labels.iter().any(|label| label.text == "!"));
    }
}
#[test]
fn annotation_resolution_fences_occurrence_signature_and_uses_exact_circle_center() {
    let (sheet, projections) = fixture();
    let (view, projection) = projections.get(&1).unwrap();
    let r = Resolver { view, projection };
    let mut a: DrawingTopologyAnchorRefDto = serde_json::from_value(anchor(1, "start")).unwrap();
    assert_eq!(r.anchor(&a), Some(paper_point(view, [0., 0.], projection)));
    a.topology_signature = Some("stale".into());
    assert!(r.anchor(&a).is_none());
    a.topology_signature = None;
    a.occurrence_id = Some(serde_json::from_value(json!(99)).unwrap());
    assert!(r.anchor(&a).is_none());
    a.occurrence_id = None;
    a.edge_id = limo_cad_core::EdgeId(99);
    assert!(
        r.anchor(&a).is_some(),
        "stable edge key survives local edge renumbering"
    );
    a.edge_key = "missing".into();
    assert!(
        r.anchor(&a).is_none(),
        "fallback model coordinates are not rendering evidence"
    );
    let mut a: DrawingTopologyAnchorRefDto = serde_json::from_value(anchor(4, "start")).unwrap();
    a.circle_center = true;
    assert_eq!(r.anchor(&a), Some(paper_point(view, [0., 0.], projection)));
    let mut sheet = sheet;
    let mut value = variants().remove(0);
    value["first"]["edge_key"] = json!("deleted");
    sheet.annotations = vec![annotation(value)];
    let art = render(&sheet, &projections, UnitSystem::Mm);
    assert_eq!(art.labels[0].text, "!");
    assert_eq!(art.labels[0].ink, Ink::Revision);
}
#[test]
fn dimensions_retain_units_tolerances_and_semantic_lengths_at_view_scale() {
    let p:DrawingDimensionPresentationDto=serde_json::from_value(json!({"tolerance":{"mode":"deviation","upper":0.2,"lower":-0.1},"basic":true,"reference":true,"fit_class":"H7","dual_units":{"unit":"inch","precision":3,"placement":"bracketed"}})).unwrap();
    assert_eq!(
        text::dimension(25.4, 2, "⌀", "", UnitSystem::Cm, &p),
        "⌀([2.54 cm +0.02/-0.01 H7 [1.000 in]])"
    );
    let (mut sheet, projections) = fixture();
    sheet.annotations = vec![annotation(variants().remove(0))];
    let art = render(&sheet, &projections, UnitSystem::In);
    assert_eq!(art.labels[0].text, "1.57 in");
    let (_, projection) = projections.get(&1).unwrap();
    let a = paper_point(&sheet.views[0], [0., 0.], projection);
    assert!(
        art.segments
            .iter()
            .any(|s| s.arrow && (f64::from(s.y1) - a[1] - 12.).abs() < 1e-5),
        "offset remains paper mm at scale 2"
    );
    let g = point_line([0., 10.], [[0., 0.], [40., 0.]], [50., 30.], 2.).unwrap();
    assert_eq!(g.value, 5.);
    assert_eq!(g.start, [50., 10.]);
    assert_eq!(g.end, [50., 0.]);
    let Some(geometry::LineDimension::Linear(g)) = line_dimension(
        [[0., 0.], [40., 0.]],
        Some([[0., 20.], [40., 20.]]),
        DrawingLineDimensionMode::Distance,
        [50., 0.],
        2.,
    ) else {
        panic!()
    };
    assert_eq!(g.value, 10.);
    assert_eq!(g.first, [40., 0.]);
    assert_eq!(g.second, [40., 20.]);
}
#[test]
fn annotation_callouts_retain_multiline_saved_text_and_bom_number() {
    let (mut sheet, projections) = fixture();
    sheet.annotations = variants().into_iter().map(annotation).collect();
    let art = render(&sheet, &projections, UnitSystem::Mm);
    let labels: Vec<_> = art.labels.iter().map(|l| l.text.as_str()).collect();
    for expected in [
        "Saved note",
        "Second line",
        "4× ⌀6 ↧12",
        "⌴ ⌀10 ↧3",
        "Saved hole note",
        "7",
        "REV B",
        "A2",
        "Ra 1.6 Grind",
        "W1",
    ] {
        assert!(labels.contains(&expected), "missing {expected}: {labels:?}");
    }
    assert!(art.segments.iter().any(|s| s.ink == Ink::Center));
    assert!(art.segments.iter().any(|s| s.ink == Ink::Revision));
}

#[test]
fn tessellated_center_circle_keeps_dash_gaps_and_revision_cloud_keeps_scallops() {
    let mut art = CheckedArt::default();
    art.circle(
        [0., 0.],
        10.,
        &DrawingLineStyleDto {
            width_mm: 0.35,
            dash_mm: vec![6., 2., 1., 2.],
        },
        Ink::Center,
    );
    let drawn: f64 = art
        .segments
        .iter()
        .map(|s| f64::from((s.x2 - s.x1).hypot(s.y2 - s.y1)))
        .sum();
    assert!(
        drawn > 25. && drawn < 50.,
        "dash phase must survive short tessellated edges: {drawn}"
    );
    let (mut sheet, projections) = fixture();
    sheet.annotations = vec![annotation(
        json!({"kind":"revision_cloud","revision":"C","points":[[20.,20.],[50.,20.],[50.,40.],[20.,40.]]}),
    )];
    let art = render(&sheet, &projections, UnitSystem::Mm);
    assert!(
        art.segments
            .iter()
            .any(|s| s.y1 < 19.5 && s.x1 > 20. && s.x1 < 50.),
        "revision cloud has the existing outward scallops, not a polygon substitute"
    );
    assert_eq!(art.labels[0].text, "REV C");
    assert_eq!(art.marks.len(), 1);
    assert_eq!(art.marks[0].id, sheet.annotations[0].id());
    assert_eq!(
        art.marks[0].center,
        [art.labels[0].x as f64, art.labels[0].y as f64]
    );
    assert!(
        art.marks[0].size[0] < 20.,
        "The label hit must not cover the polygon interior"
    );
}
