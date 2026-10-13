use super::*;

fn operation_draft(cam: &CamDocumentDto) -> Draft {
    let mut draft = Draft::new(cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(
        &mut draft,
        cam,
        &limo_cad_solid::SolidSceneDto::default(),
        &[],
    )
    .unwrap();
    draft
}

#[test]
fn native_cam_operation_sections_do_not_mutate_or_dirty_the_document() {
    let cam = job();
    let mut draft = operation_draft(&cam);
    for section in ["heights", "linking", "parameters"] {
        set(&mut draft, "/native/ui/operation_section", section);
        assert!(
            !draft.dirty(),
            "Navigating a form section is not a CAM edit"
        );
        assert_eq!(draft.edited(&cam).unwrap(), cam);
    }
}

#[test]
fn native_cam_operation_height_and_link_feed_edits_convert_document_units_once() {
    let mut cam = job();
    cam.units = CamUnits::Inches;
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/native/heights/bottom/offset", "-0.125");
    set(&mut draft, "/native/heights/clearance/reference", "retract");
    set(&mut draft, "/native/heights/clearance/offset", "0.25");
    set(&mut draft, "/native/linking/mode", "custom");
    set(&mut draft, "/native/linking/high_feed", "60");
    let next = draft.edited(&cam).unwrap();
    let operation = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(operation["target_z"], -3.175);
    assert_eq!(operation["clearance_z"], 8.35);
    assert_eq!(next.height_expressions[0].clearance.offset, 6.35);
    assert_eq!(next.linking[0].high_feed, 1524.);
    assert_eq!(
        operation["cutting"],
        serde_json::to_value(&cam.setups[0].operations[0]).unwrap()["cutting"]
    );
    assert_eq!(next.units, CamUnits::Inches);
}

#[test]
fn native_cam_height_edits_preserve_untouched_canonical_precision_in_inches() {
    let mut cam = job();
    cam.units = CamUnits::Inches;
    cam.height_expressions[0].bottom.as_mut().unwrap().offset = -0.1;
    let mut operation = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    operation["target_z"] = json!(-0.1);
    cam.setups[0].operations[0] = serde_json::from_value(operation).unwrap();
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/native/heights/clearance/offset", "0.5");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        next.height_expressions[0].bottom,
        cam.height_expressions[0].bottom
    );
    assert_eq!(
        next.height_expressions[0].top,
        cam.height_expressions[0].top
    );
    assert_eq!(
        next.height_expressions[0].feed,
        cam.height_expressions[0].feed
    );
    assert_eq!(
        next.height_expressions[0].retract,
        cam.height_expressions[0].retract
    );
    cam.height_expressions.clear();
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/native/heights/clearance/value", "0.5");
    let next = draft.edited(&cam).unwrap();
    let original = serde_json::to_value(&cam.setups[0].operations[0]).unwrap()["target_z"]
        .as_f64()
        .unwrap();
    let edited = serde_json::to_value(&next.setups[0].operations[0]).unwrap()["target_z"]
        .as_f64()
        .unwrap();
    assert_eq!(edited.to_bits(), original.to_bits());
}

