use super::model::*;
use limo_cad_cam::{CamDocumentDto, CamToolDto, CamUnits};
use serde_json::{json, Value};

fn cutter(id: u64, number: u32) -> CamToolDto {
    serde_json::from_value(json!({"id":id,"number":number,"name":format!("Cutter {id}"),
        "kind":"flat_end_mill","diameter":6.,"flute_length":20.,"overall_length":50.,"flute_count":4,
        "cutting":{"spindle_rpm":12000,"feed_xy":0.1,"feed_z":0.3,"coolant":"mist"}})).unwrap()
}

fn collection() -> Snapshot {
    let mut tool = serde_json::to_value(cutter(5, 1)).unwrap();
    tool["vendor-extension"] = json!({"stock_code":"FM-6","version":8});
    Snapshot::new(crate::cam_library::Snapshot {
        json: Some(
            json!({"next_tool_id":9,"tools":[tool],"collection-extension":{"shop":"A"}})
                .to_string(),
        ),
        path: "pinned-tool-library.json".into(),
        revision: "pinned-revision".into(),
    })
    .unwrap()
}

#[test]
fn native_project_duplicate_is_an_unsaved_exact_snapshot_with_an_editable_optional_number() {
    let mut cam = CamDocumentDto {
        units: CamUnits::Inches,
        tools: vec![cutter(5, u32::MAX)],
        next_tool_id: 6,
        ..Default::default()
    };
    cam.tools[0].diameter = 0.1;
    let before = cam.clone();
    let mut draft = super::copy_tool(&cam, 5).unwrap();
    assert!(draft.copied_tool && draft.dirty());
    assert_eq!(
        cam, before,
        "Opening a copy must not create or edit a project tool"
    );
    assert_eq!(
        draft
            .fields
            .iter()
            .find(|field| field.path == "/number")
            .unwrap()
            .text,
        ""
    );
    super::super::form::set(&mut draft, "/name", "Finished copy");
    let (created, selected) = super::create_copy(&draft, &cam).unwrap();
    assert_eq!(selected, super::Selection::Tool(6));
    assert_eq!(created.tools[0], before.tools[0]);
    let mut expected = before.tools[0].clone();
    expected.id = 6;
    expected.name = "Finished copy".into();
    expected.number = None;
    assert_eq!(created.tools[1], expected);
    assert_eq!(created.tools[1].diameter.to_bits(), 0.1_f64.to_bits());
}

#[test]
fn native_central_edits_preserve_extension_metadata_exact_values_and_opened_receipt() {
    let collection = collection();
    let original = collection.tools[0].clone();
    let mut tool = original.clone();
    tool.name = "Renamed cutter".into();
    let edit = collection.update(tool).unwrap();
    let data: Value = serde_json::from_str(&edit.json).unwrap();
    assert_eq!(data["collection-extension"], json!({"shop":"A"}));
    assert_eq!(
        data["tools"][0]["vendor-extension"],
        json!({"stock_code":"FM-6","version":8})
    );
    assert_eq!(
        data["tools"][0]["cutting"]["feed_xy"]
            .as_f64()
            .unwrap()
            .to_bits(),
        0.1_f64.to_bits()
    );
    assert_eq!(collection.tools[0], original);
    assert_eq!(collection.path, "pinned-tool-library.json");
    assert_eq!(collection.revision, "pinned-revision");
    let duplicate = collection.add(original.clone(), Some(5)).unwrap();
    let data: Value = serde_json::from_str(&duplicate.json).unwrap();
    assert_eq!(duplicate.tool.unwrap().id, 9);
    assert_eq!(data["next_tool_id"], 10);
    assert_eq!(
        data["tools"][1]["vendor-extension"],
        data["tools"][0]["vendor-extension"]
    );
    assert_eq!(
        data["tools"][0]["number"], data["tools"][1]["number"],
        "Independent central tools may share a machine number"
    );
    assert_eq!(
        collection.form_document(CamUnits::Inches).units,
        CamUnits::Inches
    );
}

