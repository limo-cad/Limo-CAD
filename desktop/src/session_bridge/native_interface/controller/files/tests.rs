use super::*;
use crate::session_bridge::native_interface::tests::Fixture;
use std::time::{Duration, Instant};

mod geometry_retention;
mod lifecycle;
mod sketch_close;
mod workspace;

pub(super) fn setup(fixture: &Fixture) -> (App, NativeServices, NativeInterfaceHandle) {
    let mut app = native_viewport::interface_scene_fixture();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let handle = NativeInterfaceHandle::new(|| {});
    let mut workspace = DocumentWorkspace::default();
    workspace
        .observe(&services.bridge, &services.engine, "main")
        .unwrap();
    initialize(app.world_mut(), Arc::new(Mutex::new(workspace)));
    app.insert_resource(services.clone())
        .insert_resource(handle.clone());
    worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    (app, services, handle)
}
pub(super) fn drain(world: &mut World, services: &NativeServices) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(outcome) = worker::poll(world, services) {
            let value = outcome.value?;
            if !worker::busy(world) {
                return Ok(value);
            }
        }
        assert!(Instant::now() < deadline, "Native file worker timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
pub(super) fn path(name: &str) -> PathBuf {
    let root = PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    root.join(name)
}

#[test]
fn new_documents_reset_the_camera_and_tabs_restore_their_own_views() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let first = fixture.owner();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let camera = |world: &World| native_viewport::interface_view_snapshot(world).1;
    let first_view = native_viewport::ViewportCamera {
        position: [20., 30., 40.],
        target: [1., 2., 3.],
        ..default()
    };
    native_viewport::apply_interface_view(
        app.world_mut(),
        &first.document_id,
        Some(first_view),
        None,
    )
    .unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::New,
    )
    .unwrap();
    let result = drain(app.world_mut(), &services).unwrap();
    assert!(result["presentation_error"].is_null(), "{result}");
    let second = fixture.owner();
    assert_eq!(camera(app.world()).target, [0.; 3]);
    assert!(
        (Vec3::from_array(camera(app.world()).position).length()
            - Vec3::from_array(native_viewport::ViewportCamera::default().position).length())
        .abs()
            < 1e-4
    );
    let second_view = native_viewport::ViewportCamera {
        position: [-100., 20., 40.],
        target: [10., 0., 5.],
        ..default()
    };
    native_viewport::apply_interface_view(
        app.world_mut(),
        &second.document_id,
        Some(second_view),
        None,
    )
    .unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &second,
        FileCommand::Activate(first.clone()),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(camera(app.world()), first_view);
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::Activate(second.clone()),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(camera(app.world()), second_view);
    execute(
        app.world_mut(),
        &handle,
        &services,
        &second,
        FileCommand::Close,
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(camera(app.world()), first_view);
    assert!(!app
        .world()
        .resource::<Files>()
        .views
        .contains_key(&second.document_id));
}

