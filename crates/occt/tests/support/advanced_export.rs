//! Bounded mixed sheet for the twelve advanced shared export routes.
//! Geometry is synthetic and exact-keyed; it is not OCCT or live-input evidence.
use super::occt;
use super::rectangle;
use limo_cad_sketch::{AssemblyDocumentDto, DrawingDocumentDto};
use limo_cad_solid::SolidSceneDto;
use occt::{drawing_export::projection_request, DrawingProjectionDto};
use serde_json::json;

pub const KINDS: [&str; 12] = [
    "chamfer_note",
    "center_line_between_edges",
    "automatic_symmetry_axis",
    "bolt_circle_center_line",
    "arc_length_dimension",
    "jogged_radius_dimension",
    "datum_feature",
    "gdt_frame",
    "surface_texture",
    "edge_requirement",
    "weld_symbol",
    "item_balloon",
];

pub fn fixture() -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
    let (mut drawing, mut scene, mut projection) = rectangle::fixture("length", 40.);
    let circles = [
        (5, [20., 15.], 6., false),
        (6, [20., 25.], 2., true),
        (7, [20. - 75_f64.sqrt(), 10.], 2., true),
        (8, [20. + 75_f64.sqrt(), 10.], 2., true),
    ];
    for (id, center, radius, closed) in circles {
        let points = occt::drawing_presentation::geometry::arc(
            center,
            radius,
            0.,
            if closed {
                std::f64::consts::TAU
            } else {
                std::f64::consts::FRAC_PI_2
            },
        );
        scene.bodies[0].edges.push(serde_json::from_value(json!({
            "id":id,"key":format!("circle-{id}"),"refinable":true,
            "points":points.iter().map(|p|json!({"x":p[0],"y":p[1],"z":0.})).collect::<Vec<_>>(),
            "circle":{"center":{"x":center[0],"y":center[1],"z":0.},
                "normal":{"x":0.,"y":0.,"z":1.},"reference":{"x":1.,"y":0.,"z":0.},"radius":radius,"closed":closed}
        })).unwrap());
        projection
            .visible
            .push(serde_json::from_value(json!({"points":points})).unwrap());
        projection.circles.push(
            serde_json::from_value(json!({
                "body_id":1,"edge_id":id,"edge_key":format!("circle-{id}"),
                "center_model":[center[0],center[1],0.],"normal_model":[0.,0.,1.],
                "center":center,"radius":radius,"closed":closed,"hidden":false
            }))
            .unwrap(),
        );
    }
    let keys = [
        "bottom", "right", "top", "left", "circle-5", "circle-6", "circle-7", "circle-8",
    ];
    let anchor = |id: usize, endpoint: &str| {
        json!({"body_id":1,"edge_id":id,"edge_key":keys[id-1],
        "topology_signature":"feature:1:rectangle-connectivity","endpoint":endpoint,"fallback_point":[999.,999.,999.]})
    };
    let line = |id: usize| {
        json!({"body_id":1,"edge_id":id,"edge_key":keys[id-1],
        "topology_signature":"feature:1:rectangle-connectivity","fallback_start":[999.,999.,999.],"fallback_end":[998.,999.,999.]})
    };
    let circle = |id: usize| {
        json!({"body_id":1,"edge_id":id,"edge_key":keys[id-1],
        "topology_signature":"feature:1:rectangle-connectivity","fallback_center":[999.,999.,999.],
        "fallback_normal":[0.,0.,1.],"fallback_radius":999.,"closed":id!=5})
    };
    let sheet = &mut drawing.sheets[0];
    sheet.format = serde_json::from_value(json!("a2")).unwrap();
    sheet.name = "Advanced annotation export QA".into();
    sheet.title_block.title = "Exact synthetic geometry; not OCCT or live-input proof".into();
    let original = sheet.views[0].clone();
    sheet.views.clear();
    sheet.annotations.clear();
    sheet.bom=vec![serde_json::from_value(json!({"id":1,"item_number":"P-7","body_id":1,
        "part_number":"QA-1","description":"Synthetic test part","quantity":1.,"material":"Steel","finish":""})).unwrap()];
    sheet.bom_table_position = Some([15., 365.]);
    for (index, kind) in KINDS.into_iter().enumerate() {
        let id = index as u64 + 1;
        let x = 75. + index.rem_euclid(3) as f64 * 180.;
        let y = 55. + (index / 3) as f64 * 85.;
        let position = [x + 28., y - 8.];
        let mut view = original.clone();
        view.id = id;
        view.name = kind.into();
        view.position = [x, y];
        view.scale = 1.;
        sheet.views.push(view);
        let mut value = match kind {
            "chamfer_note" => {
                json!({"first":anchor(1,"start"),"second":anchor(1,"end"),"position":position,"length":2.,"angle_deg":45.,"prefix":"2X "})
            }
            "center_line_between_edges" => json!({"first":line(1),"second":line(3),"extension":4.}),
            "automatic_symmetry_axis" => json!({"axis":"both","extension":4.}),
            "bolt_circle_center_line" => {
                json!({"features":[circle(6),circle(7),circle(8)],"extension":3.})
            }
            "arc_length_dimension" => {
                json!({"feature":circle(5),"first":anchor(5,"start"),"second":anchor(5,"end"),"offset":7.,"precision":3})
            }
            "jogged_radius_dimension" => {
                json!({"feature":circle(5),"jog":[x+22.,y-6.],"position":position,"precision":3})
            }
            "datum_feature" => {
                json!({"attachment":{"type":"anchor","reference":anchor(2,"end")},"position":position,"label":"A","target_index":2})
            }
            "gdt_frame" => {
                json!({"attachment":{"type":"circle","reference":circle(6)},"position":position,"characteristic":"position","tolerance":0.1,"diameter_zone":true,"material_condition":"maximum","datums":[{"label":"A","material_condition":"none"},{"label":"B","material_condition":"least"}],"projected_zone":5.,"free_state":true})
            }
            "surface_texture" => {
                json!({"attachment":{"type":"line","reference":line(2)},"position":position,"roughness_ra":3.2,"process":"GRIND","lay":"crossed","machining_allowance":0.3})
            }
            "edge_requirement" => {
                json!({"attachment":line(2),"position":position,"upper_deviation":0.1,"lower_deviation":-0.2,"note":"BREAK"})
            }
            "weld_symbol" => {
                json!({"attachment":line(2),"position":position,"weld_type":"fillet","side":"both","size":3.,"length":10.,"pitch":20.,"contour":"flush","finish":"G","all_around":true,"field_weld":true,"tail":"TIG"})
            }
            "item_balloon" => {
                json!({"attachment":{"type":"anchor","reference":anchor(2,"end")},"position":position,"bom_item_id":1})
            }
            _ => unreachable!(),
        };
        value["id"] = json!(id);
        value["view_id"] = json!(id);
        value["kind"] = json!(kind);
        sheet
            .annotations
            .push(serde_json::from_value(value).unwrap());
    }
    drawing.next_view_id = 13;
    drawing.next_annotation_id = 13;
    drawing.next_bom_item_id = 2;
    let request = projection_request(
        &sheet.views[0],
        &sheet.views,
        &scene,
        &AssemblyDocumentDto::default(),
    )
    .unwrap();
    projection.anchors = occt::drawing_projection_anchors(&scene, &request, &projection).unwrap();
    drawing.validate().unwrap();
    (drawing, scene, projection)
}