#[test]
fn native_cam_clearance_only_edit_preserves_independent_absolute_chamfer_chain_tops() {
    let mut cam = job();
    cam.height_expressions.clear();
    cam.tools[0].kind = limo_cad_cam::CamToolKind::ChamferMill;
    cam.tools[0].point_angle_degrees = Some(90.);
    let path = json!([{"x":5.,"y":5.},{"x":20.,"y":5.},{"x":20.,"y":15.},{"x":5.,"y":15.}]);
    cam.setups[0].operations[0]=serde_json::from_value(json!({"id":7,"name":"Independent bevels","kind":"chamfer2d","tool_id":5,"enabled":true,"path":path,"closed":true,"top_z":0.,"chamfer_width":0.5,"tip_offset":0.1,"wall_side":"outside","clearance_z":8.,"retract_z":2.,"feed_height_z":1.,"cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.},"additional_chains":[{"path":path,"closed":true,"top_z":-5.,"chamfer_width":0.5,"wall_side":"outside"}]})).unwrap();
    cam.validate_for_editing().unwrap();
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/native/heights/clearance/value", "9");
    let next = draft.edited(&cam).unwrap();
    let original = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
    let actual = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
    assert_eq!(actual["additional_chains"], original["additional_chains"]);
    assert_eq!(actual["top_z"], original["top_z"]);
    assert_eq!(actual["clearance_z"], 9.);
    assert!(next.height_expressions.is_empty());
}

#[test]
fn native_cam_keyed_heights_and_linking_survive_shared_regeneration_and_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let cam = seed(&f);
    let before = export(&f);
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/native/heights/bottom/offset", "-2");
    set(&mut draft, "/native/linking/mode", "custom");
    set(&mut draft, "/native/linking/high_feed", "4200");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(
        serde_json::to_value(&next.setups[0].operations[0]).unwrap()["target_z"],
        -2.
    );
    assert_eq!(
        next.height_expressions[0].bottom.as_ref().unwrap().offset,
        -2.
    );
    assert_eq!(next.linking[0].high_feed, 4200.);
    assert_eq!(
        next.toolpath_generations, cam.toolpath_generations,
        "Editing must not fabricate current toolpaths"
    );
    assert_eq!(next.tools, cam.tools);
    assert_eq!(next.setups[0].machine, cam.setups[0].machine);
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "cam_set_document",
            &serde_json::to_value(&next).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let edited = export(&f);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), edited);
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "cam_regenerate_operation",
            &json!({"operation_id":7}),
            || Ok(()),
        )
        .unwrap();
    let generated = f.engine.cam_document_snapshot();
    assert_eq!(
        serde_json::to_value(&generated.setups[0].operations[0]).unwrap()["target_z"],
        -2.,
        "Regeneration must retain the edited associative depth"
    );
    assert_eq!(generated.height_expressions, next.height_expressions);
    assert_eq!(generated.linking, next.linking);
}

#[test]
fn native_cam_height_modes_modify_only_the_selected_keyed_record() {
    let (cam, _) = duplicate(&job(), Selection::Operation(7)).unwrap();
    let untouched = cam.height_expressions[1].clone();
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/native/heights/mode", "absolute");
    set(&mut draft, "/native/heights/bottom/value", "-3");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.height_expressions, vec![untouched]);
    assert_eq!(
        serde_json::to_value(&next.setups[0].operations[0]).unwrap()["target_z"],
        -3.
    );
    assert_eq!(next.setups[0].operations[1], cam.setups[0].operations[1]);
    assert_eq!(next.setups[0].stock, cam.setups[0].stock);
    set(&mut draft, "/native/heights/bottom/value", "-30");
    assert!(draft.edited(&cam).is_err());
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/native/heights/top/reference", "retract");
    assert!(
        draft.edited(&cam).is_err(),
        "A forward height dependency must not be baked or accepted"
    );
    set(&mut draft, "/native/heights/top/reference", "stock_top");
    set(&mut draft, "/native/heights/clearance/offset", "NaN");
    assert!(draft.edited(&cam).is_err());
}