#[test]
fn save_open_tabs_and_exit_protect_inactive_edits_and_recognize_saved_checkpoints() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let first = fixture.owner();
    request(
        app.world_mut(),
        &handle,
        &services,
        &first,
        &json!({"command":"rename","name":"First design"}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let file = path("first.limo");
    request(
        app.world_mut(),
        &handle,
        &services,
        &first,
        &json!({"command":"save","path":file}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert!(file.is_file());
    assert!(!tabs(app.world(), &services, &first).unwrap()[0].dirty);

    fixture.rename(&first, "Unsaved first").unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::New,
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let second = fixture.owner();
    let mut controller = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    controller.workspace = app.world().resource::<Files>().workspace.clone();
    super::super::request_close(
        app.world_mut(),
        &mut controller,
        &services.bridge,
        &services.engine,
    )
    .unwrap();
    assert!(
        controller.close_pending,
        "An inactive unsaved tab must prevent immediate exit"
    );
    assert!(!controller.exit_after_receipt);

    execute(
        app.world_mut(),
        &handle,
        &services,
        &second,
        FileCommand::Activate(first.clone()),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Unsaved first");
    request(
        app.world_mut(),
        &handle,
        &services,
        &first,
        &json!({"command":"open","path":file}),
    )
    .unwrap();
    let token = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .token;
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::Cancel(token),
    )
    .unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Unsaved first");
    request(
        app.world_mut(),
        &handle,
        &services,
        &first,
        &json!({"command":"open","path":file}),
    )
    .unwrap();
    let token = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .token;
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::Discard(token),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "First design");
    assert_ne!(
        fixture.owner(),
        first,
        "Loading retires the old document incarnation"
    );
    controller.close_pending = false;
    super::super::request_close(
        app.world_mut(),
        &mut controller,
        &services.bridge,
        &services.engine,
    )
    .unwrap();
    assert!(!controller.close_pending);
    assert!(controller.exit_after_receipt);
}

#[test]
fn save_and_continue_closes_only_after_success_and_failed_save_preserves_dialog() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    let file = path("saved.limo");
    request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"save","path":file}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    fixture.rename(&owner, "Latest edit").unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Close,
    )
    .unwrap();
    let token = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .token;
    let receipt = current(app.world(), &services, &owner).unwrap();
    save(
        app.world_mut(),
        receipt,
        path("missing/failure.limo"),
        false,
        Some(Intent::Close),
    )
    .unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert_eq!(fixture.owner(), owner);
    assert_eq!(
        app.world()
            .resource::<Files>()
            .dialog
            .as_ref()
            .unwrap()
            .token,
        token
    );
    assert_eq!(fixture.engine.document_snapshot().name, "Latest edit");
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::SaveContinue(token),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_ne!(fixture.owner().document_id, owner.document_id);
    let next = fixture.owner();
    request(
        app.world_mut(),
        &handle,
        &services,
        &next,
        &json!({"command":"open","path":file}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Latest edit");
}

#[test]
fn stale_confirmation_and_chooser_cannot_overwrite_newer_work() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    fixture.rename(&owner, "Draft").unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Close,
    )
    .unwrap();
    let dialog = app.world().resource::<Files>().dialog.clone().unwrap();
    fixture.rename(&owner, "Newer work").unwrap();
    assert!(execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Discard(dialog.token)
    )
    .is_err());
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Cancel(dialog.token),
    )
    .unwrap();
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<Files>().picker = Some(Picker {
        receipt: dialog.receipt,
        kind: PickerKind::Project {
            save: true,
            continuation: None,
        },
        result: Mutex::new(receive),
    });
    let destination = path("stale.limo");
    send.send(Some(destination.clone())).unwrap();
    poll(app.world_mut(), &services).unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert!(!destination.exists());
    assert_eq!(fixture.engine.document_snapshot().name, "Newer work");
}

#[test]
fn cancelled_or_disconnected_picker_releases_the_interface_without_mutation() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _) = setup(&fixture);
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    for disconnected in [false, true] {
        let (send, receive) = mpsc::channel();
        app.world_mut().resource_mut::<Files>().picker = Some(Picker {
            receipt: receipt.clone(),
            kind: PickerKind::Project {
                save: false,
                continuation: None,
            },
            result: Mutex::new(receive),
        });
        if !disconnected {
            send.send(None).unwrap();
        }
        drop(send);
        assert_eq!(poll(app.world_mut(), &services).is_err(), disconnected);
        assert!(!awaiting(app.world()));
        assert!(!worker::busy(app.world()));
        assert_eq!(
            current(app.world(), &services, &fixture.owner()).unwrap(),
            receipt
        );
    }
}

#[test]
fn explicit_save_refuses_overwrite_and_invalid_paths_without_marking_clean() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    fixture.rename(&owner, "Keep these edits").unwrap();
    let destination = path("existing.limo");
    std::fs::write(&destination, b"existing bytes").unwrap();
    assert!(request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"save","path":destination})
    )
    .is_err());
    for invalid in [PathBuf::from("relative.limo"), path("wrong.txt")] {
        request(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            &json!({"command":"save","path":invalid}),
        )
        .unwrap();
        assert!(drain(app.world_mut(), &services).is_err());
    }
    assert_eq!(std::fs::read(&destination).unwrap(), b"existing bytes");
    assert!(tabs(app.world(), &services, &owner).unwrap()[0].dirty);
}

#[test]
fn save_all_before_exit_writes_every_dirty_tab_before_requesting_close() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let first = fixture.owner();
    for i in 0..2 {
        let owner = fixture.owner();
        request(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            &json!({"command":"save","path":path(&format!("exit-{i}.limo"))}),
        )
        .unwrap();
        drain(app.world_mut(), &services).unwrap();
        fixture
            .rename(&owner, &format!("Saved on exit {i}"))
            .unwrap();
        if i == 0 {
            execute(
                app.world_mut(),
                &handle,
                &services,
                &owner,
                FileCommand::New,
            )
            .unwrap();
            drain(app.world_mut(), &services).unwrap();
        }
    }
    let owner = fixture.owner();
    let start = execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::SaveAllAndExit,
    )
    .unwrap();
    assert_eq!(start["saving_before_exit"], true);
    let result = drain(app.world_mut(), &services).unwrap();
    assert_eq!(result["request_exit"], true);
    let all = tabs(app.world(), &services, &fixture.owner()).unwrap();
    assert_eq!(
        all.len(),
        2,
        "Saving before Exit retains the tabs until actual close"
    );
    assert!(all.iter().all(|tab| !tab.dirty));
    assert_eq!(fixture.owner(), first);
    for i in 0..2 {
        let owner = fixture.owner();
        request(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            &json!({"command":"open","path":path(&format!("exit-{i}.limo"))}),
        )
        .unwrap();
        drain(app.world_mut(), &services).unwrap();
        assert_eq!(
            fixture.engine.document_snapshot().name,
            format!("Saved on exit {i}")
        );
    }
}

