use super::*;
use std::{
    fs,
    sync::{mpsc, Arc},
    time::Duration,
};

pub(super) struct Fixture {
    pub(super) bridge: Arc<SessionBridgeState>,
    pub(super) engine: Arc<AppState>,
    directory: std::path::PathBuf,
    prior_directory: Option<std::ffi::OsString>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "limo-cad-native-interface-{}",
            uuid::Uuid::new_v4()
        ));
        let prior_directory = std::env::var_os("LIMO_CAD_SESSION_DIR");
        std::env::set_var("LIMO_CAD_SESSION_DIR", &directory);
        let bridge = Arc::new(SessionBridgeState::default());
        let engine = Arc::new(AppState::new());
        ok(bridge.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("tab-a")
        }));
        Self {
            bridge,
            engine,
            directory,
            prior_directory,
        }
    }

    pub(super) fn owner(&self) -> DocumentContext {
        self.bridge
            .native_document_context("main", &self.engine)
            .unwrap()
    }

    pub(super) fn rename(
        &self,
        owner: &DocumentContext,
        name: &str,
    ) -> Result<NativeMutationResult, String> {
        self.bridge.apply_native_mutation(
            &self.engine,
            owner,
            "cad_set_document_name",
            &json!({"name":name}),
            || Ok(()),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(previous) = &self.prior_directory {
            std::env::set_var("LIMO_CAD_SESSION_DIR", previous);
        } else {
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn ok(raw: String) -> Value {
    crate::session_bridge::parse_engine_envelope(raw).unwrap()
}

#[test]
fn selecting_a_sheet_keeps_the_exact_rendered_scene_without_another_model_snapshot() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let created = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "drawing_create_sheet",
            &json!({"name":"Fabrication","format":"a4","orientation":"landscape"}),
            || Ok(()),
        )
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    refresh_native_model(&fixture.engine, world, false).unwrap();
    let model_revision = native_viewport::interface_model_revision(world);
    world.insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision: created.engine_revision,
        bodies: vec![(91, "Existing scene sentinel".into())],
    });
    for prepared_worker in [true, false] {
        let selected = fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &owner,
                "drawing_select_sheet",
                &json!({"sheet_id":1}),
                || Ok(()),
            )
            .unwrap();
        let revision = selected.engine_revision;
        if prepared_worker {
            let prepared = prepare_native_presentation(
                &fixture.engine,
                &fixture.bridge,
                &selected,
                "drawing_select_sheet",
            );
            assert!(
                matches!(&prepared.scene, Ok(PreparedNativeScene::Unchanged { from_revision })
                if from_revision.checked_add(1) == Some(revision))
            );
            world.insert_resource(prepared);
        }
        let outcome = finish_mutation(
            &fixture.engine,
            &fixture.bridge,
            world,
            "drawing_select_sheet",
            selected,
        );
        assert!(outcome["render_error"].is_null());
        assert_eq!(
            world.resource::<NativeRenderedDocument>().revision,
            revision
        );
        assert_eq!(
            world.resource::<NativeRenderedDocument>().bodies,
            vec![(91, "Existing scene sentinel".into())]
        );
        assert_eq!(
            native_viewport::interface_model_revision(world),
            model_revision
        );
    }
    native_viewport::apply_interface_edit_model(world, model_snapshot(&fixture.engine)).unwrap();
    assert!(!prepared::can_retain_scene(world));
    let selected = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "drawing_select_sheet",
            &json!({"sheet_id":1}),
            || Ok(()),
        )
        .unwrap();
    world.insert_resource(prepare_native_presentation(
        &fixture.engine,
        &fixture.bridge,
        &selected,
        "drawing_select_sheet",
    ));
    let outcome = finish_mutation(
        &fixture.engine,
        &fixture.bridge,
        world,
        "drawing_select_sheet",
        selected,
    );
    assert!(outcome["render_error"].is_null());
    assert!(prepared::can_retain_scene(world));
    assert!(native_viewport::interface_model_revision(world) > model_revision);
    let changed = fixture
        .rename(&owner, "Ordinary edit still prepares a model")
        .unwrap();
    let prepared = prepare_native_presentation(
        &fixture.engine,
        &fixture.bridge,
        &changed,
        "cad_set_document_name",
    );
    assert!(matches!(prepared.scene, Ok(PreparedNativeScene::Model(..))));
}