#[test]
fn native_cam_drill_cycle_changes_clear_only_inapplicable_cycle_fields() {
    let mut cam = job();
    cam.height_expressions.clear();
    let base = json!({"id":7,"name":"Holes","kind":"drill","tool_id":5,"enabled":true,
        "points":[{"x":10.,"y":10.}],"top_z":0.,"bottom_z":-5.,"clearance_z":8.,
        "retract_z":2.,"feed_height_z":1.,"cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.}});
    for (original, destination) in [
        (
            json!({"cycle":"chip_breaking","peck_depth":2.,"peck_retract":0.5,"drill_tip_through":true,"breakthrough_depth":0.2}),
            "reaming",
        ),
        (
            json!({"cycle":"tapping_right","thread_pitch":1.,"floating_tap_holder":true}),
            "drill",
        ),
        (json!({"cycle":"reaming","feed_out":150.}), "drill"),
    ] {
        let mut value = base.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(original.as_object().unwrap().clone());
        cam.setups[0].operations[0] = serde_json::from_value(value).unwrap();
        cam.validate_for_editing().unwrap();
        let mut draft = operation_draft(&cam);
        assert_eq!(
            draft.edited(&cam).unwrap(),
            cam,
            "Opening an existing cycle must preserve it"
        );
        set(&mut draft, "/cycle", destination);
        let next = draft.edited(&cam).unwrap();
        let actual = serde_json::to_value(&next.setups[0].operations[0]).unwrap();
        let before = serde_json::to_value(&cam.setups[0].operations[0]).unwrap();
        assert_eq!(actual["cycle"], destination);
        for path in [
            "points",
            "holes",
            "cutting",
            "top_z",
            "bottom_z",
            "clearance_z",
        ] {
            assert_eq!(
                actual[path], before[path],
                "Cycle change must preserve {path}"
            );
        }
        assert!(actual["peck_depth"].is_null());
        assert!(actual["peck_retract"].is_null());
        assert!(actual["thread_pitch"].is_null());
        assert_eq!(actual["floating_tap_holder"], false);
        assert_eq!(actual["drill_tip_through"], false);
    }
    let mut draft = operation_draft(&cam);
    set(&mut draft, "/cycle", "tapping_right");
    assert!(
        draft.edited(&cam).is_err(),
        "Switching cycles must not invent tap pitch or holder confirmation"
    );
    set(&mut draft, "/thread_pitch", "1.25");
    set(&mut draft, "/floating_tap_holder", "true");
    assert!(draft.edited(&cam).is_ok());
}

#[test]
fn native_cam_operation_parameter_edits_preserve_geometry_and_unedited_records() {
    for (kind, extra, path, new_value) in [
        (
            "face",
            json!({"bounds":{"min":{"x":0.,"y":0.},"max":{"x":30.,"y":20.}},"target_z":-1.,"step_down":1.,"step_over":3.}),
            "/safe_distance",
            "6",
        ),
        (
            "contour2d",
            json!({"path":[{"x":0.,"y":0.},{"x":20.,"y":0.},{"x":20.,"y":10.},{"x":0.,"y":10.}],"bottom_z":-2.,"step_down":1.,"compensation":"outside"}),
            "/lead_in",
            "7",
        ),
        (
            "pocket2d",
            json!({"outline":[{"x":0.,"y":0.},{"x":20.,"y":0.},{"x":20.,"y":10.},{"x":0.,"y":10.}],"bottom_z":-2.,"step_down":1.,"step_over":3.}),
            "/direction",
            "conventional",
        ),
        (
            "drill",
            json!({"points":[{"x":10.,"y":10.}],"bottom_z":-2.}),
            "/dwell_seconds",
            "0.75",
        ),
        (
            "thread",
            json!({"points":[{"x":10.,"y":10.}],"bottom_z":-2.,"pitch":1.25,"major_diameter":10.,"minor_diameter":8.}),
            "/hand",
            "left",
        ),
        (
            "chamfer2d",
            json!({"path":[{"x":0.,"y":0.},{"x":20.,"y":0.},{"x":20.,"y":10.},{"x":0.,"y":10.}],"chamfer_width":0.5,"tip_offset":0.1,"wall_side":"outside"}),
            "/tip_offset",
            "0.2",
        ),
        (
            "adaptive3d",
            json!({"bottom_z":-2.,"parameters":{"optimal_load":1.,"maximum_stepdown":1.,"minimum_cutting_radius":0.5,"radial_stock_to_leave":0.,"axial_stock_to_leave":0.,"tolerance":0.2,"ramp_angle_degrees":3.,"maximum_ramp_stepdown":1.,"ramp_feed":150.,"linking_feed":800.,"stay_down_distance":20.,"machine_cavities":false}}),
            "/parameters/tolerance",
            "0.1",
        ),
        (
            "flat3d",
            json!({"bottom_z":-2.,"geometry":null,"parameters":{"step_over":2.,"radial_stock_to_leave":0.,"axial_stock_to_leave":0.,"tolerance":0.05,"direction":"climb","stay_down_distance":20.}}),
            "/parameters/direction",
            "conventional",
        ),
    ] {
        let mut cam = job();
        cam.height_expressions.clear();
        let mut value = json!({"id":7,"name":"Existing path","kind":kind,"tool_id":5,"enabled":false,"top_z":0.,"clearance_z":8.,"retract_z":2.,"feed_height_z":1.,"cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.,"coolant":"flood"}});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        cam.setups[0].operations[0] = serde_json::from_value(value).unwrap();
        let mut expected = serde_json::to_value(&cam).unwrap();
        let mut draft = operation_draft(&cam);
        set(&mut draft, path, new_value);
        let field = draft
            .fields
            .iter()
            .find(|field| field.path == path)
            .unwrap();
        let changed = if matches!(field.kind, InputKind::Choice) {
            json!(new_value)
        } else {
            json!(new_value.parse::<f64>().unwrap())
        };
        *expected["setups"][0]["operations"][0]
            .pointer_mut(path)
            .unwrap() = changed;
        assert_eq!(
            serde_json::to_value(draft.edited(&cam).unwrap()).unwrap(),
            expected,
            "{kind} must preserve geometry and every other field"
        );
    }
}