#[test]
fn bootstrap_rebinding_republishes_snapshot_ownership_before_input() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let _directory = Fixture::new();
    let services = NativeServices::default();
    let bootstrap = services
        .bridge
        .native_document_context("main", &services.engine)
        .unwrap();
    assert_eq!(bootstrap.document_id, crate::state::BOOTSTRAP_SESSION_ID);
    services
        .bridge
        .publish_native_document(&services.engine, &bootstrap, "solid")
        .unwrap();
    let session = services
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let mut workspace = DocumentWorkspace::default();
    let current = workspace
        .observe(&services.bridge, &services.engine, "main")
        .unwrap();
    assert_ne!(current.owner.document_id, bootstrap.document_id);
    assert_eq!(
        services
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap(),
        session
    );
    for name in ["heartbeat.json", "focus.json"] {
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(path(&format!("{session}/{name}"))).unwrap())
                .unwrap();
        assert_eq!(metadata["document_id"], current.owner.document_id, "{name}");
        assert_eq!(
            metadata["project_session_id"], current.owner.document_id,
            "{name}"
        );
        assert_eq!(metadata["generation"], current.revision, "{name}");
    }
    let lease: Value = serde_json::from_slice(
        &std::fs::read(path(&format!(
            "_ui/processes/{}.json",
            services.bridge.process_instance_id
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        lease["windows"][0]["active_document_id"],
        current.owner.document_id
    );
    assert_eq!(lease["windows"][0]["active_session_id"], session);
    assert!(services
        .bridge
        .with_native_document_owner(&services.engine, &bootstrap, || Ok(()))
        .is_err());
    assert_eq!(
        services.bridge.engine_revision_for_window("main").unwrap(),
        Some(current.revision)
    );
}

#[test]
fn failed_bootstrap_publication_is_retried_without_replacing_the_new_design() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let _directory = Fixture::new();
    let services = NativeServices::default();
    let bootstrap = services
        .bridge
        .native_document_context("main", &services.engine)
        .unwrap();
    services
        .bridge
        .publish_native_document(&services.engine, &bootstrap, "solid")
        .unwrap();
    let session = services
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let blocked = path(&format!("{session}/focus.json"));
    std::fs::remove_file(&blocked).unwrap();
    std::fs::create_dir(&blocked).unwrap();
    let mut workspace = DocumentWorkspace::default();
    assert!(workspace
        .observe(&services.bridge, &services.engine, "main")
        .is_err());
    let owner = services
        .bridge
        .native_document_context("main", &services.engine)
        .unwrap();
    assert!(workspace
        .summaries(&services.bridge, &owner)
        .unwrap()
        .is_empty());
    let document = services.engine.active_project_session_id();
    assert_ne!(document, bootstrap.document_id);
    std::fs::remove_dir(&blocked).unwrap();
    let current = workspace
        .observe(&services.bridge, &services.engine, "main")
        .unwrap();
    assert_eq!(current.owner.document_id, document);
    assert_eq!(
        services
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap(),
        session
    );
    let metadata: Value = serde_json::from_slice(&std::fs::read(blocked).unwrap()).unwrap();
    assert_eq!(metadata["document_id"], document);
    assert_eq!(metadata["generation"], current.revision);
}

#[test]
fn real_bootstrap_first_new_retains_both_models_and_distinct_mcp_sessions() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let _directory = Fixture::new();
    let services = NativeServices::default();
    let mut workspace = DocumentWorkspace::default();
    let first = workspace
        .observe(&services.bridge, &services.engine, "main")
        .unwrap();
    assert_ne!(first.owner.document_id, crate::state::BOOTSTRAP_SESSION_ID);
    let session = services
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let edited = services
        .bridge
        .apply_native_mutation(
            &services.engine,
            &first.owner,
            "cad_set_document_name",
            &json!({"name":"Retained first tab"}),
            || Ok(()),
        )
        .unwrap();
    let receipt = DocumentReceipt {
        owner: edited.context,
        revision: edited.engine_revision,
    };
    let second = workspace
        .new_tab_guarded(&services.bridge, &services.engine, &receipt, || Ok(()))
        .unwrap();
    assert_ne!(
        services
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap(),
        session
    );
    assert_eq!(services.engine.document_snapshot().name, "Untitled");
    let all = workspace
        .summaries(&services.bridge, &second.owner)
        .unwrap();
    assert_eq!(all.len(), 2);
    assert!(all[0].dirty);
    assert!(!all[1].dirty);
    workspace
        .activate_guarded(
            &services.bridge,
            &services.engine,
            &second,
            &first.owner,
            || Ok(()),
        )
        .unwrap();
    assert_eq!(
        services.engine.document_snapshot().name,
        "Retained first tab"
    );
    assert_eq!(
        services
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap(),
        session
    );
}

