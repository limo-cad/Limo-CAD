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

/// Genuine repeated native stock and authored slider law shared by the two
/// detached swept worker/admission regressions; no synthetic scene metadata.
pub(crate) fn swept_stock(
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
) -> limo_cad_sketch::MotionStudyDto {
    use limo_cad_sketch::{
        MotionCoordinateDto, MotionDriverDto, MotionDriverId, MotionDriverLawDto, MotionStudyDto,
        MotionStudySampleDto,
    };
    let mutate = |operation: &str, args: Value| {
        bridge
            .apply_native_mutation(engine, owner, operation, &args, || Ok(()))
            .unwrap()
            .value
    };
    for (operation, args) in [
        (
            "sketch_begin",
            json!({"name":"Swept stock","plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":6.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Swept stock","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.}}),
        ),
    ] {
        mutate(operation, args);
    }
    let scene = engine.solid_scene_snapshot();
    let body = &scene.bodies[0];
    let face = body
        .faces
        .iter()
        .find(|f| {
            f.plane
                .is_some_and(|p| p.normal[2] > 0.9 && (p.origin[2] - 3.).abs() < 1e-6)
        })
        .unwrap();
    let plane = face.plane.unwrap();
    let root = engine.with_assembly(|a| a.component_structure.occurrences[0].clone());
    let moving=mutate("assembly_create_occurrence",json!({"component_id":root.component_id,"name":"Moving repeat","local_pose":{"translation":[0,0,5],"rotation":[0,0,0,1]}}))["id"].as_u64().unwrap();
    let connector = json!({"body_id":body.id,"face_id":face.id,"face_key":face.key,"frame":{"origin":plane.origin,"primary_axis":plane.normal,"secondary_axis":plane.u}});
    let joint = mutate(
        "assembly_create_joint",
        json!({"name":"Physical slider","kind":"slider","connector_a":connector,"connector_b":connector,"flipped":true,"linear_offset_mm":5,"advanced":{"connector_a_occurrence_id":root.id,"connector_b_occurrence_id":moving},"grounded_occurrence_id":root.id}),
    );
    let mut study: MotionStudyDto = serde_json::from_value(mutate(
        "assembly_create_motion_study",
        json!({"name":"Approach","duration_seconds":1}),
    ))
    .unwrap();
    study.drivers.push(MotionDriverDto {
        id: MotionDriverId(1),
        name: "Approach".into(),
        joint_id: serde_json::from_value(joint["id"].clone()).unwrap(),
        coordinate: MotionCoordinateDto::PrimaryLinear,
        law: MotionDriverLawDto::Motor {
            initial_value: 5.,
            velocity_per_second: -5.,
            acceleration_per_second2: 0.,
        },
        enabled: true,
    });
    study.next_driver_id = 2;
    mutate(
        "assembly_update_motion_study",
        serde_json::to_value(&study).unwrap(),
    );
    for (time, z) in [(0., 5.), (1., 0.)] {
        let sample: MotionStudySampleDto = serde_json::from_value(
            crate::session_bridge::parse_engine_envelope(engine.engine_call(
                "assembly_sample_motion_study",
                &json!({"study_id":study.id,"time_seconds":time}).to_string(),
            ))
            .unwrap(),
        )
        .unwrap();
        assert!(sample.solution.solved);
        let pose = sample
            .solution
            .instance_body_poses
            .iter()
            .find(|p| p.occurrence_id.0 == moving)
            .unwrap();
        assert!(
            pose.body_id == body.id
                && pose.translation[0].abs() < 1e-6
                && pose.translation[1].abs() < 1e-6
                && (pose.translation[2] - z).abs() < 1e-6,
            "Genuine slider must move the repeated stock along Z"
        );
    }
    study
}

pub(crate) fn contact_stock(
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
) -> (
    limo_cad_sketch::MotionStudyDto,
    limo_cad_sketch::ContactSetId,
) {
    let study = swept_stock(engine, bridge, owner);
    let sample: limo_cad_sketch::MotionStudySampleDto = serde_json::from_value(
        crate::session_bridge::parse_engine_envelope(engine.engine_call(
            "assembly_sample_motion_study",
            &json!({"study_id":study.id,"time_seconds":0.}).to_string(),
        ))
        .unwrap(),
    )
    .unwrap();
    let poses = &sample.solution.instance_body_poses;
    assert_eq!(poses.len(), 2);
    let value=bridge.apply_native_mutation(engine,owner,"assembly_create_contact_set",&json!({"name":"Physical stop","occurrence_a":poses[0].occurrence_id,"body_a":poses[0].body_id,"occurrence_b":poses[1].occurrence_id,"body_b":poses[1].body_id,"clearance_mm":0.5,"stop_motion":true}),||Ok(())).unwrap().value;
    (study, serde_json::from_value(value["id"].clone()).unwrap())
}
pub(crate) fn assert_contact_stop(
    report: &limo_cad_sketch::MotionStudyEvaluationDto,
    contact: limo_cad_sketch::ContactSetId,
) {
    assert_eq!(report.stopped_by_contact, Some(contact));
    assert!(
        (report.sample.time_seconds - 0.3).abs() < 1e-5,
        "Physical contact must stop at the independent .5 mm clearance boundary"
    );
    assert!(report.sample.solution.solved);
    assert_eq!(report.contacts.pairs.len(), 1);
    let pair = &report.contacts.pairs[0];
    assert_eq!(pair.body_a, pair.body_b);
    assert_ne!(pair.occurrence_a, pair.occurrence_b);
    assert!(!pair.interfering && pair.overlap_volume_mm3.abs() < 1e-6);
    assert!((pair.minimum_clearance_mm - 0.5).abs() < 1e-5);
    let moving = report
        .sample
        .solution
        .instance_body_poses
        .iter()
        .find(|p| p.translation[2] > 1.)
        .unwrap();
    assert!(
        (moving.translation[2] - 3.5).abs() < 1e-5,
        "Authored slider position must agree with physical clearance"
    );
}

