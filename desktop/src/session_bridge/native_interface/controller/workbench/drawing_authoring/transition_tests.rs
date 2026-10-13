//! Unapplied annotation fields must survive document and window transitions.
use super::super::*;
use super::{fields, runtime, tests, Stamp};
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use limo_cad_interface::ControlKey;
use std::sync::{atomic::AtomicBool, Mutex};

#[test]
fn annotation_transition_guard_uses_the_exact_owner_and_includes_new_note_text() {
    let owner = DocumentContext {
        window_id: "main".into(),
        document_id: "drawing".into(),
        epoch: 7,
    };
    let mut editor = runtime::Editor {
        stamp: Some(Stamp {
            owner: owner.clone(),
            revision: 13,
            sheet_id: 1,
        }),
        document: Arc::new(tests::document()),
        ..default()
    };
    let mut world = World::new();
    for new_note in [false, true] {
        if new_note {
            editor.clear();
            editor.tool = Some(runtime::Tool::Note);
            editor.fields = fields::note_creation([20., 30.]);
        } else {
            editor.select(1).unwrap();
        }
        assert!(!editor.dirty(), "Unedited fields do not block a transition");
        fields::edit(
            &mut editor.fields,
            fields::Id::Note,
            &limo_cad_interface::ControlInput::SetValue("Unapplied note".into()),
        )
        .unwrap();
        world.insert_resource(editor);
        assert!(drawing_editor::guard_document_switch(&world, &owner).is_err());
        for field in 0..3 {
            let mut foreign = owner.clone();
            match field {
                0 => foreign.window_id.push_str("-other"),
                1 => foreign.document_id.push_str("-other"),
                _ => foreign.epoch += 1,
            }
            assert!(drawing_editor::guard_document_switch(&world, &foreign).is_ok());
        }
        editor = world.remove_resource::<runtime::Editor>().unwrap();
        assert_eq!(editor.fields[0].text, "Unapplied note");
    }
}