#[test]
fn closing_an_inactive_dirty_tab_prompts_for_that_document_and_keeps_the_others() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let first = fixture.owner();
    fixture.rename(&first, "Keep this design").unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::New,
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let second = fixture.owner();
    fixture.rename(&second, "Other design").unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &second,
        FileCommand::CloseTab(first.clone()),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.owner(), first);
    let dialog = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(dialog.receipt.owner, first);
    assert_eq!(tabs(app.world(), &services, &first).unwrap().len(), 2);
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::Cancel(dialog.token),
    )
    .unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Keep this design");
    assert_eq!(tabs(app.world(), &services, &first).unwrap().len(), 2);
}

#[test]
fn drawing_part_and_second_instance_document_switches_record_non_negative_durations() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let _timing = crate::session_bridge::native_interface::switch_timing::record_for_test();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let drawing_tab = fixture.owner();
    let drawing: limo_cad_sketch::DrawingDocumentDto = serde_json::from_value(json!({
        "sheets":[
            {"id":1,"name":"Sheet A","format":"a4","orientation":"landscape"},
            {"id":2,"name":"Sheet B","format":"a4","orientation":"landscape"}
        ],
        "active_sheet_id":1,
        "next_sheet_id":3
    }))
    .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &drawing_tab,
            "drawing_set_document",
            &serde_json::to_value(&drawing).unwrap(),
            || Ok(()),
        )
        .unwrap();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    workbench::observe_document(app.world_mut(), &drawing_tab);
    workbench::execute(
        app.world_mut(),
        &workbench::Command::Workspace(workbench::Workspace::Drawing),
    )
    .unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &drawing_tab,
        FileCommand::New,
    )
    .unwrap();
    let created = drain(app.world_mut(), &services).unwrap();
    assert!(created["render_error"].is_null(), "{created}");
    let part_tab = fixture.owner();
    assert_eq!(
        workbench::workspace(app.world()),
        workbench::Workspace::Solid
    );
    execute(
        app.world_mut(),
        &handle,
        &services,
        &part_tab,
        FileCommand::Activate(drawing_tab),
    )
    .unwrap();
    let drawing_switch = drain(app.world_mut(), &services).unwrap();
    assert!(drawing_switch["render_error"].is_null(), "{drawing_switch}");
    assert_eq!(
        workbench::workspace(app.world()),
        workbench::Workspace::Drawing
    );
    execute(
        app.world_mut(),
        &handle,
        &services,
        &fixture.owner(),
        FileCommand::Activate(part_tab),
    )
    .unwrap();
    let part_switch = drain(app.world_mut(), &services).unwrap();
    assert!(part_switch["render_error"].is_null(), "{part_switch}");
    assert_eq!(
        workbench::workspace(app.world()),
        workbench::Workspace::Solid
    );

    let second_engine = std::sync::Arc::new(crate::state::AppState::new());
    let second_bridge = std::sync::Arc::new(crate::session_bridge::SessionBridgeState::default());
    let bound = second_bridge.with_project_session_transition("instance-2", &second_engine, || {
        second_engine.bind_project_session("part-a")
    });
    crate::session_bridge::parse_engine_envelope(bound).expect("second instance bind");
    let mut second_documents =
        crate::session_bridge::native_interface::workspace::DocumentWorkspace::default();
    let first_document = second_documents
        .observe(&second_bridge, &second_engine, "instance-2")
        .unwrap();
    let other_document = second_documents
        .new_tab_guarded(&second_bridge, &second_engine, &first_document, || Ok(()))
        .unwrap();
    second_documents
        .activate_guarded(
            &second_bridge,
            &second_engine,
            &other_document,
            &first_document.owner,
            || Ok(()),
        )
        .unwrap();

    let timing =
        crate::session_bridge::native_interface::switch_timing::measurement_fields("instance-2");
    let measurement_path = crate::session_bridge::native_interface::switch_timing::samples_path()
        .with_file_name("switch-measurement.json");
    std::fs::write(
        &measurement_path,
        serde_json::to_vec_pretty(&timing).unwrap(),
    )
    .unwrap();
    let loaded: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&measurement_path).unwrap()).unwrap();
    for key in [
        "drawing_switch_ms",
        "part_switch_ms",
        "instance_document_switch_ms",
    ] {
        let duration = loaded[key]
            .as_f64()
            .unwrap_or_else(|| panic!("missing {key}"));
        assert!(duration.is_finite() && duration >= 0.0, "{key}={duration}");
    }
}
