use super::*;
use crate::session_bridge::{apply_one_inbox_op, inbox_dir};
use std::fs;

fn revision(fixture: &Fixture) -> u64 {
    fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap()
}

fn enqueue(fixture: &Fixture, seq: u64, name: &str, arguments: Value, base: u64) -> Value {
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let request = json!({
        "name":name,"arguments":arguments,"base_generation":base,
        "session_id":session,"window_id":"main","document_id":fixture.owner().document_id,
    });
    fs::create_dir_all(inbox_dir(&session)).unwrap();
    fs::write(
        inbox_dir(&session).join(format!("{seq}.json")),
        request.to_string(),
    )
    .unwrap();
    request
}

#[test]
fn attached_construction_visibility_preserves_edit_undo_and_redo() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    let original = model(&fixture);
    edit(
        &fixture,
        &owner,
        "solid_edit_extrude",
        json!({"feature_id":original["extrudes"][0]["feature_id"],"extrude":{
            "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":25.},"taper_angle_deg":0.,
            "flip":false,"target_body_ids":[]}}),
    );
    enqueue(
        &fixture,
        1,
        "construction_set_visibility",
        json!({"visible":false,"sketch_names":["Sketch1"]}),
        revision(&fixture),
    );
    let receipt = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(receipt["applied"], true, "{receipt}");
    let undone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(model(&fixture)["extrudes"], original["extrudes"]);
    assert_eq!(
        model(&fixture)["visibility"]["hidden_sketch_names"],
        json!(["Sketch1"])
    );
    enqueue(
        &fixture,
        2,
        "construction_set_visibility",
        json!({"visible":true,"sketch_names":["Sketch1"]}),
        revision(&fixture),
    );
    let receipt = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(receipt["applied"], true, "{receipt}");
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
        .unwrap();
    let restored = model(&fixture);
    assert_eq!(restored["extrudes"][0]["extent"]["distance"], 25.0);
    assert_eq!(restored["visibility"]["hidden_sketch_names"], json!([]));
}

fn generated_job(fixture: &Fixture, owner: &DocumentContext) -> (Value, Value) {
    let mut cam: limo_cad_cam::CamDocumentDto = serde_json::from_value(json!({
        "setups":[{"id":1,"name":"Face test","stock":{"min":{"x":0.,"y":0.,"z":-4.},"max":{"x":12.,"y":8.,"z":0.}},
            "operations":[{"kind":"face","id":1,"name":"Face top","enabled":true,"tool_id":1,
                "bounds":{"min":{"x":0.,"y":0.},"max":{"x":12.,"y":8.}},
                "top_z":0.,"target_z":-1.,"step_over":1.,"step_down":1.,
                "clearance_z":5.,"retract_z":2.,"feed_height_z":1.,
                "cutting":{"spindle_rpm":8000,"feed_xy":600.,"feed_z":150.,"coolant":"off"}}]}],
        "active_setup_id":1,"tools":[{"id":1,"number":1,"name":"2 mm end mill","kind":"flat_end_mill",
            "diameter":2.,"flute_length":8.,"overall_length":30.,"flute_count":2}],
        "next_setup_id":2,"next_tool_id":2,"next_operation_id":2
    })).unwrap();
    cam.setups[0].machine = Some(limo_cad_cam::CamMachineAssignmentDto::three_axis(
        Default::default(),
    ));
    edit(
        fixture,
        owner,
        "cam_set_document",
        serde_json::to_value(cam).unwrap(),
    );
    let before_generation = model(fixture);
    edit(
        fixture,
        owner,
        "cam_regenerate_setup",
        json!({"setup_id":1}),
    );
    let status =
        parse_engine_envelope(fixture.engine.engine_call("cam_toolpath_statuses", "")).unwrap();
    assert_eq!(status[0]["state"], "current", "{status}");
    assert!(!fixture
        .engine
        .cam_document_snapshot()
        .toolpath_generations
        .is_empty());
    (before_generation, model(fixture))
}