#[test]
fn unapplied_annotation_blocks_file_and_window_transitions_until_explicit_reset() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "drawing_set_document",
            &serde_json::to_value(tests::document()).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let owner = fixture.owner();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    let mut workspace = workspace::DocumentWorkspace::default();
    workspace
        .observe(&fixture.bridge, &fixture.engine, &owner.window_id)
        .unwrap();
    let workspace = Arc::new(Mutex::new(workspace));
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(services.clone());
    files::initialize(app.world_mut(), Arc::clone(&workspace));
    app.insert_resource(Workbench {
        workspace: Workspace::Drawing,
        owner: Some(owner.clone()),
        ..default()
    });
    let mut editor = runtime::Editor {
        stamp: Some(Stamp {
            owner: owner.clone(),
            revision: receipt.revision,
            sheet_id: 1,
        }),
        document: Arc::new(fixture.engine.drawing_snapshot()),
        ..default()
    };
    editor.select(1).unwrap();
    fields::edit(
        &mut editor.fields,
        fields::Id::Note,
        &limo_cad_interface::ControlInput::SetValue("Unapplied close test".into()),
    )
    .unwrap();
    app.insert_resource(editor);

    let handle = NativeInterfaceHandle::new(|| {});
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1000.,
        height: 800.,
    };
    handle
        .present(interface_shell::InterfaceFrame {
            context: owner.clone(),
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    let entity = app
        .world_mut()
        .spawn((
            InterfaceControl::button("file", "Transition"),
            ComputedNode {
                size: Vec2::new(80., 24.),
                inverse_scale_factor: 1.,
                ..default()
            },
            bevy::ui::UiGlobalTransform::from_translation(Vec2::new(60., 42.)),
            bevy::ui::ComputedStackIndex(1),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    let action = |world: &mut World, command| {
        bind_command(world, entity, command).unwrap();
        interface_shell::tests::publish_layout_once(world, handle.clone());
        handle
            .resolve_retained(ControlKey(entity.to_bits()))
            .unwrap()
    };
    let mut controller =
        super::super::super::Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    controller.workspace = workspace;
    let exported =
        || parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let before = exported();
    let mut other = owner.clone();
    other.document_id.push_str("-other-tab");
    for command in [
        files::FileCommand::New,
        files::FileCommand::Activate(other.clone()),
        files::FileCommand::Close,
        files::FileCommand::CloseTab(other),
        files::FileCommand::SaveAllAndExit,
        files::FileCommand::Exit,
    ] {
        let action = action(app.world_mut(), NativeCommand::File(command));
        let error = super::super::super::apply_queued_control(
            app.world_mut(),
            &handle,
            &services,
            &mut controller,
            &action,
        )
        .unwrap_err();
        assert!(
            error.contains("Apply or reset the annotation edit"),
            "{error}"
        );
        assert!(!files::awaiting(app.world()) && !worker::busy(app.world()));
        assert!(!controller.close_pending && !controller.exit_after_receipt);
    }
    let path = std::path::PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap())
        .join("unused-open.limo");
    for discard in [false, true] {
        let error = files::request(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            &json!({"command":"open","path":path,"discard_changes":discard}),
        )
        .unwrap_err();
        assert!(
            error.contains("Apply or reset the annotation edit"),
            "{error}"
        );
    }
    assert_eq!(exported(), before);
    assert_eq!(fixture.owner(), owner);
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap(),
        receipt
    );
    assert_eq!(
        app.world().resource::<runtime::Editor>().fields[0].text,
        "Unapplied close test"
    );

    // Reset is the existing visible, explicit discard action. It remains
    // available after a rejected transition and changes no saved model data.
    let serial = app.world().resource::<runtime::Editor>().serial;
    let reset = action(
        app.world_mut(),
        runtime::native(serial, runtime::Command::Reset),
    );
    super::super::super::apply_queued_control(
        app.world_mut(),
        &handle,
        &services,
        &mut controller,
        &reset,
    )
    .unwrap();
    assert!(!app.world().resource::<runtime::Editor>().dirty());
    assert_eq!(exported(), before);
    super::super::super::request_close(
        app.world_mut(),
        &mut controller,
        &fixture.bridge,
        &fixture.engine,
    )
    .unwrap();
    assert!(controller.close_pending || controller.exit_after_receipt);
}

#[test]
fn native_inbox_mutations_preserve_annotation_draft_and_allow_reads_until_explicit_reset() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "drawing_set_document",
            &serde_json::to_value(tests::document()).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let mut cam: limo_cad_cam::CamDocumentDto = serde_json::from_value(json!({
        "setups":[{"id":1,"name":"Read-only face plan",
            "stock":{"min":{"x":0.,"y":0.,"z":-4.},"max":{"x":12.,"y":8.,"z":0.}},
            "operations":[{"kind":"face","id":1,"name":"Face top","enabled":true,"tool_id":1,
                "bounds":{"min":{"x":0.,"y":0.},"max":{"x":12.,"y":8.}},
                "top_z":0.,"target_z":-1.,"step_over":1.,"step_down":1.,
                "clearance_z":5.,"retract_z":2.,"feed_height_z":1.,
                "cutting":{"spindle_rpm":8000,"feed_xy":600.,"feed_z":150.,"coolant":"off"}}]}],
        "active_setup_id":1,"tools":[{"id":1,"number":1,"name":"2 mm end mill","kind":"flat_end_mill",
            "diameter":2.,"flute_length":8.,"overall_length":30.,"flute_count":2}],
        "next_setup_id":2,"next_tool_id":2,"next_operation_id":2
    }))
    .unwrap();
    cam.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
        Default::default(),
    ));
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cam_set_document",
            &serde_json::to_value(cam).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let owner = fixture.owner();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    let session = fixture
        .bridge
        .session_id_for_window(&owner.window_id)
        .unwrap()
        .unwrap();
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(Workbench {
        workspace: Workspace::Drawing,
        owner: Some(owner.clone()),
        ..default()
    });
    let mut editor = runtime::Editor {
        stamp: Some(Stamp {
            owner: owner.clone(),
            revision: receipt.revision,
            sheet_id: 1,
        }),
        document: Arc::new(fixture.engine.drawing_snapshot()),
        ..default()
    };
    editor.select(1).unwrap();
    fields::edit(
        &mut editor.fields,
        fields::Id::Note,
        &limo_cad_interface::ControlInput::SetValue("Unapplied inbox note".into()),
    )
    .unwrap();
    app.insert_resource(editor);
    let root = crate::session_bridge::session_root()
        .join(&session)
        .join("inbox");
    std::fs::create_dir_all(&root).unwrap();
    let apply = |seq: u64, name: &str, arguments: Value, reason: Option<&str>| {
        let current = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        std::fs::write(
            root.join(format!("{seq}.json")),
            json!({"name":name,"arguments":arguments,"base_generation":current.revision,
                "session_id":session,"window_id":owner.window_id,"document_id":owner.document_id})
            .to_string(),
        )
        .unwrap();
        crate::session_bridge::apply_or_reject_one_inbox_op_with_editor_guards(
            &fixture.bridge,
            &owner.window_id,
            &fixture.engine,
            None,
            Some((&owner.document_id, &session)),
            false,
            reason,
        )
        .unwrap()
    };
    let normal_arguments = json!({"name":"Inbox draft acceptance",
        "camera":{"position":[0.,-100.,100.],"target":[0.,0.,0.],"up":[0.,0.,1.]},
        "visible_body_ids":[],"part_offsets":[]});
    for (seq, name, arguments) in [
        (1, "cad_new_project", json!({})),
        (
            2,
            "cad_load_project_model",
            json!({"model_json":before.clone()}),
        ),
        (3, "upsert_named_view", normal_arguments.clone()),
    ] {
        let reason =
            super::super::super::inbox_model_mutation_reject_reason(app.world(), &owner).unwrap();
        assert!(reason.contains("Apply or reset the annotation edit"));
        let result = apply(seq, name, arguments, Some(&reason));
        assert_eq!(result["applied"], false, "{result}");
        assert_eq!(result["dead_lettered"], true, "{result}");
        assert_eq!(result["reason"], "document_editor_draft", "{result}");
        assert_eq!(result["error"], reason, "{result}");
        assert_eq!(fixture.owner(), owner);
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &owner)
                .unwrap(),
            receipt
        );
        assert_eq!(
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
            before
        );
        assert_eq!(
            app.world().resource::<runtime::Editor>().fields[0].text,
            "Unapplied inbox note"
        );
        assert!(app.world().resource::<runtime::Editor>().dirty());
    }

    // The owning-engine inbox includes a small set of read-only CAM requests.
    // They can progress under the same pending draft without changing its receipt.
    let reason =
        super::super::super::inbox_model_mutation_reject_reason(app.world(), &owner).unwrap();
    let read = apply(4, "cam_plan_setup", json!({"setup_id":1}), Some(&reason));
    assert_eq!(read["applied"], true, "{read}");
    assert_eq!(read["model_changed"], false, "{read}");
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap(),
        receipt
    );
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    assert_eq!(
        app.world().resource::<runtime::Editor>().fields[0].text,
        "Unapplied inbox note"
    );
    assert!(app.world().resource::<runtime::Editor>().dirty());

    let handle = NativeInterfaceHandle::new(|| {});
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1000.,
        height: 800.,
    };
    handle
        .present(interface_shell::InterfaceFrame {
            context: owner.clone(),
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    let entity = app
        .world_mut()
        .spawn((
            InterfaceControl::button("annotation", "Reset annotation"),
            ComputedNode {
                size: Vec2::new(120., 24.),
                inverse_scale_factor: 1.,
                ..default()
            },
            bevy::ui::UiGlobalTransform::from_translation(Vec2::new(80., 42.)),
            bevy::ui::ComputedStackIndex(1),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    let serial = app.world().resource::<runtime::Editor>().serial;
    bind_command(
        app.world_mut(),
        entity,
        runtime::native(serial, runtime::Command::Reset),
    )
    .unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let action = handle
        .resolve_retained(ControlKey(entity.to_bits()))
        .unwrap();
    runtime::reduce(
        app.world_mut(),
        &handle,
        &fixture.engine,
        &fixture.bridge,
        &action,
        serial,
        &runtime::Command::Reset,
    )
    .unwrap();
    assert!(!app.world().resource::<runtime::Editor>().dirty());
    assert!(super::super::super::inbox_model_mutation_reject_reason(app.world(), &owner).is_none());
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    let applied = apply(5, "upsert_named_view", normal_arguments, None);
    assert_eq!(applied["applied"], true, "{applied}");
    assert_eq!(applied["model_changed"], true, "{applied}");
    assert_eq!(fixture.owner(), owner);
    assert_ne!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    let applied = apply(6, "cad_new_project", json!({}), None);
    assert_eq!(applied["applied"], true, "{applied}");
    assert_ne!(fixture.owner(), owner);
}