#[test]
fn native_central_publish_replaces_same_id_explicitly_and_delete_never_changes_a_project() {
    let collection = collection();
    let project = CamDocumentDto {
        tools: vec![cutter(5, 1)],
        next_tool_id: 6,
        ..Default::default()
    };
    let before = project.clone();
    let mut tool = project.tools[0].clone();
    tool.name = "Project snapshot".into();
    let published = collection.publish(tool).unwrap();
    let data: Value = serde_json::from_str(&published.json).unwrap();
    assert_eq!(data["tools"].as_array().unwrap().len(), 1);
    assert_eq!(data["tools"][0]["name"], "Project snapshot");
    assert!(
        data["tools"][0].get("vendor-extension").is_none(),
        "Publish is an explicit full snapshot replacement"
    );
    assert_eq!(data["collection-extension"], json!({"shop":"A"}));
    let removed: Value = serde_json::from_str(&collection.remove(5).unwrap().json).unwrap();
    assert!(removed["tools"].as_array().unwrap().is_empty());
    assert_eq!(removed["next_tool_id"], 9);
    assert_eq!(project, before);
}

#[test]
fn native_central_import_preserves_programmed_cutting_and_rejects_project_number_collisions() {
    let collection = collection();
    let mut project: CamDocumentDto = serde_json::from_value(json!({
        "tools":[cutter(5,1)],"next_tool_id":6,"next_setup_id":2,"next_operation_id":2,"active_setup_id":1,
        "setups":[{"id":1,"name":"Stock","stock":{"min":{"x":0.,"y":0.,"z":-5.},"max":{"x":20.,"y":20.,"z":0.}},
        "operations":[{"id":1,"kind":"face","name":"Face","tool_id":5,"enabled":true,
            "bounds":{"min":{"x":0.,"y":0.},"max":{"x":20.,"y":20.}},"top_z":0.,"target_z":-1.,"step_down":1.,"step_over":2.,
            "clearance_z":5.,"retract_z":2.,"feed_height_z":1.,"cutting":{"spindle_rpm":18000,"feed_xy":1000.,"feed_z":200.,"coolant":"flood"}}]}]
    })).unwrap();
    project.tools[0].name = "Earlier project copy".into();
    project.validate_for_editing().unwrap();
    let next = collection.import(&project, 5).unwrap();
    assert_eq!(next.tools[0], collection.tools[0]);
    let mut expected = project.clone();
    expected.tools[0] = collection.tools[0].clone();
    assert_eq!(
        next, expected,
        "Import changes only the named project snapshot"
    );
    project.tools[0].id = 4;
    project.setups.clear();
    project.active_setup_id = None;
    project.validate_for_editing().unwrap();
    let error = collection.import(&project, 5).unwrap_err();
    assert!(error.contains("duplicate CAM tool number 1"), "{error}");
}

#[test]
fn native_central_created_tool_handles_project_identity_collision_without_rewriting_library() {
    let collection = collection();
    let project = CamDocumentDto {
        tools: vec![cutter(9, 1), cutter(10, 2)],
        next_tool_id: 40,
        ..Default::default()
    };
    let edit = collection.add(cutter(0, 3), None).unwrap();
    let tool = edit.tool.unwrap();
    assert_eq!(tool.id, 9);
    let next = import_created(&project, tool).unwrap();
    assert_eq!(
        next.tools.iter().map(|tool| tool.id).collect::<Vec<_>>(),
        vec![9, 10, 11]
    );
    assert_eq!(next.next_tool_id, 40);
    assert_eq!(next.tools[..2], project.tools);
    let data: Value = serde_json::from_str(&edit.json).unwrap();
    assert_eq!(
        data["tools"][1]["id"], 9,
        "Project collision cannot renumber the independent central entry"
    );
}

#[test]
fn native_central_clearing_a_chamfer_removes_the_omitted_canonical_field() {
    let mut source = cutter(5, 1);
    source.corner_chamfer = Some(limo_cad_cam::CamCornerChamferDto {
        width: 0.5,
        angle_degrees: 45.,
    });
    let mut row = serde_json::to_value(&source).unwrap();
    row["vendor-extension"] = json!("retained");
    let collection = Snapshot::new(crate::cam_library::Snapshot {
        json: Some(json!({"next_tool_id":6,"tools":[row]}).to_string()),
        path: "test-library".into(),
        revision: "original".into(),
    })
    .unwrap();
    source.corner_chamfer = None;
    source.corner_radius = Some(0.25);
    let edited = collection.update(source).unwrap();
    let value: Value = serde_json::from_str(&edited.json).unwrap();
    assert!(value["tools"][0].get("corner_chamfer").is_none());
    assert_eq!(value["tools"][0]["corner_radius"], 0.25);
    assert_eq!(value["tools"][0]["vendor-extension"], "retained");
}