// Independent physical row oracle for the actual repeated-stock slider fixture.
pub(crate) fn assert_motion_csv(csv: &str, rate: usize) {
    assert!(csv.starts_with("time_seconds,occurrence_id,component_id,x_mm,y_mm,z_mm,"));
    let rows: Vec<Vec<&str>> = csv
        .lines()
        .skip(1)
        .map(|line| line.split(',').collect())
        .collect();
    assert!(rows.iter().all(|row| row.len() == 14));
    let first = rows
        .iter()
        .find(|row| {
            row[0].parse::<f64>().unwrap().abs() < 1e-9
                && (row[5].parse::<f64>().unwrap() - 5.).abs() < 1e-6
        })
        .unwrap();
    let moving: Vec<_> = rows.iter().filter(|row| row[1] == first[1]).collect();
    assert_eq!(moving.len(), rate + 1);
    for (index, row) in moving.iter().enumerate() {
        let time = index as f64 / rate as f64;
        assert!((row[0].parse::<f64>().unwrap() - time).abs() < 1e-8);
        assert!((row[5].parse::<f64>().unwrap() - (5. - 5. * time)).abs() < 1e-6);
        if index > 0 {
            assert!((row[10].parse::<f64>().unwrap() - 5.).abs() < 1e-6);
        }
    }
}

#[test]
fn composite_command_retires_owner_only_after_valid_snapshot_load() {
    let _lock = super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let original = fixture.engine.project_snapshot().unwrap();
    let snapshot = limo_cad_project_file::ProjectSnapshot::new(
        original.model_json().to_owned(),
        vec![limo_cad_project_file::SnapshotIntent {
            name: "intent/command.cbor".into(),
            payload: vec![0, 255, 7],
        }],
    )
    .unwrap();
    let loaded = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "cad_load_project_snapshot",
            &json!({"snapshot_cbor_base64":snapshot.encode_base64().unwrap()}),
            || Ok(()),
        )
        .unwrap();
    assert_ne!(loaded.context, owner);
    assert_eq!(
        fixture.engine.project_snapshot().unwrap().intents(),
        snapshot.intents()
    );
    assert!(fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &loaded.context,
            "cad_load_project_snapshot",
            &json!({"snapshot_cbor_base64":"***"}),
            || Ok(())
        )
        .is_err());
    assert_eq!(fixture.owner(), loaded.context);
    assert_eq!(
        fixture.engine.project_snapshot().unwrap().intents(),
        snapshot.intents()
    );
}

#[test]
fn drafting_dispatch_shares_native_history_and_rejects_stale_owners() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let export = || ok(fixture.engine.engine_call("project_export_model", ""));
    let before = export();
    let owner = fixture.owner();
    let command = json!({"type":"create_layer","arguments":{
        "name":"Geometry","visible":true,"locked":false,"plottable":true,
        "style":{"line_type":null,"color_rgb":[0,0,0],"width_mm":0.25}
    }});
    let inbox_error =
        dispatch_inbox_on_engine(&fixture.engine, "drawing_drafting_apply", &command).unwrap_err();
    assert!(inbox_error.contains("unsupported inbox mutate"));
    assert_eq!(export(), before);
    let created = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "drawing_drafting_apply",
            &command,
            || Ok(()),
        )
        .unwrap();
    assert_eq!(created.value["drafting"]["layers"][0]["id"], 1);
    let after = export();
    assert_ne!(after, before);
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    assert!(fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "drawing_drafting_apply",
            &json!({"type":"delete_entity","arguments":{"id":99}}),
            || Ok(())
        )
        .is_err());
    assert_eq!(export(), after);
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap()
            .revision,
        receipt.revision
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(
        export(),
        before,
        "Failed edit must not insert an extra undo entry"
    );
    assert_ne!(fixture.owner(), owner);
    assert!(fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "drawing_drafting_apply",
            &command,
            || Ok(())
        )
        .is_err());
    assert_eq!(export(), before);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(), after);
}
