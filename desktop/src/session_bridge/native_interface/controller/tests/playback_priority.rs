use super::*;

#[test]
fn playback_status_poll_cannot_starve_an_authorized_modeling_step() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut controls, handle, _) = prepare(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(
        controls
            .world_mut()
            .remove_resource::<Controller>()
            .unwrap(),
    );
    app.insert_resource(
        controls
            .world_mut()
            .remove_resource::<NativeServices>()
            .unwrap(),
    );
    app.insert_resource(handle.clone());
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    app.init_resource::<Messages<NativeHostInput>>();
    let services = app.world().resource::<NativeServices>().clone();
    worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    let owner = fixture.owner();
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &owner, "solid")
        .unwrap();
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    app.world_mut().resource_mut::<Controller>().cached_session = Some(session.clone());
    let root = crate::session_bridge::session_root().join(&session);
    fs::create_dir_all(root.join("controls")).unwrap();
    fs::create_dir_all(root.join("inbox")).unwrap();
    let revision = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    fs::write(root.join("inbox/1.json"), json!({
        "name":"sketch_begin", "arguments":{"name":"Single step","plane":{"type":"origin_plane","plane":"xy"}},
        "base_generation":revision,"session_id":session,"window_id":"main",
        "document_id":owner.document_id
    }).to_string()).unwrap();
    for command in ["pause", "step"] {
        presentation::request(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            &json!({"command":command}),
        )
        .unwrap();
    }
    let status_path = root.join("controls/40-1.request.json");
    fs::write(
        &status_path,
        json!({"id":"40-1","expires_ms":now_ms()+30_000,
        "ui":{"action":"presentation","command":"status"}})
        .to_string(),
    )
    .unwrap();
    let default_status_path = root.join("controls/40-2.request.json");
    fs::write(
        &default_status_path,
        json!({"id":"40-2","expires_ms":now_ms()+30_000,
            "ui":{"action":"presentation"}})
        .to_string(),
    )
    .unwrap();
    app.world_mut()
        .resource_scope(|world, mut state: Mut<Controller>| {
            update_inner(world, &handle, &services, &mut state).unwrap();
            assert!(
                state.polled_control.is_none(),
                "Read-only polling must not claim the worker before a permitted step"
            );
        });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let result = loop {
        if let Some(outcome) = worker::poll(app.world_mut(), &services) {
            break outcome.value.unwrap();
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(result["result"]["applied"], true);
    assert_eq!(result["result"]["name"], "sketch_begin");
    assert!(result["render_error"].is_null());
    assert!(result["publication_error"].is_null());
    assert!(
        status_path.exists() && default_status_path.exists(),
        "The polling request must remain available for its own reply"
    );
    assert_eq!(
        presentation::gate(app.world_mut(), &owner),
        presentation::Gate::Waiting,
        "Exactly one modeling operation consumes the single-step credit"
    );
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        Some(revision + 1)
    );
}
