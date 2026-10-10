use super::*;
use serde_json::json;

#[test]
fn invalid_center_geometry_keeps_diagnostic_ink_without_a_paper_pick_target() {
    let reference = json!({"body_id":1,"edge_id":1,"edge_key":"round","topology_signature":"exact","fallback_center":[999.,999.,999.],"fallback_normal":[0.,0.,1.],"fallback_radius":3.,"closed":true});
    let mut sheet:DrawingSheetDto=serde_json::from_value(json!({"id":1,"name":"Center","format":"a4","orientation":"landscape", "views":[{"id":1,"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,100.],"scale":1.}],"annotations":[{"kind":"center_mark","id":1,"view_id":1,"feature":reference,"extension":2.5}]})).unwrap();
    let mut p:DrawingProjectionDto=serde_json::from_value(json!({"bounds":[0.,0.,40.,30.],"visible":[],"hidden":[],"topology_signatures":{"1":"exact"},"circles":[{"body_id":1,"edge_id":1,"edge_key":"round","center_model":[20.,15.,0.],"normal_model":[0.,0.,1.],"center":[20.,15.],"radius":3.,"closed":true,"hidden":false}]})).unwrap();
    for line in [false, true] {
        if line {
            sheet.annotations[0]=serde_json::from_value(json!({"kind":"center_line","id":1,"view_id":1,"first":reference,"second":reference,"extension":2.5})).unwrap();
        }
        p.circles[0].radius = if line { 3. } else { 0. };
        let projections = BTreeMap::from([(1, (sheet.views[0].clone(), p.clone()))]);
        let art = try_render(&sheet, &projections, UnitSystem::Mm).unwrap();
        assert!(art.labels.iter().any(|label| label.text == "!"));
        assert!(
            art.marks.is_empty(),
            "Diagnostic fallback ink cannot stand in for resolved geometry; the inspector owns repair selection"
        );
    }
}