struct StorageFixture(std::path::PathBuf);
impl StorageFixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("native-central-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn seed(&self) -> crate::cam_library::Snapshot {
        let empty = crate::cam_library::load(&self.0).unwrap();
        let source = json!({"next_tool_id":6,"tools":[cutter(5,1)],"shop_extension":{"keep":true}});
        crate::cam_library::save(&self.0, &source.to_string(), &empty.path, &empty.revision)
            .unwrap()
    }
}
impl Drop for StorageFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_central_storage_rejects_a_stale_edit_without_overwriting_the_other_writer() {
    let folder = StorageFixture::new();
    let loaded = folder.seed();
    let first = Snapshot::new(loaded.clone()).unwrap();
    let second = Snapshot::new(loaded).unwrap();
    let mut first_tool = first.tools[0].clone();
    first_tool.name = "Saved by the other window".into();
    let first_edit = first.update(first_tool).unwrap();
    let committed =
        crate::cam_library::save(&folder.0, &first_edit.json, &first.path, &first.revision)
            .unwrap();
    let expected_bytes = std::fs::read(&committed.path).unwrap();
    let mut second_tool = second.tools[0].clone();
    second_tool.name = "Uncommitted stale edit".into();
    let second_edit = second.update(second_tool).unwrap();
    let error =
        crate::cam_library::save(&folder.0, &second_edit.json, &second.path, &second.revision)
            .unwrap_err();
    assert!(error.contains("changed after loading"), "{error}");
    assert_eq!(std::fs::read(&committed.path).unwrap(), expected_bytes);
    assert_eq!(
        crate::cam_library::load(&folder.0).unwrap().revision,
        committed.revision
    );
    assert_eq!(
        second.tools[0].name, "Cutter 5",
        "Rejected storage never mutates the opened snapshot"
    );
    let refreshed = Snapshot::new(crate::cam_library::load(&folder.0).unwrap()).unwrap();
    let mut retry = refreshed.tools[0].clone();
    retry.name = "Explicit edit after refresh".into();
    let retry = refreshed.update(retry).unwrap();
    let saved =
        crate::cam_library::save(&folder.0, &retry.json, &refreshed.path, &refreshed.revision)
            .unwrap();
    let value: Value = serde_json::from_str(saved.json.as_deref().unwrap()).unwrap();
    assert_eq!(value["tools"][0]["name"], "Explicit edit after refresh");
    assert_eq!(value["shop_extension"], json!({"keep":true}));
}

#[test]
fn native_central_storage_receipt_rejects_a_new_location_even_with_identical_bytes() {
    let folder = StorageFixture::new();
    let opened = Snapshot::new(folder.seed()).unwrap();
    let original_bytes = std::fs::read(&opened.path).unwrap();
    let target = folder.0.join("other-collection");
    std::fs::create_dir(&target).unwrap();
    let target_file = target.join("cam-tool-library.json");
    std::fs::write(&target_file, &original_bytes).unwrap();
    crate::cam_library::set_location(
        &folder.0,
        Some(&target),
        crate::cam_library::LocationAction::UseExisting,
    )
    .unwrap();
    let current = crate::cam_library::load(&folder.0).unwrap();
    assert_eq!(
        current.revision, opened.revision,
        "This case isolates path ownership from content freshness"
    );
    assert_ne!(current.path, opened.path);
    let mut tool = opened.tools[0].clone();
    tool.name = "Must not reach another collection".into();
    let edit = opened.update(tool).unwrap();
    let error = crate::cam_library::save(&folder.0, &edit.json, &opened.path, &opened.revision)
        .unwrap_err();
    assert!(error.contains("location changed"), "{error}");
    assert_eq!(std::fs::read(&target_file).unwrap(), original_bytes);
    assert_eq!(std::fs::read(&opened.path).unwrap(), original_bytes);
    assert_eq!(
        crate::cam_library::load(&folder.0).unwrap().path,
        current.path
    );
}

