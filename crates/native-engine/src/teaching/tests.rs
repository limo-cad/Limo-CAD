use super::*;
use serde_json::json;

fn measurement_frame() -> Frame {
    // Two exact vertex selections on one owned edge. The retained report is
    // expressed in original model coordinates, independently of any assembly.
    serde_json::from_value(json!({
        "caption":"Measure", "scene":{"bodies":[{
            "id":1,"name":"Stock","feature_id":2,"faces":[],
            "edges":[{"id":11,"key":"edge:0","points":[
                {"x":0,"y":0,"z":0},{"x":8,"y":0,"z":0}
            ]}],
            "mesh":{"positions":[0,0,0,8,0,0,0,2,0],
                "normals":[0,0,1,0,0,1,0,0,1],"indices":[0,1,2]}
        }],"errors":[]},
        "inspection":{"measurement":{
            "request":{"selections":[
                {"body_id":1,"target":{"kind":"vertex","edge_id":11,"end":false}},
                {"body_id":1,"target":{"kind":"vertex","edge_id":11,"end":true}}
            ]},
            "report":{"selections":[{},{}],"distance":8,
                "closest_points":[[0,0,0],[8,0,0]]},
            "body_poses":[{"body_id":1,"translation":[0,0,0],"rotation":[0,0,0,1]}]
        }}
    }))
    .unwrap()
}

fn translated_assembly() -> limo_cad_sketch::AssemblySolutionDto {
    serde_json::from_value(json!({"solved":true,"diagnostics":[],
        "body_poses":[{"body_id":1,"translation":[1000,0,0],"rotation":[0,0,0,1]}]
    }))
    .unwrap()
}

#[test]
fn mixed_assembly_and_measurement_placements_fail_closed() {
    let mut frame = measurement_frame();
    validate_frames(std::slice::from_ref(&frame)).unwrap();
    frame.assembly = Some(translated_assembly());
    // Both individual validators accept their own data. Combining them would
    // draw the mesh at X=1000 while the measurement leaders remain at X=0/8.
    assembly::validate(&frame).unwrap();
    frame.inspection.as_ref().unwrap().validate(&frame).unwrap();
    let point = assembly::body_poses(frame.assembly.as_ref(), 1)[0].transform_point([0., 0., 0.]);
    assert_eq!(point, [1000., 0., 0.]);
    assert_eq!(
        frame
            .inspection
            .as_ref()
            .unwrap()
            .measurement
            .as_ref()
            .unwrap()
            .report
            .closest_points
            .unwrap(),
        [[0., 0., 0.], [8., 0., 0.]]
    );
    assert!(validate_frames(&[frame])
        .unwrap_err()
        .contains("assembly and inspection"));
}

#[test]
fn inspection_only_and_assembly_only_frames_remain_valid_together() {
    let inspection = measurement_frame();
    let original = validate_frames(std::slice::from_ref(&inspection)).unwrap();
    assert_eq!(original.center, [4., 1., 0.]);
    let mut assembly = inspection.clone();
    assembly.inspection = None;
    assembly.assembly = Some(translated_assembly());
    let placed = validate_frames(std::slice::from_ref(&assembly)).unwrap();
    assert_eq!(placed.center, [1004., 1., 0.]);
    assert_eq!(placed.radius, original.radius);
    validate_frames(&[inspection, assembly.clone()]).unwrap();
    let restored: Frame = serde_json::from_slice(&serde_json::to_vec(&assembly).unwrap()).unwrap();
    assert!(restored.inspection.is_none());
    assert_eq!(validate_frames(&[restored]).unwrap().center, placed.center);
}

#[test]
fn mixed_modes_reject_even_empty_inspection_or_identity_placement() {
    let frame = Frame {
        inspection: Some(Inspection::default()),
        assembly: Some(limo_cad_sketch::AssemblySolutionDto {
            solved: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(validate_frames(&[frame])
        .unwrap_err()
        .contains("assembly and inspection"));
    validate_frames(&[Frame::default()]).unwrap();
}
