use super::*;
use limo_cad_cam::{CamLinkingDto, CamToolKind, Point2Dto};
use limo_cad_solid::SolidSceneDto;
#[path = "linking_pick.rs"]
mod pick;

fn cam() -> CamDocumentDto {
    let mut cam = job();
    cam.height_expressions.clear();
    cam.setups[0].operations[0] = serde_json::from_value(json!({
        "kind":"contour2d","id":7,"name":"Linked contour","tool_id":5,"enabled":true,
        "path":[{"x":2.,"y":2.},{"x":20.,"y":2.},{"x":20.,"y":10.},{"x":2.,"y":10.}],
        "closed":true,"compensation":"outside","top_z":0.,"bottom_z":-2.,"step_down":1.,
        "clearance_z":8.,"retract_z":2.,"feed_height_z":1.,
        "cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.}
    }))
    .unwrap();
    cam.linking.push(CamLinkingDto {
        operation_id: 7,
        ..Default::default()
    });
    cam.validate_for_editing().unwrap();
    cam
}

fn operation_draft(cam: &CamDocumentDto, scene: &SolidSceneDto) -> Draft {
    let mut draft = Draft::new(cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, cam, scene, &[]).unwrap();
    set(&mut draft, "/native/ui/operation_section", "linking");
    draft
}

fn edit(draft: &mut Draft, cam: &CamDocumentDto, path: &str, value: &str) {
    set(draft, path, value);
    operation_editor::changed(draft, cam, path).unwrap();
}

fn row(key: &str) -> String {
    operation_geometry::points::cursor(&format!("/native/linking/{key}"))
}

#[test]
fn native_cam_linking_point_rows_are_lazy_clean_and_preserve_exact_inches() {
    let mut cam = cam();
    cam.units = CamUnits::Inches;
    cam.linking[0].predrill_positions = (0..32)
        .map(|i| Point2Dto::new(f64::from(i) + 0.1, 0.3))
        .collect();
    cam.linking[0].entry_positions = vec![Point2Dto::new(0.1, 0.2)];
    cam.linking[0].exit_positions = vec![Point2Dto::new(0.3, 0.4)];
    let mut draft = operation_draft(&cam, &SolidSceneDto::default());
    let fields = draft.fields.len();
    assert!(!draft
        .fields
        .iter()
        .any(|f| f.path == "/native/linking/predrill_positions/31/x"));
    edit(&mut draft, &cam, &row("predrill_positions"), "32");
    assert_eq!(draft.fields.len(), fields + 3);
    assert!(!draft.dirty());
    assert_eq!(draft.edited(&cam).unwrap(), cam);
    assert!(operation_editor::visible(
        &draft,
        &row("predrill_positions")
    ));
    assert!(!operation_editor::visible(
        &draft,
        "/native/linking/predrill_positions/0/x"
    ));
    assert!(operation_editor::visible(
        &draft,
        "/native/linking/predrill_positions/31/x"
    ));
    edit(&mut draft, &cam, "/native/linking/high_feed", "60");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.linking[0].predrill_positions,
        cam.linking[0].predrill_positions
    );
    assert_eq!(
        next.linking[0].entry_positions,
        cam.linking[0].entry_positions
    );
    assert_eq!(
        next.linking[0].exit_positions,
        cam.linking[0].exit_positions
    );
    edit(
        &mut draft,
        &cam,
        "/native/linking/predrill_positions/31/x",
        "0.5",
    );
    let next = draft.edited(&cam).unwrap();
    let mut expected = cam.clone();
    expected.linking[0].high_feed = 1524.;
    expected.linking[0].predrill_positions[31].x = 12.7;
    assert_eq!(next, expected);
    assert_eq!(
        next.linking[0].predrill_positions[31].y.to_bits(),
        0.3_f64.to_bits()
    );
}

#[test]
fn native_cam_linking_manual_points_require_coordinates_and_use_shared_limits() {
    let cam = cam();
    let mut draft = operation_draft(&cam, &SolidSceneDto::default());
    edit(
        &mut draft,
        &cam,
        "/native/linking/entry_positions/count",
        "1",
    );
    assert!(
        draft.edited(&cam).is_err(),
        "A new point is never silently placed at zero"
    );
    edit(&mut draft, &cam, "/native/linking/entry_positions/0/x", "5");
    edit(&mut draft, &cam, "/native/linking/entry_positions/0/y", "6");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.linking[0].entry_positions,
        vec![Point2Dto::new(5., 6.)]
    );
    assert_eq!(next.setups, cam.setups);
    assert_eq!(next.height_expressions, cam.height_expressions);
    for (path, text) in [
        ("/native/linking/entry_positions/count", "2"),
        ("/native/linking/predrill_positions/count", "33"),
        ("/native/linking/exit_positions/count", "-1"),
        ("/native/linking/entry_positions/0/x", "NaN"),
    ] {
        let old = form::text(&draft, path).unwrap().to_owned();
        set(&mut draft, path, text);
        assert!(
            draft.edited(&cam).is_err(),
            "Invalid {path}={text} must not apply"
        );
        set(&mut draft, path, &old);
    }
    edit(
        &mut draft,
        &cam,
        "/native/linking/entry_positions/count",
        "0",
    );
    assert_eq!(draft.edited(&cam).unwrap(), cam);
}

