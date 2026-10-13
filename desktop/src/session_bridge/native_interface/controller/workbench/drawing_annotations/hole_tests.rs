use super::*;
use serde_json::json;
#[test]
fn hole_note_resolved_lines_are_selectable_and_stale_references_only_paint_diagnostics() {
    let feature = json!({"occurrence_id":31,"body_id":1,"edge_id":5,"edge_key":"round","topology_signature":"exact","fallback_center":[999.,999.,999.],"fallback_normal":[0.,0.,1.],"fallback_radius":99.,"closed":true});
    let sheet:DrawingSheetDto=serde_json::from_value(json!({"id":1,"name":"Holes","format":"a4","orientation":"landscape",
        "views":[{"id":1,"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,100.],"scale":2.}],
        "annotations":[{"kind":"hole_note","id":1,"view_id":1,"feature":feature,"position":[120.,115.],"quantity":2,"diameter":6.,"depth":12.,"note":"Deburr\nInspect"}]})).unwrap();
    let projection:DrawingProjectionDto=serde_json::from_value(json!({"bounds":[0.,0.,40.,30.],"visible":[],"hidden":[],"topology_signatures":{"1":"exact"},
        "circles":[{"occurrence_id":31,"body_id":1,"edge_id":5,"edge_key":"round","center_model":[20.,15.,6.],"normal_model":[0.,0.,1.],"center":[20.,15.],"radius":3.,"closed":true,"hidden":false}]})).unwrap();
    let projections = BTreeMap::from([(1, (sheet.views[0].clone(), projection.clone()))]);
    let art = try_render(&sheet, &projections, UnitSystem::Mm).unwrap();
    assert_eq!(art.marks.len(), 3);
    assert!(art.marks.iter().all(|m| m.id == 1 && m.position_resolved));
    assert_eq!(
        art.marks.iter().map(|m| m.part).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    for occurrence in [false, true] {
        let mut stale = projection.clone();
        if occurrence {
            stale.circles[0].occurrence_id = Some(serde_json::from_value(json!(32)).unwrap());
        } else {
            stale
                .topology_signatures
                .insert("1".into(), "changed".into());
        }
        let broken = try_render(
            &sheet,
            &BTreeMap::from([(1, (sheet.views[0].clone(), stale))]),
            UnitSystem::Mm,
        )
        .unwrap();
        assert!(
            broken.marks.is_empty(),
            "A stale note's diagnostic has no paper hit area; its saved record remains in the inspector"
        );
        assert!(broken.labels.iter().any(|label| label.text == "!"));
    }
}
