//! Explicit synthetic circular projections for deterministic graphics review.
use super::occt;
use limo_cad_sketch::{DrawingDocumentDto, SketchManager};
use limo_cad_solid::SolidSceneDto;
use occt::DrawingProjectionDto;
use serde_json::json;

pub fn fixture(
    kind: &str,
    scale: f64,
    custom: bool,
) -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
    let edges: Vec<_> = [(1, [10., 20.], 5.), (2, [40., 40.], 8.)]
        .into_iter()
        .map(|(id, c, r)| {
            json!({"id":id,"key":format!("circle-{id}"),"points":[],"refinable":true,
            "circle":{"center":{"x":c[0],"y":c[1],"z":0.},"normal":{"x":0.,"y":0.,"z":1.},
                "reference":{"x":1.,"y":0.,"z":0.},"radius":r,"closed":true}})
        })
        .collect();
    let scene: SolidSceneDto = serde_json::from_value(json!({"bodies":[{"id":1,"name":"Synthetic circular edges","feature_id":7,
        "topology_signature":"two-closed-circles","mesh":{"positions":[],"normals":[],"indices":[]},"faces":[],"edges":edges}],"errors":[]})).unwrap();
    let mut manager = SketchManager::new();
    let mut drawing = manager
        .drawing_command(
            serde_json::from_value(json!({"type":"create_sheet","arguments":{
        "name":"Center export QA","format":"a4","orientation":"landscape"}}))
            .unwrap(),
        )
        .unwrap();
    let reference = |id| {
        json!({"body_id":1,"edge_id":id,"edge_key":format!("circle-{id}"),
        "topology_signature":"feature:7:two-closed-circles","fallback_center":[999.,999.,999.],
        "fallback_normal":[0.,0.,1.],"fallback_radius":999.,"closed":true})
    };
    let sheet = &mut drawing.sheets[0];
    sheet.views.push(
        serde_json::from_value(
            json!({"id":1,"name":"Synthetic circles","kind":"top","body_ids":[1],
        "direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[130.,80.],"scale":scale}),
        )
        .unwrap(),
    );
    let annotation = match kind {
        "mark" => {
            json!({"kind":"center_mark","id":1,"view_id":1,"feature":reference(1),"extension":4.})
        }
        "line" => {
            json!({"kind":"center_line","id":1,"view_id":1,"first":reference(1),"second":reference(2),"extension":4.})
        }
        _ => panic!("Unknown center fixture"),
    };
    sheet
        .annotations
        .push(serde_json::from_value(annotation).unwrap());
    sheet.annotations.push(serde_json::from_value(json!({"kind":"note","id":2,"text":"Synthetic circular projection QA - no OCC or physical-input proof","position":[20.,22.]})).unwrap());
    if custom {
        sheet.style.center.width_mm = 0.55;
        sheet.style.center.dash_mm = vec![5., 1., 0.8, 1.];
    }
    drawing.next_view_id = 2;
    drawing.next_annotation_id = 3;
    let circles: Vec<_>=[(1,[10.,20.],5.),(2,[40.,40.],8.)].into_iter().map(|(id,c,r)| json!({
        "body_id":1,"edge_id":id,"edge_key":format!("circle-{id}"),"center_model":[c[0],c[1],0.],"normal_model":[0.,0.,1.],
        "center":c,"radius":r,"closed":true,"hidden":false})).collect();
    let visible: Vec<_>=[([10.,20.],5.),([40.,40.],8.)].into_iter().map(|(c,r)| json!({"points":occt::drawing_presentation::geometry::arc(c,r,0.,std::f64::consts::TAU)})).collect();
    let projection: DrawingProjectionDto = serde_json::from_value(
        json!({"topology_signatures":{"1":"feature:7:two-closed-circles"},
        "visible":visible,"hidden":[],"circles":circles,"bounds":[5.,15.,48.,48.]}),
    )
    .unwrap();
    drawing.validate().unwrap();
    (drawing, scene, projection)
}
