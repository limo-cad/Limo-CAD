use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use limo_cad_cam::CamUnits;

mod advanced;
mod geometry;
mod height_picking;
mod hole_picking;
mod linking_points;
mod machines;
mod operations;
mod presets;
mod reorder;
mod reorder_drag;
mod replay;
mod wcs_picking;

fn job() -> CamDocumentDto {
    let mut cam: CamDocumentDto = serde_json::from_value(json!({
        "setups":[{"id":3,"name":"Top setup","work_offset":"g55",
            "stock":{"min":{"x":0.,"y":0.,"z":-12.},"max":{"x":30.,"y":20.,"z":0.}},
            "operations":[{"kind":"face","id":7,"name":"Face stock","enabled":true,"tool_id":5,
                "bounds":{"min":{"x":0.,"y":0.},"max":{"x":30.,"y":20.}},
                "top_z":0.,"target_z":-1.,"step_over":3.,"step_down":1.,
                "clearance_z":8.,"retract_z":2.,"feed_height_z":1.,
                "cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.,"coolant":"flood"}}]}],
        "active_setup_id":3,
        "tools":[{"id":5,"number":1,"name":"6 mm flat end mill","kind":"flat_end_mill",
            "diameter":6.,"flute_length":20.,"overall_length":50.,"flute_count":4}],
        "height_expressions":[{"operation_id":7,
            "clearance":{"reference":"stock_top","offset":8.},
            "retract":{"reference":"stock_top","offset":2.},
            "feed":{"reference":"stock_top","offset":1.},
            "top":{"reference":"stock_top","offset":0.},
            "bottom":{"reference":"stock_top","offset":-1.}}],
        "next_setup_id":4,"next_tool_id":6,"next_operation_id":8
    }))
    .unwrap();
    cam.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
        Default::default(),
    ));
    cam.validate_for_editing().unwrap();
    cam
}
fn set(draft: &mut Draft, path: &str, text: &str) {
    draft
        .fields
        .iter_mut()
        .find(|f| f.path == path)
        .unwrap()
        .text = text.into();
}
fn seed(f: &Fixture) -> CamDocumentDto {
    parse_engine_envelope(
        f.engine
            .engine_call("cam_set_document", &serde_json::to_string(&job()).unwrap()),
    )
    .unwrap();
    parse_engine_envelope(f.engine.engine_call("cam_regenerate_setup", "3")).unwrap();
    f.engine.cam_document_snapshot()
}
fn export(f: &Fixture) -> Value {
    parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap()
}

#[test]
fn native_cam_edits_preserve_unedited_shared_records_and_exact_numbers() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let cam = seed(&f);
    for selected in [
        Selection::Setup(3),
        Selection::Tool(5),
        Selection::Operation(7),
    ] {
        let mut draft = Draft::new(&cam, selected).unwrap();
        set(&mut draft, "/name", "Renamed in Bevy");
        let next = draft.edited(&cam).unwrap();
        let mut expected = serde_json::to_value(&cam).unwrap();
        match selected {
            Selection::Setup(_) => expected["setups"][0]["name"] = json!("Renamed in Bevy"),
            Selection::Tool(_) => expected["tools"][0]["name"] = json!("Renamed in Bevy"),
            Selection::Operation(_) => {
                expected["setups"][0]["operations"][0]["name"] = json!("Renamed in Bevy")
            }
        }
        assert_eq!(serde_json::to_value(next).unwrap(), expected);
    }
    let mut cam = cam;
    cam.units = CamUnits::Inches;
    cam.tools[0].overall_length = f64::from_bits(0x4049800000000001);
    let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
    set(&mut draft, "/diameter", "0.25");
    let next = draft.edited(&cam).unwrap();
    assert_eq!(next.tools[0].diameter, 6.35);
    assert_eq!(
        next.tools[0].overall_length.to_bits(),
        cam.tools[0].overall_length.to_bits()
    );
    assert_eq!(next.height_expressions, cam.height_expressions);
    assert_eq!(next.toolpath_generations, cam.toolpath_generations);
}

#[test]
fn native_cam_invalid_edits_and_referenced_tool_deletion_leave_project_unchanged() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let cam = seed(&f);
    let before = export(&f);
    for text in ["NaN", "inf", "0", "-1"] {
        let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
        set(&mut draft, "/diameter", text);
        assert!(draft.edited(&cam).is_err(), "{text}");
    }
    let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
    set(&mut draft, "/flute_length", "100");
    assert!(draft.edited(&cam).is_err());
    let mut draft = Draft::new(&cam, Selection::Setup(3)).unwrap();
    set(&mut draft, "/work_offset", "G59");
    set(&mut draft, "/work_offset_count", "2");
    assert!(draft.edited(&cam).is_err());
    assert!(remove(&cam, Selection::Tool(5)).is_err());
    assert_eq!(export(&f), before);
}

