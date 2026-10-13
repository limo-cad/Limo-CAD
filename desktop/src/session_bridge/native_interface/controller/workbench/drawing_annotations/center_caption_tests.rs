use super::*;
use serde_json::json;

fn fixture(
    line: bool,
    scale: f64,
    width: f64,
) -> (
    DrawingSheetDto,
    BTreeMap<u64, (DrawingViewDto, DrawingProjectionDto)>,
) {
    let reference = |id| {
        json!({"body_id":1,"edge_id":id,"edge_key":format!("round-{id}"),
        "topology_signature":"exact","fallback_center":[999.,999.,999.],
        "fallback_normal":[0.,0.,1.],"fallback_radius":999.,"closed":true})
    };
    let annotation = if line {
        json!({"kind":"center_line","id":1,"view_id":1,"first":reference(1),"second":reference(2),"extension":4.})
    } else {
        json!({"kind":"center_mark","id":1,"view_id":1,"feature":reference(1),"extension":4.})
    };
    let mut sheet: DrawingSheetDto = serde_json::from_value(json!({"id":1,"name":"Centers",
        "format":"a4","orientation":"landscape", "views":[
            {"id":1,"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,100.],"scale":scale},
            {"id":2,"name":"Neighbor","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[200.,100.],"scale":scale}],
        "annotations":[annotation]})).unwrap();
    sheet.style.center.width_mm = width;
    sheet.style.center.dash_mm.clear();
    sheet.style.small_text_height_mm = 2.5;
    let projection: DrawingProjectionDto = serde_json::from_value(json!({
        "bounds":[0.,0.,40.,30.],"visible":[],"hidden":[],"topology_signatures":{"1":"exact"},
        "circles":[
            {"body_id":1,"edge_id":1,"edge_key":"round-1","center_model":[20.,0.,0.],"normal_model":[0.,0.,1.],"center":[20.,0.],"radius":5.,"closed":true,"hidden":false},
            {"body_id":1,"edge_id":2,"edge_key":"round-2","center_model":[20.,30.,0.],"normal_model":[0.,0.,1.],"center":[20.,30.],"radius":8.,"closed":true,"hidden":false}]})).unwrap();
    let projections = sheet
        .views
        .iter()
        .map(|view| (view.id, (view.clone(), projection.clone())))
        .collect();
    (sheet, projections)
}

#[test]
fn native_view_caption_clears_only_its_rendered_centers_at_both_scales_and_widths() {
    for line in [false, true] {
        for scale in [1., 2.] {
            for width in [0.18, 0.55] {
                let (sheet, projections) = fixture(line, scale, width);
                let before = serde_json::to_string(&sheet).unwrap();
                let art = try_render_decorated(&sheet, &projections, UnitSystem::Mm, &[]).unwrap();
                let caption = art
                    .labels
                    .iter()
                    .find(|label| label.text.starts_with("Top "))
                    .unwrap();
                let (view, projection) = &projections[&1];
                let original = super::super::view_name_label(view, projection, 2.5, None);
                let ink_bottom = 104. + 20. * scale + width * 0.5;
                let expected_baseline = ink_bottom + 2.5 + 1.;
                assert!((f64::from(caption.y) - (expected_baseline - 1.)).abs() < 1e-4);
                assert!(caption.y > original.y);
                assert_eq!(caption.x, original.x);
                assert_eq!(caption.align, original.align);
                assert_eq!(caption.width_mm, original.width_mm);
                let neighbor = art
                    .labels
                    .iter()
                    .find(|label| label.text.starts_with("Neighbor "))
                    .unwrap();
                let expected = super::super::view_name_label(
                    &projections[&2].0,
                    &projections[&2].1,
                    2.5,
                    None,
                );
                assert_eq!(neighbor.y, expected.y);
                assert_eq!(neighbor.x, expected.x);
                assert_eq!(serde_json::to_string(&sheet).unwrap(), before);
            }
        }
    }
}

#[test]
fn ring_ink_is_counted_but_missing_centers_and_unrelated_notes_do_not_move_captions() {
    let (mut sheet, mut projections) = fixture(false, 2., 0.05);
    let DrawingAnnotationDto::CenterMark { extension, .. } = &mut sheet.annotations[0] else {
        panic!()
    };
    *extension = 0.;
    projections.get_mut(&1).unwrap().1.circles[0].radius = 0.05;
    let rendered = render_checked(
        &sheet,
        &projections,
        UnitSystem::Mm,
        budget::Limits::default(),
    )
    .unwrap();
    assert!((rendered.center_bottom[&1] - 130.66).abs() < 1e-4);

    projections
        .get_mut(&1)
        .unwrap()
        .1
        .topology_signatures
        .insert("1".into(), "stale".into());
    sheet.annotations.push(
        serde_json::from_value(json!({"kind":"note","id":2,
        "text":"A lower note does not move the view caption", "position":[100.,190.]}))
        .unwrap(),
    );
    let broken = try_render_decorated(&sheet, &projections, UnitSystem::Mm, &[]).unwrap();
    assert!(broken.labels.iter().any(|label| label.text == "!"));
    let caption = broken
        .labels
        .iter()
        .find(|label| label.text.starts_with("Top "))
        .unwrap();
    let expected = super::super::view_name_label(&projections[&1].0, &projections[&1].1, 2.5, None);
    assert_eq!(caption.y, expected.y);
    sheet.annotations.remove(0);
    let no_center = try_render_decorated(&sheet, &projections, UnitSystem::Mm, &[]).unwrap();
    let caption = no_center
        .labels
        .iter()
        .find(|label| label.text.starts_with("Top "))
        .unwrap();
    assert_eq!(caption.y, expected.y);
}
