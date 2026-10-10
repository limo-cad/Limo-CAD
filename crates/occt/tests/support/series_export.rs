//! Synthetic exact-anchor series fixtures; no OCCT or live-input claim.
use super::occt;
use super::rectangle;
use limo_cad_sketch::DrawingDocumentDto;
use limo_cad_solid::SolidSceneDto;
use occt::DrawingProjectionDto;
pub use rectangle::full_presentation;
use serde_json::json;

pub fn fixture(layout: &str) -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
    let (mut document, scene, projection) = rectangle::fixture("length", 40.);
    let anchor = |id: usize, key: &str, endpoint: &str| {
        json!({
            "body_id":1,"edge_id":id,"edge_key":key,
            "topology_signature":"feature:1:rectangle-connectivity",
            "endpoint":endpoint,"fallback_point":[999.,999.,999.]
        })
    };
    document.sheets[0].annotations = vec![serde_json::from_value(json!({
        "id":1,"kind":"chain_dimension","view_id":1,
        "anchors":[anchor(1,"bottom","start"),anchor(1,"bottom","end"),anchor(2,"right","end")],
        "mode":"aligned","layout":layout,"offset":12.,"spacing":7.,"precision":2
    }))
    .unwrap()];
    (document, scene, projection)
}