#[test]
fn superseded_completion_cannot_replace_a_newer_rendered_document() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let first = fixture.rename(&owner, "Earlier worker completion").unwrap();
    let prepared = prepare_native_presentation(
        &fixture.engine,
        &fixture.bridge,
        &first,
        "cad_set_document_name",
    );
    let latest = fixture.rename(&owner, "Later committed revision").unwrap();
    let mut world = World::new();
    world.insert_resource(prepared);
    world.insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision: latest.engine_revision,
        bodies: vec![(91, "Newer scene sentinel".into())],
    });
    let outcome = finish_mutation(
        &fixture.engine,
        &fixture.bridge,
        &mut world,
        "cad_set_document_name",
        first,
    );
    assert!(outcome["render_error"]
        .as_str()
        .unwrap()
        .contains("superseded"));
    let rendered = world.resource::<NativeRenderedDocument>();
    assert_eq!(rendered.owner, owner);
    assert_eq!(rendered.revision, latest.engine_revision);
    assert_eq!(rendered.bodies, vec![(91, "Newer scene sentinel".into())]);
}

#[test]
fn native_publication_writes_the_committed_revision_without_replaying_the_edit() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let result = fixture.rename(&owner, "Published native model").unwrap();
    let receipt = fixture
        .bridge
        .publish_native_document(&fixture.engine, &owner, "solid")
        .unwrap();
    assert_eq!(receipt["skipped"], false);
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let dir = fixture.directory.join(&session);
    let expected = ok(fixture.engine.engine_call("project_export_model", ""));
    assert_eq!(
        fs::read_to_string(dir.join("model.json")).unwrap(),
        expected.as_str().unwrap()
    );
    let heartbeat: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("heartbeat.json")).unwrap()).unwrap();
    assert_eq!(heartbeat["published_generation"], result.engine_revision);
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &owner, "solid")
        .unwrap();
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        Some(result.engine_revision)
    );
}

#[test]
fn active_sketch_publication_preserves_the_completed_model_and_publishes_live_entities() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
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
    let dir = fixture.directory.join(session);
    let completed = fs::read(dir.join("model.json")).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "sketch_begin",
            &json!({"type":"origin_plane","plane":"xy"}),
            || Ok(()),
        )
        .unwrap();
    let last=fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"sketch_add_rectangle",&json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":12.,"y":8.},"ctrl_held":false}),||Ok(())).unwrap();
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &owner, "sketch")
        .unwrap();
    assert_eq!(fs::read(dir.join("model.json")).unwrap(), completed);
    let active: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("active-sketch.json")).unwrap()).unwrap();
    assert_eq!(active, ok(fixture.engine.engine_call("active_sketch", "")));
    assert!(!active["entities"].as_array().unwrap().is_empty());
    let heartbeat: Value =
        serde_json::from_str(&fs::read_to_string(dir.join("heartbeat.json")).unwrap()).unwrap();
    assert_eq!(heartbeat["active_sketch_generation"], last.engine_revision);
    assert_eq!(heartbeat["published_generation"], last.engine_revision);
}

#[test]
fn publication_rejects_retired_owners_without_writing_into_the_successor() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let old = fixture.owner();
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &old, "solid")
        .unwrap();
    let changed = fixture
        .bridge
        .apply_native_mutation(&fixture.engine, &old, "cad_new_project", &json!({}), || {
            Ok(())
        })
        .unwrap();
    assert_ne!(old, changed.context);
    let successor = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    assert!(fixture
        .bridge
        .publish_native_document(&fixture.engine, &old, "solid")
        .is_err());
    assert!(!fixture
        .directory
        .join(&successor)
        .join("model.json")
        .exists());
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &changed.context, "solid")
        .unwrap();
    assert!(fixture
        .directory
        .join(successor)
        .join("model.json")
        .is_file());
}

#[test]
fn failed_snapshot_io_does_not_repeat_or_roll_back_a_successful_native_edit() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let changed = fixture.rename(&owner, "One successful edit").unwrap();
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let block = fixture.directory.join(session).join("model.json");
    fs::create_dir_all(&block).unwrap();
    assert!(fixture
        .bridge
        .publish_native_document(&fixture.engine, &owner, "solid")
        .is_err());
    assert_eq!(
        fixture.engine.document_snapshot().name,
        "One successful edit"
    );
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        Some(changed.engine_revision)
    );
    fs::remove_dir(&block).unwrap();
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &owner, "solid")
        .unwrap();
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        Some(changed.engine_revision)
    );
}

#[test]
fn native_mutation_uses_live_engine_and_advances_revision_once() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let initial = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    let result = fixture.rename(&owner, "Native command").unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Native command");
    assert_eq!(result.context, owner);
    assert_eq!(result.engine_revision, initial + 1);
    assert_eq!(
        fixture.owner(),
        owner,
        "Ordinary edits do not replace the document"
    );
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        Some(initial + 1)
    );

    let rejection = fixture.bridge.apply_native_mutation(
        &fixture.engine,
        &owner,
        "cad_set_document_name",
        &json!({"name":"Wrong binding"}),
        || Err("Control was rebound before dispatch".into()),
    );
    assert!(rejection.is_err());
    assert_eq!(fixture.engine.document_snapshot().name, "Native command");
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        Some(initial + 1)
    );
    assert!(fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "invented_operation",
            &json!({}),
            || Ok(())
        )
        .is_err());
}

