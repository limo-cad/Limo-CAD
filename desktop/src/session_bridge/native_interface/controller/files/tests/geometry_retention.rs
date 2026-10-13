//! Count real retained scene identities across File operations. These are
//! headless lifecycle checks, not a reproduction of switching latency.
use super::*;

fn extrude(fixture: &Fixture, app: &mut App, width: f64) {
    let owner = fixture.owner();
    for (operation, arguments) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":width,"y":8.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap();
    }
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
}

fn snapshot(app: &mut App) -> Value {
    native_viewport::interface_geometry_fixture_snapshot(app.world_mut())
}

fn command(
    fixture: &Fixture,
    app: &mut App,
    services: &NativeServices,
    handle: &NativeInterfaceHandle,
    command: FileCommand,
) {
    execute(app.world_mut(), handle, services, &fixture.owner(), command).unwrap();
    if worker::busy(app.world()) {
        let result = drain(app.world_mut(), services).unwrap();
        assert!(result["render_error"].is_null(), "{result}");
        assert!(result["presentation_error"].is_null(), "{result}");
    }
}

#[test]
fn closing_real_solid_tabs_releases_only_closed_geometry_and_keeps_open_sessions() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let first = fixture.owner();
    extrude(&fixture, &mut app, 12.);
    let first_model = fixture.engine.engine_call("project_export_model", "");
    let initial = snapshot(&mut app);
    let first_rows = initial["sessions"][&first.document_id].clone();
    assert!(
        first_rows.as_array().unwrap().len() > 1,
        "Real solid has body and face metadata"
    );

    let foreign_id = "other-window-retained-model";
    let mut foreign = model_snapshot(&fixture.engine);
    foreign.session_id = foreign_id.into();
    native_viewport::apply_interface_model(app.world_mut(), foreign).unwrap();
    let foreign_rows = snapshot(&mut app)["sessions"][foreign_id].clone();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let baseline = snapshot(&mut app);
    assert_eq!(baseline["sessions"][&first.document_id], first_rows);
    assert_eq!(baseline["sessions"].as_object().unwrap().len(), 2);

    for cycle in 0..3 {
        command(&fixture, &mut app, &services, &handle, FileCommand::New);
        let transient = fixture.owner();
        extrude(&fixture, &mut app, 20. + cycle as f64);
        let both = snapshot(&mut app);
        let transient_rows = both["sessions"][&transient.document_id].clone();
        assert!(transient_rows.as_array().unwrap().len() > 1);
        assert_eq!(both["sessions"].as_object().unwrap().len(), 3);

        command(
            &fixture,
            &mut app,
            &services,
            &handle,
            FileCommand::Activate(first.clone()),
        );
        let first_again = snapshot(&mut app);
        assert_eq!(
            first_again["sessions"][&first.document_id]
                .as_array()
                .unwrap()
                .len(),
            first_rows.as_array().unwrap().len()
        );
        assert_eq!(
            first_again["sessions"][&transient.document_id],
            transient_rows
        );
        assert_eq!(
            fixture.engine.engine_call("project_export_model", ""),
            first_model
        );
        command(
            &fixture,
            &mut app,
            &services,
            &handle,
            FileCommand::Activate(transient.clone()),
        );
        let both = snapshot(&mut app);
        assert_eq!(
            both["sessions"][&first.document_id], first_again["sessions"][&first.document_id],
            "Switching away retains the inactive tab's entities and strong asset handles"
        );
        assert_eq!(both["sessions"].as_object().unwrap().len(), 3);
        assert_eq!(both["sessions"][foreign_id], foreign_rows);
        assert!(both["cache"].get(&first.document_id).is_some());
        assert!(both["cache"].get(&transient.document_id).is_some());

        command(&fixture, &mut app, &services, &handle, FileCommand::Close);
        let token = app
            .world()
            .resource::<Files>()
            .dialog
            .as_ref()
            .unwrap()
            .token;
        assert_eq!(snapshot(&mut app), both);
        command(
            &fixture,
            &mut app,
            &services,
            &handle,
            FileCommand::Cancel(token),
        );
        assert_eq!(snapshot(&mut app), both);

        let stale = current(app.world_mut(), &services, &transient).unwrap();
        fixture.rename(&transient, "Changed before Close").unwrap();
        transition(app.world_mut(), stale, None, Some(true)).unwrap();
        assert!(drain(app.world_mut(), &services)
            .unwrap_err()
            .contains("document changed"));
        assert_eq!(snapshot(&mut app), both);

        command(&fixture, &mut app, &services, &handle, FileCommand::Close);
        let token = app
            .world()
            .resource::<Files>()
            .dialog
            .as_ref()
            .unwrap()
            .token;
        command(
            &fixture,
            &mut app,
            &services,
            &handle,
            FileCommand::Discard(token),
        );
        assert_eq!(fixture.owner(), first);
        assert_eq!(
            fixture.engine.engine_call("project_export_model", ""),
            first_model
        );
        let after_close = snapshot(&mut app);
        println!(
            "native_geometry_lifecycle {}",
            json!({
                "cycle": cycle,
                "retained_engine_tabs": tabs(app.world(), &services, &first).unwrap().len(),
                "retained_cache_sessions": after_close["cache"].as_object().unwrap().len(),
                "retained_geometry_sessions": after_close["sessions"].as_object().unwrap().len(),
                "closed_tab_entities": after_close["sessions"].get(&transient.document_id)
                    .and_then(Value::as_array).map_or(0, Vec::len),
                "expected_foreign_sessions": 1,
            })
        );
        assert!(
            after_close["cache"].get(&transient.document_id).is_none(),
            "Closed tab still has a geometry cache entry: {after_close}"
        );
        assert!(
            after_close["sessions"]
                .get(&transient.document_id)
                .is_none(),
            "Closed tab still owns renderer entities and strong asset handles: {after_close}"
        );
        assert_eq!(
            after_close["sessions"][&first.document_id]
                .as_array()
                .unwrap()
                .len(),
            first_rows.as_array().unwrap().len()
        );
        assert_eq!(after_close["sessions"][foreign_id], foreign_rows);
        assert_eq!(
            after_close["sessions"].as_object().unwrap().len(),
            2,
            "Repeated closed tabs must not grow retained scene ownership"
        );
        assert_eq!(after_close["cache"].as_object().unwrap().len(), 2);
    }
}