fn drill(id: u64, enabled: bool) -> CamOperationDto {
    serde_json::from_value(json!({"kind":"drill","id":id,"name":format!("Drill {id}"),
        "enabled":enabled,"tool_id":6,"points":[{"x":0.1,"y":0.3}],
        "holes":[{"point":{"x":0.3,"y":0.1},"top_z":0.,"bottom_z":-3.,"axis":[0.,0.,1.],"face_key":"11:2"}],
        "top_z":0.,"bottom_z":-3.,"clearance_z":8.,"retract_z":2.,"feed_height_z":1.,
        "cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.}})).unwrap()
}

#[test]
fn native_cam_predrill_candidates_are_only_prior_enabled_drill_centers_and_copy_exactly() {
    let mut cam = cam();
    cam.units = CamUnits::Inches;
    let mut tool = cam.tools[0].clone();
    tool.id = 6;
    tool.number = Some(2);
    tool.kind = CamToolKind::Drill;
    tool.point_angle_degrees = Some(118.);
    cam.tools.push(tool);
    cam.next_tool_id = 7;
    cam.next_operation_id = 9;
    let contour = cam.setups[0].operations[0].clone();
    cam.setups[0].operations = vec![drill(1, true), drill(2, false), contour, drill(8, true)];
    cam.validate_for_editing().unwrap();
    let mut draft = operation_draft(&cam, &SolidSceneDto::default());
    edit(
        &mut draft,
        &cam,
        "/native/linking/predrill_positions/count",
        "1",
    );
    let source = "/native/linking/predrill_positions/0/candidate";
    let field = draft
        .fields
        .iter()
        .find(|field| field.path == source)
        .unwrap();
    assert_eq!(
        field
            .options
            .as_ref()
            .unwrap()
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        vec!["manual", "drill:1:0", "drill:1:1"]
    );
    assert_eq!(field.text, "manual");
    edit(&mut draft, &cam, source, "drill:1:0");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.linking[0].predrill_positions[0].x.to_bits(),
        0.1_f64.to_bits()
    );
    assert_eq!(
        next.linking[0].predrill_positions[0].y.to_bits(),
        0.3_f64.to_bits()
    );
    assert_eq!(next.setups, cam.setups);
    edit(&mut draft, &cam, source, "manual");
    assert_eq!(
        draft.edited(&cam).unwrap(),
        next,
        "Switching a copied inch point to manual must preserve both canonical axes"
    );
    edit(&mut draft, &cam, source, "drill:1:1");
    edit(
        &mut draft,
        &cam,
        "/native/linking/predrill_positions/0/x",
        "0.25",
    );
    assert_eq!(form::text(&draft, source).unwrap(), "manual");
    assert_eq!(
        draft.edited(&cam).unwrap().linking[0].predrill_positions[0].x,
        6.35
    );
    assert_eq!(
        draft.edited(&cam).unwrap().linking[0].predrill_positions[0]
            .y
            .to_bits(),
        0.1_f64.to_bits(),
        "Editing copied X must preserve the untouched canonical Y in inches"
    );
    set(&mut draft, source, "drill:8:0");
    assert!(operation_editor::changed(&mut draft, &cam, source).is_err());
    assert!(draft.edited(&cam).is_err());
}

#[test]
fn native_cam_preferred_vertex_candidates_use_only_setup_bodies_and_the_setup_frame() {
    let mut cam = cam();
    cam.setups[0].body_ids = vec![limo_cad_core::BodyId(11)];
    cam.setups[0].wcs = serde_json::from_value(json!({"origin":{"x":10.,"y":20.,"z":3.},
        "x_axis":[0.,1.,0.],"y_axis":[-1.,0.,0.],"z_axis":[0.,0.,1.]}))
    .unwrap();
    let body = |id| {
        json!({"id":id,"name":format!("Body {id}"),"feature_id":1,
        "mesh":{"positions":[12.,23.,7.,12.,23.,7.,13.,24.,7.],"normals":[],"indices":[]},"faces":[],"edges":[]})
    };
    let scene = serde_json::from_value(json!({"bodies":[body(11),body(12)],"errors":[]})).unwrap();
    let mut draft = operation_draft(&cam, &scene);
    edit(
        &mut draft,
        &cam,
        "/native/linking/entry_positions/count",
        "1",
    );
    let source = "/native/linking/entry_positions/0/candidate";
    let options = draft
        .fields
        .iter()
        .find(|f| f.path == source)
        .unwrap()
        .options
        .as_ref()
        .unwrap();
    assert_eq!(
        options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        vec!["manual", "vertex:11:0", "vertex:11:2"]
    );
    edit(&mut draft, &cam, source, "vertex:11:0");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.linking[0].entry_positions,
        vec![Point2Dto::new(3., -2.)]
    );
    assert_eq!(
        next.setups, cam.setups,
        "Picking a station must not change geometry references"
    );
    let reopened = operation_draft(&next, &SolidSceneDto::default());
    assert_eq!(form::text(&reopened, source).unwrap(), "manual");
    assert_eq!(
        reopened.edited(&next).unwrap(),
        next,
        "Copied points survive without their source vertex"
    );
}

#[test]
fn native_cam_linking_point_visibility_matches_operation_support_and_legacy_mode() {
    let mut cam = cam();
    let mut draft = operation_draft(&cam, &SolidSceneDto::default());
    let count = "/native/linking/entry_positions/count";
    assert!(operation_editor::visible(&draft, count));
    edit(&mut draft, &cam, "/native/linking/mode", "legacy");
    assert!(!operation_editor::visible(&draft, count));
    assert!(!operation_editor::visible(&draft, &row("entry_positions")));
    assert!(draft.edited(&cam).unwrap().linking.is_empty());
    let mut record = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    record["kind"] = json!("adaptive3d");
    record["enabled"] = json!(false);
    record["parameters"] = json!({"optimal_load":1.,"maximum_stepdown":1.,"minimum_cutting_radius":0.5,"radial_stock_to_leave":0.,"axial_stock_to_leave":0.,"tolerance":0.2,"ramp_angle_degrees":3.,"maximum_ramp_stepdown":1.,"ramp_feed":150.,"linking_feed":800.,"stay_down_distance":20.,"machine_cavities":false});
    cam.setups[0].operations[0] = serde_json::from_value(record).unwrap();
    cam.linking[0].exit_positions = vec![Point2Dto::new(0.1, 0.2)];
    let mut draft = operation_draft(&cam, &SolidSceneDto::default());
    assert!(!draft
        .fields
        .iter()
        .any(|f| f.path.starts_with("/native/linking/exit_positions")));
    assert!(operation_editor::visible(&draft, count));
    edit(
        &mut draft,
        &cam,
        "/native/linking/entry_positions/count",
        "1",
    );
    edit(&mut draft, &cam, "/native/linking/entry_positions/0/x", "5");
    edit(&mut draft, &cam, "/native/linking/entry_positions/0/y", "6");
    assert_eq!(
        draft.edited(&cam).unwrap().linking[0].exit_positions,
        cam.linking[0].exit_positions
    );
    let face = job();
    let face = operation_draft(&face, &SolidSceneDto::default());
    assert!(!face
        .fields
        .iter()
        .any(|f| f.path.starts_with("/native/linking/entry_positions")));
}

#[test]
fn native_cam_linking_point_edits_cannot_bypass_closed_contour_or_predrill_validation() {
    let mut cam = cam();
    let mut draft = operation_draft(&cam, &SolidSceneDto::default());
    edit(&mut draft, &cam, "/native/linking/ramp_enabled", "true");
    edit(&mut draft, &cam, "/native/linking/ramp_type", "predrill");
    assert!(draft
        .edited(&cam)
        .unwrap_err()
        .contains("earlier enabled drilling"));
    if let CamOperationDto::Contour2d { closed, .. } = &mut cam.setups[0].operations[0] {
        *closed = false;
    }
    let mut draft = operation_draft(&cam, &SolidSceneDto::default());
    edit(
        &mut draft,
        &cam,
        "/native/linking/entry_positions/count",
        "1",
    );
    edit(&mut draft, &cam, "/native/linking/entry_positions/0/x", "5");
    edit(&mut draft, &cam, "/native/linking/entry_positions/0/y", "6");
    assert!(draft.edited(&cam).unwrap_err().contains("closed contour"));
}