#[test]
fn queued_native_action_cannot_follow_a_tab_switch_or_same_tab_replacement() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner_a = fixture.owner();
    fixture.rename(&owner_a, "Original A").unwrap();
    ok(fixture
        .bridge
        .with_project_session_transition("main", &fixture.engine, || {
            fixture.engine.create_project_session("tab-b")
        }));
    let owner_b = fixture.owner();
    fixture.rename(&owner_b, "Original B").unwrap();
    assert!(fixture.rename(&owner_a, "Wrong tab").is_err());
    assert_eq!(fixture.engine.document_snapshot().name, "Original B");

    ok(fixture
        .bridge
        .with_project_session_transition("main", &fixture.engine, || {
            fixture.engine.activate_project_session("tab-a")
        }));
    assert_eq!(fixture.engine.document_snapshot().name, "Original A");
    let replaced = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner_a,
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(replaced.context.document_id, owner_a.document_id);
    assert_ne!(replaced.context.epoch, owner_a.epoch);
    assert_eq!(replaced.context, fixture.owner());
    assert!(fixture.rename(&owner_a, "Wrong incarnation").is_err());
    fixture.rename(&replaced.context, "Replacement A").unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Replacement A");

    let mut wrong_window = replaced.context;
    wrong_window.window_id = "recording".into();
    assert!(fixture.rename(&wrong_window, "Wrong window").is_err());
    assert_eq!(fixture.engine.document_snapshot().name, "Replacement A");
}

#[test]
fn rejected_unchanged_load_keeps_native_incarnation_and_model() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    fixture.rename(&owner, "Keep this design").unwrap();
    let before = fixture.engine.engine_call("project_export_model", "");
    let revision = fixture.bridge.engine_revision_for_window("main").unwrap();
    let result = fixture.bridge.apply_native_mutation(
        &fixture.engine,
        &owner,
        "cad_load_project_model",
        &json!({"model_json":r#"{"format":"limo-cad-project","schema_version":9999}"#}),
        || Ok(()),
    );
    assert!(result.is_err());
    assert_eq!(fixture.owner(), owner);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        revision
    );
}

#[test]
fn native_action_serializes_with_a_concurrent_project_transition() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let (validated_tx, validated_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let action_bridge = fixture.bridge.clone();
    let action_engine = fixture.engine.clone();
    let action = std::thread::spawn(move || {
        action_bridge.apply_native_mutation(
            &action_engine,
            &owner,
            "cad_set_document_name",
            &json!({"name":"Committed to A"}),
            || {
                validated_tx.send(()).unwrap();
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|error| error.to_string())
            },
        )
    });
    validated_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        fixture.bridge.publishers.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    let transition_bridge = fixture.bridge.clone();
    let transition_engine = fixture.engine.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let transition = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        transition_bridge.with_project_session_transition("main", &transition_engine, || {
            transition_engine.create_project_session("tab-b")
        })
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    resume_tx.send(()).unwrap();
    action.join().unwrap().unwrap();
    ok(transition.join().unwrap());
    assert_ne!(fixture.engine.document_snapshot().name, "Committed to A");
    ok(fixture
        .bridge
        .with_project_session_transition("main", &fixture.engine, || {
            fixture.engine.activate_project_session("tab-a")
        }));
    assert_eq!(fixture.engine.document_snapshot().name, "Committed to A");
}

#[test]
fn view_effects_share_owner_fence_without_advancing_model_revision() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let revision = fixture.bridge.engine_revision_for_window("main").unwrap();
    fixture
        .bridge
        .with_native_document_owner(&fixture.engine, &owner, || {
            assert!(matches!(
                fixture.bridge.publishers.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        revision
    );
    ok(fixture
        .bridge
        .with_project_session_transition("main", &fixture.engine, || {
            fixture.engine.create_project_session("tab-b")
        }));
    assert!(fixture
        .bridge
        .with_native_document_owner(&fixture.engine, &owner, || -> Result<(), String> {
            panic!("stale view must not apply")
        })
        .is_err());
}

#[test]
fn revision_exhaustion_rejects_before_mutating_the_live_model() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let before = fixture.engine.document_snapshot();
    fixture
        .bridge
        .publishers
        .lock()
        .unwrap()
        .get_mut("main")
        .unwrap()
        .active_mut()
        .engine_revision = u64::MAX;
    assert!(fixture.rename(&owner, "Must not commit").is_err());
    assert_eq!(fixture.engine.document_snapshot(), before);
}