#[test]
fn native_cam_duplicate_copies_intent_without_fabricating_generation_evidence() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let cam = seed(&f);
    for selected in [Selection::Setup(3), Selection::Operation(7)] {
        let (next, _) = duplicate(&cam, selected).unwrap();
        assert_eq!(next.toolpath_generations, cam.toolpath_generations);
        assert_eq!(next.height_expressions.len(), 2);
        assert_eq!(next.height_expressions[1].operation_id, 8);
        assert_eq!(
            next.height_expressions[1].top,
            cam.height_expressions[0].top
        );
        assert_eq!(next.tools, cam.tools);
        assert_eq!(next.next_operation_id, 9);
        let cleaned = remove(&next, Selection::Operation(8)).unwrap();
        assert_eq!(cleaned.toolpath_generations, cam.toolpath_generations);
        assert_eq!(cleaned.height_expressions, cam.height_expressions);
    }
    let (copy, selected) = duplicate(&cam, Selection::Tool(5)).unwrap();
    assert_eq!(selected, Selection::Tool(6));
    assert_eq!(copy.tools[1].number, None);
    assert_eq!(copy.tools[1].diameter, cam.tools[0].diameter);
}

#[test]
fn native_cam_commits_have_exact_history_and_reject_stale_receipts() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let cam = seed(&f);
    let before = export(&f);
    let receipt = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let mut draft = Draft::new(&cam, Selection::Operation(7)).unwrap();
    set(&mut draft, "/cutting/feed_xy", "900");
    let next = draft.edited(&cam).unwrap();
    let args = serde_json::to_value(next).unwrap();
    f.bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "cam_set_document",
            &args,
            || Ok(()),
        )
        .unwrap();
    let after = export(&f);
    assert_ne!(after, before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), after);
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "cam_set_document",
            &args,
            || Ok(())
        )
        .is_err());
    assert_eq!(export(&f), after);
    let editor = Editor {
        owner: Some(receipt.owner.clone()),
        revision: receipt.revision,
        ..default()
    };
    assert!(ensure_current(&editor, &receipt.owner, receipt.revision + 1).is_err());
    parse_engine_envelope(
        f.bridge
            .with_project_session_transition("main", &f.engine, || {
                f.engine.create_project_session("tab-b")
            }),
    )
    .unwrap();
    let switched = f.owner();
    assert!(ensure_current(&editor, &switched, receipt.revision).is_err());
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "cam_set_document",
            &args,
            || Ok(())
        )
        .is_err());
    assert!(f.engine.cam_document_snapshot().setups.is_empty());
}

#[test]
fn native_cam_panel_exposes_retained_fields_without_implicit_edits() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let cam = seed(&f);
    let before = export(&f);
    let services = NativeServices {
        engine: f.engine.clone(),
        bridge: f.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    for height in [600., 860.] {
        synchronize(world, camera, &services, &f.owner(), height, 280., true).unwrap();
        {
            let mut editor = world.resource_mut::<Editor>();
            editor.tab = Tab::Tools;
            editor.draft = Some(Draft::new(&cam, Selection::Tool(5)).unwrap());
        }
        synchronize(world, camera, &services, &f.owner(), height, 280., true).unwrap();
        assert!(world
            .query::<&InterfaceControl>()
            .iter(world)
            .any(|c| c.label == "Name" && c.role == "textbox"));
        assert!(world
            .query::<&InterfaceControl>()
            .iter(world)
            .any(|c| c.label == "Apply" && !c.disabled));
    }
    assert_eq!(export(&f), before);
}

