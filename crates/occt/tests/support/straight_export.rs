//! Explicit synthetic projection fixture shared by unit checks and artifact QA.
//! This is not OCC hidden-line or physical-input validation.
use super::occt;
use limo_cad_sketch::{AssemblyDocumentDto, DrawingDocumentDto, DrawingViewDto, SketchManager};
use limo_cad_solid::SolidSceneDto;
use occt::{drawing_export::projection_request, DrawingProjectionDto};
use serde_json::{json, Value};

pub fn fixture(
    kind: &str,
    width: f64,
) -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
    let vertices = [[0., 0.], [width, 0.], [width, 30.], [0., 30.]];
    let keys = ["bottom", "right", "top", "left"];
    let edges: Vec<_> = (0..4).map(|i| {
        let a = vertices[i];
        let b = vertices[(i + 1) % 4];
        json!({"id":i+1,"key":keys[i],"points":[{"x":a[0],"y":a[1],"z":0.},{"x":b[0],"y":b[1],"z":0.}],"circle":null,"refinable":true})
    }).collect();
    let scene: SolidSceneDto = serde_json::from_value(json!({
        "bodies":[{"id":1,"name":"Synthetic rectangle","feature_id":1,"topology_signature":"rectangle-connectivity",
            "mesh":{"positions":[],"normals":[],"indices":[]},"faces":[],"edges":edges}],"errors":[]
    })).unwrap();
    let mut manager = SketchManager::new();
    let mut document = manager.drawing_command(serde_json::from_value(json!({
        "type":"create_sheet","arguments":{"name":"Straight export QA","format":"a3","orientation":"landscape"}
    })).unwrap()).unwrap();
    let view: DrawingViewDto = serde_json::from_value(json!({
        "id":1,"name":"Synthetic top projection","kind":"top","body_ids":[1],
        "direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[145.,115.],"scale":2.
    }))
    .unwrap();
    document.sheets[0].views.push(view);
    document.next_view_id = 2;
    let line = |id: usize| {
        json!({"body_id":1,"edge_id":id,"edge_key":keys[id-1],
        "topology_signature":"feature:1:rectangle-connectivity", "fallback_start":[999.,999.,999.],"fallback_end":[998.,999.,999.]})
    };
    let anchor = |id: usize, endpoint: &str| {
        json!({"body_id":1,"edge_id":id,"edge_key":keys[id-1],
        "topology_signature":"feature:1:rectangle-connectivity", "endpoint":endpoint,"fallback_point":[999.,999.,999.]})
    };
    let annotation = match kind {
        "length" => {
            json!({"kind":"line_dimension","id":1,"view_id":1,"first":line(1),"mode":"length","position":[145.,165.]})
        }
        "distance" => {
            json!({"kind":"line_dimension","id":1,"view_id":1,"first":line(3),"second":line(1),"mode":"distance","position":[220.,115.]})
        }
        "angle" => {
            json!({"kind":"line_dimension","id":1,"view_id":1,"first":line(1),"second":line(2),"mode":"angle","position":[210.,175.]})
        }
        "point-line" => {
            json!({"kind":"point_line_dimension","id":1,"view_id":1,"point":anchor(3,"end"),"line":line(1),"position":[220.,115.]})
        }
        _ => panic!("Unknown synthetic annotation case"),
    };
    let sheet = &mut document.sheets[0];
    sheet
        .annotations
        .push(serde_json::from_value(annotation).unwrap());
    sheet.annotations.push(
        serde_json::from_value(json!({"kind":"note","id":2,"position":[25.,30.],
        "text":"Synthetic projection QA - not OCC or physical-input validation"}))
        .unwrap(),
    );
    sheet.title_block.title = format!("{kind} / preserved title");
    sheet.release = serde_json::from_value(
        json!({"status":"released","released_revision":"R7","released_at":"2026-09-27"}),
    )
    .unwrap();
    sheet.revisions.push(serde_json::from_value(json!({"id":17,"revision":"R7","description":"Keep full revision metadata",
        "date":"2026-09-27","author":"Author","checked_by":"Checker","approved_by":"Approver","change_order":"CO-17","status":"released"})).unwrap());
    document.next_revision_id = 18;
    document.next_annotation_id = 3;
    let request = projection_request(
        &sheet.views[0],
        &sheet.views,
        &scene,
        &AssemblyDocumentDto::default(),
    )
    .unwrap();
    let visible: Vec<Value> = (0..4)
        .map(|i| json!({"points":[vertices[i],vertices[(i+1)%4]]}))
        .collect();
    let mut projection: DrawingProjectionDto = serde_json::from_value(json!({
        "visible":visible,"hidden":[],"section":[],"bounds":[0.,0.,width,30.]
    }))
    .unwrap();
    projection.anchors = occt::drawing_projection_anchors(&scene, &request, &projection).unwrap();
    projection.topology_signatures =
        limo_cad_sketch::drawing_topology::drawing_topology_signatures(&scene);
    document
        .validate()
        .expect("Synthetic export fixture must satisfy the shared drawing model");
    (document, scene, projection)
}

pub fn full_presentation(document: &mut DrawingDocumentDto) {
    let mut value = serde_json::to_value(&document.sheets[0].annotations[0]).unwrap();
    value["prefix"] = json!("QA ");
    value["suffix"] = json!(" exact");
    value["precision"] = json!(3);
    value["presentation"] = json!({"basic":true,"reference":true,"fit_class":"H7",
        "tolerance":{"mode":"deviation","upper":0.2,"lower":-0.1},
        "dual_units":{"unit":"inch","precision":3,"placement":"bracketed"}});
    document.sheets[0].annotations[0] = serde_json::from_value(value).unwrap();
}
