use super::*;
use limo_cad_cam::{CamToolKind, Point2Dto, PostDialect};

fn thread_job(with_bore: bool, drill_bottom: f64) -> CamDocumentDto {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../cam/fixtures/lead-clearance.json")).unwrap();
    let mut doc: CamDocumentDto = serde_json::from_value(
        data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "chip-breaking-0.1-retract")
            .unwrap()["document"]
            .clone(),
    )
    .unwrap();
    doc.tools[0].diameter = 5.0;
    doc.tools[0].point_angle_degrees = Some(118.0);
    if let CamOperationDto::Drill {
        enabled,
        bottom_z,
        points,
        ..
    } = &mut doc.setups[0].operations[0]
    {
        *enabled = with_bore;
        *bottom_z = drill_bottom;
        *points = vec![Point2Dto::new(20.0, 20.0)];
    }
    let mut mill = doc.tools[0].clone();
    mill.id = 2;
    mill.number = Some(2);
    mill.name = "Thread mill".into();
    mill.kind = CamToolKind::ThreadMill;
    mill.diameter = 3.0;
    mill.point_angle_degrees = None;
    doc.tools.push(mill);
    doc.setups[0].operations.push(serde_json::from_value(serde_json::json!({
        "id":2,"kind":"thread","name":"Thread","enabled":true,"tool_id":2,
        "points":[{"x":20.0,"y":20.0}],"holes":[],"top_z":0.0,"bottom_z":-3.0,
        "pitch":1.0,"major_diameter":6.0,"minor_diameter":5.0,"hand":"right","direction":"climb","radial_passes":1,
        "clearance_z":15.0,"retract_z":2.0,"feed_height_z":1.0,
        "cutting":{"spindle_rpm":6000,"feed_xy":300.0,"feed_z":100.0,"coolant":"off"}
    })).unwrap());
    doc.next_operation_id = 3;
    doc.next_tool_id = 3;
    doc.post_defaults.dialect = PostDialect::LinuxCnc;
    doc.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
        doc.post_defaults.clone(),
    ));
    doc
}

#[test]
fn current_thread_cannot_export_without_an_actually_cleared_entry() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(thread_job(false, -6.0)).unwrap();
    manager.cam_regenerate_setup(1).unwrap();
    assert_eq!(
        manager
            .cam_toolpath_statuses()
            .unwrap()
            .iter()
            .find(|s| s.operation_id == 2)
            .unwrap()
            .state,
        CamToolpathStateDto::Current
    );
    let request = CamPostRequestDto {
        setup_id: 1,
        post: None,
        program_name: None,
    };
    assert!(manager
        .cam_post(request.clone())
        .unwrap_err()
        .to_string()
        .contains("thread-tool entry"));
    assert!(manager
        .cam_post_events(1)
        .unwrap_err()
        .to_string()
        .contains("thread-tool entry"));
    manager.set_cam_document(thread_job(true, -6.0)).unwrap();
    manager.cam_regenerate_setup(1).unwrap();
    assert!(
        manager
            .cam_post(request.clone())
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("UNVERIFIED")),
        "no CAD target must never be called clearance verified"
    );

    manager.set_cam_document(thread_job(true, -3.0)).unwrap();
    manager.cam_regenerate_setup(1).unwrap();
    assert!(manager
        .cam_post(request)
        .unwrap_err()
        .to_string()
        .contains("thread-tool entry"));
}

#[test]
fn moving_bore_after_thread_keeps_motion_current_but_blocks_unverified_nc_entry() {
    let mut manager = SketchManager::new();
    manager.set_cam_document(thread_job(true, -6.0)).unwrap();
    manager.cam_regenerate_setup(1).unwrap();
    let mut reordered = manager.cam_document();
    reordered.setups[0].operations.swap(0, 1);
    manager.set_cam_document(reordered).unwrap();
    assert!(manager
        .cam_toolpath_statuses()
        .unwrap()
        .iter()
        .all(|s| s.state == CamToolpathStateDto::Current));
    let error = manager
        .cam_post(CamPostRequestDto {
            setup_id: 1,
            post: None,
            program_name: None,
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("thread-tool entry"), "{error}");
}

#[test]
fn individual_regeneration_retains_enabled_incoming_face_evidence() {
    let mut doc = thread_job(true, -6.0);
    doc.setups[0].operations.truncate(1);
    let stock = doc.setups[0].stock;
    let mut face_tool = doc.tools[0].clone();
    face_tool.id = 3;
    face_tool.number = Some(3);
    face_tool.kind = CamToolKind::FaceMill;
    face_tool.diameter = 20.0;
    face_tool.point_angle_degrees = None;
    doc.tools.push(face_tool);
    doc.setups[0].operations.insert(0, serde_json::from_value(serde_json::json!({
        "id":3,"kind":"face","name":"Proven face","enabled":true,"tool_id":3,
        "bounds":{"min":{"x":stock.min.x,"y":stock.min.y},"max":{"x":stock.max.x,"y":stock.max.y}},
        "top_z":stock.max.z,"target_z":-1.0,"step_over":8.0,"step_down":1.0,"safe_distance":3.0,"direction":"climb",
        "clearance_z":15.0,"retract_z":2.0,"feed_height_z":1.0,
        "cutting":{"spindle_rpm":6000,"feed_xy":300.0,"feed_z":100.0,"coolant":"off"}
    })).unwrap());
    if let CamOperationDto::Drill {
        top_z,
        feed_height_z,
        ..
    } = &mut doc.setups[0].operations[1]
    {
        *top_z = -1.0;
        *feed_height_z = -0.5;
    }
    doc.next_operation_id = 4;
    doc.next_tool_id = 4;
    let mut manager = SketchManager::new();
    manager.set_cam_document(doc.clone()).unwrap();
    manager.cam_regenerate_operation(1).unwrap();
    if let CamOperationDto::Face { enabled, .. } = &mut doc.setups[0].operations[0] {
        *enabled = false;
    }
    manager.set_cam_document(doc).unwrap();
    assert!(manager
        .cam_regenerate_operation(1)
        .unwrap_err()
        .to_string()
        .contains("incoming stock top"));
}