#[test]
fn native_cam_choices_show_tool_names_and_cycle_only_existing_values() {
    let cam = job();
    let tools = choices(&cam, "/tool_id").unwrap();
    assert_eq!(tools[0].label, "T1 · 6 mm flat end mill");
    assert_eq!(choose(&tools, "5", &ControlInput::Click).unwrap(), "5");
    assert!(choose(&tools, "5", &ControlInput::SetValue("99".into())).is_err());
    let offsets = choices(&cam, "/work_offset").unwrap();
    assert_eq!(
        choose(&offsets, "g55", &ControlInput::Click).unwrap(),
        "g56"
    );
    assert_eq!(
        choose(&offsets, "g55", &ControlInput::Key(KeyChord::plain("Home"))).unwrap(),
        "g54"
    );
    assert_eq!(
        choose(&offsets, "g55", &ControlInput::Key(KeyChord::plain("End"))).unwrap(),
        "g59"
    );
}

#[test]
fn native_cam_history_refresh_keeps_selection_and_discards_old_drafts() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let mut cam = seed(&f);
    cam.tools[0].name = "Edited tool".into();
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "cam_set_document",
            &serde_json::to_value(&cam).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let services = NativeServices {
        engine: f.engine.clone(),
        bridge: f.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    synchronize(world, camera, &services, &f.owner(), 860., 280., true).unwrap();
    {
        let mut editor = world.resource_mut::<Editor>();
        editor.tab = Tab::Tools;
        let mut draft = Draft::new(&cam, Selection::Tool(5)).unwrap();
        set(&mut draft, "/name", "Unapplied stale edit");
        editor.draft = Some(draft);
    }
    let old_owner = f.owner();
    f.bridge
        .apply_native_history(&f.engine, &old_owner, false, || Ok(()))
        .unwrap();
    assert_ne!(f.owner().epoch, old_owner.epoch);
    synchronize(world, camera, &services, &f.owner(), 860., 280., true).unwrap();
    let editor = world.resource::<Editor>();
    assert_eq!(editor.tab, Tab::Tools);
    assert_eq!(editor.draft.as_ref().unwrap().selection, Selection::Tool(5));
    assert!(!editor.draft.as_ref().unwrap().dirty());
    assert_eq!(
        editor.draft.as_ref().unwrap().record["name"],
        "6 mm flat end mill"
    );
    assert!(ensure_current(editor, &old_owner, editor.revision).is_err());
}