fn drain_library_worker(
    world: &mut bevy::prelude::World,
    services: &crate::session_bridge::native_interface::controller::NativeServices,
) -> Result<Value, String> {
    use crate::session_bridge::native_interface::controller::worker;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Some(outcome) = worker::poll(world, services) {
            assert!(!worker::busy(world));
            return outcome.value;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Library worker did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn native_central_worker_rejects_retired_owner_and_revision_before_storage_or_project_writes() {
    use super::{io, State};
    use crate::session_bridge::native_interface::{
        controller::{worker, NativeServices},
        tests::Fixture,
        NativeInterfaceHandle,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for retire_owner in [false, true] {
        let fixture = Fixture::new();
        let owner = fixture.owner();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        let folder = StorageFixture::new();
        let source = folder.seed();
        let before_bytes = std::fs::read(&source.path).unwrap();
        if retire_owner {
            fixture
                .bridge
                .apply_native_mutation(
                    &fixture.engine,
                    &owner,
                    "cad_new_project",
                    &json!({}),
                    || Ok(()),
                )
                .unwrap();
            assert_ne!(fixture.owner(), owner);
        } else {
            fixture.rename(&owner, "Newer project revision").unwrap();
        }
        let model = || {
            crate::session_bridge::parse_engine_envelope(
                fixture.engine.engine_call("project_export_model", ""),
            )
            .unwrap()
        };
        let before_project = model();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let mut world = bevy::prelude::World::new();
        worker::install(
            &mut world,
            services.clone(),
            NativeInterfaceHandle::new(|| {}),
        )
        .unwrap();
        world.insert_resource(State {
            receipt: Some(receipt.clone()),
            serial: 7,
            ..Default::default()
        });
        let ran = Arc::new(AtomicBool::new(false));
        let operation_ran = ran.clone();
        let config = folder.0.clone();
        let snapshot = Snapshot::new(source.clone()).unwrap();
        let mut tool = snapshot.tools[0].clone();
        tool.name = "Stale owner attempted save".into();
        let edit = snapshot.update(tool).unwrap();
        io::task(
            &mut world,
            receipt.clone(),
            7,
            move || {
                operation_ran.store(true, Ordering::SeqCst);
                serde_json::to_value(crate::cam_library::save(
                    &config,
                    &edit.json,
                    &snapshot.path,
                    &snapshot.revision,
                )?)
                .map_err(|error| error.to_string())
            },
            |_, _| panic!("A rejected task cannot install a central snapshot"),
        )
        .unwrap();
        assert!(drain_library_worker(&mut world, &services).is_err());
        assert!(!ran.load(Ordering::SeqCst));
        assert_eq!(std::fs::read(&source.path).unwrap(), before_bytes);
        assert_eq!(model(), before_project);
        let next = CamDocumentDto {
            tools: vec![cutter(5, 1)],
            next_tool_id: 6,
            ..Default::default()
        };
        io::import(&mut world, receipt, next, 5).unwrap();
        assert!(drain_library_worker(&mut world, &services).is_err());
        assert_eq!(
            model(),
            before_project,
            "A stale central import cannot edit the current project"
        );
    }
}

#[test]
fn native_central_late_worker_completion_cannot_replace_a_new_dialog_snapshot() {
    use super::{io, State};
    use crate::session_bridge::native_interface::{
        controller::{worker, NativeServices},
        tests::Fixture,
        NativeInterfaceHandle,
    };
    use std::{
        sync::{mpsc, Arc},
        time::Duration,
    };
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut world = bevy::prelude::World::new();
    worker::install(
        &mut world,
        services.clone(),
        NativeInterfaceHandle::new(|| {}),
    )
    .unwrap();
    let folder = StorageFixture::new();
    let original = Arc::new(Snapshot::new(folder.seed()).unwrap());
    world.insert_resource(State {
        receipt: Some(receipt),
        serial: 11,
        snapshot: Some(original.clone()),
        ..Default::default()
    });
    let receipt = world.resource::<State>().receipt.clone().unwrap();
    let (started, start) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let config = folder.0.clone();
    io::task(
        &mut world,
        receipt,
        11,
        move || {
            started.send(()).map_err(|error| error.to_string())?;
            released
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| error.to_string())?;
            serde_json::to_value(crate::cam_library::load(&config)?)
                .map_err(|error| error.to_string())
        },
        |state, value| {
            io::install(state, &value, None, "Old completion must not appear")?;
            Ok(json!({"installed":true}))
        },
    )
    .unwrap();
    start.recv_timeout(Duration::from_secs(10)).unwrap();
    {
        let mut state = world.resource_mut::<State>();
        super::close(&mut state);
        state.status = "A later dialog owns its presentation".into();
        state.error = "Keep this later message".into();
    }
    release.send(()).unwrap();
    let result = drain_library_worker(&mut world, &services).unwrap();
    assert_eq!(result["dialog_changed"], true);
    let state = world.resource::<State>();
    assert!(Arc::ptr_eq(state.snapshot.as_ref().unwrap(), &original));
    assert!(state.receipt.is_none());
    assert!(state.draft.is_none());
    assert_eq!(state.status, "A later dialog owns its presentation");
    assert_eq!(state.error, "Keep this later message");
}