#[test]
fn attached_cam_reads_preserve_solid_and_generated_job_across_native_undo_redo() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    let (before_generation, generated) = generated_job(&fixture, &owner);
    let original_revision = revision(&fixture);
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let mut receipts = vec![];
    for (index, (name, args)) in [
        ("cam_plan_setup", json!({"setup_id":1})),
        ("cam_post_setup", json!({"setup_id":1})),
        ("cam_post_events", json!({"setup_id":1})),
        (
            "cam_simulate_setup",
            json!({"setup_id":1,"voxel_size":1.,"max_voxels":2048}),
        ),
        (
            "cam_simulate_gcode",
            json!({"setup_id":1,"voxel_size":1.,"max_voxels":2048,
                "dialect":"iso","source":"G21 G90 G17\nG54\nT1 M6\nS8000 M3\nG0 X0 Y0 Z5\nG1 Z-1 F150\nG1 X10 F600\nG0 Z5\nM30"}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let seq = index as u64 + 1;
        let request = enqueue(&fixture, seq, name, args, revision(&fixture));
        let receipt = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
        assert_eq!(receipt["applied"], true, "{name}: {receipt}");
        assert_eq!(
            model(&fixture),
            generated,
            "{name} must not change the saved project"
        );
        let archived: Value = serde_json::from_str(
            &fs::read_to_string(inbox_dir(&session).join(format!("applied/{seq}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(archived, request);
        let result: Value = serde_json::from_str(
            &fs::read_to_string(inbox_dir(&session).join(format!("results/{seq}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(result, receipt["result"]);
        receipts.push(receipt);
    }
    let undone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(
        fixture.engine.viewport_snapshot().2.bodies.len(),
        1,
        "A read-only CAM query must not redirect Undo to the unrelated solid extrusion"
    );
    assert_eq!(model(&fixture), before_generation);
    let revision_after_undo = revision(&fixture);
    enqueue(
        &fixture,
        6,
        "cam_plan_setup",
        json!({"setup_id":1}),
        revision_after_undo,
    );
    let read = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(read["applied"], true);
    assert_eq!(revision(&fixture), revision_after_undo);
    let redone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
        .unwrap();
    assert_eq!(model(&fixture), generated);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    assert_ne!(redone.context.epoch, undone.context.epoch);
    for receipt in receipts {
        assert_eq!(receipt["engine_revision"], original_revision);
    }
}

#[test]
fn cam_reads_keep_queued_edit_fences_and_reject_stale_or_misowned_queries() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    let (_, generated) = generated_job(&fixture, &owner);
    let base = revision(&fixture);
    let mut edited = fixture.engine.cam_document_snapshot();
    edited.setups[0].name = "Queued rename".into();
    enqueue(&fixture, 1, "cam_plan_setup", json!({"setup_id":1}), base);
    enqueue(
        &fixture,
        2,
        "cam_set_document",
        serde_json::to_value(edited).unwrap(),
        base,
    );
    enqueue(&fixture, 3, "cam_plan_setup", json!({"setup_id":1}), base);
    let read = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(read["applied"], true);
    assert_eq!(read["model_changed"], false);
    assert_eq!(revision(&fixture), base);
    let changed = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(changed["applied"], true);
    assert_eq!(changed["model_changed"], true);
    assert_eq!(revision(&fixture), base + 1);
    let current = model(&fixture);
    let stale = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(stale["reason"], "generation_conflict");
    assert_eq!(stale["applied"], false);
    let mut validated = false;
    assert!(fixture
        .bridge
        .apply_native_mutation_at(
            &fixture.engine,
            &owner,
            base,
            "cam_plan_setup",
            &json!({"setup_id":1}),
            || {
                validated = true;
                Ok(())
            }
        )
        .unwrap_err()
        .contains("design changed"));
    assert!(!validated);
    let native = fixture
        .bridge
        .apply_native_mutation_at(
            &fixture.engine,
            &owner,
            base + 1,
            "cam_plan_setup",
            &json!({"setup_id":1}),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(native.engine_revision, base + 1);
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let mut wrong = enqueue(
        &fixture,
        4,
        "cam_plan_setup",
        json!({"setup_id":1}),
        base + 1,
    );
    wrong["window_id"] = json!("other-window");
    fs::write(inbox_dir(&session).join("4.json"), wrong.to_string()).unwrap();
    let misowned = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(misowned["reason"], "session_identity_mismatch");
    enqueue(
        &fixture,
        5,
        "cam_plan_setup",
        json!({"setup_id":999}),
        base + 1,
    );
    let failed = apply_one_inbox_op(&fixture.bridge, "main", &fixture.engine).unwrap();
    assert_eq!(failed["dead_lettered"], true);
    assert_eq!(failed["applied"], false);
    assert_eq!(revision(&fixture), base + 1);
    assert_eq!(model(&fixture), current);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(model(&fixture), generated);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
}

#[test]
fn inbox_worker_prepares_replacement_geometry_for_edits_but_not_cam_reads() {
    use crate::native_viewport::interface_shell::NativeInterfaceHandle;
    use crate::session_bridge::native_interface::{
        controller::{worker, NativeServices},
        prepared::PreparedNativePresentation,
        NativeMutationResult,
    };
    use std::time::{Duration, Instant};

    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    generated_job(&fixture, &owner);
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = bevy::prelude::App::new();
    worker::install(
        app.world_mut(),
        services.clone(),
        NativeInterfaceHandle::new(|| {}),
    )
    .unwrap();
    let mut cam = fixture.engine.cam_document_snapshot();
    cam.setups[0].name = "An actual edit".into();
    for (index, (name, args, changes_model)) in [
        ("cam_plan_setup", json!({"setup_id":1}), false),
        ("cam_set_document", serde_json::to_value(cam).unwrap(), true),
    ]
    .into_iter()
    .enumerate()
    {
        let base = revision(&fixture);
        enqueue(&fixture, index as u64 + 1, name, args, base);
        worker::enqueue_inbox(
            app.world_mut(),
            |services, _guard| {
                let value = apply_one_inbox_op(&services.bridge, "main", &services.engine)?;
                let owner = services
                    .bridge
                    .native_document_context("main", &services.engine)?;
                let receipt = services
                    .bridge
                    .native_document_receipt(&services.engine, &owner)?;
                Ok(NativeMutationResult {
                    context: owner,
                    engine_revision: receipt.revision,
                    value,
                })
            },
            move |world, _, result| {
                assert_eq!(
                    world.contains_resource::<PreparedNativePresentation>(),
                    changes_model,
                    "Only an edit may prepare a replacement scene and publication"
                );
                Ok(result?.value)
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let receipt = loop {
            if let Some(outcome) = worker::poll(app.world_mut(), &services) {
                break outcome.value.unwrap();
            }
            assert!(Instant::now() < deadline, "Inbox worker did not complete");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(receipt["applied"], true);
        assert_eq!(receipt["model_changed"], changes_model);
        assert_eq!(revision(&fixture), base + u64::from(changes_model));
    }
}