#[test]
fn native_cam_first_items_require_explicit_choices_and_regenerate_from_real_solid() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    for (op, args) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":25.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
        ),
    ] {
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), op, &args, || Ok(()))
            .unwrap();
    }
    let scene = f.engine.viewport_snapshot().2;
    let empty = f.engine.cam_document_snapshot();
    let mut draft = creation::draft(
        Tab::Setups,
        &empty,
        creation::Context::new(&scene, &empty).unwrap(),
    );
    assert!(
        creation::create(&draft, &empty).is_err(),
        "A setup must not guess its model body"
    );
    set(&mut draft, "/body_id", &scene.bodies[0].id.0.to_string());
    let (cam, selected) = creation::create(&draft, &empty).unwrap();
    assert_eq!(selected, Selection::Setup(1));
    assert_eq!(
        cam.setups[0].wcs.origin,
        limo_cad_cam::Point3Dto::new(-2., -2., 7.)
    );
    assert_eq!(
        cam.setups[0].stock.min,
        limo_cad_cam::Point3Dto::new(0., 0., -9.)
    );
    assert_eq!(
        cam.setups[0].stock.max,
        limo_cad_cam::Point3Dto::new(44., 29., 0.)
    );
    assert_eq!(cam.setups[0].body_ids, vec![scene.bodies[0].id]);
    assert!(cam.setups[0].operations.is_empty() && cam.tools.is_empty());
    let mut draft = creation::draft(
        Tab::Tools,
        &cam,
        creation::Context::new(&scene, &cam).unwrap(),
    );
    assert!(
        creation::create(&draft, &cam).is_err(),
        "Cutter geometry/cutting data must be entered"
    );
    for (path, value) in [
        ("/name", "End mill"),
        ("/diameter", "6"),
        ("/flute_length", "20"),
        ("/overall_length", "50"),
        ("/spindle_rpm", "12000"),
        ("/feed_xy", "800"),
        ("/feed_z", "200"),
    ] {
        set(&mut draft, path, value);
    }
    let (cam, selected) = creation::create(&draft, &cam).unwrap();
    assert_eq!(selected, Selection::Tool(1));
    let mut draft = creation::draft(
        Tab::Toolpaths,
        &cam,
        creation::Context::new(&scene, &cam).unwrap(),
    );
    assert!(creation::create(&draft, &cam).is_err());
    set(&mut draft, "/native/create/setup_id", "1");
    set(&mut draft, "/tool_id", "1");
    creation::seed_choices(&mut draft, &cam).unwrap();
    let (created_cam, selected) = creation::create(&draft, &cam).unwrap();
    assert_eq!(selected, Selection::Operation(1));
    set(&mut draft, "/native/heights/top/offset", "0.2");
    assert!(creation::create(&draft, &cam).is_err());
    let cam = created_cam;
    assert_eq!(
        cam.height_expressions[0].top.reference,
        limo_cad_cam::CamHeightReferenceDto::StockTop
    );
    assert_eq!(cam.height_expressions[0].top.offset, 0.);
    assert_eq!(cam.height_expressions[0].clearance.offset, 11.);
    assert_eq!(
        cam.height_expressions[0].bottom.as_ref().unwrap().offset,
        0.
    );
    assert!(cam.toolpath_generations.is_empty());
    let mut stock_draft = creation::draft(
        Tab::Setups,
        &empty,
        creation::Context::new(&scene, &empty).unwrap(),
    );
    set(
        &mut stock_draft,
        "/body_id",
        &scene.bodies[0].id.0.to_string(),
    );
    set(&mut stock_draft, "/z_max", "20");
    let (mut tall_stock_cam, _) = creation::create(&stock_draft, &empty).unwrap();
    tall_stock_cam.tools = cam.tools.clone();
    tall_stock_cam.next_tool_id = cam.next_tool_id;
    let mut face_draft = creation::draft(
        Tab::Toolpaths,
        &tall_stock_cam,
        creation::Context::new(&scene, &tall_stock_cam).unwrap(),
    );
    set(&mut face_draft, "/native/create/setup_id", "1");
    set(&mut face_draft, "/tool_id", "1");
    creation::seed_choices(&mut face_draft, &tall_stock_cam).unwrap();
    let (tall_stock_cam, _) = creation::create(&face_draft, &tall_stock_cam).unwrap();
    let face = serde_json::to_value(&tall_stock_cam.setups[0].operations[0]).unwrap();
    assert_eq!(face["top_z"], 0.);
    assert_eq!(face["target_z"], -20.);
    assert_eq!(face["clearance_z"], 10.);
    assert_eq!(face["retract_z"], 5.);
    assert_eq!(face["feed_height_z"], 5.);
    let before = export(&f);
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "cam_set_document",
            &serde_json::to_value(&cam).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let saved = export(&f);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), saved);
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "cam_regenerate_setup",
            &json!({"setup_id":1}),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(
        f.engine.cam_document_snapshot().toolpath_generations.len(),
        1
    );
    assert_eq!(f.engine.viewport_snapshot().2, scene);
}
