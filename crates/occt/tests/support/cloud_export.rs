//! Synthetic saved paper annotations; no OCCT projection or input is implied.
use limo_cad_sketch::{DrawingDocumentDto, SketchManager};
use limo_cad_solid::SolidSceneDto;
use serde_json::json;

pub fn fixture(kind: &str) -> (DrawingDocumentDto, SolidSceneDto) {
    let points = match kind {
        "triangle" => vec![[40., 55.], [115., 50.], [75., 115.]],
        "quad" => vec![[40., 55.], [135., 55.], [135., 115.], [40., 115.]],
        "loaded-seven" => vec![
            [40., 55.],
            [85., 45.],
            [125., 60.],
            [145., 90.],
            [115., 125.],
            [75., 115.],
            [35., 85.],
        ],
        "clipped-right" => vec![[260., 35.], [310., 35.], [310., 70.], [260., 70.]],
        _ => panic!("Unknown cloud case"),
    };
    let mut manager = SketchManager::new();
    let mut drawing=manager.drawing_command(serde_json::from_value(json!({"type":"create_sheet","arguments":{"name":"Revision cloud export QA","format":"a4","orientation":"landscape"}})).unwrap()).unwrap();
    let sheet = &mut drawing.sheets[0];
    sheet.title_block.revision = "B".into();
    sheet.annotations.push(serde_json::from_value(json!({"kind":"revision_cloud","id":1,"revision":if kind=="clipped-right" {"B"} else {"B<2> & cω\nCafé 零件"},"points":points})).unwrap());
    sheet.annotations.push(serde_json::from_value(json!({"kind":"note","id":2,"text":"Saved paper cloud QA - no OCC or physical input","position":[20.,22.]})).unwrap());
    drawing.next_annotation_id = 3;
    drawing.validate().unwrap();
    (
        drawing,
        serde_json::from_value(json!({"bodies":[],"errors":[]})).unwrap(),
    )
}
